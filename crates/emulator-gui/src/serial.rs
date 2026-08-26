//! Interactive UART Serial Console with bidirectional TX/RX buffers,
//! configurable baud rate and line parameters, hex/ASCII views, and MMIO/Port support.

use egui::{Color32, RichText, ScrollArea, Ui};
use emulator_core::bus::MemoryBus;
use std::collections::VecDeque;

/// Standard UART Line Endings.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SerialLineEnding {
    CRLF, // \r\n
    LF,   // \n
    CR,   // \r
}

impl SerialLineEnding {
    pub fn name(&self) -> &'static str {
        match self {
            SerialLineEnding::CRLF => "CRLF (\\r\\n)",
            SerialLineEnding::LF => "LF (\\n)",
            SerialLineEnding::CR => "CR (\\r)",
        }
    }

    pub fn bytes(&self) -> &'static [u8] {
        match self {
            SerialLineEnding::CRLF => b"\r\n",
            SerialLineEnding::LF => b"\n",
            SerialLineEnding::CR => b"\r",
        }
    }
}

/// Standard UART Parity modes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SerialParity {
    None,
    Even,
    Odd,
}

impl SerialParity {
    pub fn name(&self) -> &'static str {
        match self {
            SerialParity::None => "None (N)",
            SerialParity::Even => "Even (E)",
            SerialParity::Odd => "Odd (O)",
        }
    }
}

/// A timestamped log entry in the serial console.
#[derive(Clone, Debug)]
pub struct SerialLogEntry {
    pub text: String,
    pub is_tx: bool, // true: CPU -> Serial (TX), false: User -> CPU (RX)
    pub timestamp_ms: u64,
}

/// Interactive Serial Console state.
pub struct SerialConsole {
    pub log_entries: Vec<SerialLogEntry>,
    pub rx_queue: VecDeque<u8>,
    pub tx_buffer: Vec<u8>,
    pub baud_rate: u32,
    pub data_bits: u8,
    pub parity: SerialParity,
    pub stop_bits: u8,
    pub mmio_base_addr: u64,
    pub auto_scroll: bool,
    pub show_hex: bool,
    pub show_timestamps: bool,
    pub line_ending: SerialLineEnding,
    pub input_line: String,
    pub loopback: bool,
    pub dtr: bool,
    pub rts: bool,
    pub cts: bool,
    pub dsr: bool,
    pub tx_bytes_total: usize,
    pub rx_bytes_total: usize,
    start_time_instant: std::time::Instant,
}

impl Default for SerialConsole {
    fn default() -> Self {
        Self::new()
    }
}

impl SerialConsole {
    /// Creates a new Serial Console initialized to 115200 8N1.
    pub fn new() -> Self {
        let mut console = Self {
            log_entries: Vec::new(),
            rx_queue: VecDeque::with_capacity(1024),
            tx_buffer: Vec::with_capacity(1024),
            baud_rate: 115200,
            data_bits: 8,
            parity: SerialParity::None,
            stop_bits: 1,
            mmio_base_addr: 0x03F8, // Standard COM1 UART base (or customizable MMIO)
            auto_scroll: true,
            show_hex: false,
            show_timestamps: true,
            line_ending: SerialLineEnding::CRLF,
            input_line: String::new(),
            loopback: false,
            dtr: true,
            rts: true,
            cts: true,
            dsr: true,
            tx_bytes_total: 0,
            rx_bytes_total: 0,
            start_time_instant: std::time::Instant::now(),
        };

        console.load_startup_banner();
        console
    }

    /// Loads initial startup banner into serial log.
    pub fn load_startup_banner(&mut self) {
        self.log_entries.clear();
        self.append_log("[SYSTEM] UART 16550A Controller Initialized at Port/MMIO 0x03F8", true);
        self.append_log("[SYSTEM] Serial Parameters: 115200 baud, 8 Data Bits, No Parity, 1 Stop Bit (8N1)", true);
        self.append_log("[SYSTEM] Serial Console ready for TX/RX stream.", true);
    }

    fn current_timestamp_ms(&self) -> u64 {
        self.start_time_instant.elapsed().as_millis() as u64
    }

    /// Appends a line of text to the console log.
    pub fn append_log(&mut self, text: &str, is_tx: bool) {
        self.log_entries.push(SerialLogEntry {
            text: text.to_string(),
            is_tx,
            timestamp_ms: self.current_timestamp_ms(),
        });
    }

    /// Receives a byte transmitted by the CPU (UART TX).
    pub fn write_tx_byte(&mut self, byte: u8) {
        self.tx_bytes_total += 1;
        self.tx_buffer.push(byte);

        if byte == b'\n' || self.tx_buffer.len() >= 256 {
            let s = String::from_utf8_lossy(&self.tx_buffer).trim_end_matches(['\r', '\n']).to_string();
            if !s.is_empty() {
                self.append_log(&s, true);
            }
            self.tx_buffer.clear();
        }

        if self.loopback {
            self.rx_queue.push_back(byte);
            self.rx_bytes_total += 1;
        }
    }

