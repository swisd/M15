//! Supported architecture definitions, metadata, and CPU dispatch.

use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

#[cfg(feature = "arch-8086")]
pub mod i8086;
#[cfg(feature = "arch-alpha")]
pub mod alpha;
#[cfg(feature = "arch-arm32")]
pub mod arm32;
#[cfg(feature = "arch-arm64")]
pub mod arm64;
#[cfg(feature = "arch-avr")]
pub mod avr;
#[cfg(feature = "arch-ia64")]
pub mod ia64;
#[cfg(feature = "arch-m68k")]
pub mod m68k;
#[cfg(feature = "arch-mips")]
pub mod mips;
#[cfg(feature = "arch-parisc")]
pub mod parisc;
#[cfg(feature = "arch-powerpc")]
pub mod powerpc;
#[cfg(feature = "arch-riscv")]
pub mod riscv;
#[cfg(feature = "arch-sparc")]
pub mod sparc;
#[cfg(feature = "arch-superh")]
pub mod superh;
#[cfg(feature = "arch-x86")]
pub mod x86;
#[cfg(feature = "arch-x86_64")]
pub mod x86_64;
#[cfg(feature = "arch-6502")]
pub mod mos6502;

/// Target CPU Architecture identifiers supported by the emulator.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Architecture {
    I8086,
    X86,
    X86_64,
    Arm32,
    Arm64,
    RiscV,
    Ia64,
    Mips,
    PowerPc,
    Sparc,
    Avr,
    SuperH,
    PaRisc,
    DecAlpha,
    Motorola68000,
    Mos6502,
}

