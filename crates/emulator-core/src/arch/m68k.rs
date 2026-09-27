//! Motorola 68000 (m68k / CISC) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// Motorola 68000 register state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct M68kState {
    pub d: [u32; 8], // Data registers D0-D7
    pub a: [u32; 8], // Address registers A0-A7 (A7 is SP)
    pub usp: u32,    // User Stack Pointer
    pub ssp: u32,    // Supervisor Stack Pointer
    pub pc: u32,
    pub sr: u16,     // Status Register
    pub halted: bool,
}

/// Motorola 68000 CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct M68kCpu {
    pub state: M68kState,
}

impl M68kCpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    pub fn push_u32(&mut self, bus: &mut dyn MemoryBus, val: u32) -> Result<(), CpuError> {
        self.state.a[7] = self.state.a[7].wrapping_sub(4);
        bus.write_u32(self.state.a[7] as u64, val, Endianness::BigEndian)?;
        Ok(())
    }

    pub fn pop_u32(&mut self, bus: &mut dyn MemoryBus) -> Result<u32, CpuError> {
        let val = bus.read_u32(self.state.a[7] as u64, Endianness::BigEndian)?;
        self.state.a[7] = self.state.a[7].wrapping_add(4);
        Ok(val)
    }
}

impl CpuEngine for M68kCpu {
    fn arch(&self) -> Architecture {
        Architecture::Motorola68000
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
        self.state.a[7] as u64
    }

    fn set_sp(&mut self, val: u64) {
        self.state.a[7] = (val & 0xFFFF_FFFF) as u32;
    }

