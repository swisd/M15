//! Integration tests for project configuration, architecture autodetection,
//! multi-architecture assembly loading, and execution.

use emulator_core::arch::{AnyCpu, Architecture};
use emulator_core::bus::{DynamicMemory, MemoryBus};
use emulator_core::cpu::{CpuEngine, StepOutcome};
use emulator_core::project::{
    DetectionResult, LoadedProject, ProjectConfig, assemble_source, detect_architecture,
};

#[test]
fn test_mconfig_toml_parsing() {
    let toml = r#"
        [project]
        name = "RISC-V Math Demo"
        arch = "risc-v"
        description = "Performs arithmetic and stack operations"
        entry_point = "0x1000"
        sp = "0x20000"
        main = "main.asm"
        files = ["main.asm", "helper.asm"]
    "#;

    let config = ProjectConfig::parse(toml).expect("Parsing valid mconfig.toml should succeed");
    assert_eq!(config.name, "RISC-V Math Demo");
    assert_eq!(config.arch, Some(Architecture::RiscV));
    assert_eq!(config.entry_point, Some(0x1000));
    assert_eq!(config.sp, Some(0x20000));
    assert_eq!(config.main.as_deref(), Some("main.asm"));
    assert_eq!(config.files, vec!["main.asm", "helper.asm"]);
}

#[test]
fn test_mconfig_toml_to_string_roundtrip() {
    let config = ProjectConfig {
        name: "8086 Boot Loader".to_string(),
        arch: Some(Architecture::I8086),
        entry_point: Some(0x7C00),
        sp: Some(0xFFF0),
        main: Some("boot.asm".to_string()),
        ..Default::default()
    };

    let toml_str = config.to_toml_string();
    let reparsed = ProjectConfig::parse(&toml_str).expect("Reparsing generated TOML should succeed");

    assert_eq!(reparsed.name, "8086 Boot Loader");
    assert_eq!(reparsed.arch, Some(Architecture::I8086));
    assert_eq!(reparsed.entry_point, Some(0x7C00));
    assert_eq!(reparsed.sp, Some(0xFFF0));
    assert_eq!(reparsed.main.as_deref(), Some("boot.asm"));
}

#[test]
fn test_autodetect_architecture_directives() {
    let src1 = "; arch: 8086\nmov ax, 0x1234\n";
    assert_eq!(
        detect_architecture(Some("code.asm"), src1),
        DetectionResult::Detected(Architecture::I8086)
    );

    let src2 = ".arch riscv\naddi x1, x0, 10\n";
    assert_eq!(
        detect_architecture(Some("program.s"), src2),
        DetectionResult::Detected(Architecture::RiscV)
    );

    let src3 = "processor 6502\nlda #$10\nsta $20\n";
    assert_eq!(
        detect_architecture(Some("main.asm"), src3),
        DetectionResult::Detected(Architecture::Mos6502)
    );

    let src4 = ".device atmega328p\nldi r16, 0x42\n";
    assert_eq!(
        detect_architecture(Some("firmware.asm"), src4),
        DetectionResult::Detected(Architecture::Avr)
    );
}

#[test]
fn test_autodetect_architecture_tokens() {
    // 8086
    let i8086_src = "mov ax, 0x1000\npush ax\npop bx\nhlt\n";
    assert_eq!(
        detect_architecture(None, i8086_src),
        DetectionResult::Detected(Architecture::I8086)
    );

    // RISC-V
    let riscv_src = "addi x1, zero, 42\nadd x2, x1, x1\njal x1, done\nebreak\n";
    assert_eq!(
        detect_architecture(None, riscv_src),
        DetectionResult::Detected(Architecture::RiscV)
    );

    // 6502
    let mos6502_src = "lda #$20\ntax\npha\npla\nrts\n";
    assert_eq!(
        detect_architecture(None, mos6502_src),
        DetectionResult::Detected(Architecture::Mos6502)
    );

    // AVR
    let avr_src = "ldi r16, 0x55\nmov r17, r16\npush r16\npop r17\nret\n";
    assert_eq!(
        detect_architecture(None, avr_src),
        DetectionResult::Detected(Architecture::Avr)
    );
}

