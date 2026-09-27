use emulator_core::{
    AnyCpu, Architecture, ArrayMemory, CpuEngine, Endianness, MemoryBus, StackGrowth,
    StepOutcome, WordSize, create_cpu, inspect_stack,
};

#[cfg(feature = "alloc")]
use emulator_core::inspect_stack_vec;

#[test]
fn test_all_16_supported_arch_names() {
    let supported_raw = "8086, X86, X86_64, arm32, arm64, risc-v, IA-64, MIPS, PowerPC, SPARC, AVR, SuperH, PA-RISC, DEC Alpha, Motorola 68000, MOS 6502";
    let arch_tokens: Vec<&str> = supported_raw.split(',').map(|s| s.trim()).collect();

    assert_eq!(arch_tokens.len(), 16);
    assert_eq!(Architecture::all().len(), 16);

    for token in &arch_tokens {
        let arch = Architecture::from_name(token)
            .unwrap_or_else(|| panic!("Failed to parse architecture name '{}'", token));
        assert_eq!(arch.name(), *token);
    }
}

#[test]
fn test_architecture_metadata() {
    // 8086
    assert_eq!(Architecture::I8086.word_size(), WordSize::Bytes2);
    assert_eq!(Architecture::I8086.endianness(), Endianness::LittleEndian);
    assert_eq!(Architecture::I8086.stack_growth(), StackGrowth::Downwards);

    // x86
    assert_eq!(Architecture::X86.word_size(), WordSize::Bytes4);
    assert_eq!(Architecture::X86.endianness(), Endianness::LittleEndian);
    assert_eq!(Architecture::X86.stack_growth(), StackGrowth::Downwards);

    // x86_64
    assert_eq!(Architecture::X86_64.word_size(), WordSize::Bytes8);
    assert_eq!(Architecture::X86_64.endianness(), Endianness::LittleEndian);
    assert_eq!(Architecture::X86_64.stack_growth(), StackGrowth::Downwards);

    // ARM32
    assert_eq!(Architecture::Arm32.word_size(), WordSize::Bytes4);
    assert_eq!(Architecture::Arm32.endianness(), Endianness::LittleEndian);
    assert_eq!(Architecture::Arm32.stack_growth(), StackGrowth::Downwards);

    // ARM64
    assert_eq!(Architecture::Arm64.word_size(), WordSize::Bytes8);
    assert_eq!(Architecture::Arm64.endianness(), Endianness::LittleEndian);
    assert_eq!(Architecture::Arm64.stack_growth(), StackGrowth::Downwards);

    // RISC-V
    assert_eq!(Architecture::RiscV.word_size(), WordSize::Bytes8);
    assert_eq!(Architecture::RiscV.endianness(), Endianness::LittleEndian);
    assert_eq!(Architecture::RiscV.stack_growth(), StackGrowth::Downwards);

    // IA-64
    assert_eq!(Architecture::Ia64.word_size(), WordSize::Bytes8);
    assert_eq!(Architecture::Ia64.endianness(), Endianness::LittleEndian);
    assert_eq!(Architecture::Ia64.stack_growth(), StackGrowth::Downwards);

    // MIPS
    assert_eq!(Architecture::Mips.word_size(), WordSize::Bytes4);
    assert_eq!(Architecture::Mips.endianness(), Endianness::BigEndian);
    assert_eq!(Architecture::Mips.stack_growth(), StackGrowth::Downwards);

    // PowerPC
    assert_eq!(Architecture::PowerPc.word_size(), WordSize::Bytes4);
    assert_eq!(Architecture::PowerPc.endianness(), Endianness::BigEndian);
    assert_eq!(Architecture::PowerPc.stack_growth(), StackGrowth::Downwards);

    // SPARC
    assert_eq!(Architecture::Sparc.word_size(), WordSize::Bytes4);
    assert_eq!(Architecture::Sparc.endianness(), Endianness::BigEndian);
    assert_eq!(Architecture::Sparc.stack_growth(), StackGrowth::Downwards);

    // AVR
    assert_eq!(Architecture::Avr.word_size(), WordSize::Bytes1);
    assert_eq!(Architecture::Avr.endianness(), Endianness::LittleEndian);
    assert_eq!(Architecture::Avr.stack_growth(), StackGrowth::Downwards);

    // SuperH
    assert_eq!(Architecture::SuperH.word_size(), WordSize::Bytes4);
    assert_eq!(Architecture::SuperH.endianness(), Endianness::BigEndian);
    assert_eq!(Architecture::SuperH.stack_growth(), StackGrowth::Downwards);

    // PA-RISC (UPWARD STACK GROWTH!)
    assert_eq!(Architecture::PaRisc.word_size(), WordSize::Bytes4);
    assert_eq!(Architecture::PaRisc.endianness(), Endianness::BigEndian);
    assert_eq!(Architecture::PaRisc.stack_growth(), StackGrowth::Upwards);

    // DEC Alpha
    assert_eq!(Architecture::DecAlpha.word_size(), WordSize::Bytes8);
    assert_eq!(Architecture::DecAlpha.endianness(), Endianness::LittleEndian);
    assert_eq!(Architecture::DecAlpha.stack_growth(), StackGrowth::Downwards);

    // Motorola 68000
    assert_eq!(Architecture::Motorola68000.word_size(), WordSize::Bytes4);
    assert_eq!(Architecture::Motorola68000.endianness(), Endianness::BigEndian);
    assert_eq!(Architecture::Motorola68000.stack_growth(), StackGrowth::Downwards);

    // MOS 6502
    assert_eq!(Architecture::Mos6502.word_size(), WordSize::Bytes1);
    assert_eq!(Architecture::Mos6502.endianness(), Endianness::LittleEndian);
    assert_eq!(Architecture::Mos6502.stack_growth(), StackGrowth::Downwards);
}

