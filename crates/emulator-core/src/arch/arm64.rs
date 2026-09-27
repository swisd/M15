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

    pub fn get_x(&self, reg: usize) -> u64 {
        if reg < 31 {
            self.state.x[reg]
        } else {
            0
        }
    }

    pub fn set_x(&mut self, reg: usize, val: u64) {
        if reg < 31 {
            self.state.x[reg] = val;
        }
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
            _ => {
                // RET Rn (0xD65F0000 | (rn << 5))
                if (instr & 0xFFFFFC1F) == 0xD65F0000 {
                    let rn = ((instr >> 5) & 0x1F) as usize;
                    self.state.pc = self.get_x(rn);
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }

                // BRK (0xD4200000..=0xD43FFFFF)
                if (instr & 0xFFE00000) == 0xD4200000 {
                    return Ok(StepOutcome::Breakpoint);
                }

                // SVC (0xD4000001..=0xD401FFFF)
                if (instr & 0xFFE0001F) == 0xD4000001 {
                    let imm16 = (instr >> 5) & 0xFFFF;
                    return Ok(StepOutcome::Interrupt(imm16));
                }

                // B / BL (0x14000000 / 0x94000000)
                if (instr & 0x7C000000) == 0x14000000 {
                    let is_bl = (instr & 0x80000000) != 0;
                    let imm26 = instr & 0x03FFFFFF;
                    let sign_ext = if imm26 & 0x02000000 != 0 {
                        imm26 | 0xFC000000
                    } else {
                        imm26
                    };
                    let offset = ((sign_ext as i32) as i64) << 2;
                    if is_bl {
                        self.state.x[30] = self.state.pc;
                    }
                    self.state.pc = ((self.state.pc as i64).wrapping_add(offset)) as u64;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }

                // MOVZ Xd, #imm16, LSL #hw (0xD2800000..=0xD29FFFFF)
                if (instr & 0xFF800000) == 0xD2800000 {
                    let hw = (((instr >> 21) & 3) * 16) as u32;
                    let imm16 = ((instr >> 5) & 0xFFFF) as u64;
                    let rd = (instr & 0x1F) as usize;
                    self.set_x(rd, imm16 << hw);
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // ADD Xd, Xn, Xm (0x8B000000..=0x8B1FFFFF)
                if (instr & 0xFF200000) == 0x8B000000 {
                    let rm = ((instr >> 16) & 0x1F) as usize;
                    let rn = ((instr >> 5) & 0x1F) as usize;
                    let rd = (instr & 0x1F) as usize;
                    self.set_x(rd, self.get_x(rn).wrapping_add(self.get_x(rm)));
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // ADD Xd, Xn, #imm12 (0x91000000..=0x913FFFFF)
                if (instr & 0xFF800000) == 0x91000000 {
                    let imm12 = ((instr >> 10) & 0xFFF) as u64;
                    let rn = ((instr >> 5) & 0x1F) as usize;
                    let rd = (instr & 0x1F) as usize;
                    let rn_val = if rn == 31 { self.state.sp } else { self.get_x(rn) };
                    let res = rn_val.wrapping_add(imm12);
                    if rd == 31 { self.state.sp = res; } else { self.set_x(rd, res); }
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // SUB Xd, Xn, Xm (0xCB000000..=0xCB1FFFFF)
                if (instr & 0xFF200000) == 0xCB000000 {
                    let rm = ((instr >> 16) & 0x1F) as usize;
                    let rn = ((instr >> 5) & 0x1F) as usize;
                    let rd = (instr & 0x1F) as usize;
                    self.set_x(rd, self.get_x(rn).wrapping_sub(self.get_x(rm)));
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // SUB Xd, Xn, #imm12 (0xD1000000..=0xD13FFFFF)
                if (instr & 0xFF800000) == 0xD1000000 {
                    let imm12 = ((instr >> 10) & 0xFFF) as u64;
                    let rn = ((instr >> 5) & 0x1F) as usize;
                    let rd = (instr & 0x1F) as usize;
                    let rn_val = if rn == 31 { self.state.sp } else { self.get_x(rn) };
                    let res = rn_val.wrapping_sub(imm12);
                    if rd == 31 { self.state.sp = res; } else { self.set_x(rd, res); }
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // LDR Xd, [Xn, #pimm] (0xF9400000..=0xF97FFFFF)
                if (instr & 0xFFC00000) == 0xF9400000 {
                    let pimm = (((instr >> 10) & 0xFFF) as u64) * 8;
                    let rn = ((instr >> 5) & 0x1F) as usize;
                    let rd = (instr & 0x1F) as usize;
                    let addr = (if rn == 31 { self.state.sp } else { self.get_x(rn) }).wrapping_add(pimm);
                    let val = bus.read_u64(addr, Endianness::LittleEndian)?;
                    self.set_x(rd, val);
                    return Ok(StepOutcome::Continue { cycles: 3 });
                }

                // STR Xd, [Xn, #pimm] (0xF9000000..=0xF93FFFFF)
                if (instr & 0xFFC00000) == 0xF9000000 {
                    let pimm = (((instr >> 10) & 0xFFF) as u64) * 8;
                    let rn = ((instr >> 5) & 0x1F) as usize;
                    let rd = (instr & 0x1F) as usize;
                    let addr = (if rn == 31 { self.state.sp } else { self.get_x(rn) }).wrapping_add(pimm);
                    let val = self.get_x(rd);
                    bus.write_u64(addr, val, Endianness::LittleEndian)?;
                    return Ok(StepOutcome::Continue { cycles: 3 });
                }

                Err(CpuError::InvalidInstruction {
                    opcode: instr as u64,
                    pc,
                })
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