#[test]
fn test_assemble_and_run_8086_program() {
    let asm = r#"
        .org 0x1000
        mov ax, 0x0042
        mov bx, 0x0010
        add ax, bx
        push ax
        pop cx
        hlt
    "#;

    let program = assemble_source(Architecture::I8086, asm, Some(0x1000), Some(0xFFF8))
        .expect("Assembling 8086 program should succeed");

    assert_eq!(program.arch, Architecture::I8086);
    assert_eq!(program.entry_point, 0x1000);
    assert_eq!(program.initial_sp, 0xFFF8);

    let mut cpu = AnyCpu::new(Architecture::I8086);
    let mut bus = DynamicMemory::new(64 * 1024);
    program.load_into(&mut cpu, &mut bus);

    assert_eq!(cpu.pc(), 0x1000);
    assert_eq!(cpu.sp(), 0xFFF8);

    // Step through the assembled instructions
    for _ in 0..10 {
        if let Ok(outcome) = cpu.step(&mut bus)
            && outcome == StepOutcome::Halted
        {
            break;
        }
    }

    let ax = cpu.get_register("AX").expect("AX should exist");
    let cx = cpu.get_register("CX").expect("CX should exist");
    assert_eq!(ax, 0x0052);
    assert_eq!(cx, 0x0052);
}

#[test]
fn test_assemble_and_run_riscv_program() {
    let asm = r#"
        .org 0x1000
        addi x1, x0, 15
        addi x2, x0, 25
        add x3, x1, x2
        ebreak
    "#;

    let program = assemble_source(Architecture::RiscV, asm, Some(0x1000), Some(0x10000))
        .expect("Assembling RISC-V program should succeed");

    let mut cpu = AnyCpu::new(Architecture::RiscV);
    let mut bus = DynamicMemory::new(64 * 1024);
    program.load_into(&mut cpu, &mut bus);

    for _ in 0..10 {
        if let Ok(outcome) = cpu.step(&mut bus)
            && outcome == StepOutcome::Breakpoint
        {
            break;
        }
    }

    let x3 = cpu.get_register("x3").expect("x3 should exist");
    assert_eq!(x3, 40);
}

#[test]
fn test_assemble_and_run_6502_program() {
    let asm = r#"
        .org 0x0600
        lda #$12
        tax
        pha
        pla
    "#;

    let program = assemble_source(Architecture::Mos6502, asm, Some(0x0600), Some(0x01FF))
        .expect("Assembling 6502 program should succeed");

    let mut cpu = AnyCpu::new(Architecture::Mos6502);
    let mut bus = DynamicMemory::new(64 * 1024);
    program.load_into(&mut cpu, &mut bus);

    for i in 0..4 {
        let outcome = cpu.step(&mut bus).unwrap_or_else(|e| panic!("Step {} failed: {:?}", i, e));
        eprintln!("Step {}: outcome = {:?}, A = {:#X}, X = {:#X}, PC = {:#X}", i, outcome, cpu.get_register("A").unwrap(), cpu.get_register("X").unwrap(), cpu.pc());
    }

    let a = cpu.get_register("A").expect("A should exist");
    let x = cpu.get_register("X").expect("X should exist");
    assert_eq!(a, 0x12);
    assert_eq!(x, 0x12);
}

#[test]
fn test_assemble_and_run_avr_program() {
    let asm = r#"
        .org 0x0000
        ldi r16, 0x30
        ldi r17, 0x15
        add r16, r17
    "#;

    let program = assemble_source(Architecture::Avr, asm, Some(0x0000), Some(0x08FF))
        .expect("Assembling AVR program should succeed");

    let mut cpu = AnyCpu::new(Architecture::Avr);
    let mut bus = DynamicMemory::new(64 * 1024);
    program.load_into(&mut cpu, &mut bus);

    for _ in 0..3 {
        let _ = cpu.step(&mut bus);
    }

    let r16 = cpu.get_register("r16").expect("r16 should exist");
    assert_eq!(r16, 0x45);
}