impl Architecture {
    /// Canonical name as listed in `supported_arch`.
    pub const fn name(&self) -> &'static str {
        match self {
            Architecture::I8086 => "8086",
            Architecture::X86 => "X86",
            Architecture::X86_64 => "X86_64",
            Architecture::Arm32 => "arm32",
            Architecture::Arm64 => "arm64",
            Architecture::RiscV => "risc-v",
            Architecture::Ia64 => "IA-64",
            Architecture::Mips => "MIPS",
            Architecture::PowerPc => "PowerPC",
            Architecture::Sparc => "SPARC",
            Architecture::Avr => "AVR",
            Architecture::SuperH => "SuperH",
            Architecture::PaRisc => "PA-RISC",
            Architecture::DecAlpha => "DEC Alpha",
            Architecture::Motorola68000 => "Motorola 68000",
            Architecture::Mos6502 => "MOS 6502",
        }
    }

    /// Descriptive human-readable display name.
    pub const fn display_name(&self) -> &'static str {
        match self {
            Architecture::I8086 => "Intel 8086 (16-bit)",
            Architecture::X86 => "Intel x86 (IA-32 32-bit)",
            Architecture::X86_64 => "AMD64 / x86-64 (64-bit)",
            Architecture::Arm32 => "ARMv7 / AArch32 (32-bit)",
            Architecture::Arm64 => "ARMv8 / AArch64 (64-bit)",
            Architecture::RiscV => "RISC-V (RV32 / RV64)",
            Architecture::Ia64 => "Intel IA-64 (Itanium)",
            Architecture::Mips => "MIPS (MIPS32 / MIPS64)",
            Architecture::PowerPc => "PowerPC (PPC32 / PPC64)",
            Architecture::Sparc => "SPARC (SPARC V8 / V9)",
            Architecture::Avr => "Atmel AVR (8-bit)",
            Architecture::SuperH => "Renesas SuperH (SH-2 / SH-4)",
            Architecture::PaRisc => "HP PA-RISC (1.1 / 2.0)",
            Architecture::DecAlpha => "DEC Alpha (Alpha AXP 64-bit)",
            Architecture::Motorola68000 => "Motorola 68000 (m68k)",
            Architecture::Mos6502 => "MOS Technology 6502 (8-bit)",
        }
    }

    /// Default word size for registers and memory addressing.
    pub const fn word_size(&self) -> WordSize {
        match self {
            Architecture::Avr | Architecture::Mos6502 => WordSize::Bytes1,
            Architecture::I8086 => WordSize::Bytes2,
            Architecture::X86
            | Architecture::Arm32
            | Architecture::Mips
            | Architecture::PowerPc
            | Architecture::Sparc
            | Architecture::SuperH
            | Architecture::PaRisc
            | Architecture::Motorola68000 => WordSize::Bytes4,
            Architecture::X86_64
            | Architecture::Arm64
            | Architecture::RiscV
            | Architecture::Ia64
            | Architecture::DecAlpha => WordSize::Bytes8,
        }
    }

    /// Default endianness.
    pub const fn endianness(&self) -> Endianness {
        match self {
            Architecture::I8086
            | Architecture::X86
            | Architecture::X86_64
            | Architecture::Arm32
            | Architecture::Arm64
            | Architecture::RiscV
            | Architecture::Ia64
            | Architecture::Avr
            | Architecture::DecAlpha
            | Architecture::Mos6502 => Endianness::LittleEndian,

            Architecture::Mips
            | Architecture::PowerPc
            | Architecture::Sparc
            | Architecture::SuperH
            | Architecture::PaRisc
            | Architecture::Motorola68000 => Endianness::BigEndian,
        }
    }

    /// Stack growth direction.
    pub const fn stack_growth(&self) -> StackGrowth {
        match self {
            Architecture::PaRisc => StackGrowth::Upwards,
            _ => StackGrowth::Downwards,
        }
    }

    /// All supported architectures.
    pub const ALL: &'static [Architecture] = &[
        Architecture::I8086,
        Architecture::X86,
        Architecture::X86_64,
        Architecture::Arm32,
        Architecture::Arm64,
        Architecture::RiscV,
        Architecture::Ia64,
        Architecture::Mips,
        Architecture::PowerPc,
        Architecture::Sparc,
        Architecture::Avr,
        Architecture::SuperH,
        Architecture::PaRisc,
        Architecture::DecAlpha,
        Architecture::Motorola68000,
        Architecture::Mos6502,
    ];

    /// All supported architectures.
    pub const fn all() -> &'static [Architecture] {
        Self::ALL
    }

    /// Parse an architecture identifier from a string.
    pub fn from_name(s: &str) -> Option<Architecture> {
        let trimmed = s.trim();
        if trimmed.eq_ignore_ascii_case("8086") || trimmed.eq_ignore_ascii_case("i8086") {
            return Some(Architecture::I8086);
        }
        if trimmed.eq_ignore_ascii_case("x86")
            || trimmed.eq_ignore_ascii_case("i386")
            || trimmed.eq_ignore_ascii_case("ia32")
        {
            return Some(Architecture::X86);
        }
        if trimmed.eq_ignore_ascii_case("x86_64")
            || trimmed.eq_ignore_ascii_case("x86-64")
            || trimmed.eq_ignore_ascii_case("amd64")
            || trimmed.eq_ignore_ascii_case("x64")
        {
            return Some(Architecture::X86_64);
        }
        if trimmed.eq_ignore_ascii_case("arm32")
            || trimmed.eq_ignore_ascii_case("arm")
            || trimmed.eq_ignore_ascii_case("aarch32")
            || trimmed.eq_ignore_ascii_case("armv7")
        {
            return Some(Architecture::Arm32);
        }
        if trimmed.eq_ignore_ascii_case("arm64")
            || trimmed.eq_ignore_ascii_case("aarch64")
            || trimmed.eq_ignore_ascii_case("armv8")
        {
            return Some(Architecture::Arm64);
        }
        if trimmed.eq_ignore_ascii_case("risc-v")
            || trimmed.eq_ignore_ascii_case("riscv")
            || trimmed.eq_ignore_ascii_case("rv32")
            || trimmed.eq_ignore_ascii_case("rv64")
        {
            return Some(Architecture::RiscV);
        }
        if trimmed.eq_ignore_ascii_case("ia-64")
            || trimmed.eq_ignore_ascii_case("ia64")
            || trimmed.eq_ignore_ascii_case("itanium")
        {
            return Some(Architecture::Ia64);
        }
        if trimmed.eq_ignore_ascii_case("mips") || trimmed.eq_ignore_ascii_case("mips32") {
            return Some(Architecture::Mips);
        }
        if trimmed.eq_ignore_ascii_case("powerpc")
            || trimmed.eq_ignore_ascii_case("ppc")
            || trimmed.eq_ignore_ascii_case("ppc32")
        {
            return Some(Architecture::PowerPc);
        }
        if trimmed.eq_ignore_ascii_case("sparc") || trimmed.eq_ignore_ascii_case("sparcv8") {
            return Some(Architecture::Sparc);
        }
        if trimmed.eq_ignore_ascii_case("avr") || trimmed.eq_ignore_ascii_case("atmega") {
            return Some(Architecture::Avr);
        }
        if trimmed.eq_ignore_ascii_case("superh")
            || trimmed.eq_ignore_ascii_case("sh")
            || trimmed.eq_ignore_ascii_case("sh-2")
            || trimmed.eq_ignore_ascii_case("sh-4")
        {
            return Some(Architecture::SuperH);
        }
        if trimmed.eq_ignore_ascii_case("pa-risc")
            || trimmed.eq_ignore_ascii_case("parisc")
            || trimmed.eq_ignore_ascii_case("hppa")
        {
            return Some(Architecture::PaRisc);
        }
        if trimmed.eq_ignore_ascii_case("dec alpha")
            || trimmed.eq_ignore_ascii_case("alpha")
            || trimmed.eq_ignore_ascii_case("axp")
        {
            return Some(Architecture::DecAlpha);
        }
        if trimmed.eq_ignore_ascii_case("motorola 68000")
            || trimmed.eq_ignore_ascii_case("m68k")
            || trimmed.eq_ignore_ascii_case("68000")
            || trimmed.eq_ignore_ascii_case("68k")
        {
            return Some(Architecture::Motorola68000);
        }
        if trimmed.eq_ignore_ascii_case("mos 6502")
            || trimmed.eq_ignore_ascii_case("6502")
            || trimmed.eq_ignore_ascii_case("m6502")
        {
            return Some(Architecture::Mos6502);
        }
        None
    }
}

