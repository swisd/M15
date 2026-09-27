use emulator_core::arch::Architecture;
use emulator_core::bus::{DynamicMemory, MemoryBus};
use emulator_core::cpu::CpuEngine;
use emulator_gui::{
    parse_hex_addr, EmulatorApp, ExecutionState, InterruptMode, SerialConsole, SerialLineEnding,
    SerialParity, TerminalCell, TerminalScreen, VgaBufferConfig, VgaBufferPreset,
    DEFAULT_VRAM_BASE_ADDR, DEMOS, TERMINAL_COLS, TERMINAL_ROWS,
};

#[test]
fn test_all_16_demos_load() {
    assert_eq!(DEMOS.len(), 16);
    for &arch in Architecture::ALL {
        let app = EmulatorApp::new(arch);
        assert_eq!(app.selected_arch, arch);
        assert_eq!(app.execution_state, ExecutionState::Stopped);
        assert!(app.cpu.sp() > 0);
        assert!(!app.show_terminal);
        assert!(!app.show_serial_console);
    }
}

#[test]
fn test_app_switching_and_stepping() {
    let mut app = EmulatorApp::new(Architecture::I8086);
    assert_eq!(app.selected_arch, Architecture::I8086);

    // Step once
    app.step();
    assert!(app.step_count > 0);

    // Switch to RISC-V
    app.switch_arch(Architecture::RiscV);
    assert_eq!(app.selected_arch, Architecture::RiscV);
    assert_eq!(app.step_count, 0);

    // Step RISC-V
    app.step();
    assert_eq!(app.step_count, 1);

    // Reset CPU
    app.reset_cpu();
    assert_eq!(app.step_count, 0);
}

#[test]
fn test_terminal_screen_grid_and_operations() {
    let mut term = TerminalScreen::new();
    assert_eq!(TERMINAL_COLS, 80);
    assert_eq!(TERMINAL_ROWS, 25);

    // Clear with light green on black
    term.clear(0x0A);
    assert_eq!(term.cursor_col, 0);
    assert_eq!(term.cursor_row, 0);
    assert_eq!(term.get_cell(0, 0), Some(&TerminalCell::new(b' ', 0x0A)));

    // Set individual cell
    term.set_cell(10, 5, b'X', 0x1F); // Bright white on blue
    let cell = term.get_cell(10, 5).unwrap();
    assert_eq!(cell.char_code, b'X');
    assert_eq!(cell.attribute, 0x1F);
    assert_eq!(cell.fg_color_index(), 15);
    assert_eq!(cell.bg_color_index(), 1);
    assert_eq!(cell.as_char(), 'X');

    // Teletype stream write
    term.clear(0x07);
    term.write_str("Line 1\nLine 2\rLine 2-Rewritten", 0x07);
    assert_eq!(term.cursor_row, 1);

    // Test scrolling
    term.clear(0x07);
    for i in 0..30 {
        term.write_str(&format!("Row {}\n", i), 0x07);
    }
    assert_eq!(term.cursor_row, 24);

    // Test keyboard queue
    term.push_key(b'A');
    term.push_key(b'B');
    assert_eq!(term.keyboard_queue.pop_front(), Some(b'A'));
    assert_eq!(term.keyboard_queue.pop_front(), Some(b'B'));
}

