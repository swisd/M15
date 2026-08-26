//! RISC-V (RV32 / RV64) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// RISC-V register state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct RiscvState {
    pub x: [u64; 32], // x0 is hardwired zero, x1=ra, x2=sp, x8=s0/fp
    pub pc: u64,
    pub is_64bit: bool,
    pub halted: bool,
}

/// RISC-V CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RiscvCpu {
    pub state: RiscvState,
}

impl RiscvCpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    pub fn new_64() -> Self {
        let mut cpu = Self {
            state: RiscvState {
                is_64bit: true,
                ..Default::default()
            },
        };
        cpu.reset();
        cpu
    }

    pub fn push(&mut self, bus: &mut dyn MemoryBus, val: u64) -> Result<(), CpuError> {
        let size = if self.state.is_64bit { 8 } else { 4 };
        self.state.x[2] = self.state.x[2].wrapping_sub(size);
        let sp = self.state.x[2];
        if self.state.is_64bit {
            bus.write_u64(sp, val, Endianness::LittleEndian)?;
        } else {
            bus.write_u32(sp, val as u32, Endianness::LittleEndian)?;
        }
        Ok(())
    }

    pub fn pop(&mut self, bus: &mut dyn MemoryBus) -> Result<u64, CpuError> {
        let size = if self.state.is_64bit { 8 } else { 4 };
        let sp = self.state.x[2];
        let val = if self.state.is_64bit {
            bus.read_u64(sp, Endianness::LittleEndian)?
        } else {
            bus.read_u32(sp, Endianness::LittleEndian)? as u64
        };
        self.state.x[2] = self.state.x[2].wrapping_add(size);
        Ok(val)
    }
}

impl CpuEngine for RiscvCpu {
    fn arch(&self) -> Architecture {
        Architecture::RiscV
    }

    fn endianness(&self) -> Endianness {
        Endianness::LittleEndian
    }

    fn stack_growth(&self) -> StackGrowth {
        StackGrowth::Downwards
    }

    fn word_size(&self) -> WordSize {
        if self.state.is_64bit {
            WordSize::Bytes8
        } else {
            WordSize::Bytes4
        }
    }

    fn pc(&self) -> u64 {
        self.state.pc
    }

    fn set_pc(&mut self, val: u64) {
        self.state.pc = val;
    }

    fn sp(&self) -> u64 {
        self.state.x[2]
    }

    fn set_sp(&mut self, val: u64) {
        self.state.x[2] = val;
    }

