//! Intel x86 (IA-32 / 32-bit) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// x86 32-bit register state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct X86State {
    pub eax: u32,
    pub ecx: u32,
    pub edx: u32,
    pub ebx: u32,
    pub esp: u32,
    pub ebp: u32,
    pub esi: u32,
    pub edi: u32,
    pub eip: u32,
    pub eflags: u32,
    pub cs: u16,
    pub ds: u16,
    pub ss: u16,
    pub es: u16,
    pub fs: u16,
    pub gs: u16,
    pub halted: bool,
}

/// x86 32-bit CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct X86Cpu {
    pub state: X86State,
}

impl X86Cpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    pub fn push_u32(&mut self, bus: &mut dyn MemoryBus, val: u32) -> Result<(), CpuError> {
        self.state.esp = self.state.esp.wrapping_sub(4);
        bus.write_u32(self.state.esp as u64, val, Endianness::LittleEndian)?;
        Ok(())
    }

    pub fn pop_u32(&mut self, bus: &mut dyn MemoryBus) -> Result<u32, CpuError> {
        let val = bus.read_u32(self.state.esp as u64, Endianness::LittleEndian)?;
        self.state.esp = self.state.esp.wrapping_add(4);
        Ok(val)
    }

    pub fn get_reg32(&self, reg: u8) -> u32 {
        match reg & 7 {
            0 => self.state.eax,
            1 => self.state.ecx,
            2 => self.state.edx,
            3 => self.state.ebx,
            4 => self.state.esp,
            5 => self.state.ebp,
            6 => self.state.esi,
            7 => self.state.edi,
            _ => 0,
        }
    }

    pub fn set_reg32(&mut self, reg: u8, val: u32) {
        match reg & 7 {
            0 => self.state.eax = val,
            1 => self.state.ecx = val,
            2 => self.state.edx = val,
            3 => self.state.ebx = val,
            4 => self.state.esp = val,
            5 => self.state.ebp = val,
            6 => self.state.esi = val,
            7 => self.state.edi = val,
            _ => {}
        }
    }

    fn update_flags_logic(&mut self, val: u32) {
        self.state.eflags &= !(0x40 | 0x80 | 0x01 | 0x800);
        if val == 0 {
            self.state.eflags |= 0x40; // ZF
        }
        if (val as i32) < 0 {
            self.state.eflags |= 0x80; // SF
        }
    }

    fn update_flags_add(&mut self, a: u32, b: u32, res: u32) {
        self.state.eflags &= !(0x40 | 0x80 | 0x01 | 0x800);
        if res == 0 {
            self.state.eflags |= 0x40; // ZF
        }
        if (res as i32) < 0 {
            self.state.eflags |= 0x80; // SF
        }
        if (res as u64) < (a as u64) {
            self.state.eflags |= 0x01; // CF
        }
        if ((a ^ res) & (b ^ res) & 0x8000_0000) != 0 {
            self.state.eflags |= 0x800; // OF
        }
    }

    fn update_flags_sub(&mut self, a: u32, b: u32, res: u32) {
        self.state.eflags &= !(0x40 | 0x80 | 0x01 | 0x800);
        if res == 0 {
            self.state.eflags |= 0x40; // ZF
        }
        if (res as i32) < 0 {
            self.state.eflags |= 0x80; // SF
        }
        if a < b {
            self.state.eflags |= 0x01; // CF
        }
        if ((a ^ b) & (a ^ res) & 0x8000_0000) != 0 {
            self.state.eflags |= 0x800; // OF
        }
    }
}

impl CpuEngine for X86Cpu {
    fn arch(&self) -> Architecture {
        Architecture::X86
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
        self.state.eip as u64
    }

    fn set_pc(&mut self, val: u64) {
        self.state.eip = (val & 0xFFFF_FFFF) as u32;
    }

    fn sp(&self) -> u64 {
        self.state.esp as u64
    }

    fn set_sp(&mut self, val: u64) {
        self.state.esp = (val & 0xFFFF_FFFF) as u32;
    }