    /// Writes a complete string from CPU into TX stream.
    pub fn write_tx_str(&mut self, text: &str) {
        for &b in text.as_bytes() {
            self.write_tx_byte(b);
        }
    }

    /// Pulls a byte from the RX buffer to be read by the CPU.
    pub fn read_rx_byte(&mut self) -> Option<u8> {
        self.rx_queue.pop_front()
    }

    /// Checks if there is pending data for the CPU to read.
    pub fn has_rx_data(&self) -> bool {
        !self.rx_queue.is_empty()
    }

    /// Enqueues bytes from the UI to be delivered to the CPU.
    pub fn push_rx_bytes(&mut self, data: &[u8]) {
        for &b in data {
            self.rx_queue.push_back(b);
            self.rx_bytes_total += 1;
        }
        let s = String::from_utf8_lossy(data).trim_end_matches(['\r', '\n']).to_string();
        if !s.is_empty() {
            self.append_log(&s, false);
        }
    }

    /// Sends current user input line with configured line ending.
    pub fn send_input_line(&mut self) {
        if self.input_line.is_empty() {
            return;
        }

        let mut data = self.input_line.as_bytes().to_vec();
        data.extend_from_slice(self.line_ending.bytes());
        self.push_rx_bytes(&data);
        self.input_line.clear();
    }

    /// Clears console log history.
    pub fn clear(&mut self) {
        self.log_entries.clear();
        self.tx_buffer.clear();
    }

    /// Sends an ASCII/Hex test message from the console.
    pub fn send_test_pattern(&mut self) {
        self.append_log("[TEST] Serial UART Echo & Frame Test", true);
        self.append_log("The quick brown fox jumps over the lazy dog 0123456789", true);
        self.append_log("ASCII Table Range: !\"#$%&'()*+,-./0123456789:;<=>?@ABCDEFGHIJKLMNOPQRSTUVWXYZ", true);
    }

    /// Synchronizes MMIO registers with emulator memory bus.
    /// Standard 16550 UART Registers:
    /// 0x00: RBR (read) / THR (write)
    /// 0x05: LSR (Line Status Register) - bit 0 = DR (Data Ready), bit 5 = THRE (Transmitter Holding Empty)
    pub fn sync_with_bus(&mut self, bus: &mut dyn MemoryBus) {
        // Read TX byte if CPU wrote to THR
        if let Ok(val @ 1..=255) = bus.read_u8(self.mmio_base_addr) {
            self.write_tx_byte(val);
            let _ = bus.write_u8(self.mmio_base_addr, 0); // Clear THR
        }

        // Update LSR (Line Status Register at offset +5)
        // Bit 0: Data Ready (DR), Bit 5: Transmit Holding Register Empty (THRE = 1)
        let mut lsr = 0x20; // THRE is always ready
        if self.has_rx_data() {
            lsr |= 0x01; // Data Ready
        }
        let _ = bus.write_u8(self.mmio_base_addr + 5, lsr);

        // If CPU reads RBR, deliver next byte in queue
        // In simple MMIO polling, we expose peek at offset 0 if DR is set
        if let Some(&next_byte) = self.rx_queue.front() {
            let _ = bus.write_u8(self.mmio_base_addr + 1, next_byte); // RBR Peek at offset 1
        }
    }

    /// Renders the independent Serial Console window.
    pub fn render_window(&mut self, ctx: &egui::Context, open: &mut bool) {
        if !*open {
            return;
        }

        egui::Window::new("📟 Serial Console (UART 16550A)")
            .open(open)
            .default_width(680.0)
            .default_height(450.0)
            .min_width(450.0)
            .min_height(300.0)
            .resizable(true)
            .show(ctx, |ui| {
                // Top Settings & Status Toolbar
                self.render_toolbar(ui);

                ui.separator();

                // Signal LEDs Bar
                self.render_signals_bar(ui);

                ui.separator();

                // Log Viewport
                self.render_log_viewport(ui);

                ui.separator();

                // Input Send Bar
                self.render_input_bar(ui);
            });
    }

    fn render_toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            // Baud Rate
            ui.label("Baud:");
            egui::ComboBox::from_id_salt("serial_baud_combo")
                .selected_text(format!("{}", self.baud_rate))
                .show_ui(ui, |ui| {
                    for &baud in &[9600, 19200, 38400, 57600, 115200, 230400] {
                        ui.selectable_value(&mut self.baud_rate, baud, format!("{}", baud));
                    }
                });

            ui.separator();

