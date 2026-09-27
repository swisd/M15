//! HP PA-RISC (1.1 / 2.0) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// PA-RISC register state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct PaRiscState {
    pub gr: [u32; 32], // gr0=0, gr2=rp, gr30=sp
    pub sr: [u32; 8],  // Space registers sr0-sr7
    pub pc: u32,
    pub psw: u32, // Processor Status Word
    pub halted: bool,
}

/// PA-RISC CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PaRiscCpu {
    pub state: PaRiscState,
}

impl PaRiscCpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    /// Pushes a word onto the stack. Note: PA-RISC stack grows **UPWARDS**!
    pub fn push_u32(&mut self, bus: &mut dyn MemoryBus, val: u32) -> Result<(), CpuError> {
        bus.write_u32(self.state.gr[30] as u64, val, Endianness::BigEndian)?;
        self.state.gr[30] = self.state.gr[30].wrapping_add(4);
        Ok(())
    }

    /// Pops a word from the stack.
    pub fn pop_u32(&mut self, bus: &mut dyn MemoryBus) -> Result<u32, CpuError> {
        self.state.gr[30] = self.state.gr[30].wrapping_sub(4);
        let val = bus.read_u32(self.state.gr[30] as u64, Endianness::BigEndian)?;
        Ok(val)
    }
}

impl CpuEngine for PaRiscCpu {
    fn arch(&self) -> Architecture {
        Architecture::PaRisc
    }

    fn endianness(&self) -> Endianness {
        Endianness::BigEndian
    }

    fn stack_growth(&self) -> StackGrowth {
        StackGrowth::Upwards
    }

    fn word_size(&self) -> WordSize {
        WordSize::Bytes4
    }

    fn pc(&self) -> u64 {
        self.state.pc as u64
    }

    fn set_pc(&mut self, val: u64) {
        self.state.pc = (val & 0xFFFF_FFFF) as u32;
    }

    fn sp(&self) -> u64 {
        self.state.gr[30] as u64
    }

    fn set_sp(&mut self, val: u64) {
        self.state.gr[30] = (val & 0xFFFF_FFFF) as u32;
    }

