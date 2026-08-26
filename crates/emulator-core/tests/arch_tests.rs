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
