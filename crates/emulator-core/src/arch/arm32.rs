//! ARMv7 / AArch32 (32-bit ARM) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// ARM32 register state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Arm32State {
    pub r: [u32; 13], // R0 - R12
    pub sp: u32,      // R13
    pub lr: u32,      // R14
    pub pc: u32,      // R15
    pub cpsr: u32,    // Current Program Status Register
    pub halted: bool,
}

/// ARM32 CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Arm32Cpu {
    pub state: Arm32State,
}

impl Arm32Cpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    pub fn get_reg(&self, reg: usize) -> u32 {
        match reg {
            0..=12 => self.state.r[reg],
            13 => self.state.sp,
            14 => self.state.lr,
            15 => self.state.pc,
            _ => 0,
        }
    }

    pub fn set_reg(&mut self, reg: usize, val: u32) {
        match reg {
            0..=12 => self.state.r[reg] = val,
            13 => self.state.sp = val,
            14 => self.state.lr = val,
            15 => self.state.pc = val,
            _ => {}
        }
    }

    pub fn push_u32(&mut self, bus: &mut dyn MemoryBus, val: u32) -> Result<(), CpuError> {
        self.state.sp = self.state.sp.wrapping_sub(4);
        bus.write_u32(self.state.sp as u64, val, Endianness::LittleEndian)?;
        Ok(())
    }

    pub fn pop_u32(&mut self, bus: &mut dyn MemoryBus) -> Result<u32, CpuError> {
        let val = bus.read_u32(self.state.sp as u64, Endianness::LittleEndian)?;
        self.state.sp = self.state.sp.wrapping_add(4);
        Ok(val)
    }
}

impl CpuEngine for Arm32Cpu {
    fn arch(&self) -> Architecture {
        Architecture::Arm32
    }

    fn endianness(&self) -> Endianness {
        Endianness::LittleEndian
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
        self.state.sp as u64
    }

    fn set_sp(&mut self, val: u64) {
        self.state.sp = (val & 0xFFFF_FFFF) as u32;
    }

