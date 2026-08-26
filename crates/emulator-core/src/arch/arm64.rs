//! ARMv8 / AArch64 (64-bit ARM) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// ARM64 register state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Arm64State {
    pub x: [u64; 31], // X0 - X30 (X30 is Link Register)
    pub sp: u64,
    pub pc: u64,
    pub pstate: u64, // NZCV and execution state
    pub halted: bool,
}

/// ARM64 CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Arm64Cpu {
    pub state: Arm64State,
}

impl Arm64Cpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    pub fn push_u64(&mut self, bus: &mut dyn MemoryBus, val: u64) -> Result<(), CpuError> {
        self.state.sp = self.state.sp.wrapping_sub(8);
        bus.write_u64(self.state.sp, val, Endianness::LittleEndian)?;
        Ok(())
    }

    pub fn pop_u64(&mut self, bus: &mut dyn MemoryBus) -> Result<u64, CpuError> {
        let val = bus.read_u64(self.state.sp, Endianness::LittleEndian)?;
        self.state.sp = self.state.sp.wrapping_add(8);
        Ok(val)
    }
}

impl CpuEngine for Arm64Cpu {
    fn arch(&self) -> Architecture {
        Architecture::Arm64
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
        self.state.sp
    }

    fn set_sp(&mut self, val: u64) {
        self.state.sp = val;
    }

    fn reset(&mut self) {
        self.state = Arm64State {
            pc: 0x0000_0000_0000_0000,
            sp: 0x0000_7FFF_FFFF_0000,
            pstate: 0x0000_0000_0000_03C5, // EL1h, all DAIF masked
            ..Default::default()
        };
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let pc = self.pc();
        let instr = bus.read_u32(pc, Endianness::LittleEndian)?;
        self.state.pc = self.state.pc.wrapping_add(4);

        match instr {
            0xD503201F => {
                // NOP
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0xD503207F => {
                // WFI (Halt/Wait)
                self.state.halted = true;
                Ok(StepOutcome::Halted)
            }
            0xD65F03C0 => {
                // RET (Branch to LR/X30)
                self.state.pc = self.state.x[30];
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xD4200000 => {
                // BRK 0
                Ok(StepOutcome::Breakpoint)
            }
            _ => {
                // Check for B imm26 (0x14000000)
                if (instr & 0xFC000000) == 0x14000000 {
                    let imm26 = instr & 0x03FFFFFF;
                    let sign_ext = if imm26 & 0x02000000 != 0 {
                        imm26 | 0xFC000000
                    } else {
                        imm26
                    };
                    let offset = ((sign_ext as i32) as i64) << 2;
                    self.state.pc = ((self.state.pc as i64).wrapping_add(offset)) as u64;
                    Ok(StepOutcome::Continue { cycles: 2 })
                } else {
                    Err(CpuError::InvalidInstruction {
                        opcode: instr as u64,
                        pc,
                    })
                }
            }
        }
    }

    fn register_count(&self) -> usize {
        34
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0..=30 => {
                const REG_NAMES: [&str; 31] = [
                    "X0", "X1", "X2", "X3", "X4", "X5", "X6", "X7", "X8", "X9", "X10", "X11",
                    "X12", "X13", "X14", "X15", "X16", "X17", "X18", "X19", "X20", "X21",
                    "X22", "X23", "X24", "X25", "X26", "X27", "X28", "X29", "X30",
                ];
                Some(RegisterInfo {
                    name: REG_NAMES[index],
                    value: RegisterValue::U64(self.state.x[index]),
                    is_pc: false,
                    is_sp: false,
                    is_flags: false,
                })
            }
            31 => Some(RegisterInfo {
                name: "SP",
                value: RegisterValue::U64(self.state.sp),
                is_pc: false,
                is_sp: true,
                is_flags: false,
            }),
            32 => Some(RegisterInfo {
                name: "PC",
                value: RegisterValue::U64(self.state.pc),
                is_pc: true,
                is_sp: false,
                is_flags: false,
            }),
            33 => Some(RegisterInfo {
                name: "PSTATE",
                value: RegisterValue::U64(self.state.pstate),
                is_pc: false,
                is_sp: false,
                is_flags: true,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        if let Some(num_str) = name.strip_prefix('x').or_else(|| name.strip_prefix('X'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 31 {
                    return Some(self.state.x[idx]);
                }
        match name {
            s if s.eq_ignore_ascii_case("SP") => Some(self.state.sp),
            s if s.eq_ignore_ascii_case("PC") => Some(self.state.pc),
            s if s.eq_ignore_ascii_case("LR") => Some(self.state.x[30]),
            s if s.eq_ignore_ascii_case("FP") => Some(self.state.x[29]),
            s if s.eq_ignore_ascii_case("PSTATE") || s.eq_ignore_ascii_case("NZCV") => {
                Some(self.state.pstate)
            }
            _ => None,
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        if let Some(num_str) = name.strip_prefix('x').or_else(|| name.strip_prefix('X'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 31 {
                    self.state.x[idx] = val;
                    return Ok(());
                }
        match name {
            s if s.eq_ignore_ascii_case("SP") => self.state.sp = val,
            s if s.eq_ignore_ascii_case("PC") => self.state.pc = val,
            s if s.eq_ignore_ascii_case("LR") => self.state.x[30] = val,
            s if s.eq_ignore_ascii_case("FP") => self.state.x[29] = val,
            s if s.eq_ignore_ascii_case("PSTATE") || s.eq_ignore_ascii_case("NZCV") => {
                self.state.pstate = val
            }
            _ => return Err(CpuError::RegisterNotFound),
        }
        Ok(())
    }
}
