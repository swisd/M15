//! PowerPC (PPC32 / PPC64) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// PowerPC register state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct PowerPcState {
    pub r: [u32; 32], // r1 is SP
    pub pc: u32,
    pub lr: u32,  // Link Register
    pub ctr: u32, // Count Register
    pub cr: u32,  // Condition Register
    pub xer: u32, // Fixed-point Exception Register
    pub halted: bool,
}

/// PowerPC CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PowerPcCpu {
    pub state: PowerPcState,
}

impl PowerPcCpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    pub fn push_u32(&mut self, bus: &mut dyn MemoryBus, val: u32) -> Result<(), CpuError> {
        self.state.r[1] = self.state.r[1].wrapping_sub(4);
        bus.write_u32(self.state.r[1] as u64, val, Endianness::BigEndian)?;
        Ok(())
    }

    pub fn pop_u32(&mut self, bus: &mut dyn MemoryBus) -> Result<u32, CpuError> {
        let val = bus.read_u32(self.state.r[1] as u64, Endianness::BigEndian)?;
        self.state.r[1] = self.state.r[1].wrapping_add(4);
        Ok(val)
    }
}

impl CpuEngine for PowerPcCpu {
    fn arch(&self) -> Architecture {
        Architecture::PowerPc
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
        self.state.r[1] as u64
    }

    fn set_sp(&mut self, val: u64) {
        self.state.r[1] = (val & 0xFFFF_FFFF) as u32;
    }