    fn reset(&mut self) {
        let is_64 = self.state.is_64bit;
        self.state = RiscvState {
            pc: 0x0000_1000,
            x: [0; 32],
            is_64bit: is_64,
            halted: false,
        };
        self.state.x[2] = if is_64 {
            0x0000_7FFF_FFFF_0000
        } else {
            0x7FFF_0000
        };
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let pc = self.pc();
        let instr = bus.read_u32(pc, Endianness::LittleEndian)?;
        let next_pc = pc.wrapping_add(4);
        self.state.pc = next_pc;

        let opcode = instr & 0x7F;
        let rd = ((instr >> 7) & 0x1F) as usize;
        let funct3 = (instr >> 12) & 0x7;
        let rs1 = ((instr >> 15) & 0x1F) as usize;
        let rs2 = ((instr >> 20) & 0x1F) as usize;
        let funct7 = (instr >> 25) & 0x7F;

        match opcode {
            0x37 => {
                // LUI rd, imm20
                let imm = (instr & 0xFFFF_F000) as i32;
                if rd != 0 {
                    self.state.x[rd] = if self.state.is_64bit {
                        imm as i64 as u64
                    } else {
                        (imm as u32) as u64
                    };
                }
                self.state.x[0] = 0;
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x17 => {
                // AUIPC rd, imm20
                let imm = (instr & 0xFFFF_F000) as i32;
                let target = (pc as i64).wrapping_add(imm as i64);
                if rd != 0 {
                    self.state.x[rd] = if self.state.is_64bit {
                        target as u64
                    } else {
                        (target as u32) as u64
                    };
                }
                self.state.x[0] = 0;
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x6F => {
                // JAL rd, imm
                let imm20 = (instr >> 31) & 0x1;
                let imm10_1 = (instr >> 21) & 0x3FF;
                let imm11 = (instr >> 20) & 0x1;
                let imm19_12 = (instr >> 12) & 0xFF;
                let imm = (imm20 << 20) | (imm19_12 << 12) | (imm11 << 11) | (imm10_1 << 1);
                let sign_ext = if imm20 != 0 {
                    (imm | 0xFFE00000) as i32
                } else {
                    imm as i32
                };

                if rd != 0 {
                    self.state.x[rd] = if self.state.is_64bit {
                        next_pc
                    } else {
                        (next_pc as u32) as u64
                    };
                }
                self.state.pc = ((pc as i64).wrapping_add(sign_ext as i64)) as u64;
                self.state.x[0] = 0;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x67 => {
                // JALR rd, rs1, imm
                let imm = ((instr as i32) >> 20) as i64;
                let target = ((self.state.x[rs1] as i64).wrapping_add(imm) as u64) & !1;
                if rd != 0 {
                    self.state.x[rd] = if self.state.is_64bit {
                        next_pc
                    } else {
                        (next_pc as u32) as u64
                    };
                }
                self.state.pc = target;
                self.state.x[0] = 0;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x63 => {
                // BRANCH (BEQ, BNE, BLT, BGE, BLTU, BGEU)
                let imm12 = (instr >> 31) & 0x1;
                let imm10_5 = (instr >> 25) & 0x3F;
                let imm4_1 = (instr >> 8) & 0xF;
                let imm11 = (instr >> 7) & 0x1;
                let imm = (imm12 << 12) | (imm11 << 11) | (imm10_5 << 5) | (imm4_1 << 1);
                let sign_ext = if imm12 != 0 {
                    (imm | 0xFFFFE000) as i32
                } else {
                    imm as i32
                };

                let v1 = self.state.x[rs1];
                let v2 = self.state.x[rs2];
                let take_branch = match funct3 {
                    0 => v1 == v2,                                         // BEQ
                    1 => v1 != v2,                                         // BNE
                    4 => (v1 as i64) < (v2 as i64),                        // BLT
                    5 => (v1 as i64) >= (v2 as i64),                       // BGE
                    6 => v1 < v2,                                          // BLTU
                    7 => v1 >= v2,                                         // BGEU
                    _ => false,
                };

                if take_branch {
                    self.state.pc = ((pc as i64).wrapping_add(sign_ext as i64)) as u64;
                }
                self.state.x[0] = 0;
                Ok(StepOutcome::Continue {
                    cycles: if take_branch { 2 } else { 1 },
                })
            }
            0x03 => {
                // LOAD (LB, LH, LW, LBU, LHU, LWU, LD)
                let imm = ((instr as i32) >> 20) as i64;
                let addr = (self.state.x[rs1] as i64).wrapping_add(imm) as u64;

                let val = match funct3 {
                    0 => (bus.read_u8(addr)? as i8) as i64 as u64, // LB
                    1 => (bus.read_u16(addr, Endianness::LittleEndian)? as i16) as i64 as u64, // LH
                    2 => (bus.read_u32(addr, Endianness::LittleEndian)? as i32) as i64 as u64, // LW
                    4 => bus.read_u8(addr)? as u64,                // LBU
                    5 => bus.read_u16(addr, Endianness::LittleEndian)? as u64, // LHU
                    6 if self.state.is_64bit => {
                        bus.read_u32(addr, Endianness::LittleEndian)? as u64 // LWU
                    }
                    3 if self.state.is_64bit => {
                        bus.read_u64(addr, Endianness::LittleEndian)? // LD
                    }
                    _ => {
                        return Err(CpuError::InvalidInstruction {
                            opcode: instr as u64,
                            pc,
                        })
                    }
                };

                if rd != 0 {
                    self.state.x[rd] = if self.state.is_64bit {
                        val
                    } else {
                        (val as u32) as u64
                    };
                }
                self.state.x[0] = 0;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x23 => {
                // STORE (SB, SH, SW, SD)
                let imm11_5 = (instr >> 25) & 0x7F;
                let imm4_0 = (instr >> 7) & 0x1F;
                let imm = (imm11_5 << 5) | imm4_0;
                let sign_ext = if (imm & 0x800) != 0 {
                    (imm | 0xFFFFF000) as i32
                } else {
                    imm as i32
                };
                let addr = (self.state.x[rs1] as i64).wrapping_add(sign_ext as i64) as u64;
                let val = self.state.x[rs2];

                match funct3 {
                    0 => bus.write_u8(addr, val as u8)?, // SB
                    1 => bus.write_u16(addr, val as u16, Endianness::LittleEndian)?, // SH
                    2 => bus.write_u32(addr, val as u32, Endianness::LittleEndian)?, // SW
                    3 if self.state.is_64bit => {
                        bus.write_u64(addr, val, Endianness::LittleEndian)? // SD
                    }
                    _ => {
                        return Err(CpuError::InvalidInstruction {
                            opcode: instr as u64,
                            pc,
                        })
                    }
                }

                self.state.x[0] = 0;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x13 => {
                // OP-IMM (ADDI, SLTI, SLTIU, XORI, ORI, ANDI, SLLI, SRLI, SRAI)
                let imm = ((instr as i32) >> 20) as i64;
                let v1 = self.state.x[rs1];

                let res = match funct3 {
                    0 => (v1 as i64).wrapping_add(imm) as u64, // ADDI
                    1 => {
                        // SLLI
                        let shamt = (imm & 0x3F) as u32;
                        v1 << shamt
                    }
                    2 => {
                        // SLTI
                        if (v1 as i64) < imm {
                            1
                        } else {
                            0
                        }
                    }
                    3 => {
                        // SLTIU
                        if v1 < (imm as u64) {
                            1
                        } else {
                            0
                        }
                    }
                    4 => v1 ^ (imm as u64), // XORI
                    5 => {
                        let shamt = (imm & 0x3F) as u32;
                        if (funct7 & 0x20) != 0 {
                            // SRAI
                            ((v1 as i64) >> shamt) as u64
                        } else {
                            // SRLI
                            v1 >> shamt
                        }
                    }
                    6 => v1 | (imm as u64), // ORI
                    7 => v1 & (imm as u64), // ANDI
                    _ => {
                        return Err(CpuError::InvalidInstruction {
                            opcode: instr as u64,
                            pc,
                        })
                    }
                };

                if rd != 0 {
                    self.state.x[rd] = if self.state.is_64bit {
                        res
                    } else {
                        (res as u32) as u64
                    };
                }
                self.state.x[0] = 0;
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x33 => {
                // OP (ADD, SUB, SLL, SLT, SLTU, XOR, SRL, SRA, OR, AND)
                let v1 = self.state.x[rs1];
                let v2 = self.state.x[rs2];

                let res = match (funct7, funct3) {
                    (0x00, 0) => v1.wrapping_add(v2),                       // ADD
                    (0x20, 0) => v1.wrapping_sub(v2),                       // SUB
                    (0x00, 1) => v1 << (v2 & 0x3F),                        // SLL
                    (0x00, 2) => if (v1 as i64) < (v2 as i64) { 1 } else { 0 }, // SLT
                    (0x00, 3) => if v1 < v2 { 1 } else { 0 },               // SLTU
                    (0x00, 4) => v1 ^ v2,                                  // XOR
                    (0x00, 5) => v1 >> (v2 & 0x3F),                        // SRL
                    (0x20, 5) => ((v1 as i64) >> (v2 & 0x3F)) as u64,       // SRA
                    (0x00, 6) => v1 | v2,                                  // OR
                    (0x00, 7) => v1 & v2,                                  // AND
                    _ => {
                        return Err(CpuError::InvalidInstruction {
                            opcode: instr as u64,
                            pc,
                        })
                    }
                };

                if rd != 0 {
                    self.state.x[rd] = if self.state.is_64bit {
                        res
                    } else {
                        (res as u32) as u64
                    };
                }
                self.state.x[0] = 0;
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x0F => {
                // FENCE
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x73 => {
                // SYSTEM
                match instr {
                    0x00000073 => Ok(StepOutcome::Interrupt(0)), // ECALL
                    0x00100073 => Ok(StepOutcome::Breakpoint),   // EBREAK
                    0x10500073 => {
                        // WFI
                        self.state.halted = true;
                        Ok(StepOutcome::Halted)
                    }
                    _ => {
                        // CSR Instructions or custom system ops
                        if rd != 0 {
                            self.state.x[rd] = 0;
                        }
                        self.state.x[0] = 0;
                        Ok(StepOutcome::Continue { cycles: 1 })
                    }
                }
            }
            _ => Err(CpuError::InvalidInstruction {
                opcode: instr as u64,
                pc,
            }),
        }
    }

    fn register_count(&self) -> usize {
        33
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0..=31 => {
                const ABI_NAMES: [&str; 32] = [
                    "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0/fp", "s1", "a0", "a1",
                    "a2", "a3", "a4", "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7",
                    "s8", "s9", "s10", "s11", "t3", "t4", "t5", "t6",
                ];
                Some(RegisterInfo {
                    name: ABI_NAMES[index],
                    value: if self.state.is_64bit {
                        RegisterValue::U64(self.state.x[index])
                    } else {
                        RegisterValue::U32(self.state.x[index] as u32)
                    },
                    is_pc: false,
                    is_sp: index == 2,
                    is_flags: false,
                })
            }
            32 => Some(RegisterInfo {
                name: "pc",
                value: if self.state.is_64bit {
                    RegisterValue::U64(self.state.pc)
                } else {
                    RegisterValue::U32(self.state.pc as u32)
                },
                is_pc: true,
                is_sp: false,
                is_flags: false,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        if let Some(num_str) = name.strip_prefix('x').or_else(|| name.strip_prefix('X'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 32 {
                    return Some(self.state.x[idx]);
                }
        match name {
            s if s.eq_ignore_ascii_case("zero") => Some(0),
            s if s.eq_ignore_ascii_case("ra") => Some(self.state.x[1]),
            s if s.eq_ignore_ascii_case("sp") => Some(self.state.x[2]),
            s if s.eq_ignore_ascii_case("gp") => Some(self.state.x[3]),
            s if s.eq_ignore_ascii_case("tp") => Some(self.state.x[4]),
            s if s.eq_ignore_ascii_case("t0") => Some(self.state.x[5]),
            s if s.eq_ignore_ascii_case("t1") => Some(self.state.x[6]),
            s if s.eq_ignore_ascii_case("t2") => Some(self.state.x[7]),
            s if s.eq_ignore_ascii_case("s0") || s.eq_ignore_ascii_case("fp") => {
                Some(self.state.x[8])
            }
            s if s.eq_ignore_ascii_case("s1") => Some(self.state.x[9]),
            s if s.eq_ignore_ascii_case("a0") => Some(self.state.x[10]),
            s if s.eq_ignore_ascii_case("a1") => Some(self.state.x[11]),
            s if s.eq_ignore_ascii_case("a2") => Some(self.state.x[12]),
            s if s.eq_ignore_ascii_case("a3") => Some(self.state.x[13]),
            s if s.eq_ignore_ascii_case("a4") => Some(self.state.x[14]),
            s if s.eq_ignore_ascii_case("a5") => Some(self.state.x[15]),
            s if s.eq_ignore_ascii_case("a6") => Some(self.state.x[16]),
            s if s.eq_ignore_ascii_case("a7") => Some(self.state.x[17]),
            s if s.eq_ignore_ascii_case("pc") => Some(self.state.pc),
            _ => None,
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        if let Some(num_str) = name.strip_prefix('x').or_else(|| name.strip_prefix('X'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 32 {
                    if idx != 0 {
                        self.state.x[idx] = val;
                    }
                    return Ok(());
                }
        match name {
            s if s.eq_ignore_ascii_case("zero") => {}
            s if s.eq_ignore_ascii_case("ra") => self.state.x[1] = val,
            s if s.eq_ignore_ascii_case("sp") => self.state.x[2] = val,
            s if s.eq_ignore_ascii_case("gp") => self.state.x[3] = val,
            s if s.eq_ignore_ascii_case("tp") => self.state.x[4] = val,
            s if s.eq_ignore_ascii_case("s0") || s.eq_ignore_ascii_case("fp") => {
                self.state.x[8] = val
            }
            s if s.eq_ignore_ascii_case("pc") => self.state.pc = val,
            _ => return Err(CpuError::RegisterNotFound),
        }
        self.state.x[0] = 0;
        Ok(())
    }
}
