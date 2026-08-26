//! 80x25 Text Terminal Screen with CGA/VGA 16-color text-mode emulation,
//! memory-mapped VRAM synchronization (0xB8000), cursor positioning, and interactive typing.

use egui::{Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use emulator_core::bus::MemoryBus;
use std::collections::VecDeque;

/// Standard 80x25 text mode dimensions.
pub const TERMINAL_COLS: usize = 80;
pub const TERMINAL_ROWS: usize = 25;
pub const TOTAL_CELLS: usize = TERMINAL_COLS * TERMINAL_ROWS;
pub const VRAM_BYTES_PER_CELL: usize = 2;
pub const VRAM_TOTAL_SIZE: usize = TOTAL_CELLS * VRAM_BYTES_PER_CELL; // 4000 bytes

/// Default IBM PC compatible video RAM base address for CGA/VGA text mode.
pub const DEFAULT_VRAM_BASE_ADDR: u64 = 0x000B_8000;

/// Standard 16-color CGA/EGA/VGA Palette.
pub const CGA_PALETTE: [Color32; 16] = [
    Color32::from_rgb(0, 0, 0),       // 0: Black
    Color32::from_rgb(0, 0, 170),     // 1: Blue
    Color32::from_rgb(0, 170, 0),     // 2: Green
    Color32::from_rgb(0, 170, 170),   // 3: Cyan
    Color32::from_rgb(170, 0, 0),     // 4: Red
    Color32::from_rgb(170, 0, 170),   // 5: Magenta
    Color32::from_rgb(170, 85, 0),    // 6: Brown
    Color32::from_rgb(170, 170, 170), // 7: Light Gray (Standard foreground)
    Color32::from_rgb(85, 85, 85),    // 8: Dark Gray (Bright Black)
    Color32::from_rgb(85, 85, 255),   // 9: Bright Blue
    Color32::from_rgb(85, 255, 85),   // 10: Bright Green
    Color32::from_rgb(85, 255, 255),  // 11: Bright Cyan
    Color32::from_rgb(255, 85, 85),   // 12: Bright Red
    Color32::from_rgb(255, 85, 255),  // 13: Bright Magenta
    Color32::from_rgb(255, 255, 85),  // 14: Yellow
    Color32::from_rgb(255, 255, 255), // 15: Bright White
];

/// A single 80x25 terminal character cell (character byte + attribute byte).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct TerminalCell {
    pub char_code: u8,
    pub attribute: u8,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            char_code: b' ',
            attribute: 0x07, // Light gray on black
        }
    }
}

impl TerminalCell {
    pub fn new(char_code: u8, attribute: u8) -> Self {
        Self { char_code, attribute }
    }

    #[inline]
    pub fn fg_color_index(&self) -> usize {
        (self.attribute & 0x0F) as usize
    }

    #[inline]
    pub fn bg_color_index(&self) -> usize {
        ((self.attribute >> 4) & 0x0F) as usize
    }

    #[inline]
    pub fn fg_color(&self) -> Color32 {
        CGA_PALETTE[self.fg_color_index()]
    }

    #[inline]
    pub fn bg_color(&self) -> Color32 {
        CGA_PALETTE[self.bg_color_index()]
    }

    #[inline]
    pub fn as_char(&self) -> char {
        if self.char_code >= 0x20 && self.char_code <= 0x7E {
            self.char_code as char
        } else if self.char_code == 0 {
            ' '
        } else {
            // Map common extended CP437 box-drawing / symbols to Unicode or placeholder
            match self.char_code {
                0xDB => '█',
                0xB0 => '░',
                0xB1 => '▒',
                0xB2 => '▓',
                0xC4 => '─',
                0xB3 => '│',
                0xDA => '┌',
                0xBF => '┐',
                0xC0 => '└',
                0xD9 => '┘',
                0xC3 => '├',
                0xB4 => '┤',
                0xC2 => '┬',
                0xC1 => '┴',
                0xC5 => '┼',
                0xCD => '═',
                0xBA => '║',
                0xC9 => '╔',
                0xBB => '╗',
                0xC8 => '╚',
                0xBC => '╝',
                0xCC => '╠',
                0xB9 => '╣',
                0xCB => '╦',
                0xCA => '╩',
                0xCE => '╬',
                0x18 => '↑',
                0x19 => '↓',
                0x1A => '→',
                0x1B => '←',
                0x01 => '☺',
                0x02 => '☻',
                0x03 => '♥',
                0x04 => '♦',
                0x05 => '♣',
                0x06 => '♠',
                _ => '·',
            }
        }
    }
}