#[test]
fn test_cpu_engine_instantiation_and_registers_all_arch() {
    for &arch in Architecture::all() {
        let mut cpu = create_cpu(arch)
            .unwrap_or_else(|| panic!("create_cpu failed for {:?}", arch));

        assert_eq!(cpu.arch(), arch);
        assert!(cpu.register_count() > 0);

        let mut has_pc = false;
        let mut has_sp = false;

        for i in 0..cpu.register_count() {
            let info = cpu.register_info(i).expect("register info");
            if info.is_pc {
                has_pc = true;
            }
            if info.is_sp {
                has_sp = true;
            }
        }

        assert!(has_pc, "Architecture {:?} must mark at least one register as PC", arch);
        assert!(has_sp, "Architecture {:?} must mark at least one register as SP", arch);

        // Test set and get PC / SP
        let pc_mask = 0xFFFF;
        cpu.set_pc(0x1234);
        assert_eq!(cpu.get_register("PC").map(|v| v & pc_mask), Some(0x1234 & pc_mask));

        let sp_mask = if arch == Architecture::Mos6502 {
            0xFF
        } else {
            0xFFFF
        };
        cpu.set_sp(0x5678);
        assert_eq!(cpu.get_register("SP").map(|v| v & sp_mask), Some(0x5678 & sp_mask));
    }
}

#[test]
fn test_stack_inspector_downwards() {
    let mut cpu = create_cpu(Architecture::X86_64).unwrap();
    let mut mem = ArrayMemory::<65536>::new();

    let sp = 0x1000;
    cpu.set_sp(sp);

    // Write some stack values (downward stack: SP, SP+8, SP+16)
    mem.write_u64(sp, 0x1122334455667788, Endianness::LittleEndian).unwrap();
    mem.write_u64(sp + 8, 0xAABBCCDDEEFF0011, Endianness::LittleEndian).unwrap();

    let mut slots = [emulator_core::StackSlot {
        address: 0,
        raw_bytes: [0; 8],
        size: 0,
        value: 0,
        offset_from_sp: 0,
    }; 2];

    let count = inspect_stack(&cpu, &mem, &mut slots);
    assert_eq!(count, 2);
    assert_eq!(slots[0].address, sp);
    assert_eq!(slots[0].value, 0x1122334455667788);
    assert_eq!(slots[0].offset_from_sp, 0);

    assert_eq!(slots[1].address, sp + 8);
    assert_eq!(slots[1].value, 0xAABBCCDDEEFF0011);
    assert_eq!(slots[1].offset_from_sp, 8);

    #[cfg(feature = "alloc")]
    {
        let vec_slots = inspect_stack_vec(&cpu, &mem, 2);
        assert_eq!(vec_slots.len(), 2);
        assert_eq!(vec_slots[0].value, 0x1122334455667788);
    }
}

#[test]
fn test_riscv_rv32i_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::RiscV).unwrap();

    cpu.set_pc(0x1000);
    cpu.set_sp(0x8000);

    // Instructions:
    // 0x1000: ADDI x10, x0, 10    (0x00A00513) (a0 = 10)
    // 0x1004: ADDI x11, x0, 32    (0x02000593) (a1 = 32)
    // 0x1008: ADD  x10, x10, x11  (0x00B50533) (a0 = 42)
    // 0x100C: SW   x10, -4(x2)    (0xFEA12E23) (mem[0x7FFC] = 42)
    // 0x1010: LW   x12, -4(x2)    (0xFFC12603) (a2 = 42)
    // 0x1014: BEQ  x10, x12, 8    (0x00C50463) (branch to 0x101C)
    // 0x1018: ADDI x10, x0, 0     (0x00000513) (skipped)
    // 0x101C: EBREAK              (0x00100073)
    mem.write_u32(0x1000, 0x00A00513, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1004, 0x02000593, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1008, 0x00B50533, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x100C, 0xFEA12E23, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1010, 0xFFC12603, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1014, 0x00C50463, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1018, 0x00000513, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x101C, 0x00100073, Endianness::LittleEndian).unwrap();

    // Step ADDI a0
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("a0"), Some(10));

    // Step ADDI a1
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("a1"), Some(32));

    // Step ADD a0, a0, a1
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("a0"), Some(42));

    // Step SW
    cpu.step(&mut mem).unwrap();
    let stored_val = mem.read_u32(0x7FFC, Endianness::LittleEndian).unwrap();
    assert_eq!(stored_val, 42);

    // Step LW
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("a2"), Some(42));

    // Step BEQ
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.pc(), 0x101C);

    // Step EBREAK
    let outcome = cpu.step(&mut mem).unwrap();
    assert_eq!(outcome, StepOutcome::Breakpoint);
}