#[test]
fn test_terminal_vram_bus_synchronization() {
    let mut term = TerminalScreen::new();
    let mut bus = DynamicMemory::new(1024 * 1024);

    term.clear(0x07);
    term.set_cell(0, 0, b'H', 0x4F); // White on Red
    term.set_cell(1, 0, b'i', 0x2E); // Yellow on Green

    // Sync to memory at 0xB8000
    term.sync_to_vram(&mut bus);
    assert_eq!(bus.read_u8(DEFAULT_VRAM_BASE_ADDR).unwrap(), b'H');
    assert_eq!(bus.read_u8(DEFAULT_VRAM_BASE_ADDR + 1).unwrap(), 0x4F);
    assert_eq!(bus.read_u8(DEFAULT_VRAM_BASE_ADDR + 2).unwrap(), b'i');
    assert_eq!(bus.read_u8(DEFAULT_VRAM_BASE_ADDR + 3).unwrap(), 0x2E);

    // External write into VRAM
    bus.write_u8(DEFAULT_VRAM_BASE_ADDR + 4, b'!').unwrap();
    bus.write_u8(DEFAULT_VRAM_BASE_ADDR + 5, 0x0E).unwrap(); // Yellow on Black

    // Sync from memory
    term.sync_from_vram(&bus);
    let cell = term.get_cell(2, 0).unwrap();
    assert_eq!(cell.char_code, b'!');
    assert_eq!(cell.attribute, 0x0E);
}

#[test]
fn test_serial_console_bidirectional_and_mmio() {
    let mut serial = SerialConsole::new();
    let mut bus = DynamicMemory::new(1024 * 1024);

    assert_eq!(serial.baud_rate, 115200);
    assert_eq!(serial.data_bits, 8);
    assert_eq!(serial.parity, SerialParity::None);
    assert_eq!(serial.line_ending, SerialLineEnding::CRLF);

    // TX from CPU
    serial.write_tx_str("Hello UART!\n");
    assert!(serial.log_entries.iter().any(|e| e.text.contains("Hello UART!")));

    // RX from user input
    serial.push_rx_bytes(b"PING\r\n");
    assert!(serial.has_rx_data());
    assert_eq!(serial.read_rx_byte(), Some(b'P'));
    assert_eq!(serial.read_rx_byte(), Some(b'I'));
    assert_eq!(serial.read_rx_byte(), Some(b'N'));
    assert_eq!(serial.read_rx_byte(), Some(b'G'));
    assert_eq!(serial.read_rx_byte(), Some(b'\r'));
    assert_eq!(serial.read_rx_byte(), Some(b'\n'));
    assert!(!serial.has_rx_data());

    // MMIO bus sync: CPU writes 'Z' to UART THR (0x10000000)
    bus.write_u8(serial.mmio_base_addr, b'Z').unwrap();
    serial.sync_with_bus(&mut bus);
    assert_eq!(bus.read_u8(serial.mmio_base_addr).unwrap(), 0); // THR cleared

    // Line Status Register (LSR at +5)
    let lsr = bus.read_u8(serial.mmio_base_addr + 5).unwrap();
    assert_eq!(lsr & 0x20, 0x20); // THRE is set

    // Enqueue RX data and check LSR DR flag
    serial.push_rx_bytes(b"K");
    serial.sync_with_bus(&mut bus);
    let lsr_with_dr = bus.read_u8(serial.mmio_base_addr + 5).unwrap();
    assert_eq!(lsr_with_dr & 0x01, 0x01); // DR is set
    assert_eq!(bus.read_u8(serial.mmio_base_addr + 1).unwrap(), b'K');
}

#[test]
fn test_window_toggle_controls_in_app() {
    let mut app = EmulatorApp::new(Architecture::RiscV);
    assert!(!app.show_terminal);
    assert!(!app.show_serial_console);

    // Toggle on
    app.show_terminal = true;
    app.show_serial_console = true;
    assert!(app.show_terminal);
    assert!(app.show_serial_console);

    // Toggle off independently
    app.show_terminal = false;
    assert!(!app.show_terminal);
    assert!(app.show_serial_console);
}

#[test]
fn test_layout_splits_and_expansion() {
    let mut app = EmulatorApp::new(Architecture::X86_64);
    assert!((app.horizontal_split - 0.48).abs() < 0.01);
    assert!((app.left_vertical_split - 0.50).abs() < 0.01);
    assert!((app.right_vertical_split - 0.50).abs() < 0.01);

    // Test custom split configuration
    app.horizontal_split = 0.60;
    app.left_vertical_split = 0.40;
    app.right_vertical_split = 0.60;

    assert!((app.horizontal_split - 0.60).abs() < 0.01);
    assert!((app.left_vertical_split - 0.40).abs() < 0.01);
    assert!((app.right_vertical_split - 0.60).abs() < 0.01);
}