    fn reset(&mut self) {
        self.state = Arm32State {
            pc: 0x0000_0000,
            sp: 0x8000_0000,
            cpsr: 0x0000_01D3, // Supervisor mode, IRQ/FIQ disabled
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
            0xE1A00000 => {
                // NOP (mov r0, r0)
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0xE320F003 => {
                // WFI (Wait for Interrupt / Halt)
                self.state.halted = true;
                Ok(StepOutcome::Halted)
            }
            0xE1200070 => {
                // BKPT 0
                Ok(StepOutcome::Breakpoint)
            }
            _ => {
                // BX Rm (0xE12FFF1m)
                if (instr & 0xFFFFFFF0) == 0xE12FFF10 {
                    let rm = (instr & 0x0F) as usize;
                    self.state.pc = self.get_reg(rm);
                    return Ok(StepOutcome::Continue { cycles: 3 });
                }

                // B / BL (0xEAxxxxxx / 0xEBxxxxxx)
                if (instr & 0xFE000000) == 0xEA000000 {
                    let is_bl = (instr & 0x01000000) != 0;
                    let imm24 = instr & 0x00FFFFFF;
                    let sign_ext = if imm24 & 0x00800000 != 0 {
                        imm24 | 0xFF000000
                    } else {
                        imm24
                    };
                    let offset = (sign_ext as i32) << 2;
                    if is_bl {
                        self.state.lr = self.state.pc;
                    }
                    self.state.pc = ((self.state.pc as i32).wrapping_add(offset)) as u32;
                    return Ok(StepOutcome::Continue { cycles: 3 });
                }

                // Data processing (0xE0xxxxxx .. 0xE3xxxxxx)
                if (instr & 0xFC000000) == 0xE0000000 {
                    let is_imm = (instr & 0x02000000) != 0;
                    let op = (instr >> 21) & 0x0F;
                    let _s_bit = (instr & 0x00100000) != 0;
                    let rn = ((instr >> 16) & 0x0F) as usize;
                    let rd = ((instr >> 12) & 0x0F) as usize;
                    let op2 = if is_imm {
                        let imm8 = instr & 0xFF;
                        let rot = ((instr >> 8) & 0x0F) * 2;
                        imm8.rotate_right(rot)
                    } else {
                        let rm = (instr & 0x0F) as usize;
                        self.get_reg(rm)
                    };
                    let rn_val = self.get_reg(rn);

                    match op {
                        0x0 => { // AND
                            let res = rn_val & op2;
                            self.set_reg(rd, res);
                        }
                        0x1 => { // EOR
                            let res = rn_val ^ op2;
                            self.set_reg(rd, res);
                        }
                        0x2 => { // SUB
                            let res = rn_val.wrapping_sub(op2);
                            self.set_reg(rd, res);
                        }
                        0x4 => { // ADD
                            let res = rn_val.wrapping_add(op2);
                            self.set_reg(rd, res);
                        }
                        0xA => { // CMP
                            let res = rn_val.wrapping_sub(op2);
                            if res == 0 { self.state.cpsr |= 0x4000_0000; } else { self.state.cpsr &= !0x4000_0000; }
                            if (res as i32) < 0 { self.state.cpsr |= 0x8000_0000; } else { self.state.cpsr &= !0x8000_0000; }
                        }
                        0xC => { // ORR
                            let res = rn_val | op2;
                            self.set_reg(rd, res);
                        }
                        0xD => { // MOV
                            self.set_reg(rd, op2);
                        }
                        0xE => { // BIC
                            let res = rn_val & !op2;
                            self.set_reg(rd, res);
                        }
                        0xF => { // MVN
                            let res = !op2;
                            self.set_reg(rd, res);
                        }
                        _ => {}
                    }
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // LDR / STR (0xE4xxxxxx .. 0xE5xxxxxx)
                if (instr & 0xFC000000) == 0xE4000000 || (instr & 0xFC000000) == 0xE5000000 {
                    let is_load = (instr & 0x00100000) != 0;
                    let is_up = (instr & 0x00800000) != 0;
                    let rn = ((instr >> 16) & 0x0F) as usize;
                    let rd = ((instr >> 12) & 0x0F) as usize;
                    let offset = instr & 0xFFF;
                    let addr = if is_up {
                        self.get_reg(rn).wrapping_add(offset)
                    } else {
                        self.get_reg(rn).wrapping_sub(offset)
                    };
                    if is_load {
                        let val = bus.read_u32(addr as u64, Endianness::LittleEndian)?;
                        self.set_reg(rd, val);
                    } else {
                        let val = self.get_reg(rd);
                        bus.write_u32(addr as u64, val, Endianness::LittleEndian)?;
                    }
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
        17
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0..=12 => {
                const REG_NAMES: [&str; 13] = [
                    "R0", "R1", "R2", "R3", "R4", "R5", "R6", "R7", "R8", "R9", "R10", "R11", "R12",
                ];
                Some(RegisterInfo {
                    name: REG_NAMES[index],
                    value: RegisterValue::U32(self.state.r[index]),
                    is_pc: false,
                    is_sp: false,
                    is_flags: false,
                })
            }
            13 => Some(RegisterInfo {
                name: "SP",
                value: RegisterValue::U32(self.state.sp),
                is_pc: false,
                is_sp: true,
                is_flags: false,
            }),
            14 => Some(RegisterInfo {
                name: "LR",
                value: RegisterValue::U32(self.state.lr),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            15 => Some(RegisterInfo {
                name: "PC",
                value: RegisterValue::U32(self.state.pc),
                is_pc: true,
                is_sp: false,
                is_flags: false,
            }),
            16 => Some(RegisterInfo {
                name: "CPSR",
                value: RegisterValue::U32(self.state.cpsr),
                is_pc: false,
                is_sp: false,
                is_flags: true,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        if let Some(num_str) = name.strip_prefix('r').or_else(|| name.strip_prefix('R'))
            && let Ok(idx) = num_str.parse::<usize>() {
                if idx < 13 {
                    return Some(self.state.r[idx] as u64);
                } else if idx == 13 {
                    return Some(self.state.sp as u64);
                } else if idx == 14 {
                    return Some(self.state.lr as u64);
                } else if idx == 15 {
                    return Some(self.state.pc as u64);
                }
            }
        match name {
            s if s.eq_ignore_ascii_case("SP") => Some(self.state.sp as u64),
            s if s.eq_ignore_ascii_case("LR") => Some(self.state.lr as u64),
            s if s.eq_ignore_ascii_case("PC") => Some(self.state.pc as u64),
            s if s.eq_ignore_ascii_case("CPSR") => Some(self.state.cpsr as u64),
            _ => None,
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        let v = (val & 0xFFFF_FFFF) as u32;
        if let Some(num_str) = name.strip_prefix('r').or_else(|| name.strip_prefix('R'))
            && let Ok(idx) = num_str.parse::<usize>() {
                if idx < 13 {
                    self.state.r[idx] = v;
                    return Ok(());
                } else if idx == 13 {
                    self.state.sp = v;
                    return Ok(());
                } else if idx == 14 {
                    self.state.lr = v;
                    return Ok(());
                } else if idx == 15 {
                    self.state.pc = v;
                    return Ok(());
                }
            }
        match name {
            s if s.eq_ignore_ascii_case("SP") => self.state.sp = v,
            s if s.eq_ignore_ascii_case("LR") => self.state.lr = v,
            s if s.eq_ignore_ascii_case("PC") => self.state.pc = v,
            s if s.eq_ignore_ascii_case("CPSR") => self.state.cpsr = v,
            _ => return Err(CpuError::RegisterNotFound),
        }
        Ok(())
    }
}
