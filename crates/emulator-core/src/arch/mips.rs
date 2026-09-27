//! MIPS (MIPS32 / MIPS I-IV) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// MIPS register state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct MipsState {
    pub r: [u32; 32], // r0 is zero, r29=sp, r31=ra
    pub pc: u32,
    pub hi: u32,
    pub lo: u32,
    pub halted: bool,
}

/// MIPS CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MipsCpu {
    pub state: MipsState,
}

impl MipsCpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    pub fn push_u32(&mut self, bus: &mut dyn MemoryBus, val: u32) -> Result<(), CpuError> {
        self.state.r[29] = self.state.r[29].wrapping_sub(4);
        bus.write_u32(self.state.r[29] as u64, val, Endianness::BigEndian)?;
        Ok(())
    }

    pub fn pop_u32(&mut self, bus: &mut dyn MemoryBus) -> Result<u32, CpuError> {
        let val = bus.read_u32(self.state.r[29] as u64, Endianness::BigEndian)?;
        self.state.r[29] = self.state.r[29].wrapping_add(4);
        Ok(val)
    }
}

impl CpuEngine for MipsCpu {
    fn arch(&self) -> Architecture {
        Architecture::Mips
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
        self.state.r[29] as u64
    }

    fn set_sp(&mut self, val: u64) {
        self.state.r[29] = (val & 0xFFFF_FFFF) as u32;
    }