#[test]
fn test_8086_model_stack_and_variables_twolargest() {
    let asm = r#"
.model small
.stack 100h
.data
    num1 dw 10
    num2 dw 20
    largest dw ?
.code
main proc
    mov ax, @data        ; Initialize data segment
    mov ds, ax
    mov ax, num1         ; Load num1 into AX
    mov bx, num2         ; Load num2 into BX
    cmp ax, bx           ; Compare AX and BX
    jg ax_is_larger      ; Jump if AX > BX
    mov ax, bx           ; Otherwise, BX is larger
ax_is_larger:
    mov largest, ax      ; Store the largest number
    mov ax, 4c00h        ; Exit program
    int 21h
main endp
end main
    "#;

    let program = assemble_source(Architecture::I8086, asm, None, None)
        .expect("Assembling 8086 twolargest with .model, .stack, proc should succeed");

    assert_eq!(program.arch, Architecture::I8086);
    let mut cpu = AnyCpu::new(Architecture::I8086);
    let mut bus = DynamicMemory::new(64 * 1024);
    program.load_into(&mut cpu, &mut bus);

    let mut halted = false;
    for _ in 0..20 {
        match cpu.step(&mut bus) {
            Ok(StepOutcome::Interrupt(0x21)) | Ok(StepOutcome::Halted) => {
                halted = true;
                break;
            }
            Ok(_) => {}
            Err(e) => panic!("CPU step error: {:?}", e),
        }
    }

    assert!(halted, "Program should reach INT 21h or halt");
    let ax = cpu.get_register("AX").expect("AX should exist");
    assert_eq!(ax, 0x4C00);

    // Verify largest variable stored in memory at label 'largest'
    let largest_addr = program.labels.get("largest").copied().expect("largest label should exist");
    let val_lo = bus.read_u8(largest_addr).unwrap();
    let val_hi = bus.read_u8(largest_addr + 1).unwrap();
    let largest_val = (val_hi as u16) << 8 | (val_lo as u16);
    assert_eq!(largest_val, 20);
}

#[test]
fn test_8086_addtwo_with_model_and_stack() {
    let asm = r#"
.model small
.stack 100h
.data
    num1 dw 5       ; First number
    num2 dw 3       ; Second number
    result dw ?     ; Result variable
.code
main proc
    mov ax, @data        ; Initialize data segment
    mov ds, ax
    mov ax, num1         ; Load num1 into AX
    add ax, num2         ; Add num2 to AX
    mov result, ax       ; Store the result
    mov ax, 4c00h        ; Exit program
    int 21h
main endp
end main
    "#;

    let program = assemble_source(Architecture::I8086, asm, None, None)
        .expect("Assembling 8086 addtwo should succeed");

    let mut cpu = AnyCpu::new(Architecture::I8086);
    let mut bus = DynamicMemory::new(64 * 1024);
    program.load_into(&mut cpu, &mut bus);

    for _ in 0..20 {
        if let Ok(StepOutcome::Interrupt(0x21)) = cpu.step(&mut bus) {
            break;
        }
    }

    let result_addr = program.labels.get("result").copied().expect("result label should exist");
    let val_lo = bus.read_u8(result_addr).unwrap();
    let val_hi = bus.read_u8(result_addr + 1).unwrap();
    let result_val = (val_hi as u16) << 8 | (val_lo as u16);
    assert_eq!(result_val, 8);
}