/// Static CPU container supporting static `#![no_std]` dispatch without heap allocation.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(clippy::large_enum_variant)]
pub enum AnyCpu {
    #[cfg(feature = "arch-8086")]
    I8086(i8086::I8086Cpu),
    #[cfg(feature = "arch-x86")]
    X86(x86::X86Cpu),
    #[cfg(feature = "arch-x86_64")]
    X86_64(x86_64::X86_64Cpu),
    #[cfg(feature = "arch-arm32")]
    Arm32(arm32::Arm32Cpu),
    #[cfg(feature = "arch-arm64")]
    Arm64(arm64::Arm64Cpu),
    #[cfg(feature = "arch-riscv")]
    RiscV(riscv::RiscvCpu),
    #[cfg(feature = "arch-ia64")]
    Ia64(ia64::Ia64Cpu),
    #[cfg(feature = "arch-mips")]
    Mips(mips::MipsCpu),
    #[cfg(feature = "arch-powerpc")]
    PowerPc(powerpc::PowerPcCpu),
    #[cfg(feature = "arch-sparc")]
    Sparc(sparc::SparcCpu),
    #[cfg(feature = "arch-avr")]
    Avr(avr::AvrCpu),
    #[cfg(feature = "arch-superh")]
    SuperH(superh::SuperHCpu),
    #[cfg(feature = "arch-parisc")]
    PaRisc(parisc::PaRiscCpu),
    #[cfg(feature = "arch-alpha")]
    DecAlpha(alpha::AlphaCpu),
    #[cfg(feature = "arch-m68k")]
    Motorola68000(m68k::M68kCpu),
    #[cfg(feature = "arch-6502")]
    Mos6502(mos6502::Mos6502Cpu),
}

