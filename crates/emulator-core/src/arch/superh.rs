//! Renesas / Hitachi SuperH (SH-2 / SH-4) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// SuperH register state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct SuperHState {
    pub r: [u32; 16], // R0-R15 (R15 is SP)
    pub pc: u32,
    pub pr: u32,   // Procedure Register (Return PC)
    pub gbr: u32,  // Global Base Register
    pub vbr: u32,  // Vector Base Register
    pub mach: u32, // Multiply-accumulate High
    pub macl: u32, // Multiply-accumulate Low
    pub sr: u32,   // Status Register (with T-bit at bit 0)
    pub halted: bool,
}

/// SuperH CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SuperHCpu {
    pub state: SuperHState,
}

impl SuperHCpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    pub fn push_u32(&mut self, bus: &mut dyn MemoryBus, val: u32) -> Result<(), CpuError> {
        self.state.r[15] = self.state.r[15].wrapping_sub(4);
        bus.write_u32(self.state.r[15] as u64, val, Endianness::BigEndian)?;
        Ok(())
    }

    pub fn pop_u32(&mut self, bus: &mut dyn MemoryBus) -> Result<u32, CpuError> {
        let val = bus.read_u32(self.state.r[15] as u64, Endianness::BigEndian)?;
        self.state.r[15] = self.state.r[15].wrapping_add(4);
        Ok(val)
    }
}

impl CpuEngine for SuperHCpu {
    fn arch(&self) -> Architecture {
        Architecture::SuperH
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
    }

    fn sp(&self) -> u64 {
        self.state.r[15] as u64
    }

    fn set_sp(&mut self, val: u64) {
        self.state.r[15] = (val & 0xFFFF_FFFF) as u32;
    }

