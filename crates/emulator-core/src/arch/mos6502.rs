//! MOS Technology 6502 (8-bit) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// MOS 6502 Processor Status Flags in register P.
pub mod flags {
    pub const CARRY: u8 = 1 << 0;     // C: Carry Flag
    pub const ZERO: u8 = 1 << 1;      // Z: Zero Flag
    pub const INTERRUPT: u8 = 1 << 2; // I: Interrupt Disable
    pub const DECIMAL: u8 = 1 << 3;   // D: Decimal Mode
    pub const BREAK: u8 = 1 << 4;     // B: Break Command
    pub const UNUSED: u8 = 1 << 5;    // Bit 5 is always 1 when pushed
    pub const OVERFLOW: u8 = 1 << 6;  // V: Overflow Flag
    pub const NEGATIVE: u8 = 1 << 7;  // N: Negative Flag
}

/// MOS 6502 register state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Mos6502State {
    pub a: u8,       // Accumulator
    pub x: u8,       // X Index Register
    pub y: u8,       // Y Index Register
    pub sp: u8,      // Stack Pointer (points to $0100 + SP)
    pub pc: u16,     // Program Counter
    pub p: u8,       // Processor Status Register
    pub halted: bool,
}

impl Mos6502State {
    /// Returns the absolute memory address of the current stack slot ($0100 + SP).
    #[inline]
    pub const fn stack_address(sp: u8) -> u64 {
        0x0100 + (sp as u64)
    }

    /// Sets or clears the Zero (Z) and Negative (N) flags based on an 8-bit result.
    #[inline]
    pub fn update_zn(&mut self, val: u8) {
        if val == 0 {
            self.p |= flags::ZERO;
        } else {
            self.p &= !flags::ZERO;
        }

        if (val & 0x80) != 0 {
            self.p |= flags::NEGATIVE;
        } else {
            self.p &= !flags::NEGATIVE;
        }
    }
}

/// MOS 6502 CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mos6502Cpu {
    pub state: Mos6502State,
}

impl Mos6502Cpu {
    /// Creates a new MOS 6502 CPU with standard reset state.
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    /// Pushes an 8-bit value onto Page 1 ($0100 + SP).
    pub fn push_u8(&mut self, bus: &mut dyn MemoryBus, val: u8) -> Result<(), CpuError> {
        let addr = Mos6502State::stack_address(self.state.sp);
        bus.write_u8(addr, val)?;
        self.state.sp = self.state.sp.wrapping_sub(1);
        Ok(())
    }

    /// Pulls an 8-bit value from Page 1 ($0100 + SP).
    pub fn pull_u8(&mut self, bus: &mut dyn MemoryBus) -> Result<u8, CpuError> {
        self.state.sp = self.state.sp.wrapping_add(1);
        let addr = Mos6502State::stack_address(self.state.sp);
        let val = bus.read_u8(addr)?;
        Ok(val)
    }

    /// Pushes a 16-bit address (high byte first, then low byte).
    pub fn push_u16(&mut self, bus: &mut dyn MemoryBus, val: u16) -> Result<(), CpuError> {
        self.push_u8(bus, (val >> 8) as u8)?;
        self.push_u8(bus, (val & 0xFF) as u8)?;
        Ok(())
    }

    /// Pulls a 16-bit address (low byte first, then high byte).
    pub fn pull_u16(&mut self, bus: &mut dyn MemoryBus) -> Result<u16, CpuError> {
        let lo = self.pull_u8(bus)? as u16;
        let hi = self.pull_u8(bus)? as u16;
        Ok((hi << 8) | lo)
    }

    // --- Addressing mode helpers ---

    fn read_imm(&mut self, bus: &mut dyn MemoryBus) -> Result<u8, CpuError> {
        let val = bus.read_u8(self.state.pc as u64)?;
        self.state.pc = self.state.pc.wrapping_add(1);
        Ok(val)
    }

    fn read_u16_pc(&mut self, bus: &mut dyn MemoryBus) -> Result<u16, CpuError> {
        let lo = bus.read_u8(self.state.pc as u64)? as u16;
        self.state.pc = self.state.pc.wrapping_add(1);
        let hi = bus.read_u8(self.state.pc as u64)? as u16;
        self.state.pc = self.state.pc.wrapping_add(1);
        Ok((hi << 8) | lo)
    }

    fn addr_zp(&mut self, bus: &mut dyn MemoryBus) -> Result<u16, CpuError> {
        let zp = self.read_imm(bus)? as u16;
        Ok(zp)
    }

    fn addr_zpx(&mut self, bus: &mut dyn MemoryBus) -> Result<u16, CpuError> {
        let zp = self.read_imm(bus)?;
        Ok(zp.wrapping_add(self.state.x) as u16)
    }