#[test]
fn test_avr_equ_expressions_and_include_program() {
    let asm = r#"
.include "m328pdef.inc"

.equ F_CPU = 16000000
.equ BAUD = 9600
.equ UBRR_VAL = (F_CPU/(16*BAUD))-1

.org 0x0000
    rjmp reset

reset:
    ; Initialize Stack Pointer
    ldi r16, high(RAMEND)
    out SPH, r16
    ldi r16, low(RAMEND)
    out SPL, r16

    ; Set Baud Rate
    ldi r16, high(UBRR_VAL)
    out UBRR0H, r16
    ldi r16, low(UBRR_VAL)
    out UBRR0L, r16

    ; Enable Transmitter
    ldi r16, (1<<TXEN0)
    out UCSR0B, r16

    ; Set Frame Format: 8 data bits, 1 stop bit
    ldi r16, (1<<UCSZ01) | (1<<UCSZ00)
    out UCSR0C, r16

send_string:
    ; Load pointer to Hello World string (Z register)
    ldi ZH, high(msg * 2)
    ldi ZL, low(msg * 2)

transmit_loop:
    lpm r16, Z+          ; Load byte from program memory into r16
    cpi r16, 0           ; Check for null terminator
    breq hang            ; If zero, finish/hang

    ; Set buffer empty bit so loop can progress
    in r17, UCSR0A
    sbr r17, (1<<UDRE0)
    out UCSR0A, r17

    ; Wait for empty transmit buffer
wait_buffer:
    in r17, UCSR0A
    sbrs r17, UDRE0
    rjmp wait_buffer

    ; Send character
    out UDR0, r16
    rjmp transmit_loop

hang:
    rjmp hang

msg:
    .db "Hello World!", 13, 10, 0
    "#;

    let program = assemble_source(Architecture::Avr, asm, None, None)
        .expect("Assembling AVR program with .equ, .include, expressions should succeed");

    assert_eq!(program.arch, Architecture::Avr);
    assert_eq!(program.entry_point, 0x0000);

    let mut cpu = AnyCpu::new(Architecture::Avr);
    let mut bus = DynamicMemory::new(64 * 1024);
    program.load_into(&mut cpu, &mut bus);

    // Initial step: rjmp reset
    let _ = cpu.step(&mut bus);
    // Execute reset and setup instructions
    for _ in 0..15 {
        let _ = cpu.step(&mut bus);
    }

    let r16 = cpu.get_register("r16").unwrap();
    // After loading first char 'H' (ASCII 0x48 / 72)
    assert!(r16 > 0);
}

#[test]
fn test_avr_equ_syntax_variants() {
    let asm = r#"
.equ CONST_A = 10
.EQU CONST_B, 20
CONST_C .equ 30
CONST_D equ 40
CONST_E = CONST_A + CONST_B + CONST_C + CONST_D

.org 0x0000
    ldi r16, CONST_A
    ldi r17, CONST_B
    ldi r18, CONST_C
    ldi r19, CONST_D
    ldi r20, CONST_E
    "#;

    let program = assemble_source(Architecture::Avr, asm, None, None)
        .expect("Assembling AVR program with various .equ forms should succeed");

    assert_eq!(*program.labels.get("CONST_A").unwrap(), 10);
    assert_eq!(*program.labels.get("CONST_B").unwrap(), 20);
    assert_eq!(*program.labels.get("CONST_C").unwrap(), 30);
    assert_eq!(*program.labels.get("CONST_D").unwrap(), 40);
    assert_eq!(*program.labels.get("CONST_E").unwrap(), 100);

    let mut cpu = AnyCpu::new(Architecture::Avr);
    let mut bus = DynamicMemory::new(64 * 1024);
    program.load_into(&mut cpu, &mut bus);

    for _ in 0..5 {
        let _ = cpu.step(&mut bus);
    }

    assert_eq!(cpu.get_register("r16").unwrap(), 10);
    assert_eq!(cpu.get_register("r17").unwrap(), 20);
    assert_eq!(cpu.get_register("r18").unwrap(), 30);
    assert_eq!(cpu.get_register("r19").unwrap(), 40);
    assert_eq!(cpu.get_register("r20").unwrap(), 100);
}