/// 80x25 Text Terminal state.
pub struct TerminalScreen {
    pub cells: [TerminalCell; TOTAL_CELLS],
    pub cursor_col: usize,
    pub cursor_row: usize,
    pub cursor_visible: bool,
    pub cursor_blink: bool,
    pub default_attr: u8,
    pub vram_base_addr: u64,
    pub vram_sync_enabled: bool,
    pub font_size: f32,
    pub keyboard_queue: VecDeque<u8>,
    pub input_line: String,
    pub local_echo: bool,
}

impl Default for TerminalScreen {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalScreen {
    pub fn new() -> Self {
        let mut term = Self {
            cells: [TerminalCell::default(); TOTAL_CELLS],
            cursor_col: 0,
            cursor_row: 0,
            cursor_visible: true,
            cursor_blink: true,
            default_attr: 0x07, // Light Gray on Black
            vram_base_addr: DEFAULT_VRAM_BASE_ADDR,
            vram_sync_enabled: true,
            font_size: 13.0,
            keyboard_queue: VecDeque::with_capacity(256),
            input_line: String::new(),
            local_echo: true,
        };
        term.load_startup_banner();
        term
    }

    /// Clears the entire terminal screen with the specified attribute and blanks.
    pub fn clear(&mut self, attr: u8) {
        for cell in self.cells.iter_mut() {
            *cell = TerminalCell::new(b' ', attr);
        }
        self.cursor_col = 0;
        self.cursor_row = 0;
    }

    /// Sets character and attribute at specific (col, row).
    pub fn set_cell(&mut self, col: usize, row: usize, ch: u8, attr: u8) {
        if col < TERMINAL_COLS && row < TERMINAL_ROWS {
            self.cells[row * TERMINAL_COLS + col] = TerminalCell::new(ch, attr);
        }
    }

    /// Gets cell reference at (col, row).
    pub fn get_cell(&self, col: usize, row: usize) -> Option<&TerminalCell> {
        if col < TERMINAL_COLS && row < TERMINAL_ROWS {
            Some(&self.cells[row * TERMINAL_COLS + col])
        } else {
            None
        }
    }

    /// Writes a single byte as a teletype stream, interpreting standard control characters.
    pub fn write_byte(&mut self, byte: u8, attr: u8) {
        match byte {
            b'\n' => {
                self.cursor_col = 0;
                self.cursor_row += 1;
                if self.cursor_row >= TERMINAL_ROWS {
                    self.scroll_up(1, attr);
                    self.cursor_row = TERMINAL_ROWS - 1;
                }
            }
            b'\r' => {
                self.cursor_col = 0;
            }
            0x08 => {
                // Backspace
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                    self.set_cell(self.cursor_col, self.cursor_row, b' ', attr);
                }
            }
            b'\t' => {
                // Tab (advance to next 8-column boundary)
                let next_tab = (self.cursor_col + 8) & !7;
                self.cursor_col = next_tab.min(TERMINAL_COLS - 1);
            }
            0x0C => {
                // Form feed / Clear screen
                self.clear(attr);
            }
            ch => {
                self.set_cell(self.cursor_col, self.cursor_row, ch, attr);
                self.cursor_col += 1;
                if self.cursor_col >= TERMINAL_COLS {
                    self.cursor_col = 0;
                    self.cursor_row += 1;
                    if self.cursor_row >= TERMINAL_ROWS {
                        self.scroll_up(1, attr);
                        self.cursor_row = TERMINAL_ROWS - 1;
                    }
                }
            }
        }
    }

    /// Writes a UTF-8 string to the terminal.
    pub fn write_str(&mut self, text: &str, attr: u8) {
        for &b in text.as_bytes() {
            self.write_byte(b, attr);
        }
    }

    /// Scrolls the screen upwards by `lines` rows, filling bottom rows with blank space.
    pub fn scroll_up(&mut self, lines: usize, fill_attr: u8) {
        if lines >= TERMINAL_ROWS {
            self.clear(fill_attr);
            return;
        }

        let cells_to_move = (TERMINAL_ROWS - lines) * TERMINAL_COLS;
        let shift_offset = lines * TERMINAL_COLS;

        for i in 0..cells_to_move {
            self.cells[i] = self.cells[i + shift_offset];
        }

        for i in cells_to_move..TOTAL_CELLS {
            self.cells[i] = TerminalCell::new(b' ', fill_attr);
        }
    }