    fn reset(&mut self) {
        self.state = X86State {
            eip: 0x0000_0000,
            esp: 0x7FFF_FFFF,
            eflags: 0x0000_0002,
            cs: 0x0008,
            ds: 0x0010,
            ss: 0x0010,
            es: 0x0010,
            fs: 0x0010,
            gs: 0x0010,
            ..Default::default()
        };
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let pc = self.pc();
        let opcode = bus.read_u8(pc)?;
        self.state.eip = self.state.eip.wrapping_add(1);

        match opcode {
            0x90 => {
                // NOP
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0xF4 => {
                // HLT
                self.state.halted = true;
                Ok(StepOutcome::Halted)
            }
            0xCC => {
                // INT 3
                Ok(StepOutcome::Breakpoint)
            }
            0xCD => {
                // INT imm8
                let vector = bus.read_u8(self.pc())? as u32;
                self.state.eip = self.state.eip.wrapping_add(1);
                Ok(StepOutcome::Interrupt(vector))
            }
            0x50..=0x57 => {
                // PUSH r32
                let reg = opcode - 0x50;
                let val = self.get_reg32(reg);
                self.push_u32(bus, val)?;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x58..=0x5F => {
                // POP r32
                let reg = opcode - 0x58;
                let val = self.pop_u32(bus)?;
                self.set_reg32(reg, val);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x68 => {
                // PUSH imm32
                let imm = bus.read_u32(self.pc(), Endianness::LittleEndian)?;
                self.state.eip = self.state.eip.wrapping_add(4);
                self.push_u32(bus, imm)?;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x6A => {
                // PUSH imm8 (sign-extended)
                let imm = (bus.read_u8(self.pc())? as i8) as i32 as u32;
                self.state.eip = self.state.eip.wrapping_add(1);
                self.push_u32(bus, imm)?;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xB8..=0xBF => {
                // MOV r32, imm32
                let reg = opcode - 0xB8;
                let val = bus.read_u32(self.pc(), Endianness::LittleEndian)?;
                self.state.eip = self.state.eip.wrapping_add(4);
                self.set_reg32(reg, val);
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x89 => {
                // MOV r/m32, r32
                let modrm = bus.read_u8(self.pc())?;
                self.state.eip = self.state.eip.wrapping_add(1);
                let reg = (modrm >> 3) & 7;
                let rm = modrm & 7;
                let val = self.get_reg32(reg);
                if (modrm >> 6) == 3 {
                    self.set_reg32(rm, val);
                } else {
                    let addr = self.get_reg32(rm) as u64;
                    bus.write_u32(addr, val, Endianness::LittleEndian)?;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x8B => {
                // MOV r32, r/m32
                let modrm = bus.read_u8(self.pc())?;
                self.state.eip = self.state.eip.wrapping_add(1);
                let reg = (modrm >> 3) & 7;
                let rm = modrm & 7;
                let val = if (modrm >> 6) == 3 {
                    self.get_reg32(rm)
                } else {
                    let addr = self.get_reg32(rm) as u64;
                    bus.read_u32(addr, Endianness::LittleEndian)?
                };
                self.set_reg32(reg, val);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x01 => {
                // ADD r/m32, r32
                let modrm = bus.read_u8(self.pc())?;
                self.state.eip = self.state.eip.wrapping_add(1);
                let reg = (modrm >> 3) & 7;
                let rm = modrm & 7;
                let r_val = self.get_reg32(reg);
                let rm_val = self.get_reg32(rm);
                let res = rm_val.wrapping_add(r_val);
                self.update_flags_add(rm_val, r_val, res);
                if (modrm >> 6) == 3 {
                    self.set_reg32(rm, res);
                } else {
                    let addr = self.get_reg32(rm) as u64;
                    bus.write_u32(addr, res, Endianness::LittleEndian)?;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x03 => {
                // ADD r32, r/m32
                let modrm = bus.read_u8(self.pc())?;
                self.state.eip = self.state.eip.wrapping_add(1);
                let reg = (modrm >> 3) & 7;
                let rm = modrm & 7;
                let rm_val = if (modrm >> 6) == 3 {
                    self.get_reg32(rm)
                } else {
                    let addr = self.get_reg32(rm) as u64;
                    bus.read_u32(addr, Endianness::LittleEndian)?
                };
                let r_val = self.get_reg32(reg);
                let res = r_val.wrapping_add(rm_val);
                self.update_flags_add(r_val, rm_val, res);
                self.set_reg32(reg, res);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x29 => {
                // SUB r/m32, r32
                let modrm = bus.read_u8(self.pc())?;
                self.state.eip = self.state.eip.wrapping_add(1);
                let reg = (modrm >> 3) & 7;
                let rm = modrm & 7;
                let r_val = self.get_reg32(reg);
                let rm_val = self.get_reg32(rm);
                let res = rm_val.wrapping_sub(r_val);
                self.update_flags_sub(rm_val, r_val, res);
                if (modrm >> 6) == 3 {
                    self.set_reg32(rm, res);
                } else {
                    let addr = self.get_reg32(rm) as u64;
                    bus.write_u32(addr, res, Endianness::LittleEndian)?;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x2B => {
                // SUB r32, r/m32
                let modrm = bus.read_u8(self.pc())?;
                self.state.eip = self.state.eip.wrapping_add(1);
                let reg = (modrm >> 3) & 7;
                let rm = modrm & 7;
                let rm_val = if (modrm >> 6) == 3 {
                    self.get_reg32(rm)
                } else {
                    let addr = self.get_reg32(rm) as u64;
                    bus.read_u32(addr, Endianness::LittleEndian)?
                };
                let r_val = self.get_reg32(reg);
                let res = r_val.wrapping_sub(rm_val);
                self.update_flags_sub(r_val, rm_val, res);
                self.set_reg32(reg, res);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x31 => {
                // XOR r/m32, r32
                let modrm = bus.read_u8(self.pc())?;
                self.state.eip = self.state.eip.wrapping_add(1);
                let reg = (modrm >> 3) & 7;
                let rm = modrm & 7;
                let r_val = self.get_reg32(reg);
                let rm_val = self.get_reg32(rm);
                let res = rm_val ^ r_val;
                self.update_flags_logic(res);
                if (modrm >> 6) == 3 {
                    self.set_reg32(rm, res);
                } else {
                    let addr = self.get_reg32(rm) as u64;
                    bus.write_u32(addr, res, Endianness::LittleEndian)?;
                }
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x39 => {
                // CMP r/m32, r32
                let modrm = bus.read_u8(self.pc())?;
                self.state.eip = self.state.eip.wrapping_add(1);
                let reg = (modrm >> 3) & 7;
                let rm = modrm & 7;
                let r_val = self.get_reg32(reg);
                let rm_val = self.get_reg32(rm);
                let res = rm_val.wrapping_sub(r_val);
                self.update_flags_sub(rm_val, r_val, res);
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0xC3 => {
                // RET
                self.state.eip = self.pop_u32(bus)?;
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xE8 => {
                // CALL rel32
                let rel = bus.read_u32(self.pc(), Endianness::LittleEndian)? as i32;
                self.state.eip = self.state.eip.wrapping_add(4);
                let ret_addr = self.state.eip;
                self.push_u32(bus, ret_addr)?;
                self.state.eip = (self.state.eip as i32).wrapping_add(rel) as u32;
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xE9 => {
                // JMP rel32
                let rel = bus.read_u32(self.pc(), Endianness::LittleEndian)? as i32;
                self.state.eip = self.state.eip.wrapping_add(4);
                self.state.eip = (self.state.eip as i32).wrapping_add(rel) as u32;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xEB => {
                // JMP rel8
                let rel = bus.read_u8(self.pc())? as i8;
                self.state.eip = self.state.eip.wrapping_add(1);
                self.state.eip = (self.state.eip as i32).wrapping_add(rel as i32) as u32;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x74 => {
                // JZ / JE rel8
                let rel = bus.read_u8(self.pc())? as i8;
                self.state.eip = self.state.eip.wrapping_add(1);
                if (self.state.eflags & 0x40) != 0 {
                    self.state.eip = (self.state.eip as i32).wrapping_add(rel as i32) as u32;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x75 => {
                // JNZ / JNE rel8
                let rel = bus.read_u8(self.pc())? as i8;
                self.state.eip = self.state.eip.wrapping_add(1);
                if (self.state.eflags & 0x40) == 0 {
                    self.state.eip = (self.state.eip as i32).wrapping_add(rel as i32) as u32;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            _ => Err(CpuError::InvalidInstruction {
                opcode: opcode as u64,
                pc,
            }),
        }
    }

    fn register_count(&self) -> usize {
        16
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0 => Some(RegisterInfo {
                name: "EAX",
                value: RegisterValue::U32(self.state.eax),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            1 => Some(RegisterInfo {
                name: "ECX",
                value: RegisterValue::U32(self.state.ecx),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            2 => Some(RegisterInfo {
                name: "EDX",
                value: RegisterValue::U32(self.state.edx),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            3 => Some(RegisterInfo {
                name: "EBX",
                value: RegisterValue::U32(self.state.ebx),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            4 => Some(RegisterInfo {
                name: "ESP",
                value: RegisterValue::U32(self.state.esp),
                is_pc: false,
                is_sp: true,
                is_flags: false,
            }),
            5 => Some(RegisterInfo {
                name: "EBP",
                value: RegisterValue::U32(self.state.ebp),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            6 => Some(RegisterInfo {
                name: "ESI",
                value: RegisterValue::U32(self.state.esi),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            7 => Some(RegisterInfo {
                name: "EDI",
                value: RegisterValue::U32(self.state.edi),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            8 => Some(RegisterInfo {
                name: "EIP",
                value: RegisterValue::U32(self.state.eip),
                is_pc: true,
                is_sp: false,
                is_flags: false,
            }),
            9 => Some(RegisterInfo {
                name: "EFLAGS",
                value: RegisterValue::U32(self.state.eflags),
                is_pc: false,
                is_sp: false,
                is_flags: true,
            }),
            10 => Some(RegisterInfo {
                name: "CS",
                value: RegisterValue::U16(self.state.cs),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            11 => Some(RegisterInfo {
                name: "DS",
                value: RegisterValue::U16(self.state.ds),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            12 => Some(RegisterInfo {
                name: "SS",
                value: RegisterValue::U16(self.state.ss),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            13 => Some(RegisterInfo {
                name: "ES",
                value: RegisterValue::U16(self.state.es),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            14 => Some(RegisterInfo {
                name: "FS",
                value: RegisterValue::U16(self.state.fs),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            15 => Some(RegisterInfo {
                name: "GS",
                value: RegisterValue::U16(self.state.gs),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        match name {
            s if s.eq_ignore_ascii_case("EAX") => Some(self.state.eax as u64),
            s if s.eq_ignore_ascii_case("ECX") => Some(self.state.ecx as u64),
            s if s.eq_ignore_ascii_case("EDX") => Some(self.state.edx as u64),
            s if s.eq_ignore_ascii_case("EBX") => Some(self.state.ebx as u64),
            s if s.eq_ignore_ascii_case("ESP") || s.eq_ignore_ascii_case("SP") => {
                Some(self.state.esp as u64)
            }
            s if s.eq_ignore_ascii_case("EBP") || s.eq_ignore_ascii_case("BP") => {
                Some(self.state.ebp as u64)
            }
            s if s.eq_ignore_ascii_case("ESI") || s.eq_ignore_ascii_case("SI") => {
                Some(self.state.esi as u64)
            }
            s if s.eq_ignore_ascii_case("EDI") || s.eq_ignore_ascii_case("DI") => {
                Some(self.state.edi as u64)
            }
            s if s.eq_ignore_ascii_case("EIP") || s.eq_ignore_ascii_case("PC") => {
                Some(self.state.eip as u64)
            }
            s if s.eq_ignore_ascii_case("EFLAGS") || s.eq_ignore_ascii_case("FLAGS") => {
                Some(self.state.eflags as u64)
            }
            s if s.eq_ignore_ascii_case("CS") => Some(self.state.cs as u64),
            s if s.eq_ignore_ascii_case("DS") => Some(self.state.ds as u64),
            s if s.eq_ignore_ascii_case("SS") => Some(self.state.ss as u64),
            s if s.eq_ignore_ascii_case("ES") => Some(self.state.es as u64),
            s if s.eq_ignore_ascii_case("FS") => Some(self.state.fs as u64),
            s if s.eq_ignore_ascii_case("GS") => Some(self.state.gs as u64),
            _ => None,
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        let v32 = (val & 0xFFFF_FFFF) as u32;
        let v16 = (val & 0xFFFF) as u16;
        match name {
            s if s.eq_ignore_ascii_case("EAX") => self.state.eax = v32,
            s if s.eq_ignore_ascii_case("ECX") => self.state.ecx = v32,
            s if s.eq_ignore_ascii_case("EDX") => self.state.edx = v32,
            s if s.eq_ignore_ascii_case("EBX") => self.state.ebx = v32,
            s if s.eq_ignore_ascii_case("ESP") || s.eq_ignore_ascii_case("SP") => {
                self.state.esp = v32
            }
            s if s.eq_ignore_ascii_case("EBP") || s.eq_ignore_ascii_case("BP") => {
                self.state.ebp = v32
            }
            s if s.eq_ignore_ascii_case("ESI") || s.eq_ignore_ascii_case("SI") => {
                self.state.esi = v32
            }
            s if s.eq_ignore_ascii_case("EDI") || s.eq_ignore_ascii_case("DI") => {
                self.state.edi = v32
            }
            s if s.eq_ignore_ascii_case("EIP") || s.eq_ignore_ascii_case("PC") => {
                self.state.eip = v32
            }
            s if s.eq_ignore_ascii_case("EFLAGS") || s.eq_ignore_ascii_case("FLAGS") => {
                self.state.eflags = v32
            }
            s if s.eq_ignore_ascii_case("CS") => self.state.cs = v16,
            s if s.eq_ignore_ascii_case("DS") => self.state.ds = v16,
            s if s.eq_ignore_ascii_case("SS") => self.state.ss = v16,
            s if s.eq_ignore_ascii_case("ES") => self.state.es = v16,
            s if s.eq_ignore_ascii_case("FS") => self.state.fs = v16,
            s if s.eq_ignore_ascii_case("GS") => self.state.gs = v16,
            _ => return Err(CpuError::RegisterNotFound),
        }
        Ok(())
    }
}
