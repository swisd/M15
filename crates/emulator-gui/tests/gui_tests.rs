use emulator_core::arch::Architecture;
use emulator_core::bus::{DynamicMemory, MemoryBus};
use emulator_core::cpu::CpuEngine;
use emulator_gui::{
    EmulatorApp, ExecutionState, SerialConsole, SerialLineEnding, SerialParity, TerminalCell,
    TerminalScreen, DEFAULT_VRAM_BASE_ADDR, DEMOS, TERMINAL_COLS, TERMINAL_ROWS,
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