            // Line Ending
            ui.label("Line Ending:");
            egui::ComboBox::from_id_salt("serial_line_ending_combo")
                .selected_text(self.line_ending.name())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.line_ending, SerialLineEnding::CRLF, "CRLF");
                    ui.selectable_value(&mut self.line_ending, SerialLineEnding::LF, "LF");
                    ui.selectable_value(&mut self.line_ending, SerialLineEnding::CR, "CR");
                });

            ui.separator();

            if ui.button("Clear Log").clicked() {
                self.clear();
            }

            if ui.button("Test Pattern").clicked() {
                self.send_test_pattern();
            }

            ui.separator();

            ui.checkbox(&mut self.auto_scroll, "Auto-Scroll");
            ui.checkbox(&mut self.show_timestamps, "Timestamps");
            ui.checkbox(&mut self.show_hex, "Hex Dump");
            ui.checkbox(&mut self.loopback, "Loopback");
        });
    }

    fn render_signals_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Status Lines:").strong().size(11.0));

            Self::render_led(ui, "TX", !self.tx_buffer.is_empty(), Color32::from_rgb(80, 220, 100));
            Self::render_led(ui, "RX", !self.rx_queue.is_empty(), Color32::from_rgb(100, 200, 255));
            Self::render_led(ui, "DTR", self.dtr, Color32::from_rgb(255, 200, 80));
            Self::render_led(ui, "RTS", self.rts, Color32::from_rgb(255, 200, 80));
            Self::render_led(ui, "CTS", self.cts, Color32::from_rgb(80, 220, 100));
            Self::render_led(ui, "DSR", self.dsr, Color32::from_rgb(80, 220, 100));

            ui.separator();

            ui.label(
                RichText::new(format!(
                    "TX: {} bytes | RX: {} bytes (Queue: {})",
                    self.tx_bytes_total,
                    self.rx_bytes_total,
                    self.rx_queue.len()
                ))
                .weak()
                .size(11.0),
            );
        });
    }

    fn render_led(ui: &mut Ui, label: &str, active: bool, active_color: Color32) {
        let color = if active {
            active_color
        } else {
            Color32::from_rgb(50, 50, 50)
        };
        ui.label(RichText::new(format!("[{}]", label)).strong().color(color).size(11.0));
    }

    fn render_log_viewport(&mut self, ui: &mut Ui) {
        let log_height = ui.available_height() - 42.0;

        egui::Frame::canvas(ui.style())
            .fill(Color32::from_rgb(15, 18, 24))
            .show(ui, |ui| {
                ui.set_min_height(log_height);
                ui.set_max_height(log_height);

                let scroll_area = ScrollArea::vertical().auto_shrink([false, false]);
                let scroll_area = if self.auto_scroll {
                    scroll_area.stick_to_bottom(true)
                } else {
                    scroll_area
                };

                scroll_area.show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;

                    if self.log_entries.is_empty() {
                        ui.label(RichText::new("Serial log is empty. Transmit from CPU or send input below.").weak().italics());
                    }

                    for entry in &self.log_entries {
                        ui.horizontal(|ui| {
                            if self.show_timestamps {
                                let sec = entry.timestamp_ms as f64 / 1000.0;
                                ui.label(
                                    RichText::new(format!("[{:>7.3}s]", sec))
                                        .color(Color32::from_rgb(120, 120, 140))
                                        .monospace()
                                        .size(11.0),
                                );
                            }

                            let (prefix, color) = if entry.is_tx {
                                ("TX ◀", Color32::from_rgb(140, 230, 140))
                            } else {
                                ("RX ▶", Color32::from_rgb(120, 200, 255))
                            };

                            ui.label(RichText::new(prefix).strong().color(color).monospace().size(11.0));

                            if self.show_hex {
                                let hex_str: Vec<String> = entry
                                    .text
                                    .as_bytes()
                                    .iter()
                                    .map(|b| format!("{:02X}", b))
                                    .collect();
                                ui.label(
                                    RichText::new(hex_str.join(" "))
                                        .color(Color32::from_rgb(200, 200, 180))
                                        .monospace(),
                                );
                            } else {
                                ui.label(RichText::new(&entry.text).color(Color32::from_rgb(230, 230, 230)).monospace());
                            }
                        });
                    }
                });
            });
    }

    fn render_input_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Send:").strong().color(Color32::from_rgb(120, 200, 255)));

            let text_edit = ui.add(
                egui::TextEdit::singleline(&mut self.input_line)
                    .hint_text("Type serial message and press Enter...")
                    .desired_width(ui.available_width() - 85.0),
            );

            let send_clicked = ui.button("Send ↵").clicked();
            let enter_pressed = text_edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

            if (send_clicked || enter_pressed) && !self.input_line.is_empty() {
                self.send_input_line();
                text_edit.request_focus();
            }
        });
    }
}