#[test]
fn test_6502_equate_and_indexed_operations() {
    let asm = r#"
ARRAY_START = $0200

    LDX #$05
    LDA #$00

LOOP:
    CLC
    ADC #$02
    PHA
    DEX
    STA $0200,X
    TXA
    BNE LOOP
    PLA
    BRK
    "#;

    let program = assemble_source(Architecture::Mos6502, asm, Some(0x0600), Some(0x01FF))
        .expect("Assembling 6502 with equate and STA $0200,X should succeed");

    let mut cpu = AnyCpu::new(Architecture::Mos6502);
    let mut bus = DynamicMemory::new(64 * 1024);
    program.load_into(&mut cpu, &mut bus);

    for _ in 0..40 {
        if let Ok(outcome) = cpu.step(&mut bus)
            && outcome == StepOutcome::Halted
        {
            break;
        }
    }

    // Check array values written to $0200..$0204
    assert_eq!(bus.read_u8(0x0204).unwrap(), 2);
    assert_eq!(bus.read_u8(0x0203).unwrap(), 6);
    assert_eq!(bus.read_u8(0x0202).unwrap(), 5);
    assert_eq!(bus.read_u8(0x0201).unwrap(), 4);
    assert_eq!(bus.read_u8(0x0200).unwrap(), 3);
    let a = cpu.get_register("A").unwrap();
    assert_eq!(a, 3);
}

#[test]
fn test_load_actual_disk_projects() {
    let p_avr = std::path::Path::new("test-projects/avr/m328p");
    if p_avr.exists() {
        let loaded = LoadedProject::load_from_dir(p_avr).expect("Loading AVR project folder should succeed");
        assert_eq!(loaded.arch, Architecture::Avr);
    }

    let p_i8086_twolargest = std::path::Path::new("test-projects/i8086/twolargest.asm");
    if p_i8086_twolargest.exists() {
        let loaded = LoadedProject::load_from_file(p_i8086_twolargest).expect("Loading 8086 twolargest.asm should succeed");
        assert_eq!(loaded.arch, Architecture::I8086);
    }

    let p_i8086_addtwo = std::path::Path::new("test-projects/i8086/addtwo.asm");
    if p_i8086_addtwo.exists() {
        let loaded = LoadedProject::load_from_file(p_i8086_addtwo).expect("Loading 8086 addtwo.asm should succeed");
        assert_eq!(loaded.arch, Architecture::I8086);
    }

    let p_6502 = std::path::Path::new("test-projects/6502/test.asm");
    if p_6502.exists() {
        let loaded = LoadedProject::load_from_file(p_6502).expect("Loading 6502 test.asm should succeed");
        assert_eq!(loaded.arch, Architecture::Mos6502);
    }
}

