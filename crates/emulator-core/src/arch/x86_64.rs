//! Intel / AMD x86-64 (AMD64 / 64-bit) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// x86-64 64-bit register state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct X86_64State {
    pub rax: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rbx: u64,
    pub rsp: u64,
    pub rbp: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
    pub cs: u16,
    pub ds: u16,
    pub ss: u16,
    pub es: u16,
    pub fs: u16,
    pub gs: u16,
    pub halted: bool,
}

/// x86-64 CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct X86_64Cpu {
    pub state: X86_64State,
}

impl X86_64Cpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    pub fn push_u64(&mut self, bus: &mut dyn MemoryBus, val: u64) -> Result<(), CpuError> {
        self.state.rsp = self.state.rsp.wrapping_sub(8);
        bus.write_u64(self.state.rsp, val, Endianness::LittleEndian)?;
        Ok(())
    }

    pub fn pop_u64(&mut self, bus: &mut dyn MemoryBus) -> Result<u64, CpuError> {
        let val = bus.read_u64(self.state.rsp, Endianness::LittleEndian)?;
        self.state.rsp = self.state.rsp.wrapping_add(8);
        Ok(val)
    }

    pub fn get_reg64(&self, reg: u8) -> u64 {
        match reg & 0x0F {
            0 => self.state.rax,
            1 => self.state.rcx,
            2 => self.state.rdx,
            3 => self.state.rbx,
            4 => self.state.rsp,
            5 => self.state.rbp,
            6 => self.state.rsi,
            7 => self.state.rdi,
            8 => self.state.r8,
            9 => self.state.r9,
            10 => self.state.r10,
            11 => self.state.r11,
            12 => self.state.r12,
            13 => self.state.r13,
            14 => self.state.r14,
            15 => self.state.r15,
            _ => 0,
        }
    }

    pub fn set_reg64(&mut self, reg: u8, val: u64) {
        match reg & 0x0F {
            0 => self.state.rax = val,
            1 => self.state.rcx = val,
            2 => self.state.rdx = val,
            3 => self.state.rbx = val,
            4 => self.state.rsp = val,
            5 => self.state.rbp = val,
            6 => self.state.rsi = val,
            7 => self.state.rdi = val,
            8 => self.state.r8 = val,
            9 => self.state.r9 = val,
            10 => self.state.r10 = val,
            11 => self.state.r11 = val,
            12 => self.state.r12 = val,
            13 => self.state.r13 = val,
            14 => self.state.r14 = val,
            15 => self.state.r15 = val,
            _ => {}
        }
    }

    fn update_flags_logic(&mut self, val: u64) {
        self.state.rflags &= !(0x40 | 0x80 | 0x01 | 0x800);
        if val == 0 {
            self.state.rflags |= 0x40; // ZF
        }
        if (val as i64) < 0 {
            self.state.rflags |= 0x80; // SF
        }
    }

    fn update_flags_add(&mut self, a: u64, b: u64, res: u64) {
        self.state.rflags &= !(0x40 | 0x80 | 0x01 | 0x800);
        if res == 0 {
            self.state.rflags |= 0x40; // ZF
        }
        if (res as i64) < 0 {
            self.state.rflags |= 0x80; // SF
        }
        if (res as u128) < (a as u128) {
            self.state.rflags |= 0x01; // CF
        }
        if ((a ^ res) & (b ^ res) & 0x8000_0000_0000_0000) != 0 {
            self.state.rflags |= 0x800; // OF
        }
    }

    fn update_flags_sub(&mut self, a: u64, b: u64, res: u64) {
        self.state.rflags &= !(0x40 | 0x80 | 0x01 | 0x800);
        if res == 0 {
            self.state.rflags |= 0x40; // ZF
        }
        if (res as i64) < 0 {
            self.state.rflags |= 0x80; // SF
        }
        if a < b {
            self.state.rflags |= 0x01; // CF
        }
        if ((a ^ b) & (a ^ res) & 0x8000_0000_0000_0000) != 0 {
            self.state.rflags |= 0x800; // OF
        }
    }
}