    fn reset(&mut self) {
        self.state = SuperHState {
            pc: 0xA000_0000,
            sr: 0x7000_00F0,
            ..Default::default()
        };
        self.state.r[15] = 0x8C00_0000; // SP (R15)
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let pc = self.pc();
        let instr = bus.read_u16(pc, Endianness::BigEndian)?;
        self.state.pc = self.state.pc.wrapping_add(2);

        match instr {
            0x0009 => {
                // NOP
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x000B => {
                // RTS (Return from Subroutine)
                self.state.pc = self.state.pr;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x001B => {
                // SLEEP (Halt)
                self.state.halted = true;
                Ok(StepOutcome::Halted)
            }
            0x003B => {
                // TRAPA (Breakpoint / Trap)
                Ok(StepOutcome::Breakpoint)
            }
            _ => {
                // TRAPA #imm (0xC3xx)
                if (instr & 0xFF00) == 0xC300 {
                    let imm = (instr & 0xFF) as u32;
                    return Ok(StepOutcome::Interrupt(imm));
                }

                // MOV #imm8, Rn (0xEnyy)
                if (instr & 0xF000) == 0xE000 {
                    let rn = ((instr >> 8) & 0x0F) as usize;
                    let imm8 = (instr & 0xFF) as i8 as i32 as u32;
                    self.state.r[rn] = imm8;
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // MOV Rm, Rn (0x6nm3)
                if (instr & 0xF00F) == 0x6003 {
                    let rn = ((instr >> 8) & 0x0F) as usize;
                    let rm = ((instr >> 4) & 0x0F) as usize;
                    self.state.r[rn] = self.state.r[rm];
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // ADD #imm8, Rn (0x7nyy)
                if (instr & 0xF000) == 0x7000 {
                    let rn = ((instr >> 8) & 0x0F) as usize;
                    let imm8 = (instr & 0xFF) as i8 as i32 as u32;
                    self.state.r[rn] = self.state.r[rn].wrapping_add(imm8);
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // ADD Rm, Rn (0x3nmC)
                if (instr & 0xF00F) == 0x300C {
                    let rn = ((instr >> 8) & 0x0F) as usize;
                    let rm = ((instr >> 4) & 0x0F) as usize;
                    self.state.r[rn] = self.state.r[rn].wrapping_add(self.state.r[rm]);
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // SUB Rm, Rn (0x3nm8)
                if (instr & 0xF00F) == 0x3008 {
                    let rn = ((instr >> 8) & 0x0F) as usize;
                    let rm = ((instr >> 4) & 0x0F) as usize;
                    self.state.r[rn] = self.state.r[rn].wrapping_sub(self.state.r[rm]);
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // BRA disp12 (0xAxxx)
                if (instr & 0xF000) == 0xA000 {
                    let d12 = instr & 0x0FFF;
                    let sign_ext = if d12 & 0x0800 != 0 {
                        d12 | 0xF000
                    } else {
                        d12
                    };
                    let offset = ((sign_ext as i16) as i32) * 2 + 2;
                    self.state.pc = ((self.state.pc as i32).wrapping_add(offset)) as u32;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }

                // BSR disp12 (0xBxxx)
                if (instr & 0xF000) == 0xB000 {
                    let d12 = instr & 0x0FFF;
                    let sign_ext = if d12 & 0x0800 != 0 {
                        d12 | 0xF000
                    } else {
                        d12
                    };
                    self.state.pr = self.state.pc.wrapping_add(2);
                    let offset = ((sign_ext as i16) as i32) * 2 + 2;
                    self.state.pc = ((self.state.pc as i32).wrapping_add(offset)) as u32;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }

                Err(CpuError::InvalidInstruction {
                    opcode: instr as u64,
                    pc,
                })
            }
        }
    }

    fn register_count(&self) -> usize {
        23
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0..=15 => {
                const SH_NAMES: [&str; 16] = [
                    "R0", "R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8", "R9", "R10", "R11",
                    "R12", "R13", "R14", "R15(SP)",
                ];
                Some(RegisterInfo {
                    name: SH_NAMES[index],
                    value: RegisterValue::U32(self.state.r[index]),
                    is_pc: false,
                    is_sp: index == 15,
                    is_flags: false,
                })
            }
            16 => Some(RegisterInfo {
                name: "PC",
                value: RegisterValue::U32(self.state.pc),
                is_pc: true,
                is_sp: false,
                is_flags: false,
            }),
            17 => Some(RegisterInfo {
                name: "PR",
                value: RegisterValue::U32(self.state.pr),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            18 => Some(RegisterInfo {
                name: "GBR",
                value: RegisterValue::U32(self.state.gbr),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            19 => Some(RegisterInfo {
                name: "VBR",
                value: RegisterValue::U32(self.state.vbr),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            20 => Some(RegisterInfo {
                name: "MACH",
                value: RegisterValue::U32(self.state.mach),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            21 => Some(RegisterInfo {
                name: "MACL",
                value: RegisterValue::U32(self.state.macl),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            22 => Some(RegisterInfo {
                name: "SR",
                value: RegisterValue::U32(self.state.sr),
                is_pc: false,
                is_sp: false,
                is_flags: true,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        if let Some(num_str) = name.strip_prefix('r').or_else(|| name.strip_prefix('R'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 16 {
                    return Some(self.state.r[idx] as u64);
                }
        match name {
            s if s.eq_ignore_ascii_case("sp") => Some(self.state.r[15] as u64),
            s if s.eq_ignore_ascii_case("pc") => Some(self.state.pc as u64),
            s if s.eq_ignore_ascii_case("pr") => Some(self.state.pr as u64),
            s if s.eq_ignore_ascii_case("gbr") => Some(self.state.gbr as u64),
            s if s.eq_ignore_ascii_case("vbr") => Some(self.state.vbr as u64),
            s if s.eq_ignore_ascii_case("mach") => Some(self.state.mach as u64),
            s if s.eq_ignore_ascii_case("macl") => Some(self.state.macl as u64),
            s if s.eq_ignore_ascii_case("sr") => Some(self.state.sr as u64),
            _ => None,
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        let v = (val & 0xFFFF_FFFF) as u32;
        if let Some(num_str) = name.strip_prefix('r').or_else(|| name.strip_prefix('R'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 16 {
                    self.state.r[idx] = v;
                    return Ok(());
                }
        match name {
            s if s.eq_ignore_ascii_case("sp") => self.state.r[15] = v,
            s if s.eq_ignore_ascii_case("pc") => self.state.pc = v,
            s if s.eq_ignore_ascii_case("pr") => self.state.pr = v,
            s if s.eq_ignore_ascii_case("gbr") => self.state.gbr = v,
            s if s.eq_ignore_ascii_case("vbr") => self.state.vbr = v,
            s if s.eq_ignore_ascii_case("mach") => self.state.mach = v,
            s if s.eq_ignore_ascii_case("macl") => self.state.macl = v,
            s if s.eq_ignore_ascii_case("sr") => self.state.sr = v,
            _ => return Err(CpuError::RegisterNotFound),
        }
        Ok(())
    }
}