    fn reset(&mut self) {
        self.state = PaRiscState {
            pc: 0x0000_0100,
            psw: 0x0000_000B,
            ..Default::default()
        };
        self.state.gr[30] = 0x1000_0000; // SP (gr30) starts at lower boundary and grows up
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let pc = self.pc();
        let instr = bus.read_u32(pc, Endianness::BigEndian)?;
        self.state.pc = self.state.pc.wrapping_add(4);

        match instr {
            0x08000240 => {
                // NOP (or 0, 0, 0)
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x00000000 => {
                // BREAK 0,0 (Breakpoint)
                Ok(StepOutcome::Breakpoint)
            }
            0xFFFFFFFF => {
                // Halt
                self.state.halted = true;
                Ok(StepOutcome::Halted)
            }
            _ => {
                let op = (instr >> 26) & 0x3F;

                match op {
                    0x0D => {
                        // LDO imm(r1), r2
                        let r1 = ((instr >> 21) & 0x1F) as usize;
                        let r2 = ((instr >> 16) & 0x1F) as usize;
                        let imm14 = (instr as i16) as i32 as u32;
                        let r1_val = if r1 == 0 { 0 } else { self.state.gr[r1] };
                        if r2 != 0 {
                            self.state.gr[r2] = r1_val.wrapping_add(imm14);
                        }
                        self.state.gr[0] = 0;
                        Ok(StepOutcome::Continue { cycles: 1 })
                    }
                    0x02 => {
                        let ext = (instr >> 5) & 0xFF;
                        let r1 = ((instr >> 21) & 0x1F) as usize;
                        let r2 = ((instr >> 16) & 0x1F) as usize;
                        let t = (instr & 0x1F) as usize;
                        let r1_val = self.state.gr[r1];
                        let r2_val = self.state.gr[r2];

                        if ext == 0x30 {
                            // ADD r1, r2, t
                            if t != 0 {
                                self.state.gr[t] = r1_val.wrapping_add(r2_val);
                            }
                        } else if ext == 0x20 {
                            // SUB r1, r2, t
                            if t != 0 {
                                self.state.gr[t] = r1_val.wrapping_sub(r2_val);
                            }
                        }
                        self.state.gr[0] = 0;
                        Ok(StepOutcome::Continue { cycles: 1 })
                    }
                    0x3A => {
                        // B,L / BV (Branch)
                        let w = instr & 0x001FFFFF;
                        let sign_ext = if (w & 0x00100000) != 0 {
                            w | 0xFFE00000
                        } else {
                            w
                        };
                        let offset = ((sign_ext as i32) << 2) as u32;
                        self.state.pc = self.state.pc.wrapping_add(offset);
                        self.state.gr[0] = 0;
                        Ok(StepOutcome::Continue { cycles: 2 })
                    }
                    _ => {
                        self.state.gr[0] = 0;
                        Ok(StepOutcome::Continue { cycles: 1 })
                    }
                }
            }
        }
    }

    fn register_count(&self) -> usize {
        42
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0..=31 => {
                const GR_NAMES: [&str; 32] = [
                    "gr0", "gr1", "gr2(rp)", "gr3", "gr4", "gr5", "gr6", "gr7", "gr8", "gr9",
                    "gr10", "gr11", "gr12", "gr13", "gr14", "gr15", "gr16", "gr17", "gr18", "gr19",
                    "gr20", "gr21", "gr22", "gr23", "gr24", "gr25", "gr26", "gr27(dp)", "gr28",
                    "gr29", "gr30(sp)", "gr31",
                ];
                Some(RegisterInfo {
                    name: GR_NAMES[index],
                    value: RegisterValue::U32(self.state.gr[index]),
                    is_pc: false,
                    is_sp: index == 30,
                    is_flags: false,
                })
            }
            32..=39 => {
                const SR_NAMES: [&str; 8] =
                    ["sr0", "sr1", "sr2", "sr3", "sr4", "sr5", "sr6", "sr7"];
                Some(RegisterInfo {
                    name: SR_NAMES[index - 32],
                    value: RegisterValue::U32(self.state.sr[index - 32]),
                    is_pc: false,
                    is_sp: false,
                    is_flags: false,
                })
            }
            40 => Some(RegisterInfo {
                name: "pc",
                value: RegisterValue::U32(self.state.pc),
                is_pc: true,
                is_sp: false,
                is_flags: false,
            }),
            41 => Some(RegisterInfo {
                name: "psw",
                value: RegisterValue::U32(self.state.psw),
                is_pc: false,
                is_sp: false,
                is_flags: true,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        if let Some(num_str) = name
            .strip_prefix("gr")
            .or_else(|| name.strip_prefix("GR"))
            .or_else(|| name.strip_prefix('r'))
            .or_else(|| name.strip_prefix('R'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 32 {
                    return Some(self.state.gr[idx] as u64);
                }
        if let Some(num_str) = name.strip_prefix("sr").or_else(|| name.strip_prefix("SR"))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    return Some(self.state.sr[idx] as u64);
                }
        match name {
            s if s.eq_ignore_ascii_case("sp") => Some(self.state.gr[30] as u64),
            s if s.eq_ignore_ascii_case("rp") => Some(self.state.gr[2] as u64),
            s if s.eq_ignore_ascii_case("dp") => Some(self.state.gr[27] as u64),
            s if s.eq_ignore_ascii_case("pc") => Some(self.state.pc as u64),
            s if s.eq_ignore_ascii_case("psw") => Some(self.state.psw as u64),
            _ => None,
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        let v = (val & 0xFFFF_FFFF) as u32;
        if let Some(num_str) = name
            .strip_prefix("gr")
            .or_else(|| name.strip_prefix("GR"))
            .or_else(|| name.strip_prefix('r'))
            .or_else(|| name.strip_prefix('R'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 32 {
                    if idx != 0 {
                        self.state.gr[idx] = v;
                    }
                    return Ok(());
                }
        if let Some(num_str) = name.strip_prefix("sr").or_else(|| name.strip_prefix("SR"))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    self.state.sr[idx] = v;
                    return Ok(());
                }
        match name {
            s if s.eq_ignore_ascii_case("sp") => self.state.gr[30] = v,
            s if s.eq_ignore_ascii_case("rp") => self.state.gr[2] = v,
            s if s.eq_ignore_ascii_case("dp") => self.state.gr[27] = v,
            s if s.eq_ignore_ascii_case("pc") => self.state.pc = v,
            s if s.eq_ignore_ascii_case("psw") => self.state.psw = v,
            _ => return Err(CpuError::RegisterNotFound),
        }
        self.state.gr[0] = 0;
        Ok(())
    }
}
