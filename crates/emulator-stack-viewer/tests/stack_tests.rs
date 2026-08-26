use emulator_core::arch::parisc::PaRiscCpu;
use emulator_core::arch::x86_64::X86_64Cpu;
use emulator_core::arch::AnyCpu;
use emulator_core::arch::Architecture;
use emulator_core::bus::{ArrayMemory, MemoryBus};
use emulator_core::cpu::CpuEngine;
use emulator_core::types::StackGrowth;
use emulator_stack_viewer::{
    analyze_stack, detect_frame_pointer, format_stack_table, DisplayFormat, StackViewOptions,
};

#[test]
fn test_stack_analysis_x86_64() {
    let mut cpu = X86_64Cpu::new();
    let mut bus = ArrayMemory::<65536>::new();

    cpu.set_sp(0x8000);
    cpu.set_register("RBP", 0x8010).unwrap();

    // Push values onto stack (0x8000, 0x8008, 0x8010)
    bus.write_u64(0x8000, 0xDEADBEEFCAFEBABE, emulator_core::types::Endianness::LittleEndian)
        .unwrap();
    bus.write_u64(0x8008, 0x1122334455667788, emulator_core::types::Endianness::LittleEndian)
        .unwrap();
    bus.write_u64(0x8010, 0x8020, emulator_core::types::Endianness::LittleEndian)
        .unwrap();

    let options = StackViewOptions {
        slot_count: 4,
        display_format: DisplayFormat::Hex,
        show_ascii: true,
        show_raw_bytes: true,
        show_relative_offset: true,
        custom_base_addr: None,
        word_size_override: None,
    };

    let analysis = analyze_stack(&cpu, &bus, &options);

    assert_eq!(analysis.arch, Architecture::X86_64);
    assert_eq!(analysis.sp, 0x8000);
    assert_eq!(analysis.fp, Some(0x8010));
    assert_eq!(analysis.growth, StackGrowth::Downwards);
    assert_eq!(analysis.word_size, 8);
    assert!(analysis.is_aligned);
    assert_eq!(analysis.entries.len(), 4);

    assert!(analysis.entries[0].is_sp);
    assert_eq!(analysis.entries[0].value, 0xDEADBEEFCAFEBABE);
    assert_eq!(analysis.entries[0].offset_from_sp, 0);

    assert_eq!(analysis.entries[1].offset_from_sp, 8);
    assert_eq!(analysis.entries[1].value, 0x1122334455667788);

    assert!(analysis.entries[2].is_fp);
    assert_eq!(analysis.entries[2].offset_from_sp, 16);

    let table = format_stack_table(&analysis, &options);
    assert!(table.contains("Top of Stack"));
    assert!(table.contains("Frame Pointer"));
    assert!(table.contains("0xDEADBEEFCAFEBABE"));
}

#[test]
fn test_stack_analysis_parisc_upwards() {
    let mut cpu = PaRiscCpu::new();
    let mut bus = ArrayMemory::<65536>::new();

    cpu.set_sp(0x8000);
    // Write 32-bit big-endian words
    bus.write_u32(0x8000, 0x12345678, emulator_core::types::Endianness::BigEndian)
        .unwrap();
    bus.write_u32(0x7FFC, 0x87654321, emulator_core::types::Endianness::BigEndian)
        .unwrap();

    let options = StackViewOptions {
        slot_count: 2,
        ..Default::default()
    };

    let analysis = analyze_stack(&cpu, &bus, &options);
    assert_eq!(analysis.arch, Architecture::PaRisc);
    assert_eq!(analysis.growth, StackGrowth::Upwards);
    assert_eq!(analysis.word_size, 4);

    assert_eq!(analysis.entries[0].address, 0x8000);
    assert_eq!(analysis.entries[0].value, 0x12345678);

    assert_eq!(analysis.entries[1].address, 0x7FFC);
    assert_eq!(analysis.entries[1].value, 0x87654321);
}

#[test]
fn test_stack_analysis_all_architectures() {
    let bus = ArrayMemory::<65536>::new();
    let options = StackViewOptions::default();

    for &arch in Architecture::ALL {
        let mut cpu = AnyCpu::new(arch);
        cpu.set_sp(0x1000);
        let analysis = analyze_stack(&cpu, &bus, &options);
        assert_eq!(analysis.arch, arch);
        assert!(!analysis.entries.is_empty());
        let _ = detect_frame_pointer(&cpu);
        let formatted = format_stack_table(&analysis, &options);
        assert!(!formatted.is_empty());
    }
}
