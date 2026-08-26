//! DEC Alpha (Alpha AXP / 64-bit RISC) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// DEC Alpha register state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct AlphaState {
    pub r: [u64; 32], // r0-r31 (r30 is SP, r31 is hardwired zero, r26 is RA)
    pub pc: u64,
    pub fpcr: u64, // Floating-point Control Register
    pub halted: bool,
}

/// DEC Alpha CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AlphaCpu {
    pub state: AlphaState,
}

impl AlphaCpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    pub fn push_u64(&mut self, bus: &mut dyn MemoryBus, val: u64) -> Result<(), CpuError> {
        self.state.r[30] = self.state.r[30].wrapping_sub(8);
        bus.write_u64(self.state.r[30], val, Endianness::LittleEndian)?;
        Ok(())
    }

    pub fn pop_u64(&mut self, bus: &mut dyn MemoryBus) -> Result<u64, CpuError> {
        let val = bus.read_u64(self.state.r[30], Endianness::LittleEndian)?;
        self.state.r[30] = self.state.r[30].wrapping_add(8);
        Ok(val)
    }
}

impl CpuEngine for AlphaCpu {
    fn arch(&self) -> Architecture {
        Architecture::DecAlpha
    }

    fn endianness(&self) -> Endianness {
        Endianness::LittleEndian
    }

    fn stack_growth(&self) -> StackGrowth {
        StackGrowth::Downwards
    }

    fn word_size(&self) -> WordSize {
        WordSize::Bytes8
    }

    fn pc(&self) -> u64 {
        self.state.pc
    }

    fn set_pc(&mut self, val: u64) {
        self.state.pc = val;
    }

    fn sp(&self) -> u64 {
        self.state.r[30]
    }

    fn set_sp(&mut self, val: u64) {
        self.state.r[30] = val;
    }

    fn reset(&mut self) {
        self.state = AlphaState {
            pc: 0x0000_0000_2000_0000,
            ..Default::default()
        };
        self.state.r[30] = 0x0000_01FF_FFFF_0000; // SP ($30)
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let pc = self.pc();
        let instr = bus.read_u32(pc, Endianness::LittleEndian)?;
        self.state.pc = self.state.pc.wrapping_add(4);

        match instr {
            0x47FF041F => {
                // NOP (bis 31, 31, 31 / or r31, r31, r31)
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x00000000 => {
                // HALT (PALcode HALT)
                self.state.halted = true;
                Ok(StepOutcome::Halted)
            }
            0x00000080 => {
                // BPT (Breakpoint / PALcode BPT)
                Ok(StepOutcome::Breakpoint)
            }
            0x00000083 => {
                // CALLSYS (System Call)
                Ok(StepOutcome::Interrupt(0))
            }
            _ => {
                self.state.r[31] = 0;
                Ok(StepOutcome::Continue { cycles: 1 })
            }
        }
    }

    fn register_count(&self) -> usize {
        34
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0..=31 => {
                const ALPHA_NAMES: [&str; 32] = [
                    "v0", "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7", "s0", "s1", "s2", "s3",
                    "s4", "s5", "fp/s6", "a0", "a1", "a2", "a3", "a4", "a5", "t8", "t9", "t10",
                    "t11", "ra", "pv/t12", "at", "gp", "sp", "zero",
                ];
                Some(RegisterInfo {
                    name: ALPHA_NAMES[index],
                    value: RegisterValue::U64(self.state.r[index]),
                    is_pc: false,
                    is_sp: index == 30,
                    is_flags: false,
                })
            }
            32 => Some(RegisterInfo {
                name: "pc",
                value: RegisterValue::U64(self.state.pc),
                is_pc: true,
                is_sp: false,
                is_flags: false,
            }),
            33 => Some(RegisterInfo {
                name: "fpcr",
                value: RegisterValue::U64(self.state.fpcr),
                is_pc: false,
                is_sp: false,
                is_flags: true,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        let clean = name.trim_start_matches('$');
        if let Some(num_str) = clean.strip_prefix('r').or_else(|| clean.strip_prefix('R'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 32 {
                    return Some(self.state.r[idx]);
                }
        match clean {
            s if s.eq_ignore_ascii_case("zero") || s.eq_ignore_ascii_case("r31") => Some(0),
            s if s.eq_ignore_ascii_case("sp") || s.eq_ignore_ascii_case("r30") => {
                Some(self.state.r[30])
            }
            s if s.eq_ignore_ascii_case("gp") || s.eq_ignore_ascii_case("r29") => {
                Some(self.state.r[29])
            }
            s if s.eq_ignore_ascii_case("ra") || s.eq_ignore_ascii_case("r26") => {
                Some(self.state.r[26])
            }
            s if s.eq_ignore_ascii_case("pc") => Some(self.state.pc),
            s if s.eq_ignore_ascii_case("fpcr") => Some(self.state.fpcr),
            _ => None,
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        let clean = name.trim_start_matches('$');
        if let Some(num_str) = clean.strip_prefix('r').or_else(|| clean.strip_prefix('R'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 32 {
                    if idx != 31 {
                        self.state.r[idx] = val;
                    }
                    return Ok(());
                }
        match clean {
            s if s.eq_ignore_ascii_case("zero") || s.eq_ignore_ascii_case("r31") => {}
            s if s.eq_ignore_ascii_case("sp") || s.eq_ignore_ascii_case("r30") => {
                self.state.r[30] = val
            }
            s if s.eq_ignore_ascii_case("gp") || s.eq_ignore_ascii_case("r29") => {
                self.state.r[29] = val
            }
            s if s.eq_ignore_ascii_case("ra") || s.eq_ignore_ascii_case("r26") => {
                self.state.r[26] = val
            }
            s if s.eq_ignore_ascii_case("pc") => self.state.pc = val,
            s if s.eq_ignore_ascii_case("fpcr") => self.state.fpcr = val,
            _ => return Err(CpuError::RegisterNotFound),
        }
        self.state.r[31] = 0;
        Ok(())
    }
}