    fn addr_zpy(&mut self, bus: &mut dyn MemoryBus) -> Result<u16, CpuError> {
        let zp = self.read_imm(bus)?;
        Ok(zp.wrapping_add(self.state.y) as u16)
    }

    fn addr_abs(&mut self, bus: &mut dyn MemoryBus) -> Result<u16, CpuError> {
        self.read_u16_pc(bus)
    }

    fn addr_absx(&mut self, bus: &mut dyn MemoryBus) -> Result<u16, CpuError> {
        let base = self.read_u16_pc(bus)?;
        Ok(base.wrapping_add(self.state.x as u16))
    }

    fn addr_absy(&mut self, bus: &mut dyn MemoryBus) -> Result<u16, CpuError> {
        let base = self.read_u16_pc(bus)?;
        Ok(base.wrapping_add(self.state.y as u16))
    }

    fn addr_indx(&mut self, bus: &mut dyn MemoryBus) -> Result<u16, CpuError> {
        let zp = self.read_imm(bus)?.wrapping_add(self.state.x);
        let lo = bus.read_u8(zp as u64)? as u16;
        let hi = bus.read_u8(zp.wrapping_add(1) as u64)? as u16;
        Ok((hi << 8) | lo)
    }

    fn addr_indy(&mut self, bus: &mut dyn MemoryBus) -> Result<u16, CpuError> {
        let zp = self.read_imm(bus)?;
        let lo = bus.read_u8(zp as u64)? as u16;
        let hi = bus.read_u8(zp.wrapping_add(1) as u64)? as u16;
        let base = (hi << 8) | lo;
        Ok(base.wrapping_add(self.state.y as u16))
    }

    fn addr_ind(&mut self, bus: &mut dyn MemoryBus) -> Result<u16, CpuError> {
        let ptr = self.read_u16_pc(bus)?;
        let lo = bus.read_u8(ptr as u64)? as u16;
        // 6502 page boundary wrap bug emulation
        let ptr_hi = if (ptr & 0x00FF) == 0x00FF {
            ptr & 0xFF00
        } else {
            ptr + 1
        };
        let hi = bus.read_u8(ptr_hi as u64)? as u16;
        Ok((hi << 8) | lo)
    }

    // --- ALU Operations ---

    fn op_adc(&mut self, val: u8) {
        if (self.state.p & flags::DECIMAL) != 0 {
            let carry = if (self.state.p & flags::CARRY) != 0 { 1u16 } else { 0u16 };
            let a = self.state.a as u16;
            let b = val as u16;
            let bin_sum = a + b + carry;
            let overflow = (!((a ^ b) & 0x80) & ((a ^ (bin_sum as u8 as u16)) & 0x80)) != 0;
            if overflow {
                self.state.p |= flags::OVERFLOW;
            } else {
                self.state.p &= !flags::OVERFLOW;
            }

            let mut lo = (a & 0x0F) + (b & 0x0F) + carry;
            let mut hi = (a >> 4) + (b >> 4);
            if lo > 9 {
                lo += 6;
                hi += 1;
            }
            if hi > 9 {
                hi += 6;
            }
            if hi > 15 {
                self.state.p |= flags::CARRY;
            } else {
                self.state.p &= !flags::CARRY;
            }
            let res = ((hi as u8) << 4) | ((lo as u8) & 0x0F);
            self.state.a = res;
            self.state.update_zn(self.state.a);
        } else {
            let carry = if (self.state.p & flags::CARRY) != 0 { 1u16 } else { 0u16 };
            let a = self.state.a as u16;
            let b = val as u16;
            let sum = a + b + carry;

            let overflow = (!((a ^ b) & 0x80) & ((a ^ sum) & 0x80)) != 0;
            if overflow {
                self.state.p |= flags::OVERFLOW;
            } else {
                self.state.p &= !flags::OVERFLOW;
            }

            if sum > 0xFF {
                self.state.p |= flags::CARRY;
            } else {
                self.state.p &= !flags::CARRY;
            }

            self.state.a = (sum & 0xFF) as u8;
            self.state.update_zn(self.state.a);
        }
    }