/// Factory function to instantiate an `AnyCpu` for the requested architecture.
pub fn create_cpu(arch: Architecture) -> Option<AnyCpu> {
    match arch {
        #[cfg(feature = "arch-8086")]
        Architecture::I8086 => Some(AnyCpu::I8086(i8086::I8086Cpu::new())),
        #[cfg(feature = "arch-x86")]
        Architecture::X86 => Some(AnyCpu::X86(x86::X86Cpu::new())),
        #[cfg(feature = "arch-x86_64")]
        Architecture::X86_64 => Some(AnyCpu::X86_64(x86_64::X86_64Cpu::new())),
        #[cfg(feature = "arch-arm32")]
        Architecture::Arm32 => Some(AnyCpu::Arm32(arm32::Arm32Cpu::new())),
        #[cfg(feature = "arch-arm64")]
        Architecture::Arm64 => Some(AnyCpu::Arm64(arm64::Arm64Cpu::new())),
        #[cfg(feature = "arch-riscv")]
        Architecture::RiscV => Some(AnyCpu::RiscV(riscv::RiscvCpu::new())),
        #[cfg(feature = "arch-ia64")]
        Architecture::Ia64 => Some(AnyCpu::Ia64(ia64::Ia64Cpu::new())),
        #[cfg(feature = "arch-mips")]
        Architecture::Mips => Some(AnyCpu::Mips(mips::MipsCpu::new())),
        #[cfg(feature = "arch-powerpc")]
        Architecture::PowerPc => Some(AnyCpu::PowerPc(powerpc::PowerPcCpu::new())),
        #[cfg(feature = "arch-sparc")]
        Architecture::Sparc => Some(AnyCpu::Sparc(sparc::SparcCpu::new())),
        #[cfg(feature = "arch-avr")]
        Architecture::Avr => Some(AnyCpu::Avr(avr::AvrCpu::new())),
        #[cfg(feature = "arch-superh")]
        Architecture::SuperH => Some(AnyCpu::SuperH(superh::SuperHCpu::new())),
        #[cfg(feature = "arch-parisc")]
        Architecture::PaRisc => Some(AnyCpu::PaRisc(parisc::PaRiscCpu::new())),
        #[cfg(feature = "arch-alpha")]
        Architecture::DecAlpha => Some(AnyCpu::DecAlpha(alpha::AlphaCpu::new())),
        #[cfg(feature = "arch-m68k")]
        Architecture::Motorola68000 => Some(AnyCpu::Motorola68000(m68k::M68kCpu::new())),
        #[cfg(feature = "arch-6502")]
        Architecture::Mos6502 => Some(AnyCpu::Mos6502(mos6502::Mos6502Cpu::new())),
        #[allow(unreachable_patterns)]
        _ => None,
    }
}

impl AnyCpu {
    /// Creates a new CPU instance for the given architecture.
    pub fn new(arch: Architecture) -> Self {
        create_cpu(arch).expect("Requested CPU architecture is not enabled in Cargo features")
    }
}

impl CpuEngine for AnyCpu {
    fn arch(&self) -> Architecture {
        match self {
            #[cfg(feature = "arch-8086")]
            AnyCpu::I8086(cpu) => cpu.arch(),
            #[cfg(feature = "arch-x86")]
            AnyCpu::X86(cpu) => cpu.arch(),
            #[cfg(feature = "arch-x86_64")]
            AnyCpu::X86_64(cpu) => cpu.arch(),
            #[cfg(feature = "arch-arm32")]
            AnyCpu::Arm32(cpu) => cpu.arch(),
            #[cfg(feature = "arch-arm64")]
            AnyCpu::Arm64(cpu) => cpu.arch(),
            #[cfg(feature = "arch-riscv")]
            AnyCpu::RiscV(cpu) => cpu.arch(),
            #[cfg(feature = "arch-ia64")]
            AnyCpu::Ia64(cpu) => cpu.arch(),
            #[cfg(feature = "arch-mips")]
            AnyCpu::Mips(cpu) => cpu.arch(),
            #[cfg(feature = "arch-powerpc")]
            AnyCpu::PowerPc(cpu) => cpu.arch(),
            #[cfg(feature = "arch-sparc")]
            AnyCpu::Sparc(cpu) => cpu.arch(),
            #[cfg(feature = "arch-avr")]
            AnyCpu::Avr(cpu) => cpu.arch(),
            #[cfg(feature = "arch-superh")]
            AnyCpu::SuperH(cpu) => cpu.arch(),
            #[cfg(feature = "arch-parisc")]
            AnyCpu::PaRisc(cpu) => cpu.arch(),
            #[cfg(feature = "arch-alpha")]
            AnyCpu::DecAlpha(cpu) => cpu.arch(),
            #[cfg(feature = "arch-m68k")]
            AnyCpu::Motorola68000(cpu) => cpu.arch(),
            #[cfg(feature = "arch-6502")]
            AnyCpu::Mos6502(cpu) => cpu.arch(),
        }
    }

