//! Interrupt emulation modes and VGA Buffer configuration for M15 CPU Emulator.

use egui::{Color32, Context, RichText, Window};
use emulator_core::arch::{AnyCpu, Architecture};
use emulator_core::bus::{DynamicMemory, MemoryBus};
use emulator_core::cpu::CpuEngine;

use crate::app::ExecutionState;
use crate::serial::SerialConsole;
use crate::terminal::{TerminalScreen, TERMINAL_COLS, TERMINAL_ROWS, VRAM_TOTAL_SIZE};

/// Target interrupt emulation profile/mode.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum InterruptMode {
    #[default]
    ArchDefault,
    MsDos,
    Bios,
    Uefi,
}

impl InterruptMode {
    /// All available interrupt emulation modes.
    pub const ALL: &'static [InterruptMode] = &[
        InterruptMode::ArchDefault,
        InterruptMode::MsDos,
        InterruptMode::Bios,
        InterruptMode::Uefi,
    ];

    /// Canonical short name for the interrupt mode.
    pub const fn name(&self) -> &'static str {
        match self {
            InterruptMode::ArchDefault => "Arch Default",
            InterruptMode::MsDos => "MS-DOS",
            InterruptMode::Bios => "BIOS",
            InterruptMode::Uefi => "UEFI",
        }
    }

    /// Display label with icon for GUI menus.
    pub const fn display_label(&self) -> &'static str {
        match self {
            InterruptMode::ArchDefault => "⚡ Arch Default",
            InterruptMode::MsDos => "💾 MS-DOS",
            InterruptMode::Bios => "🖥 BIOS",
            InterruptMode::Uefi => "🌐 UEFI",
        }
    }

    /// Descriptive overview of the interrupt profile.
    pub const fn description(&self) -> &'static str {
        match self {
            InterruptMode::ArchDefault => {
                "Native architecture hardware interrupt vectoring and exception traps"
            }
            InterruptMode::MsDos => {
                "MS-DOS API emulation (INT 21h, INT 20h DOS system calls and console I/O)"
            }
            InterruptMode::Bios => {
                "IBM PC compatible BIOS services (INT 10h Video/VGA, INT 16h Keyboard)"
            }
            InterruptMode::Uefi => {
                "UEFI Runtime & GOP Services (Graphics Output Protocol, system table calls)"
            }
        }
    }
}

/// Standard VGA Buffer base address presets.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum VgaBufferPreset {
    #[default]
    BiosDefault,
    UefiDefault,
    Custom,
}

impl VgaBufferPreset {
    /// Canonical name of preset.
    pub const fn name(&self) -> &'static str {
        match self {
            VgaBufferPreset::BiosDefault => "BIOS Default (0x000B8000)",
            VgaBufferPreset::UefiDefault => "UEFI Default (0x00A00000)",
            VgaBufferPreset::Custom => "Custom",
        }
    }

    /// Default base address in memory.
    pub const fn default_base_addr(&self) -> u64 {
        match self {
            VgaBufferPreset::BiosDefault => 0x000B_8000,
            VgaBufferPreset::UefiDefault => 0x00A0_0000,
            VgaBufferPreset::Custom => 0x000B_8000,
        }
    }
}

/// Configuration settings for the VGA text/frame buffer.
#[derive(Clone, Debug, PartialEq)]
pub struct VgaBufferConfig {
    pub preset: VgaBufferPreset,
    pub base_addr: u64,
    pub custom_addr_input: String,
    pub sync_enabled: bool,
    pub rows: usize,
    pub cols: usize,
}

impl Default for VgaBufferConfig {
    fn default() -> Self {
        Self::bios_default()
    }
}

impl VgaBufferConfig {
    /// Creates a BIOS standard default VGA buffer configuration (0xB8000).
    pub fn bios_default() -> Self {
        Self {
            preset: VgaBufferPreset::BiosDefault,
            base_addr: 0x000B_8000,
            custom_addr_input: "0x000B8000".to_string(),
            sync_enabled: true,
            rows: TERMINAL_ROWS,
            cols: TERMINAL_COLS,
        }
    }

