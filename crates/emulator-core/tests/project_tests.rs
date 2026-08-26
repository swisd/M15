//! Integration tests for project configuration, architecture autodetection,
//! multi-architecture assembly loading, and execution.

use emulator_core::arch::{AnyCpu, Architecture};
use emulator_core::bus::DynamicMemory;
use emulator_core::cpu::{CpuEngine, StepOutcome};
use emulator_core::project::{
    DetectionResult, ProjectConfig, assemble_source, detect_architecture,
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