#[test]
fn test_8086_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::I8086).unwrap();

    cpu.set_register("CS", 0x0000).unwrap();
    cpu.set_register("DS", 0x0000).unwrap();
    cpu.set_register("SS", 0x0000).unwrap();
    cpu.set_pc(0x1000);
    cpu.set_sp(0xFFFE);

    // Instructions:
    // 0x1000: B8 34 12       MOV AX, 0x1234
    // 0x1003: 50             PUSH AX
    // 0x1004: BB 0A 00       MOV BX, 0x000A
    // 0x1007: 01 D8          ADD AX, BX (AX = 0x123E)
    // 0x1009: 5B             POP BX     (BX = 0x1234)
    // 0x100A: 3D 3E 12       CMP AX, 0x123E
    // 0x100D: 74 02          JZ +2 (to 0x1011)
    // 0x100F: 90             NOP
    // 0x1011: F4             HLT
    mem.write_u8(0x1000, 0xB8).unwrap();
    mem.write_u8(0x1001, 0x34).unwrap();
    mem.write_u8(0x1002, 0x12).unwrap();

    mem.write_u8(0x1003, 0x50).unwrap();

    mem.write_u8(0x1004, 0xBB).unwrap();
    mem.write_u8(0x1005, 0x0A).unwrap();
    mem.write_u8(0x1006, 0x00).unwrap();

    mem.write_u8(0x1007, 0x01).unwrap();
    mem.write_u8(0x1008, 0xD8).unwrap();

    mem.write_u8(0x1009, 0x5B).unwrap();

    mem.write_u8(0x100A, 0x3D).unwrap();
    mem.write_u8(0x100B, 0x3E).unwrap();
    mem.write_u8(0x100C, 0x12).unwrap();

    mem.write_u8(0x100D, 0x74).unwrap();
    mem.write_u8(0x100E, 0x02).unwrap();

    mem.write_u8(0x100F, 0x90).unwrap();
    mem.write_u8(0x1010, 0x90).unwrap();
    mem.write_u8(0x1011, 0xF4).unwrap();

    // Step MOV AX
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("AX"), Some(0x1234));

    // Step PUSH AX
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.sp(), 0xFFFC);
    let pushed = mem.read_u16(0xFFFC, Endianness::LittleEndian).unwrap();
    assert_eq!(pushed, 0x1234);

    // Step MOV BX
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("BX"), Some(0x000A));

    // Step ADD AX, BX
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("AX"), Some(0x123E));

    // Step POP BX
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("BX"), Some(0x1234));
    assert_eq!(cpu.sp(), 0xFFFE);

    // Step CMP AX, 0x123E
    cpu.step(&mut mem).unwrap();
    let flags = cpu.get_register("FLAGS").unwrap();
    assert_ne!(flags & 0x40, 0, "ZF must be set");

    // Step JZ
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.pc(), 0x1011);

    // Step HLT
    let outcome = cpu.step(&mut mem).unwrap();
    assert_eq!(outcome, StepOutcome::Halted);
}

#[test]
fn test_avr_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::Avr).unwrap();

    cpu.set_pc(0x0000);
    cpu.set_sp(0x08FF);

    // Instructions:
    // 0x0000: LDI r16, 20 (0xE104)
    // 0x0002: LDI r17, 22 (0xE116)
    // 0x0004: ADD r16, r17 (0x0F01) -> r16 = 42
    // 0x0006: PUSH r16 (0x930F)
    // 0x0008: POP r18 (0x912F)
    // 0x000A: SLEEP (0x9588)
    mem.write_u16(0x0000, 0xE104, Endianness::LittleEndian).unwrap();
    mem.write_u16(0x0002, 0xE116, Endianness::LittleEndian).unwrap();
    mem.write_u16(0x0004, 0x0F01, Endianness::LittleEndian).unwrap();
    mem.write_u16(0x0006, 0x930F, Endianness::LittleEndian).unwrap();
    mem.write_u16(0x0008, 0x912F, Endianness::LittleEndian).unwrap();
    mem.write_u16(0x000A, 0x9588, Endianness::LittleEndian).unwrap();

    // Step LDI r16
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("r16"), Some(20));

    // Step LDI r17
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("r17"), Some(22));

    // Step ADD
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("r16"), Some(42));

    // Step PUSH
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.sp(), 0x08FE);
    let val_in_sram = mem.read_u8(0x08FF).unwrap();
    assert_eq!(val_in_sram, 42);

    // Step POP
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("r18"), Some(42));
    assert_eq!(cpu.sp(), 0x08FF);

    // Step SLEEP
    let outcome = cpu.step(&mut mem).unwrap();
    assert_eq!(outcome, StepOutcome::Halted);
}

