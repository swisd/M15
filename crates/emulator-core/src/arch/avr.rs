//! Atmel AVR (8-bit RISC) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// AVR Status Register (SREG) bit flags.
pub mod flags {
    pub const C: u8 = 1 << 0; // Carry Flag
    pub const Z: u8 = 1 << 1; // Zero Flag
    pub const N: u8 = 1 << 2; // Negative Flag
    pub const V: u8 = 1 << 3; // Two's Complement Overflow
    pub const S: u8 = 1 << 4; // Sign Bit (S = N ^ V)
    pub const H: u8 = 1 << 5; // Half Carry Flag
    pub const T: u8 = 1 << 6; // Transfer Bit
    pub const I: u8 = 1 << 7; // Global Interrupt Enable
}

/// AVR register state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct AvrState {
    pub r: [u8; 32], // r0-r31 (r26:r27=X, r28:r29=Y, r30:r31=Z)
    pub pc: u16,     // Byte address in code flash / memory
    pub sp: u16,     // Stack pointer (points to SRAM Data memory)
    pub sreg: u8,    // Status Register
    pub halted: bool,
}

impl AvrState {
    /// Returns 16-bit register pair X (r27:r26).
    #[inline]
    pub fn x(&self) -> u16 {
        ((self.r[27] as u16) << 8) | (self.r[26] as u16)
    }

    /// Sets 16-bit register pair X (r27:r26).
    #[inline]
    pub fn set_x(&mut self, val: u16) {
        self.r[26] = (val & 0xFF) as u8;
        self.r[27] = (val >> 8) as u8;
    }

    /// Returns 16-bit register pair Y (r29:r28).
    #[inline]
    pub fn y(&self) -> u16 {
        ((self.r[29] as u16) << 8) | (self.r[28] as u16)
    }

    /// Sets 16-bit register pair Y (r29:r28).
    #[inline]
    pub fn set_y(&mut self, val: u16) {
        self.r[28] = (val & 0xFF) as u8;
        self.r[29] = (val >> 8) as u8;
    }

    /// Returns 16-bit register pair Z (r31:r30).
    #[inline]
    pub fn z(&self) -> u16 {
        ((self.r[31] as u16) << 8) | (self.r[30] as u16)
    }

    /// Sets 16-bit register pair Z (r31:r30).
    #[inline]
    pub fn set_z(&mut self, val: u16) {
        self.r[30] = (val & 0xFF) as u8;
        self.r[31] = (val >> 8) as u8;
    }
}

/// AVR CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AvrCpu {
    pub state: AvrState,
}

impl AvrCpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    /// Pushes an 8-bit byte onto the SRAM stack.
    pub fn push_u8(&mut self, bus: &mut dyn MemoryBus, val: u8) -> Result<(), CpuError> {
        bus.write_u8(self.state.sp as u64, val)?;
        self.state.sp = self.state.sp.wrapping_sub(1);
        Ok(())
    }

    /// Pops an 8-bit byte from the SRAM stack.
    pub fn pop_u8(&mut self, bus: &mut dyn MemoryBus) -> Result<u8, CpuError> {
        self.state.sp = self.state.sp.wrapping_add(1);
        let val = bus.read_u8(self.state.sp as u64)?;
        Ok(val)
    }

    /// Pushes a 16-bit address onto the stack (high byte first, then low byte).
    pub fn push_u16(&mut self, bus: &mut dyn MemoryBus, val: u16) -> Result<(), CpuError> {
        self.push_u8(bus, (val >> 8) as u8)?;
        self.push_u8(bus, (val & 0xFF) as u8)?;
        Ok(())
    }

    /// Pops a 16-bit address from the stack (low byte first, then high byte).
    pub fn pop_u16(&mut self, bus: &mut dyn MemoryBus) -> Result<u16, CpuError> {
        let lo = self.pop_u8(bus)? as u16;
        let hi = self.pop_u8(bus)? as u16;
        Ok((hi << 8) | lo)
    }

    // --- Flag calculations ---

    fn update_flags_add(&mut self, rd: u8, rr: u8, carry_in: bool) -> u8 {
        let c_in = if carry_in && (self.state.sreg & flags::C) != 0 { 1u16 } else { 0u16 };
        let res16 = (rd as u16) + (rr as u16) + c_in;
        let res = (res16 & 0xFF) as u8;

        let rd3 = (rd >> 3) & 1;
        let rr3 = (rr >> 3) & 1;
        let r3 = (res >> 3) & 1;
        let h = ((rd3 & rr3) | (rr3 & !r3) | (!r3 & rd3)) != 0;

        let rd7 = (rd >> 7) & 1;
        let rr7 = (rr >> 7) & 1;
        let r7 = (res >> 7) & 1;
        let v = ((rd7 & rr7 & !r7) | (!rd7 & !rr7 & r7)) != 0;
        let n = r7 != 0;
        let z = res == 0;
        let c = res16 > 0xFF;
        let s = n ^ v;

        let mut sreg = self.state.sreg & !(flags::H | flags::V | flags::N | flags::Z | flags::C | flags::S);
        if h { sreg |= flags::H; }
        if v { sreg |= flags::V; }
        if n { sreg |= flags::N; }
        if z { sreg |= flags::Z; }
        if c { sreg |= flags::C; }
        if s { sreg |= flags::S; }
        self.state.sreg = sreg;

        res
    }

    fn update_flags_sub(&mut self, rd: u8, rr: u8, borrow_in: bool) -> u8 {
        let b_in = if borrow_in && (self.state.sreg & flags::C) != 0 { 1i16 } else { 0i16 };
        let res16 = (rd as i16) - (rr as i16) - b_in;
        let res = (res16 & 0xFF) as u8;

        let rd3 = (rd >> 3) & 1;
        let rr3 = (rr >> 3) & 1;
        let r3 = (res >> 3) & 1;
        let h = ((!rd3 & rr3) | (rr3 & r3) | (r3 & !rd3)) != 0;

        let rd7 = (rd >> 7) & 1;
        let rr7 = (rr >> 7) & 1;
        let r7 = (res >> 7) & 1;
        let v = ((rd7 & !rr7 & !r7) | (!rd7 & rr7 & r7)) != 0;
        let n = r7 != 0;
        let z = res == 0;
        let c = res16 < 0;
        let s = n ^ v;

        let mut sreg = self.state.sreg & !(flags::H | flags::V | flags::N | flags::Z | flags::C | flags::S);
        if h { sreg |= flags::H; }
        if v { sreg |= flags::V; }
        if n { sreg |= flags::N; }
        if z { sreg |= flags::Z; }
        if c { sreg |= flags::C; }
        if s { sreg |= flags::S; }
        self.state.sreg = sreg;

        res
    }

    fn update_flags_logic(&mut self, res: u8) {
        let n = (res & 0x80) != 0;
        let z = res == 0;
        let v = false;
        let s = n ^ v;

        let mut sreg = self.state.sreg & !(flags::V | flags::N | flags::Z | flags::S);
        if n { sreg |= flags::N; }
        if z { sreg |= flags::Z; }
        if s { sreg |= flags::S; }
        self.state.sreg = sreg;
    }
}