    fn reset(&mut self) {
        self.state = PowerPcState {
            pc: 0xFFF0_0100,
            ..Default::default()
        };
        self.state.r[1] = 0x7FFF_0000; // SP (r1)
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let pc = self.pc();
        let instr = bus.read_u32(pc, Endianness::BigEndian)?;
        self.state.pc = self.state.pc.wrapping_add(4);

        match instr {
            0x60000000 => {
                // NOP (ori 0,0,0)
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x4E800020 => {
                // BLR (Branch to Link Register / Return)
                self.state.pc = self.state.lr;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x7FE00008 => {
                // TRAP
                Ok(StepOutcome::Breakpoint)
            }
            0x44000002 => {
                // SC (System Call)
                Ok(StepOutcome::Interrupt(0))
            }
            0x00000000 => {
                // Null instruction / Halt
                self.state.halted = true;
                Ok(StepOutcome::Halted)
            }
            _ => {
                let op = (instr >> 26) & 0x3F;

                match op {
                    14 => {
                        // ADDI rt, ra, imm
                        let rt = ((instr >> 21) & 0x1F) as usize;
                        let ra = ((instr >> 16) & 0x1F) as usize;
                        let imm = (instr as i16) as i32 as u32;
                        let ra_val = if ra == 0 { 0 } else { self.state.r[ra] };
                        self.state.r[rt] = ra_val.wrapping_add(imm);
                        Ok(StepOutcome::Continue { cycles: 1 })
                    }
                    15 => {
                        // ADDIS rt, ra, imm
                        let rt = ((instr >> 21) & 0x1F) as usize;
                        let ra = ((instr >> 16) & 0x1F) as usize;
                        let imm = ((instr & 0xFFFF) as u32) << 16;
                        let ra_val = if ra == 0 { 0 } else { self.state.r[ra] };
                        self.state.r[rt] = ra_val.wrapping_add(imm);
                        Ok(StepOutcome::Continue { cycles: 1 })
                    }
                    24 => {
                        // ORI ra, rs, imm
                        let rs = ((instr >> 21) & 0x1F) as usize;
                        let ra = ((instr >> 16) & 0x1F) as usize;
                        let imm = (instr & 0xFFFF) as u32;
                        self.state.r[ra] = self.state.r[rs] | imm;
                        Ok(StepOutcome::Continue { cycles: 1 })
                    }
                    31 => {
                        let rt = ((instr >> 21) & 0x1F) as usize;
                        let ra = ((instr >> 16) & 0x1F) as usize;
                        let rb = ((instr >> 11) & 0x1F) as usize;
                        let xo = (instr >> 1) & 0x3FF;
                        match xo {
                            266 => {
                                // ADD rt, ra, rb
                                self.state.r[rt] = self.state.r[ra].wrapping_add(self.state.r[rb]);
                            }
                            40 => {
                                // SUBF rt, ra, rb (rt = rb - ra)
                                self.state.r[rt] = self.state.r[rb].wrapping_sub(self.state.r[ra]);
                            }
                            28 => {
                                // AND ra, rs, rb
                                self.state.r[ra] = self.state.r[rt] & self.state.r[rb];
                            }
                            444 => {
                                // OR ra, rs, rb
                                self.state.r[ra] = self.state.r[rt] | self.state.r[rb];
                            }
                            316 => {
                                // XOR ra, rs, rb
                                self.state.r[ra] = self.state.r[rt] ^ self.state.r[rb];
                            }
                            _ => return Err(CpuError::InvalidInstruction { opcode: instr as u64, pc }),
                        }
                        Ok(StepOutcome::Continue { cycles: 1 })
                    }
                    32 => {
                        // LWZ rt, d(ra)
                        let rt = ((instr >> 21) & 0x1F) as usize;
                        let ra = ((instr >> 16) & 0x1F) as usize;
                        let d = (instr as i16) as i32 as u32;
                        let ra_val = if ra == 0 { 0 } else { self.state.r[ra] };
                        let addr = ra_val.wrapping_add(d) as u64;
                        let val = bus.read_u32(addr, Endianness::BigEndian)?;
                        self.state.r[rt] = val;
                        Ok(StepOutcome::Continue { cycles: 2 })
                    }
                    36 => {
                        // STW rs, d(ra)
                        let rs = ((instr >> 21) & 0x1F) as usize;
                        let ra = ((instr >> 16) & 0x1F) as usize;
                        let d = (instr as i16) as i32 as u32;
                        let ra_val = if ra == 0 { 0 } else { self.state.r[ra] };
                        let addr = ra_val.wrapping_add(d) as u64;
                        bus.write_u32(addr, self.state.r[rs], Endianness::BigEndian)?;
                        Ok(StepOutcome::Continue { cycles: 2 })
                    }
                    18 => {
                        // B / BL (Opcode 18)
                        let li = instr & 0x03FF_FFFC;
                        let sign_ext = if li & 0x0200_0000 != 0 {
                            li | 0xFC00_0000
                        } else {
                            li
                        };
                        let aa = (instr & 2) != 0;
                        let lk = (instr & 1) != 0;
                        if lk {
                            self.state.lr = self.state.pc;
                        }
                        if aa {
                            self.state.pc = sign_ext;
                        } else {
                            self.state.pc = (pc as i32).wrapping_add(sign_ext as i32) as u32;
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
        37
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0..=31 => {
                const PPC_NAMES: [&str; 32] = [
                    "r0", "r1(sp)", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11",
                    "r12", "r13", "r14", "r15", "r16", "r17", "r18", "r19", "r20", "r21", "r22",
                    "r23", "r24", "r25", "r26", "r27", "r28", "r29", "r30", "r31",
                ];
                Some(RegisterInfo {
                    name: PPC_NAMES[index],
                    value: RegisterValue::U32(self.state.r[index]),
                    is_pc: false,
                    is_sp: index == 1,
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
                name: "lr",
                value: RegisterValue::U32(self.state.lr),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            34 => Some(RegisterInfo {
                name: "ctr",
                value: RegisterValue::U32(self.state.ctr),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            35 => Some(RegisterInfo {
                name: "cr",
                value: RegisterValue::U32(self.state.cr),
                is_pc: false,
                is_sp: false,
                is_flags: true,
            }),
            36 => Some(RegisterInfo {
                name: "xer",
                value: RegisterValue::U32(self.state.xer),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        if let Some(num_str) = name.strip_prefix('r').or_else(|| name.strip_prefix('R'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 32 {
                    return Some(self.state.r[idx] as u64);
                }
        match name {
            s if s.eq_ignore_ascii_case("sp") => Some(self.state.r[1] as u64),
            s if s.eq_ignore_ascii_case("pc") => Some(self.state.pc as u64),
            s if s.eq_ignore_ascii_case("lr") => Some(self.state.lr as u64),
            s if s.eq_ignore_ascii_case("ctr") => Some(self.state.ctr as u64),
            s if s.eq_ignore_ascii_case("cr") => Some(self.state.cr as u64),
            s if s.eq_ignore_ascii_case("xer") => Some(self.state.xer as u64),
            _ => None,
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        let v = (val & 0xFFFF_FFFF) as u32;
        if let Some(num_str) = name.strip_prefix('r').or_else(|| name.strip_prefix('R'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 32 {
                    self.state.r[idx] = v;
                    return Ok(());
                }
        match name {
            s if s.eq_ignore_ascii_case("sp") => self.state.r[1] = v,
            s if s.eq_ignore_ascii_case("pc") => self.state.pc = v,
            s if s.eq_ignore_ascii_case("lr") => self.state.lr = v,
            s if s.eq_ignore_ascii_case("ctr") => self.state.ctr = v,
            s if s.eq_ignore_ascii_case("cr") => self.state.cr = v,
            s if s.eq_ignore_ascii_case("xer") => self.state.xer = v,
            _ => return Err(CpuError::RegisterNotFound),
        }
        Ok(())
    }
}