impl CpuEngine for X86_64Cpu {
    fn arch(&self) -> Architecture {
        Architecture::X86_64
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
        self.state.rip
    }

    fn set_pc(&mut self, val: u64) {
        self.state.rip = val;
    }

    fn sp(&self) -> u64 {
        self.state.rsp
    }

    fn set_sp(&mut self, val: u64) {
        self.state.rsp = val;
    }

    fn reset(&mut self) {
        self.state = X86_64State {
            rip: 0x0000_0000_0000_0000,
            rsp: 0x0000_7FFF_FFFF_0000,
            rflags: 0x0000_0000_0000_0002,
            cs: 0x0033,
            ds: 0x002B,
            ss: 0x002B,
            es: 0x002B,
            fs: 0x0053,
            gs: 0x002B,
            ..Default::default()
        };
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let pc = self.pc();
        let mut opcode = bus.read_u8(pc)?;
        self.state.rip = self.state.rip.wrapping_add(1);

        let mut rex = 0u8;
        if (opcode & 0xF0) == 0x40 {
            rex = opcode;
            opcode = bus.read_u8(self.pc())?;
            self.state.rip = self.state.rip.wrapping_add(1);
        }

        let rex_b = if (rex & 1) != 0 { 8 } else { 0 };
        let rex_r = if (rex & 4) != 0 { 8 } else { 0 };

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
                self.state.rip = self.state.rip.wrapping_add(1);
                Ok(StepOutcome::Interrupt(vector))
            }
            0x50..=0x57 => {
                // PUSH r64
                let reg = (opcode - 0x50) | rex_b;
                let val = self.get_reg64(reg);
                self.push_u64(bus, val)?;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x58..=0x5F => {
                // POP r64
                let reg = (opcode - 0x58) | rex_b;
                let val = self.pop_u64(bus)?;
                self.set_reg64(reg, val);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x68 => {
                // PUSH imm32 (sign-extended to 64)
                let imm = bus.read_u32(self.pc(), Endianness::LittleEndian)? as i32 as i64 as u64;
                self.state.rip = self.state.rip.wrapping_add(4);
                self.push_u64(bus, imm)?;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x6A => {
                // PUSH imm8 (sign-extended to 64)
                let imm = bus.read_u8(self.pc())? as i8 as i64 as u64;
                self.state.rip = self.state.rip.wrapping_add(1);
                self.push_u64(bus, imm)?;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xB8..=0xBF => {
                // MOV r64, imm64
                let reg = (opcode - 0xB8) | rex_b;
                let val = bus.read_u64(self.pc(), Endianness::LittleEndian)?;
                self.state.rip = self.state.rip.wrapping_add(8);
                self.set_reg64(reg, val);
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x89 => {
                // MOV r/m64, r64
                let modrm = bus.read_u8(self.pc())?;
                self.state.rip = self.state.rip.wrapping_add(1);
                let reg = ((modrm >> 3) & 7) | rex_r;
                let rm = (modrm & 7) | rex_b;
                let val = self.get_reg64(reg);
                if (modrm >> 6) == 3 {
                    self.set_reg64(rm, val);
                } else {
                    let addr = self.get_reg64(rm);
                    bus.write_u64(addr, val, Endianness::LittleEndian)?;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x8B => {
                // MOV r64, r/m64
                let modrm = bus.read_u8(self.pc())?;
                self.state.rip = self.state.rip.wrapping_add(1);
                let reg = ((modrm >> 3) & 7) | rex_r;
                let rm = (modrm & 7) | rex_b;
                let val = if (modrm >> 6) == 3 {
                    self.get_reg64(rm)
                } else {
                    let addr = self.get_reg64(rm);
                    bus.read_u64(addr, Endianness::LittleEndian)?
                };
                self.set_reg64(reg, val);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x01 => {
                // ADD r/m64, r64
                let modrm = bus.read_u8(self.pc())?;
                self.state.rip = self.state.rip.wrapping_add(1);
                let reg = ((modrm >> 3) & 7) | rex_r;
                let rm = (modrm & 7) | rex_b;
                let r_val = self.get_reg64(reg);
                let rm_val = self.get_reg64(rm);
                let res = rm_val.wrapping_add(r_val);
                self.update_flags_add(rm_val, r_val, res);
                if (modrm >> 6) == 3 {
                    self.set_reg64(rm, res);
                } else {
                    let addr = self.get_reg64(rm);
                    bus.write_u64(addr, res, Endianness::LittleEndian)?;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x03 => {
                // ADD r64, r/m64
                let modrm = bus.read_u8(self.pc())?;
                self.state.rip = self.state.rip.wrapping_add(1);
                let reg = ((modrm >> 3) & 7) | rex_r;
                let rm = (modrm & 7) | rex_b;
                let rm_val = if (modrm >> 6) == 3 {
                    self.get_reg64(rm)
                } else {
                    let addr = self.get_reg64(rm);
                    bus.read_u64(addr, Endianness::LittleEndian)?
                };
                let r_val = self.get_reg64(reg);
                let res = r_val.wrapping_add(rm_val);
                self.update_flags_add(r_val, rm_val, res);
                self.set_reg64(reg, res);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x29 => {
                // SUB r/m64, r64
                let modrm = bus.read_u8(self.pc())?;
                self.state.rip = self.state.rip.wrapping_add(1);
                let reg = ((modrm >> 3) & 7) | rex_r;
                let rm = (modrm & 7) | rex_b;
                let r_val = self.get_reg64(reg);
                let rm_val = self.get_reg64(rm);
                let res = rm_val.wrapping_sub(r_val);
                self.update_flags_sub(rm_val, r_val, res);
                if (modrm >> 6) == 3 {
                    self.set_reg64(rm, res);
                } else {
                    let addr = self.get_reg64(rm);
                    bus.write_u64(addr, res, Endianness::LittleEndian)?;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x2B => {
                // SUB r64, r/m64
                let modrm = bus.read_u8(self.pc())?;
                self.state.rip = self.state.rip.wrapping_add(1);
                let reg = ((modrm >> 3) & 7) | rex_r;
                let rm = (modrm & 7) | rex_b;
                let rm_val = if (modrm >> 6) == 3 {
                    self.get_reg64(rm)
                } else {
                    let addr = self.get_reg64(rm);
                    bus.read_u64(addr, Endianness::LittleEndian)?
                };
                let r_val = self.get_reg64(reg);
                let res = r_val.wrapping_sub(rm_val);
                self.update_flags_sub(r_val, rm_val, res);
                self.set_reg64(reg, res);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x31 => {
                // XOR r/m64, r64
                let modrm = bus.read_u8(self.pc())?;
                self.state.rip = self.state.rip.wrapping_add(1);
                let reg = ((modrm >> 3) & 7) | rex_r;
                let rm = (modrm & 7) | rex_b;
                let r_val = self.get_reg64(reg);
                let rm_val = self.get_reg64(rm);
                let res = rm_val ^ r_val;
                self.update_flags_logic(res);
                if (modrm >> 6) == 3 {
                    self.set_reg64(rm, res);
                } else {
                    let addr = self.get_reg64(rm);
                    bus.write_u64(addr, res, Endianness::LittleEndian)?;
                }
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0x39 => {
                // CMP r/m64, r64
                let modrm = bus.read_u8(self.pc())?;
                self.state.rip = self.state.rip.wrapping_add(1);
                let reg = ((modrm >> 3) & 7) | rex_r;
                let rm = (modrm & 7) | rex_b;
                let r_val = self.get_reg64(reg);
                let rm_val = self.get_reg64(rm);
                let res = rm_val.wrapping_sub(r_val);
                self.update_flags_sub(rm_val, r_val, res);
                Ok(StepOutcome::Continue { cycles: 1 })
            }
            0xC3 => {
                // RET
                self.state.rip = self.pop_u64(bus)?;
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xE8 => {
                // CALL rel32
                let rel = bus.read_u32(self.pc(), Endianness::LittleEndian)? as i32 as i64;
                self.state.rip = self.state.rip.wrapping_add(4);
                let ret_addr = self.state.rip;
                self.push_u64(bus, ret_addr)?;
                self.state.rip = (self.state.rip as i64).wrapping_add(rel) as u64;
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xE9 => {
                // JMP rel32
                let rel = bus.read_u32(self.pc(), Endianness::LittleEndian)? as i32 as i64;
                self.state.rip = self.state.rip.wrapping_add(4);
                self.state.rip = (self.state.rip as i64).wrapping_add(rel) as u64;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xEB => {
                // JMP rel8
                let rel = bus.read_u8(self.pc())? as i8 as i64;
                self.state.rip = self.state.rip.wrapping_add(1);
                self.state.rip = (self.state.rip as i64).wrapping_add(rel) as u64;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x74 => {
                // JZ / JE rel8
                let rel = bus.read_u8(self.pc())? as i8 as i64;
                self.state.rip = self.state.rip.wrapping_add(1);
                if (self.state.rflags & 0x40) != 0 {
                    self.state.rip = (self.state.rip as i64).wrapping_add(rel) as u64;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x75 => {
                // JNZ / JNE rel8
                let rel = bus.read_u8(self.pc())? as i8 as i64;
                self.state.rip = self.state.rip.wrapping_add(1);
                if (self.state.rflags & 0x40) == 0 {
                    self.state.rip = (self.state.rip as i64).wrapping_add(rel) as u64;
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
        18
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0 => Some(RegisterInfo {
                name: "RAX",
                value: RegisterValue::U64(self.state.rax),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            1 => Some(RegisterInfo {
                name: "RCX",
                value: RegisterValue::U64(self.state.rcx),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            2 => Some(RegisterInfo {
                name: "RDX",
                value: RegisterValue::U64(self.state.rdx),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            3 => Some(RegisterInfo {
                name: "RBX",
                value: RegisterValue::U64(self.state.rbx),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            4 => Some(RegisterInfo {
                name: "RSP",
                value: RegisterValue::U64(self.state.rsp),
                is_pc: false,
                is_sp: true,
                is_flags: false,
            }),
            5 => Some(RegisterInfo {
                name: "RBP",
                value: RegisterValue::U64(self.state.rbp),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            6 => Some(RegisterInfo {
                name: "RSI",
                value: RegisterValue::U64(self.state.rsi),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            7 => Some(RegisterInfo {
                name: "RDI",
                value: RegisterValue::U64(self.state.rdi),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            8 => Some(RegisterInfo {
                name: "R8",
                value: RegisterValue::U64(self.state.r8),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            9 => Some(RegisterInfo {
                name: "R9",
                value: RegisterValue::U64(self.state.r9),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            10 => Some(RegisterInfo {
                name: "R10",
                value: RegisterValue::U64(self.state.r10),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            11 => Some(RegisterInfo {
                name: "R11",
                value: RegisterValue::U64(self.state.r11),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            12 => Some(RegisterInfo {
                name: "R12",
                value: RegisterValue::U64(self.state.r12),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            13 => Some(RegisterInfo {
                name: "R13",
                value: RegisterValue::U64(self.state.r13),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            14 => Some(RegisterInfo {
                name: "R14",
                value: RegisterValue::U64(self.state.r14),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            15 => Some(RegisterInfo {
                name: "R15",
                value: RegisterValue::U64(self.state.r15),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            16 => Some(RegisterInfo {
                name: "RIP",
                value: RegisterValue::U64(self.state.rip),
                is_pc: true,
                is_sp: false,
                is_flags: false,
            }),
            17 => Some(RegisterInfo {
                name: "RFLAGS",
                value: RegisterValue::U64(self.state.rflags),
                is_pc: false,
                is_sp: false,
                is_flags: true,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        match name {
            s if s.eq_ignore_ascii_case("RAX") => Some(self.state.rax),
            s if s.eq_ignore_ascii_case("RCX") => Some(self.state.rcx),
            s if s.eq_ignore_ascii_case("RDX") => Some(self.state.rdx),
            s if s.eq_ignore_ascii_case("RBX") => Some(self.state.rbx),
            s if s.eq_ignore_ascii_case("RSP") || s.eq_ignore_ascii_case("SP") => {
                Some(self.state.rsp)
            }
            s if s.eq_ignore_ascii_case("RBP") || s.eq_ignore_ascii_case("BP") => {
                Some(self.state.rbp)
            }
            s if s.eq_ignore_ascii_case("RSI") || s.eq_ignore_ascii_case("SI") => {
                Some(self.state.rsi)
            }
            s if s.eq_ignore_ascii_case("RDI") || s.eq_ignore_ascii_case("DI") => {
                Some(self.state.rdi)
            }
            s if s.eq_ignore_ascii_case("R8") => Some(self.state.r8),
            s if s.eq_ignore_ascii_case("R9") => Some(self.state.r9),
            s if s.eq_ignore_ascii_case("R10") => Some(self.state.r10),
            s if s.eq_ignore_ascii_case("R11") => Some(self.state.r11),
            s if s.eq_ignore_ascii_case("R12") => Some(self.state.r12),
            s if s.eq_ignore_ascii_case("R13") => Some(self.state.r13),
            s if s.eq_ignore_ascii_case("R14") => Some(self.state.r14),
            s if s.eq_ignore_ascii_case("R15") => Some(self.state.r15),
            s if s.eq_ignore_ascii_case("RIP") || s.eq_ignore_ascii_case("PC") => {
                Some(self.state.rip)
            }
            s if s.eq_ignore_ascii_case("RFLAGS") || s.eq_ignore_ascii_case("FLAGS") => {
                Some(self.state.rflags)
            }
            _ => None,
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        match name {
            s if s.eq_ignore_ascii_case("RAX") => self.state.rax = val,
            s if s.eq_ignore_ascii_case("RCX") => self.state.rcx = val,
            s if s.eq_ignore_ascii_case("RDX") => self.state.rdx = val,
            s if s.eq_ignore_ascii_case("RBX") => self.state.rbx = val,
            s if s.eq_ignore_ascii_case("RSP") || s.eq_ignore_ascii_case("SP") => {
                self.state.rsp = val
            }
            s if s.eq_ignore_ascii_case("RBP") || s.eq_ignore_ascii_case("BP") => {
                self.state.rbp = val
            }
            s if s.eq_ignore_ascii_case("RSI") || s.eq_ignore_ascii_case("SI") => {
                self.state.rsi = val
            }
            s if s.eq_ignore_ascii_case("RDI") || s.eq_ignore_ascii_case("DI") => {
                self.state.rdi = val
            }
            s if s.eq_ignore_ascii_case("R8") => self.state.r8 = val,
            s if s.eq_ignore_ascii_case("R9") => self.state.r9 = val,
            s if s.eq_ignore_ascii_case("R10") => self.state.r10 = val,
            s if s.eq_ignore_ascii_case("R11") => self.state.r11 = val,
            s if s.eq_ignore_ascii_case("R12") => self.state.r12 = val,
            s if s.eq_ignore_ascii_case("R13") => self.state.r13 = val,
            s if s.eq_ignore_ascii_case("R14") => self.state.r14 = val,
            s if s.eq_ignore_ascii_case("R15") => self.state.r15 = val,
            s if s.eq_ignore_ascii_case("RIP") || s.eq_ignore_ascii_case("PC") => {
                self.state.rip = val
            }
            s if s.eq_ignore_ascii_case("RFLAGS") || s.eq_ignore_ascii_case("FLAGS") => {
                self.state.rflags = val
            }
            _ => return Err(CpuError::RegisterNotFound),
        }
        Ok(())
    }
}