    /// Synchronizes terminal screen cells from memory bus at `vram_base_addr`.
    pub fn sync_from_vram(&mut self, bus: &dyn MemoryBus) {
        if !self.vram_sync_enabled {
            return;
        }

        for i in 0..TOTAL_CELLS {
            let addr = self.vram_base_addr + (i * 2) as u64;
            let ch = bus.read_u8(addr).unwrap_or(b' ');
            let attr = bus.read_u8(addr + 1).unwrap_or(0x07);
            self.cells[i] = TerminalCell::new(ch, attr);
        }
    }

    /// Writes terminal screen cells into memory bus at `vram_base_addr`.
    pub fn sync_to_vram(&self, bus: &mut dyn MemoryBus) {
        for (i, cell) in self.cells.iter().enumerate() {
            let addr = self.vram_base_addr + (i * 2) as u64;
            let _ = bus.write_u8(addr, cell.char_code);
            let _ = bus.write_u8(addr + 1, cell.attribute);
        }
    }

    /// Pushes a keystroke to the terminal keyboard input queue.
    pub fn push_key(&mut self, key: u8) {
        self.keyboard_queue.push_back(key);
        if self.local_echo {
            self.write_byte(key, self.default_attr);
        }
    }

    /// Loads an authentic startup boot header into the terminal.
    pub fn load_startup_banner(&mut self) {
        self.clear(0x07);
        // Header bar in Blue background
        for col in 0..TERMINAL_COLS {
            self.set_cell(col, 0, b' ', 0x1F); // Bright white on blue
        }
        let title = "  M15 Multi-Architecture 80x25 Video Terminal [VGA Mode 03h: 0xB8000]  ";
        for (i, &b) in title.as_bytes().iter().enumerate() {
            if i < TERMINAL_COLS {
                self.set_cell(i, 0, b, 0x1F);
            }
        }

        self.cursor_row = 2;
        self.cursor_col = 0;
        self.write_str("System BIOS (C) 2026 M15 Systems Inc.\n", 0x0F);
        self.write_str("CPU Core: #![no_std] Multi-Arch Engine | Display: 80x25 Character Grid\n", 0x07);
        self.write_str("VRAM Buffer: 0x000B8000 - 0x000B8FA0 (4000 Bytes, 2000 Cells)\n\n", 0x0B);
        self.write_str("Ready for CPU Execution & Interactive Input.\n", 0x0A);
        self.write_str("> ", 0x0E);
    }

    /// Renders the independent 80x25 Terminal window.
    pub fn render_window(
        &mut self,
        ctx: &egui::Context,
        open: &mut bool,
        bus: &mut dyn MemoryBus,
    ) {
        if !*open {
            return;
        }

        if self.vram_sync_enabled {
            self.sync_from_vram(bus);
        }

        egui::Window::new("🖥 80x25 Text Terminal (VRAM 0xB8000)")
            .open(open)
            .default_width(720.0)
            .default_height(480.0)
            .min_width(500.0)
            .min_height(350.0)
            .resizable(true)
            .show(ctx, |ui| {
                // Toolbar controls
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Clear Screen").clicked() {
                        self.clear(self.default_attr);
                        self.sync_to_vram(bus);
                    }
                    if ui.button("Sample Banner").clicked() {
                        self.load_startup_banner();
                        self.sync_to_vram(bus);
                    }
                    if ui.button("Test Pattern").clicked() {
                        self.load_color_test_pattern();
                        self.sync_to_vram(bus);
                    }

                    ui.separator();

                    ui.checkbox(&mut self.vram_sync_enabled, "Sync VRAM (0xB8000)");
                    ui.checkbox(&mut self.local_echo, "Local Echo");
                    ui.checkbox(&mut self.cursor_visible, "Show Cursor");

                    ui.separator();

                    ui.label("Font Size:");
                    ui.add(egui::Slider::new(&mut self.font_size, 9.0..=22.0).text("px"));
                });

                ui.separator();

                // 80x25 Character Screen Canvas
                self.render_screen_canvas(ui);

                ui.separator();

                // Interactive Input Prompt
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Input:").strong().color(Color32::from_rgb(100, 200, 255)));
                    let text_edit = ui.add(
                        egui::TextEdit::singleline(&mut self.input_line)
                            .hint_text("Type string or command and press Enter...")
                            .desired_width(ui.available_width() - 80.0),
                    );