    fn reset(&mut self) {
        self.state = MipsState {
            pc: 0xBFC0_0000,
            ..Default::default()
        };
        self.state.r[29] = 0x7FFF_0000; // SP ($sp)
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let pc = self.pc();
        let instr = bus.read_u32(pc, Endianness::BigEndian)?;
        self.state.pc = self.state.pc.wrapping_add(4);

        match instr {
            0x00000000 => {
                // NOP (sll $0, $0, 0)
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x0000000D => {
                // BREAK
                Ok(StepOutcome::Breakpoint)
            }
            0x0000000C => {
                // SYSCALL
                Ok(StepOutcome::Interrupt(0))
            }
            0x42000020 => {
                // WAIT (Halt/Sleep)
                self.state.halted = true;
                Ok(StepOutcome::Halted)
            }
            _ => {
                let opcode = (instr >> 26) & 0x3F;
                let rs = ((instr >> 21) & 0x1F) as usize;
                let rt = ((instr >> 16) & 0x1F) as usize;
                let rd = ((instr >> 11) & 0x1F) as usize;
                let sa = ((instr >> 6) & 0x1F) as u32;
                let funct = instr & 0x3F;
                let imm_signed = (instr as i16) as i32 as u32;
                let imm_unsigned = (instr & 0xFFFF) as u32;

                match opcode {
                    0x00 => {
                        // SPECIAL / R-type
                        match funct {
                            0x20 | 0x21 => {
                                // ADD / ADDU
                                if rd != 0 {
                                    self.state.r[rd] = self.state.r[rs].wrapping_add(self.state.r[rt]);
                                }
                            }
                            0x22 | 0x23 => {
                                // SUB / SUBU
                                if rd != 0 {
                                    self.state.r[rd] = self.state.r[rs].wrapping_sub(self.state.r[rt]);
                                }
                            }
                            0x24 => {
                                // AND
                                if rd != 0 {
                                    self.state.r[rd] = self.state.r[rs] & self.state.r[rt];
                                }
                            }
                            0x25 => {
                                // OR
                                if rd != 0 {
                                    self.state.r[rd] = self.state.r[rs] | self.state.r[rt];
                                }
                            }
                            0x26 => {
                                // XOR
                                if rd != 0 {
                                    self.state.r[rd] = self.state.r[rs] ^ self.state.r[rt];
                                }
                            }
                            0x27 => {
                                // NOR
                                if rd != 0 {
                                    self.state.r[rd] = !(self.state.r[rs] | self.state.r[rt]);
                                }
                            }
                            0x2A => {
                                // SLT
                                if rd != 0 {
                                    self.state.r[rd] = if (self.state.r[rs] as i32) < (self.state.r[rt] as i32) { 1 } else { 0 };
                                }
                            }
                            0x2B => {
                                // SLTU
                                if rd != 0 {
                                    self.state.r[rd] = if self.state.r[rs] < self.state.r[rt] { 1 } else { 0 };
                                }
                            }
                            0x08 => {
                                // JR rs
                                self.state.pc = self.state.r[rs];
                            }
                            0x09 => {
                                // JALR rd, rs
                                if rd != 0 {
                                    self.state.r[rd] = self.state.pc;
                                } else {
                                    self.state.r[31] = self.state.pc;
                                }
                                self.state.pc = self.state.r[rs];
                            }
                            0x00 => {
                                // SLL
                                if rd != 0 {
                                    self.state.r[rd] = self.state.r[rt].wrapping_shl(sa);
                                }
                            }
                            0x02 => {
                                // SRL
                                if rd != 0 {
                                    self.state.r[rd] = self.state.r[rt].wrapping_shr(sa);
                                }
                            }
                            0x03 => {
                                // SRA
                                if rd != 0 {
                                    self.state.r[rd] = ((self.state.r[rt] as i32) >> sa) as u32;
                                }
                            }
                            _ => {
                                return Err(CpuError::InvalidInstruction { opcode: instr as u64, pc });
                            }
                        }
                        self.state.r[0] = 0;
                        Ok(StepOutcome::Continue { cycles: 1 })
                    }
                    0x08 | 0x09 => {
                        // ADDI / ADDIU
                        if rt != 0 {
                            self.state.r[rt] = self.state.r[rs].wrapping_add(imm_signed);
                        }
                        self.state.r[0] = 0;
                        Ok(StepOutcome::Continue { cycles: 1 })
                    }
                    0x0C => {
                        // ANDI
                        if rt != 0 {
                            self.state.r[rt] = self.state.r[rs] & imm_unsigned;
                        }
                        self.state.r[0] = 0;
                        Ok(StepOutcome::Continue { cycles: 1 })
                    }
                    0x0D => {
                        // ORI
                        if rt != 0 {
                            self.state.r[rt] = self.state.r[rs] | imm_unsigned;
                        }
                        self.state.r[0] = 0;
                        Ok(StepOutcome::Continue { cycles: 1 })
                    }
                    0x0E => {
                        // XORI
                        if rt != 0 {
                            self.state.r[rt] = self.state.r[rs] ^ imm_unsigned;
                        }
                        self.state.r[0] = 0;
                        Ok(StepOutcome::Continue { cycles: 1 })
                    }
                    0x0F => {
                        // LUI
                        if rt != 0 {
                            self.state.r[rt] = imm_unsigned << 16;
                        }
                        self.state.r[0] = 0;
                        Ok(StepOutcome::Continue { cycles: 1 })
                    }
                    0x23 => {
                        // LW rt, offset(rs)
                        let addr = self.state.r[rs].wrapping_add(imm_signed) as u64;
                        let val = bus.read_u32(addr, Endianness::BigEndian)?;
                        if rt != 0 {
                            self.state.r[rt] = val;
                        }
                        self.state.r[0] = 0;
                        Ok(StepOutcome::Continue { cycles: 2 })
                    }
                    0x2B => {
                        // SW rt, offset(rs)
                        let addr = self.state.r[rs].wrapping_add(imm_signed) as u64;
                        let val = self.state.r[rt];
                        bus.write_u32(addr, val, Endianness::BigEndian)?;
                        Ok(StepOutcome::Continue { cycles: 2 })
                    }
                    0x02 => {
                        // J target
                        let target = (instr & 0x03FF_FFFF) << 2;
                        self.state.pc = (self.state.pc & 0xF000_0000) | target;
                        Ok(StepOutcome::Continue { cycles: 2 })
                    }
                    0x03 => {
                        // JAL target
                        let target = (instr & 0x03FF_FFFF) << 2;
                        self.state.r[31] = self.state.pc;
                        self.state.pc = (self.state.pc & 0xF000_0000) | target;
                        Ok(StepOutcome::Continue { cycles: 2 })
                    }
                    0x04 => {
                        // BEQ rs, rt, offset
                        if self.state.r[rs] == self.state.r[rt] {
                            let offset = ((imm_signed as i32) << 2) as u32;
                            self.state.pc = self.state.pc.wrapping_add(offset);
                        }
                        Ok(StepOutcome::Continue { cycles: 2 })
                    }
                    0x05 => {
                        // BNE rs, rt, offset
                        if self.state.r[rs] != self.state.r[rt] {
                            let offset = ((imm_signed as i32) << 2) as u32;
                            self.state.pc = self.state.pc.wrapping_add(offset);
                        }
                        Ok(StepOutcome::Continue { cycles: 2 })
                    }
                    _ => Err(CpuError::InvalidInstruction {
                        opcode: instr as u64,
                        pc,
                    }),
                }
            }
        }
    }

    fn register_count(&self) -> usize {
        35
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0..=31 => {
                const MIPS_NAMES: [&str; 32] = [
                    "$zero", "$at", "$v0", "$v1", "$a0", "$a1", "$a2", "$a3", "$t0", "$t1", "$t2",
                    "$t3", "$t4", "$t5", "$t6", "$t7", "$s0", "$s1", "$s2", "$s3", "$s4", "$s5",
                    "$s6", "$s7", "$t8", "$t9", "$k0", "$k1", "$gp", "$sp", "$fp", "$ra",
                ];
                Some(RegisterInfo {
                    name: MIPS_NAMES[index],
                    value: RegisterValue::U32(self.state.r[index]),
                    is_pc: false,
                    is_sp: index == 29,
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
                name: "hi",
                value: RegisterValue::U32(self.state.hi),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            34 => Some(RegisterInfo {
                name: "lo",
                value: RegisterValue::U32(self.state.lo),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        const MIPS_NAMES: [&str; 32] = [
            "$zero", "$at", "$v0", "$v1", "$a0", "$a1", "$a2", "$a3", "$t0", "$t1", "$t2",
            "$t3", "$t4", "$t5", "$t6", "$t7", "$s0", "$s1", "$s2", "$s3", "$s4", "$s5",
            "$s6", "$s7", "$t8", "$t9", "$k0", "$k1", "$gp", "$sp", "$fp", "$ra",
        ];
        for (i, &n) in MIPS_NAMES.iter().enumerate() {
            if name.eq_ignore_ascii_case(n) || name.eq_ignore_ascii_case(&n[1..]) {
                return Some(self.state.r[i] as u64);
            }
        }
        if let Some(num_str) = name
            .strip_prefix("$r")
            .or_else(|| name.strip_prefix('r'))
            .or_else(|| name.strip_prefix('$'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 32 {
                    return Some(self.state.r[idx] as u64);
                }
        match name {
            s if s.eq_ignore_ascii_case("pc") => Some(self.state.pc as u64),
            s if s.eq_ignore_ascii_case("hi") => Some(self.state.hi as u64),
            s if s.eq_ignore_ascii_case("lo") => Some(self.state.lo as u64),
            _ => None,
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        let v = (val & 0xFFFF_FFFF) as u32;
        const MIPS_NAMES: [&str; 32] = [
            "$zero", "$at", "$v0", "$v1", "$a0", "$a1", "$a2", "$a3", "$t0", "$t1", "$t2",
            "$t3", "$t4", "$t5", "$t6", "$t7", "$s0", "$s1", "$s2", "$s3", "$s4", "$s5",
            "$s6", "$s7", "$t8", "$t9", "$k0", "$k1", "$gp", "$sp", "$fp", "$ra",
        ];
        for (i, &n) in MIPS_NAMES.iter().enumerate() {
            if name.eq_ignore_ascii_case(n) || name.eq_ignore_ascii_case(&n[1..]) {
                if i != 0 {
                    self.state.r[i] = v;
                }
                return Ok(());
            }
        }
        if let Some(num_str) = name
            .strip_prefix("$r")
            .or_else(|| name.strip_prefix('r'))
            .or_else(|| name.strip_prefix('$'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 32 {
                    if idx != 0 {
                        self.state.r[idx] = v;
                    }
                    return Ok(());
                }
        match name {
            s if s.eq_ignore_ascii_case("pc") => self.state.pc = v,
            s if s.eq_ignore_ascii_case("hi") => self.state.hi = v,
            s if s.eq_ignore_ascii_case("lo") => self.state.lo = v,
            _ => return Err(CpuError::RegisterNotFound),
        }
        self.state.r[0] = 0;
        Ok(())
    }
}