    fn op_sbc(&mut self, val: u8) {
        if (self.state.p & flags::DECIMAL) != 0 {
            let carry = if (self.state.p & flags::CARRY) != 0 { 0u16 } else { 1u16 };
            let a = self.state.a as u16;
            let b = val as u16;
            let bin_diff = a.wrapping_sub(b).wrapping_sub(carry);
            let overflow = (((a ^ b) & 0x80) & ((a ^ bin_diff) & 0x80)) != 0;
            if overflow {
                self.state.p |= flags::OVERFLOW;
            } else {
                self.state.p &= !flags::OVERFLOW;
            }

            let mut lo = (a & 0x0F) as i16 - (b & 0x0F) as i16 - carry as i16;
            let mut hi = (a >> 4) as i16 - (b >> 4) as i16;
            if lo < 0 {
                lo -= 6;
                hi -= 1;
            }
            if hi < 0 {
                hi -= 6;
            }
            if bin_diff < 0x100 {
                self.state.p |= flags::CARRY;
            } else {
                self.state.p &= !flags::CARRY;
            }
            let res = ((hi as u8) << 4) | ((lo as u8) & 0x0F);
            self.state.a = res;
            self.state.update_zn(self.state.a);
        } else {
            self.op_adc(!val);
        }
    }

    fn op_cmp(&mut self, reg_val: u8, val: u8) {
        let diff = (reg_val as u16).wrapping_sub(val as u16);
        if reg_val >= val {
            self.state.p |= flags::CARRY;
        } else {
            self.state.p &= !flags::CARRY;
        }
        self.state.update_zn((diff & 0xFF) as u8);
    }
}

impl CpuEngine for Mos6502Cpu {
    fn arch(&self) -> Architecture {
        Architecture::Mos6502
    }

    fn endianness(&self) -> Endianness {
        Endianness::LittleEndian
    }

    fn stack_growth(&self) -> StackGrowth {
        StackGrowth::Downwards
    }

    fn word_size(&self) -> WordSize {
        WordSize::Bytes1
    }

    fn pc(&self) -> u64 {
        self.state.pc as u64
    }

    fn set_pc(&mut self, val: u64) {
        self.state.pc = (val & 0xFFFF) as u16;
    }

    fn sp(&self) -> u64 {
        Mos6502State::stack_address(self.state.sp)
    }

    fn set_sp(&mut self, val: u64) {
        self.state.sp = (val & 0xFF) as u8;
    }