#[test]
fn test_mos6502_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::Mos6502).unwrap();

    cpu.set_pc(0x0600);
    cpu.set_sp(0x01FD);

    // Instructions:
    // 0x0600: A9 1A (LDA #$1A = 26)
    // 0x0602: 18    (CLC)
    // 0x0603: 69 10 (ADC #$10 = 16 -> A = 42)
    // 0x0605: 48    (PHA -> push 42 to $01FD)
    // 0x0606: AA    (TAX -> X = 42)
    // 0x0607: 68    (PLA -> pull 42 into A)
    // 0x0608: EA    (NOP)
    mem.write_u8(0x0600, 0xA9).unwrap();
    mem.write_u8(0x0601, 0x1A).unwrap();
    mem.write_u8(0x0602, 0x18).unwrap();
    mem.write_u8(0x0603, 0x69).unwrap();
    mem.write_u8(0x0604, 0x10).unwrap();
    mem.write_u8(0x0605, 0x48).unwrap();
    mem.write_u8(0x0606, 0xAA).unwrap();
    mem.write_u8(0x0607, 0x68).unwrap();
    mem.write_u8(0x0608, 0xEA).unwrap();

    // Step LDA
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("A"), Some(26));

    // Step CLC
    cpu.step(&mut mem).unwrap();

    // Step ADC
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("A"), Some(42));

    // Step PHA
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.sp(), 0x01FC);
    let pushed_val = mem.read_u8(0x01FD).unwrap();
    assert_eq!(pushed_val, 42);

    // Step TAX
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("X"), Some(42));

    // Step PLA
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("A"), Some(42));
    assert_eq!(cpu.sp(), 0x01FD);
}

#[test]
fn test_mos6502_bcd_and_shift_instructions() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::Mos6502).unwrap();

    cpu.set_pc(0x0600);
    // BCD addition: SED; LDA #$25; CLC; ADC #$18 -> A = $43
    mem.write_u8(0x0600, 0xF8).unwrap(); // SED
    mem.write_u8(0x0601, 0xA9).unwrap(); // LDA #$25
    mem.write_u8(0x0602, 0x25).unwrap();
    mem.write_u8(0x0603, 0x18).unwrap(); // CLC
    mem.write_u8(0x0604, 0x69).unwrap(); // ADC #$18
    mem.write_u8(0x0605, 0x18).unwrap();
    mem.write_u8(0x0606, 0xD8).unwrap(); // CLD
    // ASL memory zp $80
    mem.write_u8(0x0607, 0xA9).unwrap(); // LDA #$01
    mem.write_u8(0x0608, 0x01).unwrap();
    mem.write_u8(0x0609, 0x85).unwrap(); // STA $80
    mem.write_u8(0x060A, 0x80).unwrap();
    mem.write_u8(0x060B, 0x06).unwrap(); // ASL $80 -> $80 becomes 2
    mem.write_u8(0x060C, 0x80).unwrap();

    cpu.step(&mut mem).unwrap(); // SED
    cpu.step(&mut mem).unwrap(); // LDA #$25
    cpu.step(&mut mem).unwrap(); // CLC
    cpu.step(&mut mem).unwrap(); // ADC #$18
    assert_eq!(cpu.get_register("A"), Some(0x43)); // BCD 25 + 18 = 43

    cpu.step(&mut mem).unwrap(); // CLD
    cpu.step(&mut mem).unwrap(); // LDA #1
    cpu.step(&mut mem).unwrap(); // STA $80
    cpu.step(&mut mem).unwrap(); // ASL $80
    assert_eq!(mem.read_u8(0x0080).unwrap(), 0x02);
}