    /// Creates a UEFI GOP default VGA buffer configuration (0xA0000).
    pub fn uefi_default() -> Self {
        Self {
            preset: VgaBufferPreset::UefiDefault,
            base_addr: 0x00A0_0000,
            custom_addr_input: "0x00A00000".to_string(),
            sync_enabled: true,
            rows: TERMINAL_ROWS,
            cols: TERMINAL_COLS,
        }
    }

    /// Creates a custom VGA buffer configuration.
    pub fn custom(addr: u64) -> Self {
        Self {
            preset: VgaBufferPreset::Custom,
            base_addr: addr,
            custom_addr_input: format!("{:#010X}", addr),
            sync_enabled: true,
            rows: TERMINAL_ROWS,
            cols: TERMINAL_COLS,
        }
    }

    /// Applies a preset to the configuration and updates the base address.
    pub fn apply_preset(&mut self, preset: VgaBufferPreset) {
        self.preset = preset;
        match preset {
            VgaBufferPreset::BiosDefault => {
                self.base_addr = 0x000B_8000;
                self.custom_addr_input = "0x000B8000".to_string();
            }
            VgaBufferPreset::UefiDefault => {
                self.base_addr = 0x00A0_0000;
                self.custom_addr_input = "0x00A00000".to_string();
            }
            VgaBufferPreset::Custom => {
                if let Some(parsed) = parse_hex_addr(&self.custom_addr_input) {
                    self.base_addr = parsed;
                }
            }
        }
    }

    /// Sets a custom base address and switches preset to Custom.
    pub fn set_custom_addr(&mut self, addr: u64) {
        self.preset = VgaBufferPreset::Custom;
        self.base_addr = addr;
        self.custom_addr_input = format!("{:#010X}", addr);
    }
}

/// Parses a hexadecimal or decimal memory address string.
pub fn parse_hex_addr(input: &str) -> Option<u64> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(hex_str) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex_str.replace('_', "").trim(), 16).ok()
    } else if s.ends_with('h') || s.ends_with('H') {
        u64::from_str_radix(&s[..s.len() - 1].replace('_', "").trim(), 16).ok()
    } else {
        u64::from_str_radix(&s.replace('_', ""), 16).ok()
    }
}

/// Dispatches an interrupt based on the active `InterruptMode`.
pub fn dispatch_interrupt(
    mode: InterruptMode,
    vector: u32,
    cpu: &mut AnyCpu,
    bus: &mut DynamicMemory,
    terminal: &mut TerminalScreen,
    serial: &mut SerialConsole,
) -> (Option<String>, Option<ExecutionState>) {
    match mode {
        InterruptMode::MsDos => handle_msdos_interrupt(vector, cpu, bus, terminal, serial),
        InterruptMode::Bios => handle_bios_interrupt(vector, cpu, bus, terminal, serial),
        InterruptMode::Uefi => handle_uefi_interrupt(vector, cpu, bus, terminal, serial),
        InterruptMode::ArchDefault => (
            Some(format!("Interrupt raised: vector {:#X}", vector)),
            None,
        ),
    }
}