#[test]
fn test_app_load_project_folder_with_mconfig() {
    let temp_dir = std::env::temp_dir().join("m15_test_proj_8086");
    let _ = std::fs::create_dir_all(&temp_dir);

    let mconfig_content = r#"
        [project]
        name = "Sample 8086 Project"
        arch = "8086"
        entry_point = "0x1000"
        sp = "0xFFF8"
        main = "main.asm"
    "#;
    std::fs::write(temp_dir.join("mconfig.toml"), mconfig_content).unwrap();

    let asm_content = r#"
        .org 0x1000
        mov ax, 0x0042
        mov bx, 0x0008
        add ax, bx
        hlt
    "#;
    std::fs::write(temp_dir.join("main.asm"), asm_content).unwrap();

    let mut app = EmulatorApp::new(Architecture::RiscV); // Start with RISC-V
    assert_eq!(app.selected_arch, Architecture::RiscV);

    // Load the folder
    app.load_project_from_dir(temp_dir.to_str().unwrap());

    assert_eq!(app.selected_arch, Architecture::I8086);
    assert_eq!(app.cpu.pc(), 0x1000);
    assert_eq!(app.cpu.sp(), 0xFFF8);
    assert_eq!(app.current_project_name.as_deref(), Some("Sample 8086 Project"));

    // Step through the instructions
    for _ in 0..5 {
        app.step();
        if app.execution_state == ExecutionState::Halted {
            break;
        }
    }

    assert_eq!(app.cpu.get_register("AX"), Some(0x004A));

    // Cleanup
    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_app_load_asm_file_autodetect() {
    let temp_file = std::env::temp_dir().join("riscv_math_test.s");
    let asm_content = r#"
        .arch riscv
        .org 0x1000
        addi x1, x0, 10
        addi x2, x0, 32
        add x3, x1, x2
        ebreak
    "#;
    std::fs::write(&temp_file, asm_content).unwrap();

    let mut app = EmulatorApp::new(Architecture::I8086);
    assert_eq!(app.selected_arch, Architecture::I8086);

    app.load_asm_file(temp_file.to_str().unwrap(), None);

    assert_eq!(app.selected_arch, Architecture::RiscV);
    assert_eq!(app.cpu.pc(), 0x1000);

    for _ in 0..5 {
        app.step();
        if app.execution_state == ExecutionState::Paused {
            break;
        }
    }

    assert_eq!(app.cpu.get_register("x3"), Some(42));

    let _ = std::fs::remove_file(&temp_file);
}

#[test]
fn test_app_autodetect_fallback_modal_and_manual_select() {
    let ambiguous_code = r#"
        .org 0x1000
        nop
        nop
    "#;

    let mut app = EmulatorApp::new(Architecture::X86_64);
    assert_eq!(app.selected_arch, Architecture::X86_64);

    // Load without arch hints or override
    app.load_asm_source(ambiguous_code, "mystery.asm", None, None);

    // Should prompt user by showing the arch picker modal
    assert!(app.show_arch_picker_modal);
    assert!(app.pending_file_content.is_some());

    // User selects MOS 6502
    app.apply_pending_with_arch(Architecture::Mos6502);

    assert_eq!(app.selected_arch, Architecture::Mos6502);
    assert_eq!(app.cpu.pc(), 0x1000);
}

#[test]
fn test_app_scratchpad_assemble_and_run() {
    let mut app = EmulatorApp::new(Architecture::X86_64);

    let avr_code = r#"
        .device atmega328p
        .org 0x0000
        ldi r16, 0x10
        ldi r17, 0x20
        add r16, r17
    "#;

    app.load_asm_source(avr_code, "Scratchpad Program", None, None);

    assert_eq!(app.selected_arch, Architecture::Avr);
    assert_eq!(app.cpu.pc(), 0x0000);

    for _ in 0..3 {
        app.step();
    }

    assert_eq!(app.cpu.get_register("r16"), Some(0x30));
}

#[test]
fn test_interrupt_mode_metadata_and_switching() {
    assert_eq!(InterruptMode::ALL.len(), 4);
    assert_eq!(InterruptMode::ArchDefault.name(), "Arch Default");
    assert_eq!(InterruptMode::MsDos.name(), "MS-DOS");
    assert_eq!(InterruptMode::Bios.name(), "BIOS");
    assert_eq!(InterruptMode::Uefi.name(), "UEFI");

    assert!(InterruptMode::ArchDefault.display_label().contains("Arch Default"));
    assert!(InterruptMode::MsDos.display_label().contains("MS-DOS"));
    assert!(InterruptMode::Bios.display_label().contains("BIOS"));
    assert!(InterruptMode::Uefi.display_label().contains("UEFI"));

    let mut app = EmulatorApp::new(Architecture::X86_64);
    assert_eq!(app.interrupt_mode, InterruptMode::ArchDefault);

    app.set_interrupt_mode(InterruptMode::MsDos);
    assert_eq!(app.interrupt_mode, InterruptMode::MsDos);
    assert!(app.status_message.as_ref().unwrap().contains("MS-DOS"));

    app.set_interrupt_mode(InterruptMode::Bios);
    assert_eq!(app.interrupt_mode, InterruptMode::Bios);

    app.set_interrupt_mode(InterruptMode::Uefi);
    assert_eq!(app.interrupt_mode, InterruptMode::Uefi);
}

#[test]
fn test_vga_buffer_presets_and_custom_configuration() {
    let mut config = VgaBufferConfig::default();
    assert_eq!(config.preset, VgaBufferPreset::BiosDefault);
    assert_eq!(config.base_addr, 0x000B_8000);

    // Apply UEFI preset
    config.apply_preset(VgaBufferPreset::UefiDefault);
    assert_eq!(config.preset, VgaBufferPreset::UefiDefault);
    assert_eq!(config.base_addr, 0x00A0_0000);

    // Apply Custom address
    config.set_custom_addr(0x0004_0000);
    assert_eq!(config.preset, VgaBufferPreset::Custom);
    assert_eq!(config.base_addr, 0x0004_0000);

    // Test parse_hex_addr
    assert_eq!(parse_hex_addr("0x000B8000"), Some(0xB8000));
    assert_eq!(parse_hex_addr("0xA0000"), Some(0xA0000));
    assert_eq!(parse_hex_addr("B800h"), Some(0xB800));
    assert_eq!(parse_hex_addr("0x10_0000"), Some(0x100000));
    assert_eq!(parse_hex_addr("10000"), Some(0x10000));
    assert_eq!(parse_hex_addr("   "), None);

    // Test app VGA buffer preset switching
    let mut app = EmulatorApp::new(Architecture::I8086);
    assert_eq!(app.terminal.vram_base_addr, 0x000B_8000);

    app.set_vga_buffer_preset(VgaBufferPreset::UefiDefault);
    assert_eq!(app.vga_buffer_config.preset, VgaBufferPreset::UefiDefault);
    assert_eq!(app.terminal.vram_base_addr, 0x00A0_0000);

    app.set_vga_buffer_preset(VgaBufferPreset::BiosDefault);
    assert_eq!(app.vga_buffer_config.preset, VgaBufferPreset::BiosDefault);
    assert_eq!(app.terminal.vram_base_addr, 0x000B_8000);
}

#[test]
fn test_msdos_interrupt_services_execution() {
    let mut app = EmulatorApp::new(Architecture::I8086);
    app.set_interrupt_mode(InterruptMode::MsDos);
    app.terminal.clear(0x07);

    // Setup a small MS-DOS assembly program that prints a string and exits
    let msdos_code = r#"
        .org 0x1000
        mov ax, 0x0900
        mov dx, 0x2000
        int 0x21
        mov ax, 0x4C00
        int 0x21
    "#;
    app.load_asm_source(msdos_code, "dos_test.asm", None, Some(Architecture::I8086));
    app.set_interrupt_mode(InterruptMode::MsDos);
    app.terminal.clear(0x07);

    // Place "$"-terminated string at DS:DX (0x2000)
    let msg = b"Hello DOS Terminal!$\0";
    for (i, &b) in msg.iter().enumerate() {
        app.bus.write_u8(0x2000 + i as u64, b).unwrap();
    }

    // Step through the instructions
    app.step(); // mov ax, 0x0900
    app.step(); // mov dx, 0x2000
    app.step(); // int 0x21 (AH=09h)
    assert!(app.status_message.as_ref().unwrap().contains("MS-DOS INT 21h AH=09h"));

    app.step(); // mov ax, 0x4C00
    app.step(); // int 0x21 (AH=4Ch)

    // Verify program reached exit
    assert_eq!(app.execution_state, ExecutionState::Stopped);
    assert!(app.status_message.as_ref().unwrap().contains("MS-DOS Program Terminated"));

    // Verify terminal received string output
    let cell = app.terminal.get_cell(0, 0).unwrap();
    assert_eq!(cell.char_code, b'H');
}

#[test]
fn test_bios_interrupt_services_execution() {
    let mut app = EmulatorApp::new(Architecture::I8086);
    app.set_interrupt_mode(InterruptMode::Bios);
    app.terminal.clear(0x07);

    // Setup BIOS INT 10h Teletype output: AH=0Eh, AL='Z'
    let bios_code = r#"
        .org 0x1000
        mov ax, 0x0E5A
        mov bx, 0x001F
        int 0x10
        hlt
    "#;
    app.load_asm_source(bios_code, "bios_test.asm", None, Some(Architecture::I8086));
    app.set_interrupt_mode(InterruptMode::Bios);
    app.terminal.clear(0x07);

    app.step(); // mov ax, 0x0E5A
    app.step(); // mov bx, 0x001F
    app.step(); // int 0x10
    app.step(); // hlt

    // Verify character 'Z' was written to terminal and synced to VRAM
    let cell = app.terminal.get_cell(0, 0).unwrap();
    assert_eq!(cell.char_code, b'Z');
    assert_eq!(cell.attribute, 0x1F);
    assert_eq!(app.bus.read_u8(DEFAULT_VRAM_BASE_ADDR).unwrap(), b'Z');
    assert_eq!(app.bus.read_u8(DEFAULT_VRAM_BASE_ADDR + 1).unwrap(), 0x1F);
}

#[test]
fn test_uefi_interrupt_services_execution() {
    let mut app = EmulatorApp::new(Architecture::X86_64);
    app.set_interrupt_mode(InterruptMode::Uefi);
    app.set_vga_buffer_preset(VgaBufferPreset::UefiDefault);

    // Test UEFI interrupt call
    let (msg, state) = emulator_gui::dispatch_interrupt(
        InterruptMode::Uefi,
        0x80,
        &mut app.cpu,
        &mut app.bus,
        &mut app.terminal,
        &mut app.serial_console,
    );

    assert!(msg.is_some());
    assert!(msg.unwrap().contains("UEFI Service Call"));
    assert_eq!(state, None);
}

#[test]
fn test_reset_cpu_preserves_assembled_program() {
    let mut app = EmulatorApp::new(Architecture::I8086);
    let code = r#"
        .org 0x1000
        mov ax, 0x1234
        mov bx, 0x5678
        add ax, bx
        hlt
    "#;
    app.load_asm_source(code, "add_test.asm", None, Some(Architecture::I8086));
    assert!(app.loaded_program.is_some());
    assert_eq!(app.cpu.pc(), 0x1000);

    // Step a few instructions
    app.step(); // mov ax, 0x1234
    app.step(); // mov bx, 0x5678
    assert_eq!(app.cpu.get_register("AX"), Some(0x1234));
    assert_eq!(app.cpu.get_register("BX"), Some(0x5678));
    assert!(app.step_count >= 2);

    // Reset CPU
    app.reset_cpu();

    // Verify PC and registers reset to the loaded program entry state
    assert_eq!(app.cpu.pc(), 0x1000);
    assert_eq!(app.step_count, 0);
    assert_eq!(app.execution_state, ExecutionState::Stopped);
    assert!(app.loaded_program.is_some());
    assert!(app.status_message.as_ref().unwrap().contains("Reset to assembled program"));
}

#[test]
fn test_file_browser_navigation_and_filtering() {
    use std::path::PathBuf;
    let mut browser = emulator_gui::FileBrowserModal::new();
    assert!(!browser.is_open);

    browser.open();
    assert!(browser.is_open);

    let test_dir = PathBuf::from("test-projects");
    if test_dir.is_dir() {
        browser.navigate_to(test_dir.clone());
        assert_eq!(browser.current_dir, test_dir);

        browser.navigate_up();
        assert_ne!(browser.current_dir, test_dir);
    }

    // Supported file filtering
    assert!(browser.is_supported_file(std::path::Path::new("main.asm")));
    assert!(browser.is_supported_file(std::path::Path::new("mconfig.toml")));
    assert!(browser.is_supported_file(std::path::Path::new("header.inc")));
    assert!(!browser.is_supported_file(std::path::Path::new("image.png")));

    browser.show_all_files = true;
    assert!(browser.is_supported_file(std::path::Path::new("image.png")));

    browser.close();
    assert!(!browser.is_open);
}

#[test]
fn test_disassemble_instruction_all_architectures() {
    use emulator_gui::disassemble_instruction;

    // x86 / x86_64 / i8086 NOP and HLT
    let asm = disassemble_instruction(Architecture::X86_64, 0x1000, &[0x90], &[0x90, 0, 0, 0], None);
    assert_eq!(asm, "nop");

    let asm = disassemble_instruction(Architecture::I8086, 0x1000, &[0xCD, 0x21], &[0xCD, 0x21, 0, 0], None);
    assert_eq!(asm, "int 0x21");

    // 6502 NOP & RTS
    let asm = disassemble_instruction(Architecture::Mos6502, 0x8000, &[0xEA], &[0xEA, 0, 0, 0], None);
    assert_eq!(asm, "nop");

    let asm = disassemble_instruction(Architecture::Mos6502, 0x8000, &[0x60], &[0x60, 0, 0, 0], None);
    assert_eq!(asm, "rts");

    // AVR RET
    let asm = disassemble_instruction(Architecture::Avr, 0x0000, &[0x08, 0x95], &[0x08, 0x95, 0, 0], None);
    assert_eq!(asm, "ret");

    // RISC-V NOP (addi x0, x0, 0)
    let asm = disassemble_instruction(Architecture::RiscV, 0x80000000, &[0x13, 0x00, 0x00, 0x00], &[0x13, 0x00, 0x00, 0x00], None);
    assert_eq!(asm, "nop");

    // ARM32 NOP
    let asm = disassemble_instruction(Architecture::Arm32, 0x1000, &[0x00, 0x00, 0xA0, 0xE1], &[0x00, 0x00, 0xA0, 0xE1], None);
    assert_eq!(asm, "nop");

    // ARM64 NOP
    let asm = disassemble_instruction(Architecture::Arm64, 0x1000, &[0x1F, 0x20, 0x03, 0xD5], &[0x1F, 0x20, 0x03, 0xD5], None);
    assert_eq!(asm, "nop");
}

#[test]
fn test_code_panel_state_and_traversal() {
    let mut state = emulator_gui::CodePanelState::default();
    assert!(state.lock_to_pc);
    assert_eq!(state.scroll_offset_instr, 0);

    state.lock_to_pc = false;
    state.scroll_offset_instr = 8;
    assert_eq!(state.scroll_offset_instr, 8);

    state.custom_start_addr = Some(0x2000);
    assert_eq!(state.custom_start_addr, Some(0x2000));
}