#[test]
fn test_riscv_m_extension_and_csrs() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::RiscV).unwrap();

    cpu.set_pc(0x1000);
    // ADDI x1, x0, 6 (0x00600093)
    // ADDI x2, x0, 7 (0x00700113)
    // MUL x3, x1, x2 (0x022081B3) -> x3 = 42
    // DIV x4, x3, x1 (0x0211C233) -> x4 = 7
    // CSRRW x5, 0x340, x3 (0x340192F3) -> mscratch = 42, x5 = 0
    // CSRRS x6, 0x340, x0 (0x34002333) -> x6 = 42
    mem.write_u32(0x1000, 0x00600093, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1004, 0x00700113, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1008, 0x022081B3, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x100C, 0x0211C233, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1010, 0x340192F3, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1014, 0x34002373, Endianness::LittleEndian).unwrap();

    cpu.step(&mut mem).unwrap(); // x1 = 6
    cpu.step(&mut mem).unwrap(); // x2 = 7
    cpu.step(&mut mem).unwrap(); // MUL x3 = 42
    assert_eq!(cpu.get_register("gp"), Some(42));

    cpu.step(&mut mem).unwrap(); // DIV x4 = 7
    cpu.step(&mut mem).unwrap(); // CSRRW mscratch = 42
    cpu.step(&mut mem).unwrap(); // CSRRS x6 = 42
}

#[test]
fn test_i8086_extended_instructions() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::I8086).unwrap();

    cpu.set_pc(0x1000);
    // MOV AX, 10 (0xB8 0x0A 0x00)
    // MOV CX, 5  (0xB9 0x05 0x00)
    // MUL CX     (0xF7 0xE1) -> AX = 50, DX = 0
    // SHL AX, 1  (0xD1 0xE0) -> AX = 100
    // DAA / AAM  (0xD4 0x0A) -> AH = 10, AL = 0
    mem.write_u8(0x1000, 0xB8).unwrap();
    mem.write_u8(0x1001, 0x0A).unwrap();
    mem.write_u8(0x1002, 0x00).unwrap();
    mem.write_u8(0x1003, 0xB9).unwrap();
    mem.write_u8(0x1004, 0x05).unwrap();
    mem.write_u8(0x1005, 0x00).unwrap();
    mem.write_u8(0x1006, 0xF7).unwrap();
    mem.write_u8(0x1007, 0xE1).unwrap();
    mem.write_u8(0x1008, 0xD1).unwrap();
    mem.write_u8(0x1009, 0xE0).unwrap();
    mem.write_u8(0x100A, 0xD4).unwrap();
    mem.write_u8(0x100B, 0x0A).unwrap();

    cpu.step(&mut mem).unwrap(); // MOV AX, 10
    assert_eq!(cpu.get_register("AX"), Some(10));

    cpu.step(&mut mem).unwrap(); // MOV CX, 5
    assert_eq!(cpu.get_register("CX"), Some(5));

    cpu.step(&mut mem).unwrap(); // MUL CX
    assert_eq!(cpu.get_register("AX"), Some(50));
    assert_eq!(cpu.get_register("DX"), Some(0));

    cpu.step(&mut mem).unwrap(); // SHL AX, 1
    assert_eq!(cpu.get_register("AX"), Some(100));

    cpu.step(&mut mem).unwrap(); // AAM 10 -> AH = 10, AL = 0
    assert_eq!(cpu.get_register("AX"), Some(0x0A00));
}

#[test]
fn test_avr_extended_instructions() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::Avr).unwrap();

    cpu.set_pc(0x0000);
    // LDI r24, 10 (0xE08A)
    // LDI r25, 0  (0xE090)
    // ADIW r24, 32 (0x9680) -> r25:r24 = 42
    // SWAP r24    (0x9582) -> r24 = 0x2A -> 0xA2
    mem.write_u16(0x0000, 0xE08A, Endianness::LittleEndian).unwrap();
    mem.write_u16(0x0002, 0xE090, Endianness::LittleEndian).unwrap();
    mem.write_u16(0x0004, 0x9680, Endianness::LittleEndian).unwrap();
    mem.write_u16(0x0006, 0x9582, Endianness::LittleEndian).unwrap();

    cpu.step(&mut mem).unwrap(); // LDI r24, 10
    cpu.step(&mut mem).unwrap(); // LDI r25, 0
    cpu.step(&mut mem).unwrap(); // ADIW r24, 32 -> 42 (0x002A)
    assert_eq!(cpu.get_register("r24"), Some(42));
    assert_eq!(cpu.get_register("r25"), Some(0));

    cpu.step(&mut mem).unwrap(); // SWAP r24 (0x2A -> 0xA2 = 162)
    assert_eq!(cpu.get_register("r24"), Some(0xA2));
}