/// Handles MS-DOS system call interrupts (INT 21h, INT 20h, etc.).
fn handle_msdos_interrupt(
    vector: u32,
    cpu: &mut AnyCpu,
    bus: &mut DynamicMemory,
    terminal: &mut TerminalScreen,
    serial: &mut SerialConsole,
) -> (Option<String>, Option<ExecutionState>) {
    match vector {
        0x21 => {
            let ax = cpu
                .get_register("AX")
                .or_else(|| cpu.get_register("EAX"))
                .or_else(|| cpu.get_register("RAX"))
                .unwrap_or(0);
            let ah = ((ax >> 8) & 0xFF) as u8;
            let al = (ax & 0xFF) as u8;

            match ah {
                0x09 => {
                    // Print '$'-terminated string at DS:DX (or RDX/EDX)
                    let ds = cpu.get_register("DS").unwrap_or(0);
                    let dx = cpu
                        .get_register("DX")
                        .or_else(|| cpu.get_register("EDX"))
                        .or_else(|| cpu.get_register("RDX"))
                        .unwrap_or(0);
                    let addr = match cpu.arch() {
                        Architecture::I8086 => (ds << 4).wrapping_add(dx),
                        _ => dx,
                    };

                    let mut str_bytes = Vec::new();
                    let mut curr = addr;
                    while str_bytes.len() < 4096 {
                        match bus.read_u8(curr) {
                            Ok(b'$') | Err(_) => break,
                            Ok(b) => {
                                str_bytes.push(b);
                                curr = curr.wrapping_add(1);
                            }
                        }
                    }

                    let text = String::from_utf8_lossy(&str_bytes).to_string();
                    terminal.write_str(&text, 0x07);
                    terminal.sync_to_vram(bus);
                    serial.write_tx_str(&text);

                    let summary = if text.len() > 30 {
                        format!("{}...", &text[..30])
                    } else {
                        text
                    };
                    (
                        Some(format!("MS-DOS INT 21h AH=09h: Printed string \"{}\"", summary)),
                        None,
                    )
                }
                0x02 => {
                    // Print character in DL
                    let dx = cpu
                        .get_register("DX")
                        .or_else(|| cpu.get_register("EDX"))
                        .unwrap_or(0);
                    let dl = (dx & 0xFF) as u8;
                    terminal.write_byte(dl, 0x07);
                    terminal.sync_to_vram(bus);
                    serial.write_tx_byte(dl);
                    (
                        Some(format!(
                            "MS-DOS INT 21h AH=02h: Output character '{}' ({:#04X})",
                            dl as char, dl
                        )),
                        None,
                    )
                }
                0x01 => {
                    // Read character from keyboard with echo
                    let ch = terminal.keyboard_queue.pop_front().unwrap_or(b'\r');
                    let new_ax = (ax & !0xFF) | (ch as u64);
                    let _ = cpu.set_register("AX", new_ax);
                    terminal.write_byte(ch, 0x07);
                    terminal.sync_to_vram(bus);
                    (
                        Some(format!(
                            "MS-DOS INT 21h AH=01h: Read character '{}' ({:#04X})",
                            ch as char, ch
                        )),
                        None,
                    )
                }
                0x08 => {
                    // Read character without echo
                    let ch = terminal.keyboard_queue.pop_front().unwrap_or(b'\r');
                    let new_ax = (ax & !0xFF) | (ch as u64);
                    let _ = cpu.set_register("AX", new_ax);
                    (
                        Some(format!(
                            "MS-DOS INT 21h AH=08h: Read character '{:?}' ({:#04X})",
                            ch as char, ch
                        )),
                        None,
                    )
                }
                0x4C => {
                    // Terminate process with return code AL
                    (
                        Some(format!(
                            "MS-DOS Program Terminated (INT 21h AH=4Ch, Exit Code: {})",
                            al
                        )),
                        Some(ExecutionState::Stopped),
                    )
                }
                0x30 => {
                    // Get DOS Version -> Return 5.0 in AX (AL=5, AH=0)
                    let _ = cpu.set_register("AX", 0x0005);
                    (
                        Some("MS-DOS INT 21h AH=30h: DOS Version 5.0 returned in AX".to_string()),
                        None,
                    )
                }
                0x40 => {
                    // Write to file or device handle
                    let cx = cpu.get_register("CX").unwrap_or(0) as usize;
                    let ds = cpu.get_register("DS").unwrap_or(0);
                    let dx = cpu.get_register("DX").unwrap_or(0);
                    let addr = match cpu.arch() {
                        Architecture::I8086 => (ds << 4).wrapping_add(dx),
                        _ => dx,
                    };

                    let mut str_bytes = Vec::new();
                    for i in 0..cx {
                        if let Ok(b) = bus.read_u8(addr.wrapping_add(i as u64)) {
                            str_bytes.push(b);
                        }
                    }
                    let text = String::from_utf8_lossy(&str_bytes).to_string();
                    terminal.write_str(&text, 0x07);
                    terminal.sync_to_vram(bus);
                    serial.write_tx_str(&text);
                    let _ = cpu.set_register("AX", cx as u64);
                    (
                        Some(format!("MS-DOS INT 21h AH=40h: Wrote {} bytes to handle", cx)),
                        None,
                    )
                }
                _ => (
                    Some(format!(
                        "MS-DOS INT 21h function AH={:#04X} AL={:#04X} serviced",
                        ah, al
                    )),
                    None,
                ),
            }
        }
        0x20 => (
            Some("MS-DOS Program Terminated (INT 20h)".to_string()),
            Some(ExecutionState::Stopped),
        ),
        0x10 => handle_bios_interrupt(0x10, cpu, bus, terminal, serial),
        _ => (
            Some(format!("MS-DOS Interrupt vector {:#X} raised", vector)),
            None,
        ),
    }
}

