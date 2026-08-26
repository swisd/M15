//! SPARC (SPARC V8 / V9) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// SPARC register window state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SparcState {
    pub g: [u32; 8], // %g0 is zero
    pub o: [u32; 8], // %o6 is %sp, %o7 is call return
    pub l: [u32; 8], // %l0 - %l7
    pub i: [u32; 8], // %i6 is %fp, %i7 is return PC
    pub pc: u32,
    pub npc: u32,
    pub psr: u32, // Processor State Register
    pub cwp: u8,  // Current Window Pointer
    pub halted: bool,
}

/// SPARC CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SparcCpu {
    pub state: SparcState,
}

impl SparcCpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    pub fn push_u32(&mut self, bus: &mut dyn MemoryBus, val: u32) -> Result<(), CpuError> {
        self.state.o[6] = self.state.o[6].wrapping_sub(4);
        bus.write_u32(self.state.o[6] as u64, val, Endianness::BigEndian)?;
        Ok(())
    }

    pub fn pop_u32(&mut self, bus: &mut dyn MemoryBus) -> Result<u32, CpuError> {
        let val = bus.read_u32(self.state.o[6] as u64, Endianness::BigEndian)?;
        self.state.o[6] = self.state.o[6].wrapping_add(4);
        Ok(val)
    }
}

impl CpuEngine for SparcCpu {
    fn arch(&self) -> Architecture {
        Architecture::Sparc
    }

    fn endianness(&self) -> Endianness {
        Endianness::BigEndian
    }

    fn stack_growth(&self) -> StackGrowth {
        StackGrowth::Downwards
    }

    fn word_size(&self) -> WordSize {
        WordSize::Bytes4
    }

    fn pc(&self) -> u64 {
        self.state.pc as u64
    }

    fn set_pc(&mut self, val: u64) {
        self.state.pc = (val & 0xFFFF_FFFF) as u32;
        self.state.npc = self.state.pc.wrapping_add(4);
    }

    fn sp(&self) -> u64 {
        self.state.o[6] as u64
    }

    fn set_sp(&mut self, val: u64) {
        self.state.o[6] = (val & 0xFFFF_FFFF) as u32;
    }