#[test]
fn test_x86_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::X86).unwrap();

    cpu.set_pc(0x1000);
    cpu.set_sp(0x8000);

    // MOV EAX, 10 (0xB8 0x0A 0x00 0x00 0x00)
    // MOV EBX, 20 (0xBB 0x14 0x00 0x00 0x00)
    // ADD EAX, EBX (0x01 0xD8) -> EAX = 30
    // PUSH EAX     (0x50)
    // POP ECX      (0x59) -> ECX = 30
    // HLT          (0xF4)
    mem.write_u8(0x1000, 0xB8).unwrap();
    mem.write_u32(0x1001, 10, Endianness::LittleEndian).unwrap();
    mem.write_u8(0x1005, 0xBB).unwrap();
    mem.write_u32(0x1006, 20, Endianness::LittleEndian).unwrap();
    mem.write_u8(0x100A, 0x01).unwrap();
    mem.write_u8(0x100B, 0xD8).unwrap();
    mem.write_u8(0x100C, 0x50).unwrap();
    mem.write_u8(0x100D, 0x59).unwrap();
    mem.write_u8(0x100E, 0xF4).unwrap();

    cpu.step(&mut mem).unwrap(); // MOV EAX, 10
    assert_eq!(cpu.get_register("EAX"), Some(10));

    cpu.step(&mut mem).unwrap(); // MOV EBX, 20
    assert_eq!(cpu.get_register("EBX"), Some(20));

    cpu.step(&mut mem).unwrap(); // ADD EAX, EBX
    assert_eq!(cpu.get_register("EAX"), Some(30));

    cpu.step(&mut mem).unwrap(); // PUSH EAX
    cpu.step(&mut mem).unwrap(); // POP ECX
    assert_eq!(cpu.get_register("ECX"), Some(30));

    let outcome = cpu.step(&mut mem).unwrap(); // HLT
    assert_eq!(outcome, StepOutcome::Halted);
}

#[test]
fn test_x86_64_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::X86_64).unwrap();

    cpu.set_pc(0x1000);
    cpu.set_sp(0x8000);

    // MOV RAX, 100 (0x48 0xB8 0x64 ...)
    // MOV RBX, 200 (0x48 0xBB 0xC8 ...)
    // ADD RAX, RBX (0x48 0x01 0xD8) -> RAX = 300
    // PUSH RAX     (0x50)
    // POP RDX      (0x5A) -> RDX = 300
    // HLT          (0xF4)
    mem.write_u8(0x1000, 0x48).unwrap();
    mem.write_u8(0x1001, 0xB8).unwrap();
    mem.write_u64(0x1002, 100, Endianness::LittleEndian).unwrap();
    mem.write_u8(0x100A, 0x48).unwrap();
    mem.write_u8(0x100B, 0xBB).unwrap();
    mem.write_u64(0x100C, 200, Endianness::LittleEndian).unwrap();
    mem.write_u8(0x1014, 0x48).unwrap();
    mem.write_u8(0x1015, 0x01).unwrap();
    mem.write_u8(0x1016, 0xD8).unwrap();
    mem.write_u8(0x1017, 0x50).unwrap();
    mem.write_u8(0x1018, 0x5A).unwrap();
    mem.write_u8(0x1019, 0xF4).unwrap();

    cpu.step(&mut mem).unwrap(); // MOV RAX, 100
    assert_eq!(cpu.get_register("RAX"), Some(100));

    cpu.step(&mut mem).unwrap(); // MOV RBX, 200
    assert_eq!(cpu.get_register("RBX"), Some(200));

    cpu.step(&mut mem).unwrap(); // ADD RAX, RBX
    assert_eq!(cpu.get_register("RAX"), Some(300));

    cpu.step(&mut mem).unwrap(); // PUSH RAX
    cpu.step(&mut mem).unwrap(); // POP RDX
    assert_eq!(cpu.get_register("RDX"), Some(300));

    let outcome = cpu.step(&mut mem).unwrap(); // HLT
    assert_eq!(outcome, StepOutcome::Halted);
}

#[test]
fn test_arm32_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::Arm32).unwrap();

    cpu.set_pc(0x1000);

    // MOV R0, #10 (0xE3A0000A)
    // MOV R1, #20 (0xE3A01014)
    // ADD R2, R0, R1 (0xE0802001) -> R2 = 30
    // SUB R3, R2, #5 (0xE2423005) -> R3 = 25
    // BKPT 0        (0xE1200070)
    mem.write_u32(0x1000, 0xE3A0000A, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1004, 0xE3A01014, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1008, 0xE0802001, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x100C, 0xE2423005, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1010, 0xE1200070, Endianness::LittleEndian).unwrap();

    cpu.step(&mut mem).unwrap(); // MOV R0
    assert_eq!(cpu.get_register("R0"), Some(10));

    cpu.step(&mut mem).unwrap(); // MOV R1
    assert_eq!(cpu.get_register("R1"), Some(20));

    cpu.step(&mut mem).unwrap(); // ADD R2, R0, R1
    assert_eq!(cpu.get_register("R2"), Some(30));

    cpu.step(&mut mem).unwrap(); // SUB R3, R2, 5
    assert_eq!(cpu.get_register("R3"), Some(25));

    let outcome = cpu.step(&mut mem).unwrap(); // BKPT
    assert_eq!(outcome, StepOutcome::Breakpoint);
}