/// Handles IBM PC BIOS service interrupts (INT 10h Video, INT 16h Keyboard, etc.).
fn handle_bios_interrupt(
    vector: u32,
    cpu: &mut AnyCpu,
    bus: &mut DynamicMemory,
    terminal: &mut TerminalScreen,
    serial: &mut SerialConsole,
) -> (Option<String>, Option<ExecutionState>) {
    match vector {
        0x10 => {
            // Video BIOS Services
            let ax = cpu
                .get_register("AX")
                .or_else(|| cpu.get_register("EAX"))
                .unwrap_or(0);
            let bx = cpu
                .get_register("BX")
                .or_else(|| cpu.get_register("EBX"))
                .unwrap_or(0);
            let dx = cpu
                .get_register("DX")
                .or_else(|| cpu.get_register("EDX"))
                .unwrap_or(0);

            let ah = ((ax >> 8) & 0xFF) as u8;
            let al = (ax & 0xFF) as u8;
            let bh = ((bx >> 8) & 0xFF) as u8;
            let bl = (bx & 0xFF) as u8;
            let dh = ((dx >> 8) & 0xFF) as u8;
            let dl = (dx & 0xFF) as u8;

            match ah {
                0x0E => {
                    // Teletype Output
                    let attr = if bl != 0 { bl } else { 0x07 };
                    terminal.write_byte(al, attr);
                    terminal.sync_to_vram(bus);
                    serial.write_tx_byte(al);
                    (
                        Some(format!(
                            "BIOS INT 10h AH=0Eh: Teletype character '{}' ({:#04X})",
                            al as char, al
                        )),
                        None,
                    )
                }
                0x00 => {
                    // Set Video Mode
                    terminal.clear(0x07);
                    terminal.sync_to_vram(bus);
                    (
                        Some(format!(
                            "BIOS INT 10h AH=00h: Video mode {:#04X} set (80x25 text)",
                            al
                        )),
                        None,
                    )
                }
                0x02 => {
                    // Set Cursor Position (DH=row, DL=col)
                    terminal.cursor_row = (dh as usize).min(TERMINAL_ROWS - 1);
                    terminal.cursor_col = (dl as usize).min(TERMINAL_COLS - 1);
                    (
                        Some(format!(
                            "BIOS INT 10h AH=02h: Set cursor pos to ({}, {})",
                            terminal.cursor_col, terminal.cursor_row
                        )),
                        None,
                    )
                }
                0x03 => {
                    // Get Cursor Position -> Return in DX (DH=row, DL=col)
                    let new_dx = ((terminal.cursor_row as u64 & 0xFF) << 8)
                        | (terminal.cursor_col as u64 & 0xFF);
                    let _ = cpu.set_register("DX", new_dx);
                    let _ = cpu.set_register("CX", 0x0607);
                    (
                        Some(format!(
                            "BIOS INT 10h AH=03h: Cursor pos ({}, {}) returned in DX",
                            terminal.cursor_col, terminal.cursor_row
                        )),
                        None,
                    )
                }
                0x06 => {
                    // Scroll Window Up (AL=lines, BH=fill attribute)
                    let lines = if al == 0 { TERMINAL_ROWS } else { al as usize };
                    let attr = if bh != 0 { bh } else { 0x07 };
                    terminal.scroll_up(lines, attr);
                    terminal.sync_to_vram(bus);
                    (
                        Some(format!("BIOS INT 10h AH=06h: Scrolled up {} lines", lines)),
                        None,
                    )
                }
                _ => (
                    Some(format!(
                        "BIOS INT 10h Video function AH={:#04X} AL={:#04X} serviced",
                        ah, al
                    )),
                    None,
                ),
            }
        }
        0x16 => {
            // Keyboard BIOS Services
            let ax = cpu.get_register("AX").unwrap_or(0);
            let ah = ((ax >> 8) & 0xFF) as u8;
            match ah {
                0x00 | 0x10 => {
                    // Read keystroke from queue
                    let ch = terminal.keyboard_queue.pop_front().unwrap_or(0);
                    let new_ax = (ax & !0xFFFF) | (ch as u64);
                    let _ = cpu.set_register("AX", new_ax);
                    (
                        Some(format!(
                            "BIOS INT 16h AH=00h: Keystroke '{:?}' ({:#04X}) read",
                            ch as char, ch
                        )),
                        None,
                    )
                }
                _ => (
                    Some(format!(
                        "BIOS INT 16h Keyboard function AH={:#04X} serviced",
                        ah
                    )),
                    None,
                ),
            }
        }
        _ => (
            Some(format!("BIOS Interrupt vector {:#X} serviced", vector)),
            None,
        ),
    }
}