#[test]
fn test_data_rodata_bss_segment_parsing_and_memory_loading() {
    let asm = r#"
.section .rodata
    ro_const_byte db 0xAA
    ro_const_word dw 0x1234
    ro_str db 'READONLY', 0

.section .data
    var_b db 42
    var_w dw 1000
    var_d dd 0x12345678
    var_q dq 0x1122334455667788
    msg_str db 'Hello\n\0'

.section .bss
    uninit_buf resb 16
    uninit_word resw 2

.section .text
main:
    mov ax, var_w
    mov bx, var_b
    hlt
    "#;

    let program = assemble_source(Architecture::I8086, asm, None, None)
        .expect("Assembling multi-section program with .data, .rodata, .bss should succeed");

    assert_eq!(program.arch, Architecture::I8086);
    assert_eq!(program.entry_point, 0x1000);

    let mut cpu = AnyCpu::new(Architecture::I8086);
    let mut bus = DynamicMemory::new(64 * 1024);
    program.load_into(&mut cpu, &mut bus);

    // Verify .rodata values in memory at default rodata base (0x2800)
    let ro_byte_addr = *program.labels.get("ro_const_byte").expect("ro_const_byte label");
    assert_eq!(ro_byte_addr, 0x2800);
    assert_eq!(bus.read_u8(ro_byte_addr).unwrap(), 0xAA);

    let ro_word_addr = *program.labels.get("ro_const_word").expect("ro_const_word label");
    assert_eq!(ro_word_addr, 0x2801);
    assert_eq!(bus.read_u16(ro_word_addr, emulator_core::types::Endianness::LittleEndian).unwrap(), 0x1234);

    let ro_str_addr = *program.labels.get("ro_str").expect("ro_str label");
    let mut str_buf = [0u8; 9];
    bus.read_bytes(ro_str_addr, &mut str_buf).unwrap();
    assert_eq!(&str_buf, b"READONLY\0");

    // Verify .data values in memory at default data base (0x2000)
    let var_b_addr = *program.labels.get("var_b").expect("var_b label");
    assert_eq!(var_b_addr, 0x2000);
    assert_eq!(bus.read_u8(var_b_addr).unwrap(), 42);

    let var_w_addr = *program.labels.get("var_w").expect("var_w label");
    assert_eq!(var_w_addr, 0x2001);
    assert_eq!(bus.read_u16(var_w_addr, emulator_core::types::Endianness::LittleEndian).unwrap(), 1000);

    let var_d_addr = *program.labels.get("var_d").expect("var_d label");
    assert_eq!(bus.read_u32(var_d_addr, emulator_core::types::Endianness::LittleEndian).unwrap(), 0x12345678);

    let var_q_addr = *program.labels.get("var_q").expect("var_q label");
    assert_eq!(bus.read_u64(var_q_addr, emulator_core::types::Endianness::LittleEndian).unwrap(), 0x1122334455667788);

    let msg_str_addr = *program.labels.get("msg_str").expect("msg_str label");
    let mut msg_buf = [0u8; 7];
    bus.read_bytes(msg_str_addr, &mut msg_buf).unwrap();
    assert_eq!(&msg_buf, b"Hello\n\0");

    // Verify .bss allocation in memory at default bss base (0x3000)
    let bss_buf_addr = *program.labels.get("uninit_buf").expect("uninit_buf label");
    assert_eq!(bss_buf_addr, 0x3000);
    for i in 0..16 {
        assert_eq!(bus.read_u8(bss_buf_addr + i).unwrap(), 0);
    }
    let bss_word_addr = *program.labels.get("uninit_word").expect("uninit_word label");
    assert_eq!(bss_word_addr, 0x3010);
    assert_eq!(bus.read_u32(bss_word_addr, emulator_core::types::Endianness::LittleEndian).unwrap(), 0);
}

#[test]
fn test_masm_dup_syntax_and_space_skip_directives() {
    let asm = r#"
.data
    table1 db 5 dup(0xFF)
    table2 dw 3 dup(0x1234)
    table3 dd 2 dup(?)
    skip_buf .space 8, 0x55
    fill_buf .fill 4, 2, 0xAA
.code
_start:
    nop
    "#;

    let program = assemble_source(Architecture::X86, asm, None, None)
        .expect("Assembling program with DUP and space/fill directives should succeed");

    let mut cpu = AnyCpu::new(Architecture::X86);
    let mut bus = DynamicMemory::new(1024 * 1024);
    program.load_into(&mut cpu, &mut bus);

    let t1_addr = *program.labels.get("table1").unwrap();
    for i in 0..5 {
        assert_eq!(bus.read_u8(t1_addr + i).unwrap(), 0xFF);
    }

    let t2_addr = *program.labels.get("table2").unwrap();
    assert_eq!(t2_addr, t1_addr + 5);
    for i in 0..3 {
        assert_eq!(bus.read_u16(t2_addr + (i * 2), emulator_core::types::Endianness::LittleEndian).unwrap(), 0x1234);
    }

    let t3_addr = *program.labels.get("table3").unwrap();
    assert_eq!(t3_addr, t2_addr + 6);
    assert_eq!(bus.read_u64(t3_addr, emulator_core::types::Endianness::LittleEndian).unwrap(), 0);

    let skip_addr = *program.labels.get("skip_buf").unwrap();
    for i in 0..8 {
        assert_eq!(bus.read_u8(skip_addr + i).unwrap(), 0x55);
    }

    let fill_addr = *program.labels.get("fill_buf").unwrap();
    for i in 0..8 {
        assert_eq!(bus.read_u8(fill_addr + i).unwrap(), 0xAA);
    }
}