#[test]
fn test_arm64_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::Arm64).unwrap();

    cpu.set_pc(0x1000);

    // MOVZ X0, #42 (0xD2800540)
    // MOVZ X1, #8 (0xD2800101)
    // ADD X2, X0, X1 (0x8B010002) -> X2 = 50
    // BRK 0         (0xD4200000)
    mem.write_u32(0x1000, 0xD2800540, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1004, 0xD2800101, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1008, 0x8B010002, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x100C, 0xD4200000, Endianness::LittleEndian).unwrap();

    cpu.step(&mut mem).unwrap(); // MOVZ X0
    assert_eq!(cpu.get_register("X0"), Some(42));

    cpu.step(&mut mem).unwrap(); // MOVZ X1
    assert_eq!(cpu.get_register("X1"), Some(8));

    cpu.step(&mut mem).unwrap(); // ADD X2, X0, X1
    assert_eq!(cpu.get_register("X2"), Some(50));

    let outcome = cpu.step(&mut mem).unwrap(); // BRK
    assert_eq!(outcome, StepOutcome::Breakpoint);
}

#[test]
fn test_mips_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::Mips).unwrap();

    cpu.set_pc(0x1000);

    // ADDIU $t0, $zero, 15 (0x2408000F)
    // ADDIU $t1, $zero, 25 (0x24090019)
    // ADDU  $t2, $t0, $t1  (0x01095021) -> $t2 ($10) = 40
    // BREAK                (0x0000000D)
    mem.write_u32(0x1000, 0x2408000F, Endianness::BigEndian).unwrap();
    mem.write_u32(0x1004, 0x24090019, Endianness::BigEndian).unwrap();
    mem.write_u32(0x1008, 0x01095021, Endianness::BigEndian).unwrap();
    mem.write_u32(0x100C, 0x0000000D, Endianness::BigEndian).unwrap();

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("$t0"), Some(15));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("$t1"), Some(25));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("$t2"), Some(40));

    let outcome = cpu.step(&mut mem).unwrap();
    assert_eq!(outcome, StepOutcome::Breakpoint);
}

#[test]
fn test_powerpc_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::PowerPc).unwrap();

    cpu.set_pc(0x1000);

    // ADDI r3, 0, 100 (0x38600064)
    // ADDI r4, 0, 50  (0x38800032)
    // ADD  r5, r3, r4 (0x7CA32214) -> r5 = 150
    // TRAP            (0x7FE00008)
    mem.write_u32(0x1000, 0x38600064, Endianness::BigEndian).unwrap();
    mem.write_u32(0x1004, 0x38800032, Endianness::BigEndian).unwrap();
    mem.write_u32(0x1008, 0x7CA32214, Endianness::BigEndian).unwrap();
    mem.write_u32(0x100C, 0x7FE00008, Endianness::BigEndian).unwrap();

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("r3"), Some(100));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("r4"), Some(50));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("r5"), Some(150));

    let outcome = cpu.step(&mut mem).unwrap();
    assert_eq!(outcome, StepOutcome::Breakpoint);
}

#[test]
fn test_sparc_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::Sparc).unwrap();

    cpu.set_pc(0x1000);

    // SETHI 0x12345000, %g1 (0x03048D14)
    // OR %g0, 42, %g2       (0x8410202A)
    // TA 0                  (0x91D02000)
    mem.write_u32(0x1000, 0x03048D14, Endianness::BigEndian).unwrap();
    mem.write_u32(0x1004, 0x8410202A, Endianness::BigEndian).unwrap();
    mem.write_u32(0x1008, 0x91D02000, Endianness::BigEndian).unwrap();

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("%g1"), Some(0x12345000));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("%g2"), Some(42));

    let outcome = cpu.step(&mut mem).unwrap();
    assert_eq!(outcome, StepOutcome::Breakpoint);
}

#[test]
fn test_m68k_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::Motorola68000).unwrap();

    cpu.set_pc(0x1000);
    cpu.set_sp(0x8000);

    // MOVEQ #10, D0 (0x700A)
    // MOVEQ #20, D1 (0x7214)
    // ADD.L D1, D0  (0xD081) -> D0 = 30
    // MOVE.L D0, -(SP) (0x2F00)
    // MOVE.L (SP)+, D2 (0x241F) -> D2 = 30
    // STOP          (0x4E72)
    mem.write_u16(0x1000, 0x700A, Endianness::BigEndian).unwrap();
    mem.write_u16(0x1002, 0x7214, Endianness::BigEndian).unwrap();
    mem.write_u16(0x1004, 0xD081, Endianness::BigEndian).unwrap();
    mem.write_u16(0x1006, 0x2F00, Endianness::BigEndian).unwrap();
    mem.write_u16(0x1008, 0x241F, Endianness::BigEndian).unwrap();
    mem.write_u16(0x100A, 0x4E72, Endianness::BigEndian).unwrap();

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("D0"), Some(10));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("D1"), Some(20));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("D0"), Some(30));

    cpu.step(&mut mem).unwrap();
    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("D2"), Some(30));

    let outcome = cpu.step(&mut mem).unwrap();
    assert_eq!(outcome, StepOutcome::Halted);
}