    fn reset(&mut self) {
        self.state = M68kState {
            pc: 0x0000_0400,
            sr: 0x2700, // Supervisor mode, Interrupt mask 7
            ..Default::default()
        };
        self.state.a[7] = 0x00FF_FFFE; // SP (A7)
        self.state.ssp = 0x00FF_FFFE;
        self.state.usp = 0x007F_FFFE;
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let pc = self.pc();
        let instr = bus.read_u16(pc, Endianness::BigEndian)?;
        self.state.pc = self.state.pc.wrapping_add(2);

        match instr {
            0x4E71 => {
                // NOP
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x4E72 => {
                // STOP (Halt until interrupt)
                self.state.halted = true;
                Ok(StepOutcome::Halted)
            }
            0x4E75 => {
                // RTS (Return from Subroutine)
                self.state.pc = self.pop_u32(bus)?;
                Ok(StepOutcome::Continue { cycles: 16 })
            }
            0x4E40..=0x4E4F => {
                // TRAP #vector (Breakpoint/Exception)
                let vector = (instr & 0x000F) as u32;
                Ok(StepOutcome::Interrupt(vector))
            }
            _ => {
                // MOVEQ #imm8, Dn (0x7000 | (dn << 9) | imm8)
                if (instr & 0xF100) == 0x7000 {
                    let dn = ((instr >> 9) & 7) as usize;
                    let imm8 = (instr & 0xFF) as i8 as i32 as u32;
                    self.state.d[dn] = imm8;
                    return Ok(StepOutcome::Continue { cycles: 4 });
                }

                // MOVE.L Dn, -(SP) (0x2F00 | dn)
                if (instr & 0xFFF8) == 0x2F00 {
                    let dn = (instr & 7) as usize;
                    let val = self.state.d[dn];
                    self.push_u32(bus, val)?;
                    return Ok(StepOutcome::Continue { cycles: 8 });
                }

                // MOVE.L (SP)+, Dn (0x201F | (dn << 9))
                if (instr & 0xF1FF) == 0x201F {
                    let dn = ((instr >> 9) & 7) as usize;
                    self.state.d[dn] = self.pop_u32(bus)?;
                    return Ok(StepOutcome::Continue { cycles: 8 });
                }

                // MOVE.L Dm, Dn (0x2000 | (dn << 9) | dm)
                if (instr & 0xF1F8) == 0x2000 {
                    let dn = ((instr >> 9) & 7) as usize;
                    let dm = (instr & 7) as usize;
                    self.state.d[dn] = self.state.d[dm];
                    return Ok(StepOutcome::Continue { cycles: 4 });
                }

                // ADD.L Dm, Dn (0xD080 | (dn << 9) | dm)
                if (instr & 0xF1F8) == 0xD080 {
                    let dn = ((instr >> 9) & 7) as usize;
                    let dm = (instr & 7) as usize;
                    self.state.d[dn] = self.state.d[dn].wrapping_add(self.state.d[dm]);
                    return Ok(StepOutcome::Continue { cycles: 8 });
                }

                // SUB.L Dm, Dn (0x9080 | (dn << 9) | dm)
                if (instr & 0xF1F8) == 0x9080 {
                    let dn = ((instr >> 9) & 7) as usize;
                    let dm = (instr & 7) as usize;
                    self.state.d[dn] = self.state.d[dn].wrapping_sub(self.state.d[dm]);
                    return Ok(StepOutcome::Continue { cycles: 8 });
                }

                // BRA.S rel8
                if (instr & 0xFF00) == 0x6000 {
                    let disp = (instr & 0x00FF) as i8;
                    self.state.pc = (self.state.pc as i32).wrapping_add(disp as i32) as u32;
                    return Ok(StepOutcome::Continue { cycles: 10 });
                }

                // BSR.S rel8
                if (instr & 0xFF00) == 0x6100 {
                    let disp = (instr & 0x00FF) as i8;
                    let return_addr = self.state.pc;
                    self.push_u32(bus, return_addr)?;
                    self.state.pc = (self.state.pc as i32).wrapping_add(disp as i32) as u32;
                    return Ok(StepOutcome::Continue { cycles: 18 });
                }

                Err(CpuError::InvalidInstruction {
                    opcode: instr as u64,
                    pc,
                })
            }
        }
    }

    fn register_count(&self) -> usize {
        20
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0..=7 => {
                const D_NAMES: [&str; 8] = ["D0", "D1", "D2", "D3", "D4", "D5", "D6", "D7"];
                Some(RegisterInfo {
                    name: D_NAMES[index],
                    value: RegisterValue::U32(self.state.d[index]),
                    is_pc: false,
                    is_sp: false,
                    is_flags: false,
                })
            }
            8..=15 => {
                const A_NAMES: [&str; 8] = [
                    "A0", "A1", "A2", "A3", "A4", "A5", "A6", "A7(SP)",
                ];
                Some(RegisterInfo {
                    name: A_NAMES[index - 8],
                    value: RegisterValue::U32(self.state.a[index - 8]),
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
                name: "SR",
                value: RegisterValue::U16(self.state.sr),
                is_pc: false,
                is_sp: false,
                is_flags: true,
            }),
            18 => Some(RegisterInfo {
                name: "USP",
                value: RegisterValue::U32(self.state.usp),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            19 => Some(RegisterInfo {
                name: "SSP",
                value: RegisterValue::U32(self.state.ssp),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        if let Some(num_str) = name.strip_prefix('d').or_else(|| name.strip_prefix('D'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    return Some(self.state.d[idx] as u64);
                }
        if let Some(num_str) = name.strip_prefix('a').or_else(|| name.strip_prefix('A'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    return Some(self.state.a[idx] as u64);
                }
        match name {
            s if s.eq_ignore_ascii_case("sp") => Some(self.state.a[7] as u64),
            s if s.eq_ignore_ascii_case("pc") => Some(self.state.pc as u64),
            s if s.eq_ignore_ascii_case("sr") => Some(self.state.sr as u64),
            s if s.eq_ignore_ascii_case("usp") => Some(self.state.usp as u64),
            s if s.eq_ignore_ascii_case("ssp") => Some(self.state.ssp as u64),
            _ => None,
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        let v = (val & 0xFFFF_FFFF) as u32;
        if let Some(num_str) = name.strip_prefix('d').or_else(|| name.strip_prefix('D'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    self.state.d[idx] = v;
                    return Ok(());
                }
        if let Some(num_str) = name.strip_prefix('a').or_else(|| name.strip_prefix('A'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    self.state.a[idx] = v;
                    return Ok(());
                }
        match name {
            s if s.eq_ignore_ascii_case("sp") => self.state.a[7] = v,
            s if s.eq_ignore_ascii_case("pc") => self.state.pc = v,
            s if s.eq_ignore_ascii_case("sr") => self.state.sr = val as u16,
            s if s.eq_ignore_ascii_case("usp") => self.state.usp = v,
            s if s.eq_ignore_ascii_case("ssp") => self.state.ssp = v,
            _ => return Err(CpuError::RegisterNotFound),
        }
        Ok(())
    }
}