#[test]
fn test_floating_point_data_directives() {
    let asm = r#"
.data
    val_f32 .float 3.1415927
    val_f64 .double 2.718281828459045
    dd_float dd 1.5
    dq_double dq 10.25
.text
    nop
    "#;

    let program = assemble_source(Architecture::X86_64, asm, None, None)
        .expect("Assembling floating point data directives should succeed");

    let mut cpu = AnyCpu::new(Architecture::X86_64);
    let mut bus = DynamicMemory::new(1024 * 1024);
    program.load_into(&mut cpu, &mut bus);

    let f32_addr = *program.labels.get("val_f32").unwrap();
    let f32_bits = bus.read_u32(f32_addr, emulator_core::types::Endianness::LittleEndian).unwrap();
    let f32_val = f32::from_bits(f32_bits);
    assert!((f32_val - 3.1415927).abs() < 1e-6);

    let f64_addr = *program.labels.get("val_f64").unwrap();
    let f64_bits = bus.read_u64(f64_addr, emulator_core::types::Endianness::LittleEndian).unwrap();
    let f64_val = f64::from_bits(f64_bits);
    assert!((f64_val - 2.718281828459045).abs() < 1e-12);

    let dd_addr = *program.labels.get("dd_float").unwrap();
    let dd_bits = bus.read_u32(dd_addr, emulator_core::types::Endianness::LittleEndian).unwrap();
    assert_eq!(f32::from_bits(dd_bits), 1.5f32);

    let dq_addr = *program.labels.get("dq_double").unwrap();
    let dq_bits = bus.read_u64(dq_addr, emulator_core::types::Endianness::LittleEndian).unwrap();
    assert_eq!(f64::from_bits(dq_bits), 10.25f64);
}

#[test]
fn test_avr_dseg_cseg_sram_data_loading() {
    let asm = r#"
.dseg
.org 0x0100
    counter: .byte 1
    buffer:  .byte 4

.cseg
.org 0x0000
    ldi r16, 0x42
    sts counter, r16
    lds r17, counter
    nop
    "#;

    let program = assemble_source(Architecture::Avr, asm, None, None)
        .expect("Assembling AVR .dseg and .cseg should succeed");

    let counter_addr = *program.labels.get("counter").unwrap();
    assert_eq!(counter_addr, 0x0100);

    let buffer_addr = *program.labels.get("buffer").unwrap();
    assert_eq!(buffer_addr, 0x0101);

    let mut cpu = AnyCpu::new(Architecture::Avr);
    let mut bus = DynamicMemory::new(64 * 1024);
    program.load_into(&mut cpu, &mut bus);

    // Initial bus state: SRAM counter is 1 from .byte 1
    assert_eq!(bus.read_u8(0x0100).unwrap(), 1);

    // Step 1: ldi r16, 0x42
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.get_register("r16").unwrap(), 0x42);

    // Step 2: sts counter, r16 (writes 0x42 to SRAM 0x0100)
    cpu.step(&mut bus).unwrap();
    assert_eq!(bus.read_u8(0x0100).unwrap(), 0x42);

    // Step 3: lds r17, counter (reads 0x42 from SRAM 0x0100 into r17)
    cpu.step(&mut bus).unwrap();
    assert_eq!(cpu.get_register("r17").unwrap(), 0x42);
}