#[test]
fn test_superh_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::SuperH).unwrap();

    cpu.set_pc(0x1000);

    // MOV #10, R0 (0xE00A)
    // MOV #20, R1 (0xE114)
    // ADD R1, R0  (0x301C) -> R0 = 30
    // SLEEP       (0x001B)
    mem.write_u16(0x1000, 0xE00A, Endianness::BigEndian).unwrap();
    mem.write_u16(0x1002, 0xE114, Endianness::BigEndian).unwrap();
    mem.write_u16(0x1004, 0x301C, Endianness::BigEndian).unwrap();
    mem.write_u16(0x1006, 0x001B, Endianness::BigEndian).unwrap();

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("R0"), Some(10));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("R1"), Some(20));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("R0"), Some(30));

    let outcome = cpu.step(&mut mem).unwrap();
    assert_eq!(outcome, StepOutcome::Halted);
}

#[test]
fn test_alpha_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::DecAlpha).unwrap();

    cpu.set_pc(0x1000);

    // BIS r31, 10, r0 (0x47E15400) -> r0 = 10
    // BIS r31, 20, r1 (0x47E29401) -> r1 = 20
    // ADDQ r0, r1, r2 (0x40010402) -> r2 = 30
    // BPT             (0x00000080)
    mem.write_u32(0x1000, 0x47E15400, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1004, 0x47E29401, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x1008, 0x40010402, Endianness::LittleEndian).unwrap();
    mem.write_u32(0x100C, 0x00000080, Endianness::LittleEndian).unwrap();

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("r0"), Some(10));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("r1"), Some(20));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("r2"), Some(30));

    let outcome = cpu.step(&mut mem).unwrap();
    assert_eq!(outcome, StepOutcome::Breakpoint);
}

#[test]
fn test_parisc_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::PaRisc).unwrap();

    cpu.set_pc(0x1000);

    // LDO 10(r0), r1 (0x3401000A) -> r1 = 10
    // LDO 20(r0), r2 (0x34020014) -> r2 = 20
    // ADD r1, r2, r3 (0x08220603) -> r3 = 30
    // BREAK 0,0      (0x00000000)
    mem.write_u32(0x1000, 0x3401000A, Endianness::BigEndian).unwrap();
    mem.write_u32(0x1004, 0x34020014, Endianness::BigEndian).unwrap();
    mem.write_u32(0x1008, 0x08220603, Endianness::BigEndian).unwrap();
    mem.write_u32(0x100C, 0x00000000, Endianness::BigEndian).unwrap();

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("gr1"), Some(10));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("gr2"), Some(20));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("gr3"), Some(30));

    let outcome = cpu.step(&mut mem).unwrap();
    assert_eq!(outcome, StepOutcome::Breakpoint);
}

#[test]
fn test_ia64_execution_engine() {
    let mut mem = ArrayMemory::<65536>::new();
    let mut cpu: AnyCpu = create_cpu(Architecture::Ia64).unwrap();

    cpu.set_pc(0x1000);

    // Bundle 1: load imm 50 into gr2 (bundle byte 0 = 0x03, r1=2, imm=50)
    let mut b1 = [0u8; 16];
    b1[0] = 0x03;
    b1[1] = 2;
    b1[2..10].copy_from_slice(&50u64.to_le_bytes());
    mem.write_bytes(0x1000, &b1).unwrap();

    // Bundle 2: load imm 20 into gr3 (bundle byte 0 = 0x03, r1=3, imm=20)
    let mut b2 = [0u8; 16];
    b2[0] = 0x03;
    b2[1] = 3;
    b2[2..10].copy_from_slice(&20u64.to_le_bytes());
    mem.write_bytes(0x1010, &b2).unwrap();

    // Bundle 3: add gr1 = gr2 + gr3 (bundle byte 0 = 0x02, r1=1, r2=2, r3=3)
    let mut b3 = [0u8; 16];
    b3[0] = 0x02;
    b3[1] = 1;
    b3[2] = 2;
    b3[3] = 3;
    mem.write_bytes(0x1020, &b3).unwrap();

    // Bundle 4: Breakpoint (bundle byte 0 = 0x01)
    let mut b4 = [0u8; 16];
    b4[0] = 0x01;
    mem.write_bytes(0x1030, &b4).unwrap();

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("gr2"), Some(50));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("gr3"), Some(20));

    cpu.step(&mut mem).unwrap();
    assert_eq!(cpu.get_register("gr1"), Some(70));

    let outcome = cpu.step(&mut mem).unwrap();
    assert_eq!(outcome, StepOutcome::Breakpoint);
}