/// Handles UEFI runtime & GOP framebuffer interrupt services.
fn handle_uefi_interrupt(
    vector: u32,
    cpu: &mut AnyCpu,
    bus: &mut DynamicMemory,
    terminal: &mut TerminalScreen,
    serial: &mut SerialConsole,
) -> (Option<String>, Option<ExecutionState>) {
    // Emulate UEFI GOP console output or system table runtime calls
    let r0 = cpu
        .get_register("RAX")
        .or_else(|| cpu.get_register("EAX"))
        .or_else(|| cpu.get_register("X0"))
        .or_else(|| cpu.get_register("R0"))
        .unwrap_or(0);

    let ch = (r0 & 0xFF) as u8;
    if ch >= 0x20 || ch == b'\n' || ch == b'\r' || ch == b'\t' {
        terminal.write_byte(ch, 0x0F); // Bright white for UEFI
        terminal.sync_to_vram(bus);
        serial.write_tx_byte(ch);
    }

    (
        Some(format!(
            "UEFI Service Call serviced: Vector {:#X} (GOP Console Buffer: {:#010X})",
            vector, terminal.vram_base_addr
        )),
        None,
    )
}

/// Renders the standalone VGA Buffer Configuration modal dialog.
pub fn render_vga_buffer_dialog(
    ctx: &Context,
    open: &mut bool,
    config: &mut VgaBufferConfig,
    terminal: &mut TerminalScreen,
    bus: &mut DynamicMemory,
) {
    if !*open {
        return;
    }

    let mut is_open = *open;
    let mut close_clicked = false;

    Window::new("📺 VGA Buffer Configuration")
        .open(&mut is_open)
        .default_width(460.0)
        .min_width(380.0)
        .resizable(false)
        .show(ctx, |ui| {
            ui.heading(RichText::new("Video RAM & VGA Buffer Settings").strong());
            ui.label("Configure memory-mapped VRAM buffer address, preset, and bus synchronization.");

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            ui.label(RichText::new("Memory Mapping Presets:").strong());

            let mut preset_changed = false;
            ui.horizontal(|ui| {
                if ui
                    .radio_value(
                        &mut config.preset,
                        VgaBufferPreset::BiosDefault,
                        "BIOS Default (0x000B8000)",
                    )
                    .clicked()
                {
                    preset_changed = true;
                    config.apply_preset(VgaBufferPreset::BiosDefault);
                    terminal.vram_base_addr = config.base_addr;
                    terminal.sync_to_vram(bus);
                }
            });

            ui.horizontal(|ui| {
                if ui
                    .radio_value(
                        &mut config.preset,
                        VgaBufferPreset::UefiDefault,
                        "UEFI Default (0x00A00000)",
                    )
                    .clicked()
                {
                    preset_changed = true;
                    config.apply_preset(VgaBufferPreset::UefiDefault);
                    terminal.vram_base_addr = config.base_addr;
                    terminal.sync_to_vram(bus);
                }
            });

            ui.horizontal(|ui| {
                if ui
                    .radio_value(&mut config.preset, VgaBufferPreset::Custom, "Custom Address")
                    .clicked()
                {
                    preset_changed = true;
                    config.apply_preset(VgaBufferPreset::Custom);
                    terminal.vram_base_addr = config.base_addr;
                }
            });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            // Custom Address Input Field
            ui.label(RichText::new("VRAM Base Address:").strong());
            ui.horizontal(|ui| {
                let response = ui.text_edit_singleline(&mut config.custom_addr_input);
                if response.changed() || preset_changed {
                    if let Some(parsed) = parse_hex_addr(&config.custom_addr_input) {
                        config.base_addr = parsed;
                        terminal.vram_base_addr = parsed;
                    }
                }

                if ui.button("Apply Address").clicked() {
                    if let Some(parsed) = parse_hex_addr(&config.custom_addr_input) {
                        config.set_custom_addr(parsed);
                        terminal.vram_base_addr = parsed;
                        terminal.sync_to_vram(bus);
                    }
                }
            });

            ui.label(
                RichText::new(format!(
                    "Active VRAM Base: {:#010X} | Size: {} bytes ({} cells)",
                    terminal.vram_base_addr, VRAM_TOTAL_SIZE, TERMINAL_COLS * TERMINAL_ROWS
                ))
                .monospace()
                .color(Color32::from_rgb(100, 200, 255)),
            );

            ui.label(
                RichText::new(format!(
                    "Memory Range: {:#010X} ..= {:#010X}",
                    terminal.vram_base_addr,
                    terminal.vram_base_addr + (VRAM_TOTAL_SIZE as u64) - 1
                ))
                .monospace()
                .weak(),
            );

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            // Bus Sync Checkbox
            ui.label(RichText::new("Synchronization:").strong());
            if ui
                .checkbox(
                    &mut terminal.vram_sync_enabled,
                    "Enable Memory Bus VRAM Synchronization",
                )
                .changed()
            {
                config.sync_enabled = terminal.vram_sync_enabled;
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            // Buffer Actions
            ui.label(RichText::new("Buffer Operations:").strong());
            ui.horizontal_wrapped(|ui| {
                if ui.button("Sync Screen -> Memory").clicked() {
                    terminal.sync_to_vram(bus);
                }
                if ui.button("Sync Memory -> Screen").clicked() {
                    terminal.sync_from_vram(bus);
                }
                if ui.button("Clear Video Buffer").clicked() {
                    terminal.clear(terminal.default_attr);
                    terminal.sync_to_vram(bus);
                }
                if ui.button("Reload Banner").clicked() {
                    terminal.load_startup_banner();
                    terminal.sync_to_vram(bus);
                }
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                if ui.button("Close").clicked() {
                    close_clicked = true;
                }
            });
        });

    if !is_open || close_clicked {
        *open = false;
    }
}