    fn reset(&mut self) {
        self.state = Mos6502State {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            pc: 0x0600,
            p: flags::UNUSED | flags::INTERRUPT,
            halted: false,
        };
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let opcode = bus.read_u8(self.state.pc as u64)?;
        self.state.pc = self.state.pc.wrapping_add(1);

        match opcode {
            // NOP
            0xEA => Ok(StepOutcome::Continue { cycles: 2 }),

            // BRK
            0x00 => {
                let pc = self.state.pc.wrapping_add(1);
                self.push_u16(bus, pc)?;
                self.push_u8(bus, self.state.p | flags::BREAK | flags::UNUSED)?;
                self.state.p |= flags::INTERRUPT;
                let lo = bus.read_u8(0xFFFE)? as u16;
                let hi = bus.read_u8(0xFFFF)? as u16;
                let isr = (hi << 8) | lo;
                if isr != 0 {
                    self.state.pc = isr;
                } else {
                    self.state.halted = true;
                }
                Ok(StepOutcome::Interrupt(0))
            }

            // --- LDA ---
            0xA9 => {
                let val = self.read_imm(bus)?;
                self.state.a = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xA5 => {
                let addr = self.addr_zp(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.a = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0xB5 => {
                let addr = self.addr_zpx(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.a = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xAD => {
                let addr = self.addr_abs(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.a = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xBD => {
                let addr = self.addr_absx(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.a = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xB9 => {
                let addr = self.addr_absy(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.a = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xA1 => {
                let addr = self.addr_indx(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.a = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0xB1 => {
                let addr = self.addr_indy(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.a = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 5 })
            }

            // --- LDX ---
            0xA2 => {
                let val = self.read_imm(bus)?;
                self.state.x = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xA6 => {
                let addr = self.addr_zp(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.x = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0xB6 => {
                let addr = self.addr_zpy(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.x = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xAE => {
                let addr = self.addr_abs(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.x = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xBE => {
                let addr = self.addr_absy(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.x = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // --- LDY ---
            0xA0 => {
                let val = self.read_imm(bus)?;
                self.state.y = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xA4 => {
                let addr = self.addr_zp(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.y = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0xB4 => {
                let addr = self.addr_zpx(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.y = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xAC => {
                let addr = self.addr_abs(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.y = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xBC => {
                let addr = self.addr_absx(bus)?;
                let val = bus.read_u8(addr as u64)?;
                self.state.y = val;
                self.state.update_zn(val);
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // --- STA ---
            0x85 => {
                let addr = self.addr_zp(bus)?;
                bus.write_u8(addr as u64, self.state.a)?;
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0x95 => {
                let addr = self.addr_zpx(bus)?;
                bus.write_u8(addr as u64, self.state.a)?;
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x8D => {
                let addr = self.addr_abs(bus)?;
                bus.write_u8(addr as u64, self.state.a)?;
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x9D => {
                let addr = self.addr_absx(bus)?;
                bus.write_u8(addr as u64, self.state.a)?;
                Ok(StepOutcome::Continue { cycles: 5 })
            }
            0x99 => {
                let addr = self.addr_absy(bus)?;
                bus.write_u8(addr as u64, self.state.a)?;
                Ok(StepOutcome::Continue { cycles: 5 })
            }
            0x81 => {
                let addr = self.addr_indx(bus)?;
                bus.write_u8(addr as u64, self.state.a)?;
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x91 => {
                let addr = self.addr_indy(bus)?;
                bus.write_u8(addr as u64, self.state.a)?;
                Ok(StepOutcome::Continue { cycles: 6 })
            }

            // --- STX ---
            0x86 => {
                let addr = self.addr_zp(bus)?;
                bus.write_u8(addr as u64, self.state.x)?;
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0x96 => {
                let addr = self.addr_zpy(bus)?;
                bus.write_u8(addr as u64, self.state.x)?;
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x8E => {
                let addr = self.addr_abs(bus)?;
                bus.write_u8(addr as u64, self.state.x)?;
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // --- STY ---
            0x84 => {
                let addr = self.addr_zp(bus)?;
                bus.write_u8(addr as u64, self.state.y)?;
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0x94 => {
                let addr = self.addr_zpx(bus)?;
                bus.write_u8(addr as u64, self.state.y)?;
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x8C => {
                let addr = self.addr_abs(bus)?;
                bus.write_u8(addr as u64, self.state.y)?;
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // --- Register Transfers ---
            0xAA => {
                // TAX
                self.state.x = self.state.a;
                self.state.update_zn(self.state.x);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xA8 => {
                // TAY
                self.state.y = self.state.a;
                self.state.update_zn(self.state.y);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x8A => {
                // TXA
                self.state.a = self.state.x;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x98 => {
                // TYA
                self.state.a = self.state.y;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xBA => {
                // TSX
                self.state.x = self.state.sp;
                self.state.update_zn(self.state.x);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x9A => {
                // TXS
                self.state.sp = self.state.x;
                Ok(StepOutcome::Continue { cycles: 2 })
            }

            // --- Stack Operations ---
            0x48 => {
                // PHA
                self.push_u8(bus, self.state.a)?;
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0x08 => {
                // PHP
                self.push_u8(bus, self.state.p | flags::BREAK | flags::UNUSED)?;
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0x68 => {
                // PLA
                self.state.a = self.pull_u8(bus)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x28 => {
                // PLP
                self.state.p = (self.pull_u8(bus)? & !flags::BREAK) | flags::UNUSED;
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // --- ADC ---
            0x69 => {
                let v = self.read_imm(bus)?;
                self.op_adc(v);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x65 => {
                let addr = self.addr_zp(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_adc(v);
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0x75 => {
                let addr = self.addr_zpx(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_adc(v);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x6D => {
                let addr = self.addr_abs(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_adc(v);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x7D => {
                let addr = self.addr_absx(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_adc(v);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x79 => {
                let addr = self.addr_absy(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_adc(v);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x61 => {
                let addr = self.addr_indx(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_adc(v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x71 => {
                let addr = self.addr_indy(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_adc(v);
                Ok(StepOutcome::Continue { cycles: 5 })
            }

            // --- SBC ---
            0xE9 => {
                let v = self.read_imm(bus)?;
                self.op_sbc(v);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xE5 => {
                let addr = self.addr_zp(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_sbc(v);
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0xF5 => {
                let addr = self.addr_zpx(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_sbc(v);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xED => {
                let addr = self.addr_abs(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_sbc(v);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xFD => {
                let addr = self.addr_absx(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_sbc(v);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xF9 => {
                let addr = self.addr_absy(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_sbc(v);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xE1 => {
                let addr = self.addr_indx(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_sbc(v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0xF1 => {
                let addr = self.addr_indy(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_sbc(v);
                Ok(StepOutcome::Continue { cycles: 5 })
            }

            // --- CMP ---
            0xC9 => {
                let v = self.read_imm(bus)?;
                self.op_cmp(self.state.a, v);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xC5 => {
                let addr = self.addr_zp(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_cmp(self.state.a, v);
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0xD5 => {
                let addr = self.addr_zpx(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_cmp(self.state.a, v);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xCD => {
                let addr = self.addr_abs(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_cmp(self.state.a, v);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xDD => {
                let addr = self.addr_absx(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_cmp(self.state.a, v);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xD9 => {
                let addr = self.addr_absy(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_cmp(self.state.a, v);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xC1 => {
                let addr = self.addr_indx(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_cmp(self.state.a, v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0xD1 => {
                let addr = self.addr_indy(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_cmp(self.state.a, v);
                Ok(StepOutcome::Continue { cycles: 5 })
            }

            // --- CPX ---
            0xE0 => {
                let v = self.read_imm(bus)?;
                self.op_cmp(self.state.x, v);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xE4 => {
                let addr = self.addr_zp(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_cmp(self.state.x, v);
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0xEC => {
                let addr = self.addr_abs(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_cmp(self.state.x, v);
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // --- CPY ---
            0xC0 => {
                let v = self.read_imm(bus)?;
                self.op_cmp(self.state.y, v);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xC4 => {
                let addr = self.addr_zp(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_cmp(self.state.y, v);
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0xCC => {
                let addr = self.addr_abs(bus)?;
                let v = bus.read_u8(addr as u64)?;
                self.op_cmp(self.state.y, v);
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // --- AND / ORA / EOR ---
            0x29 => {
                let v = self.read_imm(bus)?;
                self.state.a &= v;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x25 => {
                let addr = self.addr_zp(bus)?;
                self.state.a &= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0x35 => {
                let addr = self.addr_zpx(bus)?;
                self.state.a &= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x2D => {
                let addr = self.addr_abs(bus)?;
                self.state.a &= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x3D => {
                let addr = self.addr_absx(bus)?;
                self.state.a &= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x39 => {
                let addr = self.addr_absy(bus)?;
                self.state.a &= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x21 => {
                let addr = self.addr_indx(bus)?;
                self.state.a &= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x31 => {
                let addr = self.addr_indy(bus)?;
                self.state.a &= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 5 })
            }

            0x09 => {
                let v = self.read_imm(bus)?;
                self.state.a |= v;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x05 => {
                let addr = self.addr_zp(bus)?;
                self.state.a |= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0x15 => {
                let addr = self.addr_zpx(bus)?;
                self.state.a |= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x0D => {
                let addr = self.addr_abs(bus)?;
                self.state.a |= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x1D => {
                let addr = self.addr_absx(bus)?;
                self.state.a |= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x19 => {
                let addr = self.addr_absy(bus)?;
                self.state.a |= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x01 => {
                let addr = self.addr_indx(bus)?;
                self.state.a |= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x11 => {
                let addr = self.addr_indy(bus)?;
                self.state.a |= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 5 })
            }

            0x49 => {
                let v = self.read_imm(bus)?;
                self.state.a ^= v;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x45 => {
                let addr = self.addr_zp(bus)?;
                self.state.a ^= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0x55 => {
                let addr = self.addr_zpx(bus)?;
                self.state.a ^= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x4D => {
                let addr = self.addr_abs(bus)?;
                self.state.a ^= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x5D => {
                let addr = self.addr_absx(bus)?;
                self.state.a ^= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x59 => {
                let addr = self.addr_absy(bus)?;
                self.state.a ^= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x41 => {
                let addr = self.addr_indx(bus)?;
                self.state.a ^= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x51 => {
                let addr = self.addr_indy(bus)?;
                self.state.a ^= bus.read_u8(addr as u64)?;
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 5 })
            }

            // --- BIT ---
            0x24 => {
                let addr = self.addr_zp(bus)?;
                let v = bus.read_u8(addr as u64)?;
                if (self.state.a & v) == 0 {
                    self.state.p |= flags::ZERO;
                } else {
                    self.state.p &= !flags::ZERO;
                }
                self.state.p = (self.state.p & 0x3F) | (v & 0xC0);
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0x2C => {
                let addr = self.addr_abs(bus)?;
                let v = bus.read_u8(addr as u64)?;
                if (self.state.a & v) == 0 {
                    self.state.p |= flags::ZERO;
                } else {
                    self.state.p &= !flags::ZERO;
                }
                self.state.p = (self.state.p & 0x3F) | (v & 0xC0);
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // --- Increments & Decrements ---
            0xE6 => {
                let addr = self.addr_zp(bus)?;
                let v = bus.read_u8(addr as u64)?.wrapping_add(1);
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 5 })
            }
            0xF6 => {
                let addr = self.addr_zpx(bus)?;
                let v = bus.read_u8(addr as u64)?.wrapping_add(1);
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0xEE => {
                let addr = self.addr_abs(bus)?;
                let v = bus.read_u8(addr as u64)?.wrapping_add(1);
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0xFE => {
                let addr = self.addr_absx(bus)?;
                let v = bus.read_u8(addr as u64)?.wrapping_add(1);
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 7 })
            }
            0xC6 => {
                let addr = self.addr_zp(bus)?;
                let v = bus.read_u8(addr as u64)?.wrapping_sub(1);
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 5 })
            }
            0xD6 => {
                let addr = self.addr_zpx(bus)?;
                let v = bus.read_u8(addr as u64)?.wrapping_sub(1);
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0xCE => {
                let addr = self.addr_abs(bus)?;
                let v = bus.read_u8(addr as u64)?.wrapping_sub(1);
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0xDE => {
                let addr = self.addr_absx(bus)?;
                let v = bus.read_u8(addr as u64)?.wrapping_sub(1);
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 7 })
            }
            0xE8 => {
                // INX
                self.state.x = self.state.x.wrapping_add(1);
                self.state.update_zn(self.state.x);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xC8 => {
                // INY
                self.state.y = self.state.y.wrapping_add(1);
                self.state.update_zn(self.state.y);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xCA => {
                // DEX
                self.state.x = self.state.x.wrapping_sub(1);
                self.state.update_zn(self.state.x);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x88 => {
                // DEY
                self.state.y = self.state.y.wrapping_sub(1);
                self.state.update_zn(self.state.y);
                Ok(StepOutcome::Continue { cycles: 2 })
            }

            // --- Shifts & Rotates ---
            0x0A => {
                // ASL A
                let bit7 = (self.state.a & 0x80) != 0;
                self.state.a <<= 1;
                if bit7 {
                    self.state.p |= flags::CARRY;
                } else {
                    self.state.p &= !flags::CARRY;
                }
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x06 => {
                // ASL zp
                let addr = self.addr_zp(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let bit7 = (v & 0x80) != 0;
                v <<= 1;
                if bit7 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 5 })
            }
            0x16 => {
                // ASL zpx
                let addr = self.addr_zpx(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let bit7 = (v & 0x80) != 0;
                v <<= 1;
                if bit7 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x0E => {
                // ASL abs
                let addr = self.addr_abs(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let bit7 = (v & 0x80) != 0;
                v <<= 1;
                if bit7 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x1E => {
                // ASL absx
                let addr = self.addr_absx(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let bit7 = (v & 0x80) != 0;
                v <<= 1;
                if bit7 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 7 })
            }

            0x4A => {
                // LSR A
                let bit0 = (self.state.a & 0x01) != 0;
                self.state.a >>= 1;
                if bit0 {
                    self.state.p |= flags::CARRY;
                } else {
                    self.state.p &= !flags::CARRY;
                }
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x46 => {
                // LSR zp
                let addr = self.addr_zp(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let bit0 = (v & 0x01) != 0;
                v >>= 1;
                if bit0 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 5 })
            }
            0x56 => {
                // LSR zpx
                let addr = self.addr_zpx(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let bit0 = (v & 0x01) != 0;
                v >>= 1;
                if bit0 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x4E => {
                // LSR abs
                let addr = self.addr_abs(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let bit0 = (v & 0x01) != 0;
                v >>= 1;
                if bit0 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x5E => {
                // LSR absx
                let addr = self.addr_absx(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let bit0 = (v & 0x01) != 0;
                v >>= 1;
                if bit0 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 7 })
            }

            0x2A => {
                // ROL A
                let old_c = if (self.state.p & flags::CARRY) != 0 { 1 } else { 0 };
                let bit7 = (self.state.a & 0x80) != 0;
                self.state.a = (self.state.a << 1) | old_c;
                if bit7 {
                    self.state.p |= flags::CARRY;
                } else {
                    self.state.p &= !flags::CARRY;
                }
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x26 => {
                // ROL zp
                let addr = self.addr_zp(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let old_c = if (self.state.p & flags::CARRY) != 0 { 1 } else { 0 };
                let bit7 = (v & 0x80) != 0;
                v = (v << 1) | old_c;
                if bit7 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 5 })
            }
            0x36 => {
                // ROL zpx
                let addr = self.addr_zpx(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let old_c = if (self.state.p & flags::CARRY) != 0 { 1 } else { 0 };
                let bit7 = (v & 0x80) != 0;
                v = (v << 1) | old_c;
                if bit7 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x2E => {
                // ROL abs
                let addr = self.addr_abs(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let old_c = if (self.state.p & flags::CARRY) != 0 { 1 } else { 0 };
                let bit7 = (v & 0x80) != 0;
                v = (v << 1) | old_c;
                if bit7 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x3E => {
                // ROL absx
                let addr = self.addr_absx(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let old_c = if (self.state.p & flags::CARRY) != 0 { 1 } else { 0 };
                let bit7 = (v & 0x80) != 0;
                v = (v << 1) | old_c;
                if bit7 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 7 })
            }

            0x6A => {
                // ROR A
                let old_c = if (self.state.p & flags::CARRY) != 0 { 0x80 } else { 0 };
                let bit0 = (self.state.a & 0x01) != 0;
                self.state.a = (self.state.a >> 1) | old_c;
                if bit0 {
                    self.state.p |= flags::CARRY;
                } else {
                    self.state.p &= !flags::CARRY;
                }
                self.state.update_zn(self.state.a);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x66 => {
                // ROR zp
                let addr = self.addr_zp(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let old_c = if (self.state.p & flags::CARRY) != 0 { 0x80 } else { 0 };
                let bit0 = (v & 0x01) != 0;
                v = (v >> 1) | old_c;
                if bit0 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 5 })
            }
            0x76 => {
                // ROR zpx
                let addr = self.addr_zpx(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let old_c = if (self.state.p & flags::CARRY) != 0 { 0x80 } else { 0 };
                let bit0 = (v & 0x01) != 0;
                v = (v >> 1) | old_c;
                if bit0 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x6E => {
                // ROR abs
                let addr = self.addr_abs(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let old_c = if (self.state.p & flags::CARRY) != 0 { 0x80 } else { 0 };
                let bit0 = (v & 0x01) != 0;
                v = (v >> 1) | old_c;
                if bit0 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x7E => {
                // ROR absx
                let addr = self.addr_absx(bus)?;
                let mut v = bus.read_u8(addr as u64)?;
                let old_c = if (self.state.p & flags::CARRY) != 0 { 0x80 } else { 0 };
                let bit0 = (v & 0x01) != 0;
                v = (v >> 1) | old_c;
                if bit0 { self.state.p |= flags::CARRY; } else { self.state.p &= !flags::CARRY; }
                bus.write_u8(addr as u64, v)?;
                self.state.update_zn(v);
                Ok(StepOutcome::Continue { cycles: 7 })
            }

            // --- Jumps & Calls ---
            0x4C => {
                // JMP abs
                let target = self.addr_abs(bus)?;
                self.state.pc = target;
                Ok(StepOutcome::Continue { cycles: 3 })
            }
            0x6C => {
                // JMP (ind)
                let target = self.addr_ind(bus)?;
                self.state.pc = target;
                Ok(StepOutcome::Continue { cycles: 5 })
            }
            0x20 => {
                // JSR abs
                let target = self.addr_abs(bus)?;
                let ret = self.state.pc.wrapping_sub(1);
                self.push_u16(bus, ret)?;
                self.state.pc = target;
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x60 => {
                // RTS
                let ret = self.pull_u16(bus)?;
                self.state.pc = ret.wrapping_add(1);
                Ok(StepOutcome::Continue { cycles: 6 })
            }
            0x40 => {
                // RTI
                self.state.p = (self.pull_u8(bus)? & !flags::BREAK) | flags::UNUSED;
                let ret = self.pull_u16(bus)?;
                self.state.pc = ret;
                Ok(StepOutcome::Continue { cycles: 6 })
            }

            // --- Branches ---
            0x90 => {
                // BCC
                let offset = self.read_imm(bus)? as i8;
                if (self.state.p & flags::CARRY) == 0 {
                    self.state.pc = ((self.state.pc as i32) + (offset as i32)) as u16;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xB0 => {
                // BCS
                let offset = self.read_imm(bus)? as i8;
                if (self.state.p & flags::CARRY) != 0 {
                    self.state.pc = ((self.state.pc as i32) + (offset as i32)) as u16;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xF0 => {
                // BEQ
                let offset = self.read_imm(bus)? as i8;
                if (self.state.p & flags::ZERO) != 0 {
                    self.state.pc = ((self.state.pc as i32) + (offset as i32)) as u16;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xD0 => {
                // BNE
                let offset = self.read_imm(bus)? as i8;
                if (self.state.p & flags::ZERO) == 0 {
                    self.state.pc = ((self.state.pc as i32) + (offset as i32)) as u16;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x30 => {
                // BMI
                let offset = self.read_imm(bus)? as i8;
                if (self.state.p & flags::NEGATIVE) != 0 {
                    self.state.pc = ((self.state.pc as i32) + (offset as i32)) as u16;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x10 => {
                // BPL
                let offset = self.read_imm(bus)? as i8;
                if (self.state.p & flags::NEGATIVE) == 0 {
                    self.state.pc = ((self.state.pc as i32) + (offset as i32)) as u16;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x50 => {
                // BVC
                let offset = self.read_imm(bus)? as i8;
                if (self.state.p & flags::OVERFLOW) == 0 {
                    self.state.pc = ((self.state.pc as i32) + (offset as i32)) as u16;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x70 => {
                // BVS
                let offset = self.read_imm(bus)? as i8;
                if (self.state.p & flags::OVERFLOW) != 0 {
                    self.state.pc = ((self.state.pc as i32) + (offset as i32)) as u16;
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }

            // --- Status Flag Instructions ---
            0x18 => {
                // CLC
                self.state.p &= !flags::CARRY;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x38 => {
                // SEC
                self.state.p |= flags::CARRY;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x58 => {
                // CLI
                self.state.p &= !flags::INTERRUPT;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x78 => {
                // SEI
                self.state.p |= flags::INTERRUPT;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xB8 => {
                // CLV
                self.state.p &= !flags::OVERFLOW;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xD8 => {
                // CLD
                self.state.p &= !flags::DECIMAL;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0xF8 => {
                // SED
                self.state.p |= flags::DECIMAL;
                Ok(StepOutcome::Continue { cycles: 2 })
            }

            // --- Unofficial SBC (0xEB) ---
            0xEB => {
                let v = self.read_imm(bus)?;
                self.op_sbc(v);
                Ok(StepOutcome::Continue { cycles: 2 })
            }

            // --- 1-byte NOPs ---
            0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => Ok(StepOutcome::Continue { cycles: 2 }),

            // --- 2-byte NOPs (DOP: ignores immediate/zp byte) ---
            0x04 | 0x14 | 0x34 | 0x44 | 0x54 | 0x64 | 0x74 | 0x80 | 0x82 | 0x89 | 0xC2 | 0xD4 | 0xE2 | 0xF4 => {
                let _ = self.read_imm(bus)?;
                Ok(StepOutcome::Continue { cycles: 3 })
            }

            // --- 3-byte NOPs (TOP: ignores 16-bit operand) ---
            0x0C | 0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => {
                let _ = self.read_u16_pc(bus)?;
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            _ => Err(CpuError::InvalidInstruction {
                opcode: opcode as u64,
                pc: self.state.pc.wrapping_sub(1) as u64,
            }),
        }
    }

    fn register_count(&self) -> usize {
        6
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0 => Some(RegisterInfo {
                name: "A",
                value: RegisterValue::U8(self.state.a),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            1 => Some(RegisterInfo {
                name: "X",
                value: RegisterValue::U8(self.state.x),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            2 => Some(RegisterInfo {
                name: "Y",
                value: RegisterValue::U8(self.state.y),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            3 => Some(RegisterInfo {
                name: "SP",
                value: RegisterValue::U8(self.state.sp),
                is_pc: false,
                is_sp: true,
                is_flags: false,
            }),
            4 => Some(RegisterInfo {
                name: "PC",
                value: RegisterValue::U16(self.state.pc),
                is_pc: true,
                is_sp: false,
                is_flags: false,
            }),
            5 => Some(RegisterInfo {
                name: "P",
                value: RegisterValue::U8(self.state.p),
                is_pc: false,
                is_sp: false,
                is_flags: true,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        if name.eq_ignore_ascii_case("A") {
            Some(self.state.a as u64)
        } else if name.eq_ignore_ascii_case("X") {
            Some(self.state.x as u64)
        } else if name.eq_ignore_ascii_case("Y") {
            Some(self.state.y as u64)
        } else if name.eq_ignore_ascii_case("SP") || name.eq_ignore_ascii_case("S") {
            Some(self.state.sp as u64)
        } else if name.eq_ignore_ascii_case("PC") {
            Some(self.state.pc as u64)
        } else if name.eq_ignore_ascii_case("P") || name.eq_ignore_ascii_case("FLAGS") {
            Some(self.state.p as u64)
        } else {
            None
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        if name.eq_ignore_ascii_case("A") {
            self.state.a = (val & 0xFF) as u8;
            Ok(())
        } else if name.eq_ignore_ascii_case("X") {
            self.state.x = (val & 0xFF) as u8;
            Ok(())
        } else if name.eq_ignore_ascii_case("Y") {
            self.state.y = (val & 0xFF) as u8;
            Ok(())
        } else if name.eq_ignore_ascii_case("SP") || name.eq_ignore_ascii_case("S") {
            self.state.sp = (val & 0xFF) as u8;
            Ok(())
        } else if name.eq_ignore_ascii_case("PC") {
            self.state.pc = (val & 0xFFFF) as u16;
            Ok(())
        } else if name.eq_ignore_ascii_case("P") || name.eq_ignore_ascii_case("FLAGS") {
            self.state.p = (val & 0xFF) as u8;
            Ok(())
        } else {
            Err(CpuError::RegisterNotFound)
        }
    }
}