impl CpuEngine for AvrCpu {
    fn arch(&self) -> Architecture {
        Architecture::Avr
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
        self.state.sp as u64
    }

    fn set_sp(&mut self, val: u64) {
        self.state.sp = (val & 0xFFFF) as u16;
    }

    fn reset(&mut self) {
        self.state = AvrState {
            pc: 0x0000,
            sp: 0x08FF, // ATmega328P SRAM end
            sreg: 0,
            ..Default::default()
        };
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let pc = self.state.pc;
        let instr = bus.read_u16(pc as u64, Endianness::LittleEndian)?;
        self.state.pc = self.state.pc.wrapping_add(2);

        match instr {
            // NOP
            0x0000 => Ok(StepOutcome::Continue { cycles: 1 }),

            // SLEEP
            0x9588 => {
                self.state.halted = true;
                Ok(StepOutcome::Halted)
            }

            // BREAK
            0x9598 => Ok(StepOutcome::Breakpoint),

            // WDR
            0x95A8 => Ok(StepOutcome::Continue { cycles: 1 }),

            // RET
            0x9508 => {
                let target = self.pop_u16(bus)?;
                self.state.pc = target;
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // RETI
            0x9518 => {
                let target = self.pop_u16(bus)?;
                self.state.pc = target;
                self.state.sreg |= flags::I;
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            _ => {
                let top4 = (instr >> 12) & 0xF;

                // RJMP k (0xC000..0xCFFF)
                if top4 == 0xC {
                    let k = (instr & 0x0FFF) as i16;
                    let offset = if (k & 0x0800) != 0 {
                        (k | !0x0FFF) * 2
                    } else {
                        k * 2
                    };
                    self.state.pc = (self.state.pc as i32 + offset as i32) as u16;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }

                // RCALL k (0xD000..0xDFFF)
                if top4 == 0xD {
                    let k = (instr & 0x0FFF) as i16;
                    let offset = if (k & 0x0800) != 0 {
                        (k | !0x0FFF) * 2
                    } else {
                        k * 2
                    };
                    let ret_pc = self.state.pc;
                    self.push_u16(bus, ret_pc)?;
                    self.state.pc = (self.state.pc as i32 + offset as i32) as u16;
                    return Ok(StepOutcome::Continue { cycles: 3 });
                }

                // LDI Rd, K (0xE000..0xEFFF)
                if top4 == 0xE {
                    let d = (((instr >> 4) & 0x0F) + 16) as usize;
                    let k_hi = (instr >> 4) & 0xF0;
                    let k_lo = instr & 0x0F;
                    let k = (k_hi | k_lo) as u8;
                    self.state.r[d] = k;
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // Arithmetic & Logic with immediate (SUBI, SBCI, ORI, ANDI, CPI)
                if (0x3..=0x7).contains(&top4) {
                    let d = (((instr >> 4) & 0x0F) + 16) as usize;
                    let k_hi = (instr >> 4) & 0xF0;
                    let k_lo = instr & 0x0F;
                    let k = (k_hi | k_lo) as u8;
                    let rd = self.state.r[d];

                    match top4 {
                        0x3 => {
                            // CPI
                            self.update_flags_sub(rd, k, false);
                        }
                        0x4 => {
                            // SBCI
                            self.state.r[d] = self.update_flags_sub(rd, k, true);
                        }
                        0x5 => {
                            // SUBI
                            self.state.r[d] = self.update_flags_sub(rd, k, false);
                        }
                        0x6 => {
                            // ORI
                            let res = rd | k;
                            self.update_flags_logic(res);
                            self.state.r[d] = res;
                        }
                        0x7 => {
                            // ANDI
                            let res = rd & k;
                            self.update_flags_logic(res);
                            self.state.r[d] = res;
                        }
                        _ => {}
                    }
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // Branch on status bit (BRBS / BRBC 0xF000..0xF7FF)
                if top4 == 0xF && (instr & 0x0800) == 0 {
                    let bit = (instr & 0x07) as u8;
                    let is_clear = (instr & 0x0400) != 0;
                    let k = ((instr >> 3) & 0x7F) as i16;
                    let offset = if (k & 0x40) != 0 {
                        (k | !0x7F) * 2
                    } else {
                        k * 2
                    };

                    let flag_set = (self.state.sreg & (1 << bit)) != 0;
                    let take = if is_clear { !flag_set } else { flag_set };

                    if take {
                        self.state.pc = (self.state.pc as i32 + offset as i32) as u16;
                    }
                    return Ok(StepOutcome::Continue {
                        cycles: if take { 2 } else { 1 },
                    });
                }

                // Two-register arithmetic & logic (0x0000..0x2FFF)
                let top6 = (instr >> 10) & 0x3F;
                let d = ((instr >> 4) & 0x1F) as usize;
                let r = (((instr >> 5) & 0x10) | (instr & 0x0F)) as usize;

                match top6 {
                    0x01 => {
                        // MOVW Rd, Rr (0x0100)
                        let d_pair = ((instr >> 4) & 0x0F) as usize * 2;
                        let r_pair = (instr & 0x0F) as usize * 2;
                        self.state.r[d_pair] = self.state.r[r_pair];
                        self.state.r[d_pair + 1] = self.state.r[r_pair + 1];
                        return Ok(StepOutcome::Continue { cycles: 1 });
                    }
                    0x02 => {
                        // MULS / MUL
                        return Ok(StepOutcome::Continue { cycles: 2 });
                    }
                    0x03 => {
                        // ADD
                        let rd = self.state.r[d];
                        let rr = self.state.r[r];
                        self.state.r[d] = self.update_flags_add(rd, rr, false);
                        return Ok(StepOutcome::Continue { cycles: 1 });
                    }
                    0x04 => {
                        // CPC
                        let rd = self.state.r[d];
                        let rr = self.state.r[r];
                        self.update_flags_sub(rd, rr, true);
                        return Ok(StepOutcome::Continue { cycles: 1 });
                    }
                    0x05 => {
                        // CP
                        let rd = self.state.r[d];
                        let rr = self.state.r[r];
                        self.update_flags_sub(rd, rr, false);
                        return Ok(StepOutcome::Continue { cycles: 1 });
                    }
                    0x06 => {
                        // SBC
                        let rd = self.state.r[d];
                        let rr = self.state.r[r];
                        self.state.r[d] = self.update_flags_sub(rd, rr, true);
                        return Ok(StepOutcome::Continue { cycles: 1 });
                    }
                    0x07 => {
                        // SUB
                        let rd = self.state.r[d];
                        let rr = self.state.r[r];
                        self.state.r[d] = self.update_flags_sub(rd, rr, false);
                        return Ok(StepOutcome::Continue { cycles: 1 });
                    }
                    0x08 => {
                        // ADC
                        let rd = self.state.r[d];
                        let rr = self.state.r[r];
                        self.state.r[d] = self.update_flags_add(rd, rr, true);
                        return Ok(StepOutcome::Continue { cycles: 1 });
                    }
                    0x09 => {
                        // AND
                        let res = self.state.r[d] & self.state.r[r];
                        self.update_flags_logic(res);
                        self.state.r[d] = res;
                        return Ok(StepOutcome::Continue { cycles: 1 });
                    }
                    0x0A => {
                        // EOR
                        let res = self.state.r[d] ^ self.state.r[r];
                        self.update_flags_logic(res);
                        self.state.r[d] = res;
                        return Ok(StepOutcome::Continue { cycles: 1 });
                    }
                    0x0B => {
                        // OR
                        let res = self.state.r[d] | self.state.r[r];
                        self.update_flags_logic(res);
                        self.state.r[d] = res;
                        return Ok(StepOutcome::Continue { cycles: 1 });
                    }
                    0x0C => {
                        // MOV
                        self.state.r[d] = self.state.r[r];
                        return Ok(StepOutcome::Continue { cycles: 1 });
                    }
                    _ => {}
                }

                // IN / OUT (0xB000..0xBFFF)
                if top4 == 0xB {
                    let is_out = (instr & 0x0800) != 0;
                    let a = (((instr >> 5) & 0x30) | (instr & 0x0F)) as u64;
                    let reg = ((instr >> 4) & 0x1F) as usize;

                    if is_out {
                        bus.write_u8(a, self.state.r[reg])?;
                    } else {
                        self.state.r[reg] = bus.read_u8(a)?;
                    }
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // PUSH / POP (0x920F / 0x900F)
                if (instr & 0xFE0F) == 0x920F {
                    // PUSH
                    let r = ((instr >> 4) & 0x1F) as usize;
                    let val = self.state.r[r];
                    self.push_u8(bus, val)?;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFE0F) == 0x900F {
                    // POP
                    let d = ((instr >> 4) & 0x1F) as usize;
                    self.state.r[d] = self.pop_u8(bus)?;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }

                // Single register operations: COM, NEG, INC, DEC, SWAP, ASR, LSR, ROR (0x9400..0x950F)
                if (instr & 0xFE0F) == 0x9400 {
                    // COM
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let res = !self.state.r[d];
                    self.update_flags_logic(res);
                    self.state.sreg |= flags::C;
                    self.state.r[d] = res;
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }
                if (instr & 0xFE0F) == 0x9401 {
                    // NEG
                    let d = ((instr >> 4) & 0x1F) as usize;
                    self.state.r[d] = self.update_flags_sub(0, self.state.r[d], false);
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }
                if (instr & 0xFE0F) == 0x9402 {
                    // SWAP
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let val = self.state.r[d];
                    self.state.r[d] = val.rotate_right(4);
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }
                if (instr & 0xFE0F) == 0x9403 {
                    // INC
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let c = (self.state.sreg & flags::C) != 0;
                    let res = self.update_flags_add(self.state.r[d], 1, false);
                    if c { self.state.sreg |= flags::C; } else { self.state.sreg &= !flags::C; }
                    self.state.r[d] = res;
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }
                if (instr & 0xFE0F) == 0x9405 {
                    // ASR
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let val = self.state.r[d];
                    let bit0 = (val & 1) != 0;
                    let res = (val >> 1) | (val & 0x80);
                    let n = (res & 0x80) != 0;
                    let z = res == 0;
                    let c = bit0;
                    let v = n ^ c;
                    let s = n ^ v;
                    let mut sreg = self.state.sreg & !(flags::N | flags::Z | flags::C | flags::V | flags::S);
                    if n { sreg |= flags::N; }
                    if z { sreg |= flags::Z; }
                    if c { sreg |= flags::C; }
                    if v { sreg |= flags::V; }
                    if s { sreg |= flags::S; }
                    self.state.sreg = sreg;
                    self.state.r[d] = res;
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }
                if (instr & 0xFE0F) == 0x9406 {
                    // LSR
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let val = self.state.r[d];
                    let bit0 = (val & 1) != 0;
                    let res = val >> 1;
                    let n = false;
                    let z = res == 0;
                    let c = bit0;
                    let v = n ^ c;
                    let s = n ^ v;
                    let mut sreg = self.state.sreg & !(flags::N | flags::Z | flags::C | flags::V | flags::S);
                    if z { sreg |= flags::Z; }
                    if c { sreg |= flags::C; }
                    if v { sreg |= flags::V; }
                    if s { sreg |= flags::S; }
                    self.state.sreg = sreg;
                    self.state.r[d] = res;
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }
                if (instr & 0xFE0F) == 0x9407 {
                    // ROR
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let val = self.state.r[d];
                    let old_c = if (self.state.sreg & flags::C) != 0 { 0x80 } else { 0 };
                    let bit0 = (val & 1) != 0;
                    let res = (val >> 1) | old_c;
                    let n = (res & 0x80) != 0;
                    let z = res == 0;
                    let c = bit0;
                    let v = n ^ c;
                    let s = n ^ v;
                    let mut sreg = self.state.sreg & !(flags::N | flags::Z | flags::C | flags::V | flags::S);
                    if n { sreg |= flags::N; }
                    if z { sreg |= flags::Z; }
                    if c { sreg |= flags::C; }
                    if v { sreg |= flags::V; }
                    if s { sreg |= flags::S; }
                    self.state.sreg = sreg;
                    self.state.r[d] = res;
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }
                if (instr & 0xFE0F) == 0x940A {
                    // DEC
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let c = (self.state.sreg & flags::C) != 0;
                    let res = self.update_flags_sub(self.state.r[d], 1, false);
                    if c { self.state.sreg |= flags::C; } else { self.state.sreg &= !flags::C; }
                    self.state.r[d] = res;
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // Indirect Jumps & Calls (IJMP, ICALL)
                if instr == 0x9409 {
                    // IJMP
                    self.state.pc = self.state.z() * 2;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if instr == 0x9509 {
                    // ICALL
                    let ret_pc = self.state.pc;
                    self.push_u16(bus, ret_pc)?;
                    self.state.pc = self.state.z() * 2;
                    return Ok(StepOutcome::Continue { cycles: 3 });
                }

                // 32-bit JMP & CALL (0x940C, 0x940E)
                if (instr & 0xFE0E) == 0x940C {
                    // JMP k (32-bit)
                    let k_hi = ((instr >> 4) & 0x1F) | ((instr & 1) << 5);
                    let k_lo = bus.read_u16(self.state.pc as u64, Endianness::LittleEndian)?;
                    self.state.pc = self.state.pc.wrapping_add(2);
                    let target = (((k_hi as u32) << 16) | (k_lo as u32)) * 2;
                    self.state.pc = target as u16;
                    return Ok(StepOutcome::Continue { cycles: 3 });
                }
                if (instr & 0xFE0E) == 0x940E {
                    // CALL k (32-bit)
                    let k_hi = ((instr >> 4) & 0x1F) | ((instr & 1) << 5);
                    let k_lo = bus.read_u16(self.state.pc as u64, Endianness::LittleEndian)?;
                    self.state.pc = self.state.pc.wrapping_add(2);
                    let ret_pc = self.state.pc;
                    self.push_u16(bus, ret_pc)?;
                    let target = (((k_hi as u32) << 16) | (k_lo as u32)) * 2;
                    self.state.pc = target as u16;
                    return Ok(StepOutcome::Continue { cycles: 4 });
                }

                // ADIW / SBIW (0x9600, 0x9700)
                if (instr & 0xFF00) == 0x9600 {
                    // ADIW Rd, K
                    let d = 24 + (((instr >> 4) & 3) * 2) as usize;
                    let k = ((instr >> 2) & 0x30) | (instr & 0x0F);
                    let val = (self.state.r[d] as u16) | ((self.state.r[d + 1] as u16) << 8);
                    let sum = val.wrapping_add(k);
                    let r15 = (sum & 0x8000) != 0;
                    let d15 = (val & 0x8000) != 0;
                    let v = !d15 & r15;
                    let n = r15;
                    let z = sum == 0;
                    let c = !r15 & d15;
                    let s = n ^ v;

                    let mut sreg = self.state.sreg & !(flags::S | flags::V | flags::N | flags::Z | flags::C);
                    if s { sreg |= flags::S; }
                    if v { sreg |= flags::V; }
                    if n { sreg |= flags::N; }
                    if z { sreg |= flags::Z; }
                    if c { sreg |= flags::C; }
                    self.state.sreg = sreg;

                    self.state.r[d] = (sum & 0xFF) as u8;
                    self.state.r[d + 1] = (sum >> 8) as u8;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFF00) == 0x9700 {
                    // SBIW Rd, K
                    let d = 24 + (((instr >> 4) & 3) * 2) as usize;
                    let k = ((instr >> 2) & 0x30) | (instr & 0x0F);
                    let val = (self.state.r[d] as u16) | ((self.state.r[d + 1] as u16) << 8);
                    let diff = val.wrapping_sub(k);
                    let r15 = (diff & 0x8000) != 0;
                    let d15 = (val & 0x8000) != 0;
                    let v = d15 & !r15;
                    let n = r15;
                    let z = diff == 0;
                    let c = r15 & !d15;
                    let s = n ^ v;

                    let mut sreg = self.state.sreg & !(flags::S | flags::V | flags::N | flags::Z | flags::C);
                    if s { sreg |= flags::S; }
                    if v { sreg |= flags::V; }
                    if n { sreg |= flags::N; }
                    if z { sreg |= flags::Z; }
                    if c { sreg |= flags::C; }
                    self.state.sreg = sreg;

                    self.state.r[d] = (diff & 0xFF) as u8;
                    self.state.r[d + 1] = (diff >> 8) as u8;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }

                // CPSE Rd, Rr (0x1000..0x13FF)
                if (instr & 0xFC00) == 0x1000 {
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let r = (((instr >> 5) & 0x10) | (instr & 0x0F)) as usize;
                    if self.state.r[d] == self.state.r[r] {
                        let next_instr = bus.read_u16(self.state.pc as u64, Endianness::LittleEndian)?;
                        let is_32bit = (next_instr & 0xFE0E) == 0x940C || (next_instr & 0xFE0E) == 0x940E || (next_instr & 0xFE0F) == 0x9000 || (next_instr & 0xFE0F) == 0x9200;
                        let skip = if is_32bit { 4 } else { 2 };
                        self.state.pc = self.state.pc.wrapping_add(skip);
                        return Ok(StepOutcome::Continue { cycles: if is_32bit { 3 } else { 2 } });
                    }
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // SBRC / SBRS (0xFC00, 0xFE00)
                if (instr & 0xFE08) == 0xFC00 {
                    // SBRC
                    let r = ((instr >> 4) & 0x1F) as usize;
                    let bit = (instr & 7) as u8;
                    if (self.state.r[r] & (1 << bit)) == 0 {
                        let next_instr = bus.read_u16(self.state.pc as u64, Endianness::LittleEndian)?;
                        let is_32bit = (next_instr & 0xFE0E) == 0x940C || (next_instr & 0xFE0E) == 0x940E || (next_instr & 0xFE0F) == 0x9000 || (next_instr & 0xFE0F) == 0x9200;
                        let skip = if is_32bit { 4 } else { 2 };
                        self.state.pc = self.state.pc.wrapping_add(skip);
                        return Ok(StepOutcome::Continue { cycles: if is_32bit { 3 } else { 2 } });
                    }
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }
                if (instr & 0xFE08) == 0xFE00 {
                    // SBRS
                    let r = ((instr >> 4) & 0x1F) as usize;
                    let bit = (instr & 7) as u8;
                    if (self.state.r[r] & (1 << bit)) != 0 {
                        let next_instr = bus.read_u16(self.state.pc as u64, Endianness::LittleEndian)?;
                        let is_32bit = (next_instr & 0xFE0E) == 0x940C || (next_instr & 0xFE0E) == 0x940E || (next_instr & 0xFE0F) == 0x9000 || (next_instr & 0xFE0F) == 0x9200;
                        let skip = if is_32bit { 4 } else { 2 };
                        self.state.pc = self.state.pc.wrapping_add(skip);
                        return Ok(StepOutcome::Continue { cycles: if is_32bit { 3 } else { 2 } });
                    }
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // BST / BLD (0xFA00, 0xF800)
                if (instr & 0xFE08) == 0xFA00 {
                    // BST Rd, b
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let bit = (instr & 7) as u8;
                    if (self.state.r[d] & (1 << bit)) != 0 {
                        self.state.sreg |= flags::T;
                    } else {
                        self.state.sreg &= !flags::T;
                    }
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }
                if (instr & 0xFE08) == 0xF800 {
                    // BLD Rd, b
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let bit = (instr & 7) as u8;
                    if (self.state.sreg & flags::T) != 0 {
                        self.state.r[d] |= 1 << bit;
                    } else {
                        self.state.r[d] &= !(1 << bit);
                    }
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // SBI / CBI / SBIS / SBIC (0x9A00, 0x9800, 0x9B00, 0x9900)
                if (instr & 0xFF00) == 0x9A00 {
                    // SBI A, b
                    let a = ((instr >> 3) & 0x1F) as u64;
                    let b = (instr & 7) as u8;
                    let val = bus.read_u8(a)?;
                    bus.write_u8(a, val | (1 << b))?;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFF00) == 0x9800 {
                    // CBI A, b
                    let a = ((instr >> 3) & 0x1F) as u64;
                    let b = (instr & 7) as u8;
                    let val = bus.read_u8(a)?;
                    bus.write_u8(a, val & !(1 << b))?;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFF00) == 0x9B00 {
                    // SBIS A, b
                    let a = ((instr >> 3) & 0x1F) as u64;
                    let b = (instr & 7) as u8;
                    let val = bus.read_u8(a)?;
                    if (val & (1 << b)) != 0 {
                        let next_instr = bus.read_u16(self.state.pc as u64, Endianness::LittleEndian)?;
                        let is_32bit = (next_instr & 0xFE0E) == 0x940C || (next_instr & 0xFE0E) == 0x940E || (next_instr & 0xFE0F) == 0x9000 || (next_instr & 0xFE0F) == 0x9200;
                        let skip = if is_32bit { 4 } else { 2 };
                        self.state.pc = self.state.pc.wrapping_add(skip);
                        return Ok(StepOutcome::Continue { cycles: if is_32bit { 3 } else { 2 } });
                    }
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }
                if (instr & 0xFF00) == 0x9900 {
                    // SBIC A, b
                    let a = ((instr >> 3) & 0x1F) as u64;
                    let b = (instr & 7) as u8;
                    let val = bus.read_u8(a)?;
                    if (val & (1 << b)) == 0 {
                        let next_instr = bus.read_u16(self.state.pc as u64, Endianness::LittleEndian)?;
                        let is_32bit = (next_instr & 0xFE0E) == 0x940C || (next_instr & 0xFE0E) == 0x940E || (next_instr & 0xFE0F) == 0x9000 || (next_instr & 0xFE0F) == 0x9200;
                        let skip = if is_32bit { 4 } else { 2 };
                        self.state.pc = self.state.pc.wrapping_add(skip);
                        return Ok(StepOutcome::Continue { cycles: if is_32bit { 3 } else { 2 } });
                    }
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                // MUL / MULS / MULSU (0x9C00, 0x0200, 0x0300)
                if (instr & 0xFC00) == 0x9C00 {
                    // MUL Rd, Rr (unsigned)
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let r = (((instr >> 5) & 0x10) | (instr & 0x0F)) as usize;
                    let prod = (self.state.r[d] as u16) * (self.state.r[r] as u16);
                    self.state.r[0] = (prod & 0xFF) as u8;
                    self.state.r[1] = (prod >> 8) as u8;
                    let z = prod == 0;
                    let c = (prod & 0x8000) != 0;
                    if z { self.state.sreg |= flags::Z; } else { self.state.sreg &= !flags::Z; }
                    if c { self.state.sreg |= flags::C; } else { self.state.sreg &= !flags::C; }
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFF00) == 0x0200 {
                    // MULS Rd, Rr (signed)
                    let d = 16 + (((instr >> 4) & 0x0F) as usize);
                    let r = 16 + ((instr & 0x0F) as usize);
                    let prod = (self.state.r[d] as i8 as i16) * (self.state.r[r] as i8 as i16);
                    self.state.r[0] = (prod as u16 & 0xFF) as u8;
                    self.state.r[1] = ((prod as u16) >> 8) as u8;
                    let z = prod == 0;
                    let c = ((prod as u16) & 0x8000) != 0;
                    if z { self.state.sreg |= flags::Z; } else { self.state.sreg &= !flags::Z; }
                    if c { self.state.sreg |= flags::C; } else { self.state.sreg &= !flags::C; }
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }

                // Pointer LD / ST operations (X, Y, Z) & Displacement (LDD / STD)
                if (instr & 0xFE0F) == 0x900C {
                    // LD Rd, X
                    let d = ((instr >> 4) & 0x1F) as usize;
                    self.state.r[d] = bus.read_u8(self.state.x() as u64)?;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFE0F) == 0x900D {
                    // LD Rd, X+
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let x = self.state.x();
                    self.state.r[d] = bus.read_u8(x as u64)?;
                    self.state.set_x(x.wrapping_add(1));
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFE0F) == 0x900E {
                    // LD Rd, -X
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let x = self.state.x().wrapping_sub(1);
                    self.state.set_x(x);
                    self.state.r[d] = bus.read_u8(x as u64)?;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }

                if (instr & 0xFE0F) == 0x920C {
                    // ST X, Rr
                    let r = ((instr >> 4) & 0x1F) as usize;
                    bus.write_u8(self.state.x() as u64, self.state.r[r])?;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFE0F) == 0x920D {
                    // ST X+, Rr
                    let r = ((instr >> 4) & 0x1F) as usize;
                    let x = self.state.x();
                    bus.write_u8(x as u64, self.state.r[r])?;
                    self.state.set_x(x.wrapping_add(1));
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFE0F) == 0x920E {
                    // ST -X, Rr
                    let r = ((instr >> 4) & 0x1F) as usize;
                    let x = self.state.x().wrapping_sub(1);
                    self.state.set_x(x);
                    bus.write_u8(x as u64, self.state.r[r])?;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }

                // Y & Z pointer LD / ST
                if (instr & 0xFE0F) == 0x9009 {
                    // LD Rd, Y+
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let y = self.state.y();
                    self.state.r[d] = bus.read_u8(y as u64)?;
                    self.state.set_y(y.wrapping_add(1));
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFE0F) == 0x900A {
                    // LD Rd, -Y
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let y = self.state.y().wrapping_sub(1);
                    self.state.set_y(y);
                    self.state.r[d] = bus.read_u8(y as u64)?;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFE0F) == 0x9209 {
                    // ST Y+, Rr
                    let r = ((instr >> 4) & 0x1F) as usize;
                    let y = self.state.y();
                    bus.write_u8(y as u64, self.state.r[r])?;
                    self.state.set_y(y.wrapping_add(1));
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFE0F) == 0x920A {
                    // ST -Y, Rr
                    let r = ((instr >> 4) & 0x1F) as usize;
                    let y = self.state.y().wrapping_sub(1);
                    self.state.set_y(y);
                    bus.write_u8(y as u64, self.state.r[r])?;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }

                if (instr & 0xFE0F) == 0x9001 {
                    // LD Rd, Z+
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let z = self.state.z();
                    self.state.r[d] = bus.read_u8(z as u64)?;
                    self.state.set_z(z.wrapping_add(1));
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFE0F) == 0x9002 {
                    // LD Rd, -Z
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let z = self.state.z().wrapping_sub(1);
                    self.state.set_z(z);
                    self.state.r[d] = bus.read_u8(z as u64)?;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFE0F) == 0x9201 {
                    // ST Z+, Rr
                    let r = ((instr >> 4) & 0x1F) as usize;
                    let z = self.state.z();
                    bus.write_u8(z as u64, self.state.r[r])?;
                    self.state.set_z(z.wrapping_add(1));
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }
                if (instr & 0xFE0F) == 0x9202 {
                    // ST -Z, Rr
                    let r = ((instr >> 4) & 0x1F) as usize;
                    let z = self.state.z().wrapping_sub(1);
                    self.state.set_z(z);
                    bus.write_u8(z as u64, self.state.r[r])?;
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }

                // LDD / STD with Y and Z (0x8000..0x8FEF)
                if (instr & 0xD200) == 0x8000 {
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let q = ((instr >> 8) & 0x20) | ((instr >> 7) & 0x18) | (instr & 7);
                    let is_y = (instr & 0x0008) != 0;
                    let is_st = (instr & 0x0200) != 0;
                    let base_ptr = if is_y { self.state.y() } else { self.state.z() };
                    let addr = base_ptr.wrapping_add(q) as u64;

                    if is_st {
                        bus.write_u8(addr, self.state.r[d])?;
                    } else {
                        self.state.r[d] = bus.read_u8(addr)?;
                    }
                    return Ok(StepOutcome::Continue { cycles: 2 });
                }

                // LPM (0x95C8, 0x9004, 0x9005)
                if instr == 0x95C8 {
                    // LPM r0, Z
                    self.state.r[0] = bus.read_u8(self.state.z() as u64)?;
                    return Ok(StepOutcome::Continue { cycles: 3 });
                }
                if (instr & 0xFE0F) == 0x9004 {
                    // LPM Rd, Z
                    let d = ((instr >> 4) & 0x1F) as usize;
                    self.state.r[d] = bus.read_u8(self.state.z() as u64)?;
                    return Ok(StepOutcome::Continue { cycles: 3 });
                }
                if (instr & 0xFE0F) == 0x9005 {
                    // LPM Rd, Z+
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let z = self.state.z();
                    self.state.r[d] = bus.read_u8(z as u64)?;
                    self.state.set_z(z.wrapping_add(1));
                    return Ok(StepOutcome::Continue { cycles: 3 });
                }

                // 32-bit LDS / STS
                if (instr & 0xFE0F) == 0x9000 {
                    // LDS Rd, k
                    let d = ((instr >> 4) & 0x1F) as usize;
                    let addr = bus.read_u16(self.state.pc as u64, Endianness::LittleEndian)?;
                    self.state.pc = self.state.pc.wrapping_add(2);
                    self.state.r[d] = bus.read_u8(addr as u64)?;
                    return Ok(StepOutcome::Continue { cycles: 3 });
                }
                if (instr & 0xFE0F) == 0x9200 {
                    // STS k, Rr
                    let r = ((instr >> 4) & 0x1F) as usize;
                    let addr = bus.read_u16(self.state.pc as u64, Endianness::LittleEndian)?;
                    self.state.pc = self.state.pc.wrapping_add(2);
                    bus.write_u8(addr as u64, self.state.r[r])?;
                    return Ok(StepOutcome::Continue { cycles: 3 });
                }

                // BSET s / BCLR s
                if (instr & 0xFF8F) == 0x9408 {
                    let s = ((instr >> 4) & 0x07) as u8;
                    self.state.sreg |= 1 << s;
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }
                if (instr & 0xFF8F) == 0x9488 {
                    let s = ((instr >> 4) & 0x07) as u8;
                    self.state.sreg &= !(1 << s);
                    return Ok(StepOutcome::Continue { cycles: 1 });
                }

                Err(CpuError::InvalidInstruction {
                    opcode: instr as u64,
                    pc: pc as u64,
                })
            }
        }
    }

    fn register_count(&self) -> usize {
        35
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0..=31 => {
                const REG_NAMES: [&str; 32] = [
                    "r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10", "r11",
                    "r12", "r13", "r14", "r15", "r16", "r17", "r18", "r19", "r20", "r21", "r22",
                    "r23", "r24", "r25", "r26/XL", "r27/XH", "r28/YL", "r29/YH", "r30/ZL",
                    "r31/ZH",
                ];
                Some(RegisterInfo {
                    name: REG_NAMES[index],
                    value: RegisterValue::U8(self.state.r[index]),
                    is_pc: false,
                    is_sp: false,
                    is_flags: false,
                })
            }
            32 => Some(RegisterInfo {
                name: "PC",
                value: RegisterValue::U16(self.state.pc),
                is_pc: true,
                is_sp: false,
                is_flags: false,
            }),
            33 => Some(RegisterInfo {
                name: "SP",
                value: RegisterValue::U16(self.state.sp),
                is_pc: false,
                is_sp: true,
                is_flags: false,
            }),
            34 => Some(RegisterInfo {
                name: "SREG",
                value: RegisterValue::U8(self.state.sreg),
                is_pc: false,
                is_sp: false,
                is_flags: true,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        if name.eq_ignore_ascii_case("PC") {
            return Some(self.state.pc as u64);
        }
        if name.eq_ignore_ascii_case("SP") {
            return Some(self.state.sp as u64);
        }
        if name.eq_ignore_ascii_case("SREG") {
            return Some(self.state.sreg as u64);
        }
        if name.eq_ignore_ascii_case("X") {
            return Some(self.state.x() as u64);
        }
        if name.eq_ignore_ascii_case("Y") {
            return Some(self.state.y() as u64);
        }
        if name.eq_ignore_ascii_case("Z") {
            return Some(self.state.z() as u64);
        }
        for i in 0..32 {
            let reg_name = match i {
                0 => "r0", 1 => "r1", 2 => "r2", 3 => "r3", 4 => "r4", 5 => "r5",
                6 => "r6", 7 => "r7", 8 => "r8", 9 => "r9", 10 => "r10", 11 => "r11",
                12 => "r12", 13 => "r13", 14 => "r14", 15 => "r15", 16 => "r16", 17 => "r17",
                18 => "r18", 19 => "r19", 20 => "r20", 21 => "r21", 22 => "r22", 23 => "r23",
                24 => "r24", 25 => "r25", 26 => "r26", 27 => "r27", 28 => "r28", 29 => "r29",
                30 => "r30", 31 => "r31",
                _ => "",
            };
            if name.eq_ignore_ascii_case(reg_name) {
                return Some(self.state.r[i] as u64);
            }
        }
        None
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        if name.eq_ignore_ascii_case("PC") {
            self.state.pc = (val & 0xFFFF) as u16;
            return Ok(());
        }
        if name.eq_ignore_ascii_case("SP") {
            self.state.sp = (val & 0xFFFF) as u16;
            return Ok(());
        }
        if name.eq_ignore_ascii_case("SREG") {
            self.state.sreg = (val & 0xFF) as u8;
            return Ok(());
        }
        if name.eq_ignore_ascii_case("X") {
            self.state.set_x((val & 0xFFFF) as u16);
            return Ok(());
        }
        if name.eq_ignore_ascii_case("Y") {
            self.state.set_y((val & 0xFFFF) as u16);
            return Ok(());
        }
        if name.eq_ignore_ascii_case("Z") {
            self.state.set_z((val & 0xFFFF) as u16);
            return Ok(());
        }

        // Check r0-r31
        for i in 0..32 {
            let reg_name = match i {
                0 => "r0", 1 => "r1", 2 => "r2", 3 => "r3", 4 => "r4", 5 => "r5",
                6 => "r6", 7 => "r7", 8 => "r8", 9 => "r9", 10 => "r10", 11 => "r11",
                12 => "r12", 13 => "r13", 14 => "r14", 15 => "r15", 16 => "r16", 17 => "r17",
                18 => "r18", 19 => "r19", 20 => "r20", 21 => "r21", 22 => "r22", 23 => "r23",
                24 => "r24", 25 => "r25", 26 => "r26", 27 => "r27", 28 => "r28", 29 => "r29",
                30 => "r30", 31 => "r31",
                _ => "",
            };
            if name.eq_ignore_ascii_case(reg_name) {
                self.state.r[i] = (val & 0xFF) as u8;
                return Ok(());
            }
        }

        Err(CpuError::RegisterNotFound)
    }
}