    fn reset(&mut self) {
        self.state = SparcState {
            pc: 0x0000_0000,
            npc: 0x0000_0004,
            psr: 0x0000_00E0,
            cwp: 0,
            ..Default::default()
        };
        self.state.o[6] = 0x7FFF_0000; // %sp (%o6)
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let pc = self.pc();
        let instr = bus.read_u32(pc, Endianness::BigEndian)?;
        self.state.pc = self.state.npc;
        self.state.npc = self.state.npc.wrapping_add(4);

        match instr {
            0x01000000 => {
                // NOP (sethi 0, %g0)
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x91D02000 => {
                // TA 0 (Trap Always 0 / Breakpoint)
                Ok(StepOutcome::Breakpoint)
            }
            0x00000000 => {
                // Unimp / Halt
                self.state.halted = true;
                Ok(StepOutcome::Halted)
            }
            _ => {
                // General instruction or basic branch handling
                self.state.g[0] = 0;
                Ok(StepOutcome::Continue { cycles: 1 })
            }
        }
    }

    fn register_count(&self) -> usize {
        36
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0..=7 => {
                const G_NAMES: [&str; 8] = [
                    "%g0", "%g1", "%g2", "%g3", "%g4", "%g5", "%g6", "%g7",
                ];
                Some(RegisterInfo {
                    name: G_NAMES[index],
                    value: RegisterValue::U32(self.state.g[index]),
                    is_pc: false,
                    is_sp: false,
                    is_flags: false,
                })
            }
            8..=15 => {
                const O_NAMES: [&str; 8] = [
                    "%o0", "%o1", "%o2", "%o3", "%o4", "%o5", "%o6(sp)", "%o7",
                ];
                Some(RegisterInfo {
                    name: O_NAMES[index - 8],
                    value: RegisterValue::U32(self.state.o[index - 8]),
                    is_pc: false,
                    is_sp: index == 14,
                    is_flags: false,
                })
            }
            16..=23 => {
                const L_NAMES: [&str; 8] = [
                    "%l0", "%l1", "%l2", "%l3", "%l4", "%l5", "%l6", "%l7",
                ];
                Some(RegisterInfo {
                    name: L_NAMES[index - 16],
                    value: RegisterValue::U32(self.state.l[index - 16]),
                    is_pc: false,
                    is_sp: false,
                    is_flags: false,
                })
            }
            24..=31 => {
                const I_NAMES: [&str; 8] = [
                    "%i0", "%i1", "%i2", "%i3", "%i4", "%i5", "%i6(fp)", "%i7",
                ];
                Some(RegisterInfo {
                    name: I_NAMES[index - 24],
                    value: RegisterValue::U32(self.state.i[index - 24]),
                    is_pc: false,
                    is_sp: false,
                    is_flags: false,
                })
            }
            32 => Some(RegisterInfo {
                name: "pc",
                value: RegisterValue::U32(self.state.pc),
                is_pc: true,
                is_sp: false,
                is_flags: false,
            }),
            33 => Some(RegisterInfo {
                name: "npc",
                value: RegisterValue::U32(self.state.npc),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            34 => Some(RegisterInfo {
                name: "psr",
                value: RegisterValue::U32(self.state.psr),
                is_pc: false,
                is_sp: false,
                is_flags: true,
            }),
            35 => Some(RegisterInfo {
                name: "cwp",
                value: RegisterValue::U8(self.state.cwp),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        let clean = name.trim_start_matches('%');
        if let Some(num_str) = clean.strip_prefix('g').or_else(|| clean.strip_prefix('G'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    return Some(self.state.g[idx] as u64);
                }
        if let Some(num_str) = clean.strip_prefix('o').or_else(|| clean.strip_prefix('O'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    return Some(self.state.o[idx] as u64);
                }
        if let Some(num_str) = clean.strip_prefix('l').or_else(|| clean.strip_prefix('L'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    return Some(self.state.l[idx] as u64);
                }
        if let Some(num_str) = clean.strip_prefix('i').or_else(|| clean.strip_prefix('I'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    return Some(self.state.i[idx] as u64);
                }
        match clean {
            s if s.eq_ignore_ascii_case("sp") => Some(self.state.o[6] as u64),
            s if s.eq_ignore_ascii_case("fp") => Some(self.state.i[6] as u64),
            s if s.eq_ignore_ascii_case("pc") => Some(self.state.pc as u64),
            s if s.eq_ignore_ascii_case("npc") => Some(self.state.npc as u64),
            s if s.eq_ignore_ascii_case("psr") => Some(self.state.psr as u64),
            s if s.eq_ignore_ascii_case("cwp") => Some(self.state.cwp as u64),
            _ => None,
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        let clean = name.trim_start_matches('%');
        let v = (val & 0xFFFF_FFFF) as u32;
        if let Some(num_str) = clean.strip_prefix('g').or_else(|| clean.strip_prefix('G'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    if idx != 0 {
                        self.state.g[idx] = v;
                    }
                    return Ok(());
                }
        if let Some(num_str) = clean.strip_prefix('o').or_else(|| clean.strip_prefix('O'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    self.state.o[idx] = v;
                    return Ok(());
                }
        if let Some(num_str) = clean.strip_prefix('l').or_else(|| clean.strip_prefix('L'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    self.state.l[idx] = v;
                    return Ok(());
                }
        if let Some(num_str) = clean.strip_prefix('i').or_else(|| clean.strip_prefix('I'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    self.state.i[idx] = v;
                    return Ok(());
                }
        match clean {
            s if s.eq_ignore_ascii_case("sp") => self.state.o[6] = v,
            s if s.eq_ignore_ascii_case("fp") => self.state.i[6] = v,
            s if s.eq_ignore_ascii_case("pc") => self.set_pc(val),
            s if s.eq_ignore_ascii_case("npc") => self.state.npc = v,
            s if s.eq_ignore_ascii_case("psr") => self.state.psr = v,
            s if s.eq_ignore_ascii_case("cwp") => self.state.cwp = val as u8,
            _ => return Err(CpuError::RegisterNotFound),
        }
        self.state.g[0] = 0;
        Ok(())
    }
}