    fn endianness(&self) -> Endianness {
        match self {
            #[cfg(feature = "arch-8086")]
            AnyCpu::I8086(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-x86")]
            AnyCpu::X86(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-x86_64")]
            AnyCpu::X86_64(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-arm32")]
            AnyCpu::Arm32(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-arm64")]
            AnyCpu::Arm64(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-riscv")]
            AnyCpu::RiscV(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-ia64")]
            AnyCpu::Ia64(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-mips")]
            AnyCpu::Mips(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-powerpc")]
            AnyCpu::PowerPc(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-sparc")]
            AnyCpu::Sparc(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-avr")]
            AnyCpu::Avr(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-superh")]
            AnyCpu::SuperH(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-parisc")]
            AnyCpu::PaRisc(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-alpha")]
            AnyCpu::DecAlpha(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-m68k")]
            AnyCpu::Motorola68000(cpu) => cpu.endianness(),
            #[cfg(feature = "arch-6502")]
            AnyCpu::Mos6502(cpu) => cpu.endianness(),
        }
    }

    fn stack_growth(&self) -> StackGrowth {
        match self {
            #[cfg(feature = "arch-8086")]
            AnyCpu::I8086(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-x86")]
            AnyCpu::X86(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-x86_64")]
            AnyCpu::X86_64(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-arm32")]
            AnyCpu::Arm32(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-arm64")]
            AnyCpu::Arm64(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-riscv")]
            AnyCpu::RiscV(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-ia64")]
            AnyCpu::Ia64(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-mips")]
            AnyCpu::Mips(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-powerpc")]
            AnyCpu::PowerPc(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-sparc")]
            AnyCpu::Sparc(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-avr")]
            AnyCpu::Avr(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-superh")]
            AnyCpu::SuperH(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-parisc")]
            AnyCpu::PaRisc(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-alpha")]
            AnyCpu::DecAlpha(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-m68k")]
            AnyCpu::Motorola68000(cpu) => cpu.stack_growth(),
            #[cfg(feature = "arch-6502")]
            AnyCpu::Mos6502(cpu) => cpu.stack_growth(),
        }
    }

    fn word_size(&self) -> WordSize {
        match self {
            #[cfg(feature = "arch-8086")]
            AnyCpu::I8086(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-x86")]
            AnyCpu::X86(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-x86_64")]
            AnyCpu::X86_64(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-arm32")]
            AnyCpu::Arm32(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-arm64")]
            AnyCpu::Arm64(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-riscv")]
            AnyCpu::RiscV(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-ia64")]
            AnyCpu::Ia64(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-mips")]
            AnyCpu::Mips(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-powerpc")]
            AnyCpu::PowerPc(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-sparc")]
            AnyCpu::Sparc(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-avr")]
            AnyCpu::Avr(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-superh")]
            AnyCpu::SuperH(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-parisc")]
            AnyCpu::PaRisc(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-alpha")]
            AnyCpu::DecAlpha(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-m68k")]
            AnyCpu::Motorola68000(cpu) => cpu.word_size(),
            #[cfg(feature = "arch-6502")]
            AnyCpu::Mos6502(cpu) => cpu.word_size(),
        }
    }

    fn pc(&self) -> u64 {
        match self {
            #[cfg(feature = "arch-8086")]
            AnyCpu::I8086(cpu) => cpu.pc(),
            #[cfg(feature = "arch-x86")]
            AnyCpu::X86(cpu) => cpu.pc(),
            #[cfg(feature = "arch-x86_64")]
            AnyCpu::X86_64(cpu) => cpu.pc(),
            #[cfg(feature = "arch-arm32")]
            AnyCpu::Arm32(cpu) => cpu.pc(),
            #[cfg(feature = "arch-arm64")]
            AnyCpu::Arm64(cpu) => cpu.pc(),
            #[cfg(feature = "arch-riscv")]
            AnyCpu::RiscV(cpu) => cpu.pc(),
            #[cfg(feature = "arch-ia64")]
            AnyCpu::Ia64(cpu) => cpu.pc(),
            #[cfg(feature = "arch-mips")]
            AnyCpu::Mips(cpu) => cpu.pc(),
            #[cfg(feature = "arch-powerpc")]
            AnyCpu::PowerPc(cpu) => cpu.pc(),
            #[cfg(feature = "arch-sparc")]
            AnyCpu::Sparc(cpu) => cpu.pc(),
            #[cfg(feature = "arch-avr")]
            AnyCpu::Avr(cpu) => cpu.pc(),
            #[cfg(feature = "arch-superh")]
            AnyCpu::SuperH(cpu) => cpu.pc(),
            #[cfg(feature = "arch-parisc")]
            AnyCpu::PaRisc(cpu) => cpu.pc(),
            #[cfg(feature = "arch-alpha")]
            AnyCpu::DecAlpha(cpu) => cpu.pc(),
            #[cfg(feature = "arch-m68k")]
            AnyCpu::Motorola68000(cpu) => cpu.pc(),
            #[cfg(feature = "arch-6502")]
            AnyCpu::Mos6502(cpu) => cpu.pc(),
        }
    }

    fn set_pc(&mut self, val: u64) {
        match self {
            #[cfg(feature = "arch-8086")]
            AnyCpu::I8086(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-x86")]
            AnyCpu::X86(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-x86_64")]
            AnyCpu::X86_64(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-arm32")]
            AnyCpu::Arm32(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-arm64")]
            AnyCpu::Arm64(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-riscv")]
            AnyCpu::RiscV(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-ia64")]
            AnyCpu::Ia64(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-mips")]
            AnyCpu::Mips(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-powerpc")]
            AnyCpu::PowerPc(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-sparc")]
            AnyCpu::Sparc(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-avr")]
            AnyCpu::Avr(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-superh")]
            AnyCpu::SuperH(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-parisc")]
            AnyCpu::PaRisc(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-alpha")]
            AnyCpu::DecAlpha(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-m68k")]
            AnyCpu::Motorola68000(cpu) => cpu.set_pc(val),
            #[cfg(feature = "arch-6502")]
            AnyCpu::Mos6502(cpu) => cpu.set_pc(val),
        }
    }

    fn sp(&self) -> u64 {
        match self {
            #[cfg(feature = "arch-8086")]
            AnyCpu::I8086(cpu) => cpu.sp(),
            #[cfg(feature = "arch-x86")]
            AnyCpu::X86(cpu) => cpu.sp(),
            #[cfg(feature = "arch-x86_64")]
            AnyCpu::X86_64(cpu) => cpu.sp(),
            #[cfg(feature = "arch-arm32")]
            AnyCpu::Arm32(cpu) => cpu.sp(),
            #[cfg(feature = "arch-arm64")]
            AnyCpu::Arm64(cpu) => cpu.sp(),
            #[cfg(feature = "arch-riscv")]
            AnyCpu::RiscV(cpu) => cpu.sp(),
            #[cfg(feature = "arch-ia64")]
            AnyCpu::Ia64(cpu) => cpu.sp(),
            #[cfg(feature = "arch-mips")]
            AnyCpu::Mips(cpu) => cpu.sp(),
            #[cfg(feature = "arch-powerpc")]
            AnyCpu::PowerPc(cpu) => cpu.sp(),
            #[cfg(feature = "arch-sparc")]
            AnyCpu::Sparc(cpu) => cpu.sp(),
            #[cfg(feature = "arch-avr")]
            AnyCpu::Avr(cpu) => cpu.sp(),
            #[cfg(feature = "arch-superh")]
            AnyCpu::SuperH(cpu) => cpu.sp(),
            #[cfg(feature = "arch-parisc")]
            AnyCpu::PaRisc(cpu) => cpu.sp(),
            #[cfg(feature = "arch-alpha")]
            AnyCpu::DecAlpha(cpu) => cpu.sp(),
            #[cfg(feature = "arch-m68k")]
            AnyCpu::Motorola68000(cpu) => cpu.sp(),
            #[cfg(feature = "arch-6502")]
            AnyCpu::Mos6502(cpu) => cpu.sp(),
        }
    }

    fn set_sp(&mut self, val: u64) {
        match self {
            #[cfg(feature = "arch-8086")]
            AnyCpu::I8086(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-x86")]
            AnyCpu::X86(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-x86_64")]
            AnyCpu::X86_64(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-arm32")]
            AnyCpu::Arm32(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-arm64")]
            AnyCpu::Arm64(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-riscv")]
            AnyCpu::RiscV(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-ia64")]
            AnyCpu::Ia64(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-mips")]
            AnyCpu::Mips(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-powerpc")]
            AnyCpu::PowerPc(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-sparc")]
            AnyCpu::Sparc(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-avr")]
            AnyCpu::Avr(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-superh")]
            AnyCpu::SuperH(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-parisc")]
            AnyCpu::PaRisc(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-alpha")]
            AnyCpu::DecAlpha(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-m68k")]
            AnyCpu::Motorola68000(cpu) => cpu.set_sp(val),
            #[cfg(feature = "arch-6502")]
            AnyCpu::Mos6502(cpu) => cpu.set_sp(val),
        }
    }

    fn reset(&mut self) {
        match self {
            #[cfg(feature = "arch-8086")]
            AnyCpu::I8086(cpu) => cpu.reset(),
            #[cfg(feature = "arch-x86")]
            AnyCpu::X86(cpu) => cpu.reset(),
            #[cfg(feature = "arch-x86_64")]
            AnyCpu::X86_64(cpu) => cpu.reset(),
            #[cfg(feature = "arch-arm32")]
            AnyCpu::Arm32(cpu) => cpu.reset(),
            #[cfg(feature = "arch-arm64")]
            AnyCpu::Arm64(cpu) => cpu.reset(),
            #[cfg(feature = "arch-riscv")]
            AnyCpu::RiscV(cpu) => cpu.reset(),
            #[cfg(feature = "arch-ia64")]
            AnyCpu::Ia64(cpu) => cpu.reset(),
            #[cfg(feature = "arch-mips")]
            AnyCpu::Mips(cpu) => cpu.reset(),
            #[cfg(feature = "arch-powerpc")]
            AnyCpu::PowerPc(cpu) => cpu.reset(),
            #[cfg(feature = "arch-sparc")]
            AnyCpu::Sparc(cpu) => cpu.reset(),
            #[cfg(feature = "arch-avr")]
            AnyCpu::Avr(cpu) => cpu.reset(),
            #[cfg(feature = "arch-superh")]
            AnyCpu::SuperH(cpu) => cpu.reset(),
            #[cfg(feature = "arch-parisc")]
            AnyCpu::PaRisc(cpu) => cpu.reset(),
            #[cfg(feature = "arch-alpha")]
            AnyCpu::DecAlpha(cpu) => cpu.reset(),
            #[cfg(feature = "arch-m68k")]
            AnyCpu::Motorola68000(cpu) => cpu.reset(),
            #[cfg(feature = "arch-6502")]
            AnyCpu::Mos6502(cpu) => cpu.reset(),
        }
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        match self {
            #[cfg(feature = "arch-8086")]
            AnyCpu::I8086(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-x86")]
            AnyCpu::X86(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-x86_64")]
            AnyCpu::X86_64(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-arm32")]
            AnyCpu::Arm32(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-arm64")]
            AnyCpu::Arm64(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-riscv")]
            AnyCpu::RiscV(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-ia64")]
            AnyCpu::Ia64(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-mips")]
            AnyCpu::Mips(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-powerpc")]
            AnyCpu::PowerPc(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-sparc")]
            AnyCpu::Sparc(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-avr")]
            AnyCpu::Avr(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-superh")]
            AnyCpu::SuperH(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-parisc")]
            AnyCpu::PaRisc(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-alpha")]
            AnyCpu::DecAlpha(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-m68k")]
            AnyCpu::Motorola68000(cpu) => cpu.step(bus),
            #[cfg(feature = "arch-6502")]
            AnyCpu::Mos6502(cpu) => cpu.step(bus),
        }
    }

    fn register_count(&self) -> usize {
        match self {
            #[cfg(feature = "arch-8086")]
            AnyCpu::I8086(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-x86")]
            AnyCpu::X86(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-x86_64")]
            AnyCpu::X86_64(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-arm32")]
            AnyCpu::Arm32(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-arm64")]
            AnyCpu::Arm64(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-riscv")]
            AnyCpu::RiscV(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-ia64")]
            AnyCpu::Ia64(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-mips")]
            AnyCpu::Mips(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-powerpc")]
            AnyCpu::PowerPc(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-sparc")]
            AnyCpu::Sparc(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-avr")]
            AnyCpu::Avr(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-superh")]
            AnyCpu::SuperH(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-parisc")]
            AnyCpu::PaRisc(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-alpha")]
            AnyCpu::DecAlpha(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-m68k")]
            AnyCpu::Motorola68000(cpu) => cpu.register_count(),
            #[cfg(feature = "arch-6502")]
            AnyCpu::Mos6502(cpu) => cpu.register_count(),
        }
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match self {
            #[cfg(feature = "arch-8086")]
            AnyCpu::I8086(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-x86")]
            AnyCpu::X86(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-x86_64")]
            AnyCpu::X86_64(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-arm32")]
            AnyCpu::Arm32(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-arm64")]
            AnyCpu::Arm64(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-riscv")]
            AnyCpu::RiscV(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-ia64")]
            AnyCpu::Ia64(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-mips")]
            AnyCpu::Mips(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-powerpc")]
            AnyCpu::PowerPc(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-sparc")]
            AnyCpu::Sparc(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-avr")]
            AnyCpu::Avr(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-superh")]
            AnyCpu::SuperH(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-parisc")]
            AnyCpu::PaRisc(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-alpha")]
            AnyCpu::DecAlpha(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-m68k")]
            AnyCpu::Motorola68000(cpu) => cpu.register_info(index),
            #[cfg(feature = "arch-6502")]
            AnyCpu::Mos6502(cpu) => cpu.register_info(index),
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        match self {
            #[cfg(feature = "arch-8086")]
            AnyCpu::I8086(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-x86")]
            AnyCpu::X86(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-x86_64")]
            AnyCpu::X86_64(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-arm32")]
            AnyCpu::Arm32(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-arm64")]
            AnyCpu::Arm64(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-riscv")]
            AnyCpu::RiscV(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-ia64")]
            AnyCpu::Ia64(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-mips")]
            AnyCpu::Mips(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-powerpc")]
            AnyCpu::PowerPc(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-sparc")]
            AnyCpu::Sparc(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-avr")]
            AnyCpu::Avr(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-superh")]
            AnyCpu::SuperH(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-parisc")]
            AnyCpu::PaRisc(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-alpha")]
            AnyCpu::DecAlpha(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-m68k")]
            AnyCpu::Motorola68000(cpu) => cpu.get_register(name),
            #[cfg(feature = "arch-6502")]
            AnyCpu::Mos6502(cpu) => cpu.get_register(name),
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        match self {
            #[cfg(feature = "arch-8086")]
            AnyCpu::I8086(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-x86")]
            AnyCpu::X86(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-x86_64")]
            AnyCpu::X86_64(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-arm32")]
            AnyCpu::Arm32(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-arm64")]
            AnyCpu::Arm64(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-riscv")]
            AnyCpu::RiscV(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-ia64")]
            AnyCpu::Ia64(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-mips")]
            AnyCpu::Mips(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-powerpc")]
            AnyCpu::PowerPc(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-sparc")]
            AnyCpu::Sparc(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-avr")]
            AnyCpu::Avr(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-superh")]
            AnyCpu::SuperH(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-parisc")]
            AnyCpu::PaRisc(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-alpha")]
            AnyCpu::DecAlpha(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-m68k")]
            AnyCpu::Motorola68000(cpu) => cpu.set_register(name, val),
            #[cfg(feature = "arch-6502")]
            AnyCpu::Mos6502(cpu) => cpu.set_register(name, val),
        }
    }
}