                    let send_clicked = ui.button("Send ↵").clicked();
                    let enter_pressed = text_edit.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter));

                    if (send_clicked || enter_pressed) && !self.input_line.is_empty() {
                        let to_send = format!("{}\n", self.input_line);
                        for &b in to_send.as_bytes() {
                            self.push_key(b);
                        }
                        self.input_line.clear();
                        self.sync_to_vram(bus);
                        text_edit.request_focus();
                    }
                });
            });
    }

    /// Loads a 16-color test pattern demonstrating all CGA attributes and box characters.
    pub fn load_color_test_pattern(&mut self) {
        self.clear(0x07);
        self.cursor_row = 0;
        self.cursor_col = 0;

        self.write_str("=== CGA 16-Color Palette & Attribute Test (80x25) ===\n\n", 0x0E);

        for color_idx in 0..16 {
            let name = match color_idx {
                0 => "0:Black  ",
                1 => "1:Blue   ",
                2 => "2:Green  ",
                3 => "3:Cyan   ",
                4 => "4:Red    ",
                5 => "5:Magenta",
                6 => "6:Brown  ",
                7 => "7:LtGray ",
                8 => "8:DkGray ",
                9 => "9:BrBlue ",
                10 => "10:BrGrn ",
                11 => "11:BrCyan",
                12 => "12:BrRed ",
                13 => "13:BrMag ",
                14 => "14:Yellow",
                15 => "15:White ",
                _ => "",
            };
            let attr = color_idx as u8; // FG on Black
            self.write_str(name, attr);
            if (color_idx + 1) % 4 == 0 {
                self.write_str("\n", 0x07);
            } else {
                self.write_str("  ", 0x07);
            }
        }

        self.write_str("\nBox Characters: ┌─┬─┐ │ ├─┼─┤ │ └─┴─┘  ╔═╦═╗ ║ ╠═╬═╣ ║ ╚═╩═╝\n", 0x0B);
        self.write_str("Shading Blocks: ░░░ ▒▒▒ ▓▓▓ ███  Symbols: ☺ ☻ ♥ ♦ ♣ ♠ ↑ ↓ → ←\n", 0x0D);
    }

    /// Renders the 80x25 grid on an egui canvas with accurate font dimensions, background rectangles, and characters.
    fn render_screen_canvas(&self, ui: &mut Ui) {
        egui::Frame::canvas(ui.style())
            .fill(Color32::from_rgb(10, 10, 15))
            .stroke(Stroke::new(1.5, Color32::from_rgb(50, 60, 80)))
            .show(ui, |ui| {
                let char_width = self.font_size * 0.60;
                let char_height = self.font_size * 1.25;

                let total_width = char_width * (TERMINAL_COLS as f32);
                let total_height = char_height * (TERMINAL_ROWS as f32);

                let (rect, _response) = ui.allocate_exact_size(
                    Vec2::new(total_width + 8.0, total_height + 8.0),
                    Sense::click_and_drag(),
                );

                let origin = rect.min + Vec2::new(4.0, 4.0);
                let font_id = FontId::monospace(self.font_size);
                let painter = ui.painter();

                // Draw background for every non-black cell
                for row in 0..TERMINAL_ROWS {
                    for col in 0..TERMINAL_COLS {
                        let cell = &self.cells[row * TERMINAL_COLS + col];
                        let bg_color = cell.bg_color();

                        let cell_rect = Rect::from_min_size(
                            Pos2::new(
                                origin.x + (col as f32) * char_width,
                                origin.y + (row as f32) * char_height,
                            ),
                            Vec2::new(char_width, char_height),
                        );

                        if bg_color != Color32::from_rgb(0, 0, 0) {
                            painter.rect_filled(cell_rect, 0.0, bg_color);
                        }

                        let ch = cell.as_char();
                        if ch != ' ' {
                            let text_pos = Pos2::new(
                                cell_rect.min.x,
                                cell_rect.min.y + (char_height - self.font_size) * 0.5,
                            );
                            painter.text(
                                text_pos,
                                egui::Align2::LEFT_TOP,
                                ch.to_string(),
                                font_id.clone(),
                                cell.fg_color(),
                            );
                        }
                    }
                }

                // Draw Cursor
                if self.cursor_visible && self.cursor_col < TERMINAL_COLS && self.cursor_row < TERMINAL_ROWS {
                    let cursor_rect = Rect::from_min_size(
                        Pos2::new(
                            origin.x + (self.cursor_col as f32) * char_width,
                            origin.y + (self.cursor_row as f32) * char_height + char_height * 0.8,
                        ),
                        Vec2::new(char_width, char_height * 0.2),
                    );
                    painter.rect_filled(cursor_rect, 0.0, Color32::from_rgb(220, 220, 220));
                }
            });
    }
}
