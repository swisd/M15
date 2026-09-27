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
    pub csr_mstatus: u64,
    pub csr_mepc: u64,
    pub csr_mcause: u64,
    pub csr_mtvec: u64,
    pub csr_mie: u64,
    pub csr_mip: u64,
    pub csr_mscratch: u64,
    pub csr_cycle: u64,
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

    /// Reads a Control and Status Register (CSR).
    pub fn read_csr(&self, csr: u16) -> u64 {
        match csr {
            0x300 => self.state.csr_mstatus,
            0x301 => {
                // misa: extensions bitmask
                if self.state.is_64bit {
                    (2u64 << 62) | 0x1100 // RV64I + RV64M
                } else {
                    (1u64 << 30) | 0x1100 // RV32I + RV32M
                }
            }
            0x304 => self.state.csr_mie,
            0x305 => self.state.csr_mtvec,
            0x340 => self.state.csr_mscratch,
            0x341 => self.state.csr_mepc,
            0x342 => self.state.csr_mcause,
            0x344 => self.state.csr_mip,
            0xC00..=0xC02 => self.state.csr_cycle,
            0xF11..=0xF14 => 0,
            _ => 0,
        }
    }

    /// Writes a value to a Control and Status Register (CSR).
    pub fn write_csr(&mut self, csr: u16, val: u64) {
        match csr {
            0x300 => self.state.csr_mstatus = val,
            0x304 => self.state.csr_mie = val,
            0x305 => self.state.csr_mtvec = val,
            0x340 => self.state.csr_mscratch = val,
            0x341 => self.state.csr_mepc = val,
            0x342 => self.state.csr_mcause = val,
            0x344 => self.state.csr_mip = val,
            0xC00 => self.state.csr_cycle = val,
            _ => {}
        }
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
            ..Default::default()
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
                // OP (ADD, SUB, SLL, SLT, SLTU, XOR, SRL, SRA, OR, AND) & RV32M/RV64M (MUL, MULH, MULHSU, MULHU, DIV, DIVU, REM, REMU)
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

                    // RV32M / RV64M extension
                    (0x01, 0) => (v1 as i64).wrapping_mul(v2 as i64) as u64, // MUL
                    (0x01, 1) => {
                        // MULH (signed * signed, high part)
                        if self.state.is_64bit {
                            (((v1 as i64 as i128) * (v2 as i64 as i128)) >> 64) as u64
                        } else {
                            (((v1 as i32 as i64) * (v2 as i32 as i64)) >> 32) as u32 as u64
                        }
                    }
                    (0x01, 2) => {
                        // MULHSU (signed * unsigned, high part)
                        if self.state.is_64bit {
                            (((v1 as i64 as i128) * (v2 as u128 as i128)) >> 64) as u64
                        } else {
                            (((v1 as i32 as i64) * (v2 as u32 as i64)) >> 32) as u32 as u64
                        }
                    }
                    (0x01, 3) => {
                        // MULHU (unsigned * unsigned, high part)
                        if self.state.is_64bit {
                            (((v1 as u128) * (v2 as u128)) >> 64) as u64
                        } else {
                            (((v1 as u32 as u64) * (v2 as u32 as u64)) >> 32) as u32 as u64
                        }
                    }
                    (0x01, 4) => {
                        // DIV (signed / signed)
                        if self.state.is_64bit {
                            if v2 == 0 {
                                u64::MAX
                            } else if v1 as i64 == i64::MIN && v2 as i64 == -1 {
                                i64::MIN as u64
                            } else {
                                ((v1 as i64) / (v2 as i64)) as u64
                            }
                        } else {
                            let n = v1 as i32;
                            let d = v2 as i32;
                            if d == 0 {
                                -1i32 as u32 as u64
                            } else if n == i32::MIN && d == -1 {
                                i32::MIN as u32 as u64
                            } else {
                                (n / d) as u32 as u64
                            }
                        }
                    }
                    (0x01, 5) => {
                        // DIVU (unsigned / unsigned)
                        if self.state.is_64bit {
                            v1.checked_div(v2).unwrap_or(u64::MAX)
                        } else {
                            let n = v1 as u32;
                            let d = v2 as u32;
                            n.checked_div(d).map_or(u32::MAX as u64, |q| q as u64)
                        }
                    }
                    (0x01, 6) => {
                        // REM (signed % signed)
                        if self.state.is_64bit {
                            if v2 == 0 {
                                v1
                            } else if v1 as i64 == i64::MIN && v2 as i64 == -1 {
                                0
                            } else {
                                ((v1 as i64) % (v2 as i64)) as u64
                            }
                        } else {
                            let n = v1 as i32;
                            let d = v2 as i32;
                            if d == 0 {
                                n as u32 as u64
                            } else if n == i32::MIN && d == -1 {
                                0
                            } else {
                                (n % d) as u32 as u64
                            }
                        }
                    }
                    (0x01, 7) => {
                        // REMU (unsigned % unsigned)
                        if self.state.is_64bit {
                            if v2 == 0 { v1 } else { v1 % v2 }
                        } else {
                            let n = v1 as u32;
                            let d = v2 as u32;
                            if d == 0 { n as u64 } else { (n % d) as u64 }
                        }
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
                        res
                    } else {
                        (res as u32) as u64
                    };
                }
                self.state.x[0] = 0;
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x1B => {
                // OP-IMM-32 (ADDIW, SLLIW, SRLIW, SRAIW)
                let imm = ((instr as i32) >> 20) as i64;
                let v1 = self.state.x[rs1] as u32;
                let shamt = (imm & 0x1F) as u32;

                let res = match funct3 {
                    0 => (v1 as i32).wrapping_add(imm as i32) as i64 as u64, // ADDIW
                    1 => ((v1 << shamt) as i32) as i64 as u64,               // SLLIW
                    5 => {
                        if (funct7 & 0x20) != 0 {
                            (((v1 as i32) >> shamt) as i64) as u64            // SRAIW
                        } else {
                            (((v1 >> shamt) as i32) as i64) as u64            // SRLIW
                        }
                    }
                    _ => {
                        return Err(CpuError::InvalidInstruction {
                            opcode: instr as u64,
                            pc,
                        })
                    }
                };

                if rd != 0 {
                    self.state.x[rd] = res;
                }
                self.state.x[0] = 0;
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x3B => {
                // OP-32 (ADDW, SUBW, SLLW, SRLW, SRAW, MULW, DIVW, DIVUW, REMW, REMUW)
                let v1 = self.state.x[rs1] as u32;
                let v2 = self.state.x[rs2] as u32;

                let res = match (funct7, funct3) {
                    (0x00, 0) => ((v1 as i32).wrapping_add(v2 as i32) as i64) as u64, // ADDW
                    (0x20, 0) => ((v1 as i32).wrapping_sub(v2 as i32) as i64) as u64, // SUBW
                    (0x00, 1) => (((v1 << (v2 & 0x1F)) as i32) as i64) as u64,        // SLLW
                    (0x00, 5) => (((v1 >> (v2 & 0x1F)) as i32) as i64) as u64,        // SRLW
                    (0x20, 5) => (((v1 as i32) >> (v2 & 0x1F)) as i64) as u64,      // SRAW

                    // RV64M 32-bit extensions
                    (0x01, 0) => (((v1 as i32).wrapping_mul(v2 as i32)) as i64) as u64, // MULW
                    (0x01, 4) => {
                        // DIVW
                        let n = v1 as i32;
                        let d = v2 as i32;
                        if d == 0 {
                            -1i64 as u64
                        } else if n == i32::MIN && d == -1 {
                            (i32::MIN as i64) as u64
                        } else {
                            ((n / d) as i64) as u64
                        }
                    }
                    (0x01, 5) => {
                        // DIVUW
                        v1.checked_div(v2).map_or(-1i64 as u64, |q| ((q as i32) as i64) as u64)
                    }
                    (0x01, 6) => {
                        // REMW
                        let n = v1 as i32;
                        let d = v2 as i32;
                        if d == 0 {
                            (n as i64) as u64
                        } else if n == i32::MIN && d == -1 {
                            0
                        } else {
                            ((n % d) as i64) as u64
                        }
                    }
                    (0x01, 7) => {
                        // REMUW
                        if v2 == 0 {
                            ((v1 as i32) as i64) as u64
                        } else {
                            (((v1 % v2) as i32) as i64) as u64
                        }
                    }

                    _ => {
                        return Err(CpuError::InvalidInstruction {
                            opcode: instr as u64,
                            pc,
                        })
                    }
                };

                if rd != 0 {
                    self.state.x[rd] = res;
                }
                self.state.x[0] = 0;
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x2F => {
                // RV32A / RV64A Atomic Memory Operations
                let addr = self.state.x[rs1];
                let op = (funct7 >> 2) & 0x1F;

                if funct3 == 2 {
                    // 32-bit atomic word operation
                    let old_val = bus.read_u32(addr, Endianness::LittleEndian)? as i32 as i64 as u64;
                    let v2 = self.state.x[rs2] as u32;
                    let old_u32 = old_val as u32;

                    let new_val = match op {
                        0x02 => old_u32, // LR.W (load reserved)
                        0x03 => v2,      // SC.W (store conditional - always succeeds in single-thread emulator)
                        0x01 => v2,      // AMOSWAP.W
                        0x00 => old_u32.wrapping_add(v2), // AMOADD.W
                        0x04 => old_u32 ^ v2, // AMOXOR.W
                        0x0C => old_u32 & v2, // AMOAND.W
                        0x08 => old_u32 | v2, // AMOOR.W
                        0x10 => core::cmp::min(old_u32 as i32, v2 as i32) as u32, // AMOMIN.W
                        0x14 => core::cmp::max(old_u32 as i32, v2 as i32) as u32, // AMOMAX.W
                        0x18 => core::cmp::min(old_u32, v2), // AMOMINU.W
                        0x1C => core::cmp::max(old_u32, v2), // AMOMAXU.W
                        _ => return Err(CpuError::InvalidInstruction { opcode: instr as u64, pc }),
                    };

                    if op != 0x02 {
                        bus.write_u32(addr, new_val, Endianness::LittleEndian)?;
                    }

                    if rd != 0 {
                        self.state.x[rd] = if op == 0x03 {
                            0 // SC.W write 0 on success
                        } else if self.state.is_64bit {
                            old_val
                        } else {
                            (old_val as u32) as u64
                        };
                    }
                    self.state.x[0] = 0;
                    Ok(StepOutcome::Continue { cycles: 2 })
                } else if funct3 == 3 && self.state.is_64bit {
                    // 64-bit atomic doubleword operation
                    let old_val = bus.read_u64(addr, Endianness::LittleEndian)?;
                    let v2 = self.state.x[rs2];

                    let new_val = match op {
                        0x02 => old_val,
                        0x03 => v2,
                        0x01 => v2,
                        0x00 => old_val.wrapping_add(v2),
                        0x04 => old_val ^ v2,
                        0x0C => old_val & v2,
                        0x08 => old_val | v2,
                        0x10 => core::cmp::min(old_val as i64, v2 as i64) as u64,
                        0x14 => core::cmp::max(old_val as i64, v2 as i64) as u64,
                        0x18 => core::cmp::min(old_val, v2),
                        0x1C => core::cmp::max(old_val, v2),
                        _ => return Err(CpuError::InvalidInstruction { opcode: instr as u64, pc }),
                    };

                    if op != 0x02 {
                        bus.write_u64(addr, new_val, Endianness::LittleEndian)?;
                    }

                    if rd != 0 {
                        self.state.x[rd] = if op == 0x03 { 0 } else { old_val };
                    }
                    self.state.x[0] = 0;
                    Ok(StepOutcome::Continue { cycles: 2 })
                } else {
                    Err(CpuError::InvalidInstruction { opcode: instr as u64, pc })
                }
            }
            0x0F => {
                // FENCE / FENCE.I
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x73 => {
                // SYSTEM (ECALL, EBREAK, WFI, MRET, SRET, and CSR instructions)
                match instr {
                    0x00000073 => Ok(StepOutcome::Interrupt(0)), // ECALL
                    0x00100073 => Ok(StepOutcome::Breakpoint),   // EBREAK
                    0x10500073 => {
                        // WFI
                        self.state.halted = true;
                        Ok(StepOutcome::Halted)
                    }
                    0x30200073 => {
                        // MRET
                        self.state.pc = self.state.csr_mepc;
                        Ok(StepOutcome::Continue { cycles: 2 })
                    }
                    0x10200073 => {
                        // SRET
                        self.state.pc = self.state.csr_mepc;
                        Ok(StepOutcome::Continue { cycles: 2 })
                    }
                    _ => {
                        // CSR Instructions: CSRRW, CSRRS, CSRRC, CSRRWI, CSRRSI, CSRRCI
                        let csr = ((instr >> 20) & 0xFFF) as u16;
                        let old_csr = self.read_csr(csr);
                        let uimm = rs1 as u64; // zimm operand
                        let rs1_val = self.state.x[rs1];

                        let write_val = match funct3 {
                            1 => Some(rs1_val),                     // CSRRW
                            2 => Some(old_csr | rs1_val),           // CSRRS
                            3 => Some(old_csr & !rs1_val),          // CSRRC
                            5 => Some(uimm),                        // CSRRWI
                            6 => Some(old_csr | uimm),              // CSRRSI
                            7 => Some(old_csr & !uimm),             // CSRRCI
                            _ => None,
                        };

                        if let Some(val) = write_val {
                            self.write_csr(csr, val);
                        }

                        if rd != 0 {
                            self.state.x[rd] = if self.state.is_64bit {
                                old_csr
                            } else {
                                (old_csr as u32) as u64
                            };
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
