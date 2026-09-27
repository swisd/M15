//! Intel 8086 CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// 8086 Flags bitmask constants.
pub mod flags {
    pub const CF: u16 = 1 << 0;  // Carry
    pub const PF: u16 = 1 << 2;  // Parity
    pub const AF: u16 = 1 << 4;  // Auxiliary Carry
    pub const ZF: u16 = 1 << 6;  // Zero
    pub const SF: u16 = 1 << 7;  // Sign
    pub const TF: u16 = 1 << 8;  // Trap
    pub const IF: u16 = 1 << 9;  // Interrupt Enable
    pub const DF: u16 = 1 << 10; // Direction
    pub const OF: u16 = 1 << 11; // Overflow
}

/// Intel 8086 register state.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct I8086State {
    pub ax: u16,
    pub bx: u16,
    pub cx: u16,
    pub dx: u16,
    pub si: u16,
    pub di: u16,
    pub bp: u16,
    pub sp: u16,
    pub cs: u16,
    pub ds: u16,
    pub ss: u16,
    pub es: u16,
    pub ip: u16,
    pub flags: u16,
    pub halted: bool,
}

impl I8086State {
    /// Computes the 20-bit linear physical address from segment and offset.
    #[inline]
    pub const fn linear_address(seg: u16, off: u16) -> u32 {
        ((seg as u32) << 4).wrapping_add(off as u32) & 0xFFFFF
    }

    /// Computes parity flag for lower 8-bit byte.
    #[inline]
    pub fn parity(b: u8) -> bool {
        (b.count_ones() & 1) == 0
    }
}

/// Intel 8086 CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct I8086Cpu {
    pub state: I8086State,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum EffectiveAddress {
    Register(u8),
    Memory { seg: u16, offset: u16 },
}

impl I8086Cpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    /// Pushes a 16-bit word onto the stack.
    pub fn push_u16(&mut self, bus: &mut dyn MemoryBus, val: u16) -> Result<(), CpuError> {
        self.state.sp = self.state.sp.wrapping_sub(2);
        let linear = I8086State::linear_address(self.state.ss, self.state.sp);
        bus.write_u16(linear as u64, val, Endianness::LittleEndian)?;
        Ok(())
    }

    /// Pops a 16-bit word from the stack.
    pub fn pop_u16(&mut self, bus: &mut dyn MemoryBus) -> Result<u16, CpuError> {
        let linear = I8086State::linear_address(self.state.ss, self.state.sp);
        let val = bus.read_u16(linear as u64, Endianness::LittleEndian)?;
        self.state.sp = self.state.sp.wrapping_add(2);
        Ok(val)
    }

    // --- Memory fetch helpers ---

    fn fetch_u8(&mut self, bus: &mut dyn MemoryBus) -> Result<u8, CpuError> {
        let addr = I8086State::linear_address(self.state.cs, self.state.ip) as u64;
        self.state.ip = self.state.ip.wrapping_add(1);
        let val = bus.read_u8(addr)?;
        Ok(val)
    }

    fn fetch_u16(&mut self, bus: &mut dyn MemoryBus) -> Result<u16, CpuError> {
        let lo = self.fetch_u8(bus)? as u16;
        let hi = self.fetch_u8(bus)? as u16;
        Ok((hi << 8) | lo)
    }

    // --- Register helpers ---

    pub fn get_reg16(&self, reg: u8) -> u16 {
        match reg & 7 {
            0 => self.state.ax,
            1 => self.state.cx,
            2 => self.state.dx,
            3 => self.state.bx,
            4 => self.state.sp,
            5 => self.state.bp,
            6 => self.state.si,
            7 => self.state.di,
            _ => 0,
        }
    }

    pub fn set_reg16(&mut self, reg: u8, val: u16) {
        match reg & 7 {
            0 => self.state.ax = val,
            1 => self.state.cx = val,
            2 => self.state.dx = val,
            3 => self.state.bx = val,
            4 => self.state.sp = val,
            5 => self.state.bp = val,
            6 => self.state.si = val,
            7 => self.state.di = val,
            _ => {}
        }
    }

    pub fn get_reg8(&self, reg: u8) -> u8 {
        match reg & 7 {
            0 => (self.state.ax & 0xFF) as u8,
            1 => (self.state.cx & 0xFF) as u8,
            2 => (self.state.dx & 0xFF) as u8,
            3 => (self.state.bx & 0xFF) as u8,
            4 => (self.state.ax >> 8) as u8,
            5 => (self.state.cx >> 8) as u8,
            6 => (self.state.dx >> 8) as u8,
            7 => (self.state.bx >> 8) as u8,
            _ => 0,
        }
    }

    pub fn set_reg8(&mut self, reg: u8, val: u8) {
        let v16 = val as u16;
        match reg & 7 {
            0 => self.state.ax = (self.state.ax & 0xFF00) | v16,
            1 => self.state.cx = (self.state.cx & 0xFF00) | v16,
            2 => self.state.dx = (self.state.dx & 0xFF00) | v16,
            3 => self.state.bx = (self.state.bx & 0xFF00) | v16,
            4 => self.state.ax = (self.state.ax & 0x00FF) | (v16 << 8),
            5 => self.state.cx = (self.state.cx & 0x00FF) | (v16 << 8),
            6 => self.state.dx = (self.state.dx & 0x00FF) | (v16 << 8),
            7 => self.state.bx = (self.state.bx & 0x00FF) | (v16 << 8),
            _ => {}
        }
    }

    pub fn get_seg(&self, seg: u8) -> u16 {
        match seg & 3 {
            0 => self.state.es,
            1 => self.state.cs,
            2 => self.state.ss,
            3 => self.state.ds,
            _ => 0,
        }
    }

    pub fn set_seg(&mut self, seg: u8, val: u16) {
        match seg & 3 {
            0 => self.state.es = val,
            1 => self.state.cs = val,
            2 => self.state.ss = val,
            3 => self.state.ds = val,
            _ => {}
        }
    }

    // --- ModR/M & Effective Address ---

    fn decode_modrm(&mut self, bus: &mut dyn MemoryBus, default_seg_override: Option<u16>) -> Result<(EffectiveAddress, u8), CpuError> {
        let modrm = self.fetch_u8(bus)?;
        let mode = (modrm >> 6) & 3;
        let reg = (modrm >> 3) & 7;
        let rm = modrm & 7;

        if mode == 3 {
            return Ok((EffectiveAddress::Register(rm), reg));
        }

        let (base_offset, default_seg) = match rm {
            0 => (self.state.bx.wrapping_add(self.state.si), self.state.ds),
            1 => (self.state.bx.wrapping_add(self.state.di), self.state.ds),
            2 => (self.state.bp.wrapping_add(self.state.si), self.state.ss),
            3 => (self.state.bp.wrapping_add(self.state.di), self.state.ss),
            4 => (self.state.si, self.state.ds),
            5 => (self.state.di, self.state.ds),
            6 => {
                if mode == 0 {
                    let direct = self.fetch_u16(bus)?;
                    return Ok((
                        EffectiveAddress::Memory {
                            seg: default_seg_override.unwrap_or(self.state.ds),
                            offset: direct,
                        },
                        reg,
                    ));
                } else {
                    (self.state.bp, self.state.ss)
                }
            }
            7 => (self.state.bx, self.state.ds),
            _ => (0, self.state.ds),
        };

        let disp = match mode {
            1 => (self.fetch_u8(bus)? as i8) as i16 as u16,
            2 => self.fetch_u16(bus)?,
            _ => 0,
        };

        let offset = base_offset.wrapping_add(disp);
        let seg = default_seg_override.unwrap_or(default_seg);

        Ok((EffectiveAddress::Memory { seg, offset }, reg))
    }

    fn read_ea_u8(&self, bus: &mut dyn MemoryBus, ea: EffectiveAddress) -> Result<u8, CpuError> {
        match ea {
            EffectiveAddress::Register(r) => Ok(self.get_reg8(r)),
            EffectiveAddress::Memory { seg, offset } => {
                let linear = I8086State::linear_address(seg, offset) as u64;
                let val = bus.read_u8(linear)?;
                Ok(val)
            }
        }
    }

    fn write_ea_u8(&mut self, bus: &mut dyn MemoryBus, ea: EffectiveAddress, val: u8) -> Result<(), CpuError> {
        match ea {
            EffectiveAddress::Register(r) => {
                self.set_reg8(r, val);
                Ok(())
            }
            EffectiveAddress::Memory { seg, offset } => {
                let linear = I8086State::linear_address(seg, offset) as u64;
                bus.write_u8(linear, val)?;
                Ok(())
            }
        }
    }

    fn read_ea_u16(&self, bus: &mut dyn MemoryBus, ea: EffectiveAddress) -> Result<u16, CpuError> {
        match ea {
            EffectiveAddress::Register(r) => Ok(self.get_reg16(r)),
            EffectiveAddress::Memory { seg, offset } => {
                let linear = I8086State::linear_address(seg, offset) as u64;
                let val = bus.read_u16(linear, Endianness::LittleEndian)?;
                Ok(val)
            }
        }
    }

    fn write_ea_u16(&mut self, bus: &mut dyn MemoryBus, ea: EffectiveAddress, val: u16) -> Result<(), CpuError> {
        match ea {
            EffectiveAddress::Register(r) => {
                self.set_reg16(r, val);
                Ok(())
            }
            EffectiveAddress::Memory { seg, offset } => {
                let linear = I8086State::linear_address(seg, offset) as u64;
                bus.write_u16(linear, val, Endianness::LittleEndian)?;
                Ok(())
            }
        }
    }

    // --- ALU Operations with Flag calculation ---

    fn alu_add_u8(&mut self, dst: u8, src: u8, carry: bool) -> u8 {
        let c = if carry && (self.state.flags & flags::CF) != 0 { 1u16 } else { 0u16 };
        let res16 = (dst as u16) + (src as u16) + c;
        let res = (res16 & 0xFF) as u8;

        let cf = res16 > 0xFF;
        let zf = res == 0;
        let sf = (res & 0x80) != 0;
        let pf = I8086State::parity(res);
        let af = ((dst & 0x0F) + (src & 0x0F) + (c as u8)) > 0x0F;
        let of = (!((dst ^ src) & 0x80) & ((dst ^ res) & 0x80)) != 0;

        self.set_flags(cf, pf, af, zf, sf, of);
        res
    }

    fn alu_add_u16(&mut self, dst: u16, src: u16, carry: bool) -> u16 {
        let c = if carry && (self.state.flags & flags::CF) != 0 { 1u32 } else { 0u32 };
        let res32 = (dst as u32) + (src as u32) + c;
        let res = (res32 & 0xFFFF) as u16;

        let cf = res32 > 0xFFFF;
        let zf = res == 0;
        let sf = (res & 0x8000) != 0;
        let pf = I8086State::parity((res & 0xFF) as u8);
        let af = ((dst & 0x0F) + (src & 0x0F) + (c as u16)) > 0x0F;
        let of = (!((dst ^ src) & 0x8000) & ((dst ^ res) & 0x8000)) != 0;

        self.set_flags(cf, pf, af, zf, sf, of);
        res
    }

    fn alu_sub_u8(&mut self, dst: u8, src: u8, borrow: bool) -> u8 {
        let b = if borrow && (self.state.flags & flags::CF) != 0 { 1i16 } else { 0i16 };
        let res16 = (dst as i16) - (src as i16) - b;
        let res = (res16 & 0xFF) as u8;

        let cf = res16 < 0;
        let zf = res == 0;
        let sf = (res & 0x80) != 0;
        let pf = I8086State::parity(res);
        let af = ((dst as i16 & 0x0F) - (src as i16 & 0x0F) - b) < 0;
        let of = (((dst ^ src) & 0x80) & ((dst ^ res) & 0x80)) != 0;

        self.set_flags(cf, pf, af, zf, sf, of);
        res
    }

    fn alu_sub_u16(&mut self, dst: u16, src: u16, borrow: bool) -> u16 {
        let b = if borrow && (self.state.flags & flags::CF) != 0 { 1i32 } else { 0i32 };
        let res32 = (dst as i32) - (src as i32) - b;
        let res = (res32 & 0xFFFF) as u16;

        let cf = res32 < 0;
        let zf = res == 0;
        let sf = (res & 0x8000) != 0;
        let pf = I8086State::parity((res & 0xFF) as u8);
        let af = ((dst as i32 & 0x0F) - (src as i32 & 0x0F) - b) < 0;
        let of = (((dst ^ src) & 0x8000) & ((dst ^ res) & 0x8000)) != 0;

        self.set_flags(cf, pf, af, zf, sf, of);
        res
    }

    fn alu_and_u8(&mut self, dst: u8, src: u8) -> u8 {
        let res = dst & src;
        self.set_flags(false, I8086State::parity(res), false, res == 0, (res & 0x80) != 0, false);
        res
    }

    fn alu_and_u16(&mut self, dst: u16, src: u16) -> u16 {
        let res = dst & src;
        self.set_flags(false, I8086State::parity((res & 0xFF) as u8), false, res == 0, (res & 0x8000) != 0, false);
        res
    }

    fn alu_or_u8(&mut self, dst: u8, src: u8) -> u8 {
        let res = dst | src;
        self.set_flags(false, I8086State::parity(res), false, res == 0, (res & 0x80) != 0, false);
        res
    }

    fn alu_or_u16(&mut self, dst: u16, src: u16) -> u16 {
        let res = dst | src;
        self.set_flags(false, I8086State::parity((res & 0xFF) as u8), false, res == 0, (res & 0x8000) != 0, false);
        res
    }

    fn alu_xor_u8(&mut self, dst: u8, src: u8) -> u8 {
        let res = dst ^ src;
        self.set_flags(false, I8086State::parity(res), false, res == 0, (res & 0x80) != 0, false);
        res
    }

    fn alu_xor_u16(&mut self, dst: u16, src: u16) -> u16 {
        let res = dst ^ src;
        self.set_flags(false, I8086State::parity((res & 0xFF) as u8), false, res == 0, (res & 0x8000) != 0, false);
        res
    }

    fn set_flags(&mut self, cf: bool, pf: bool, af: bool, zf: bool, sf: bool, of: bool) {
        let mut f = self.state.flags & !(flags::CF | flags::PF | flags::AF | flags::ZF | flags::SF | flags::OF);
        if cf { f |= flags::CF; }
        if pf { f |= flags::PF; }
        if af { f |= flags::AF; }
        if zf { f |= flags::ZF; }
        if sf { f |= flags::SF; }
        if of { f |= flags::OF; }
        self.state.flags = f | 0x0002; // Bit 1 is always set
    }

    fn shift_u8(&mut self, val: u8, count: u8, op: u8) -> u8 {
        if count == 0 {
            return val;
        }
        let mut res = val;
        let mut cf = (self.state.flags & flags::CF) != 0;
        let mut of = false;

        for _ in 0..count {
            match op {
                0 => {
                    // ROL
                    let bit7 = (res & 0x80) != 0;
                    res = (res << 1) | if bit7 { 1 } else { 0 };
                    cf = bit7;
                    of = ((res & 0x80) != 0) ^ cf;
                }
                1 => {
                    // ROR
                    let bit0 = (res & 0x01) != 0;
                    res = (res >> 1) | if bit0 { 0x80 } else { 0 };
                    cf = bit0;
                    of = ((res & 0x80) != 0) ^ ((res & 0x40) != 0);
                }
                2 => {
                    // RCL
                    let old_cf = if cf { 1 } else { 0 };
                    let bit7 = (res & 0x80) != 0;
                    res = (res << 1) | old_cf;
                    cf = bit7;
                    of = ((res & 0x80) != 0) ^ cf;
                }
                3 => {
                    // RCR
                    let old_cf = if cf { 0x80 } else { 0 };
                    let bit0 = (res & 0x01) != 0;
                    res = (res >> 1) | old_cf;
                    cf = bit0;
                    of = ((res & 0x80) != 0) ^ ((res & 0x40) != 0);
                }
                4 => {
                    // SHL / SAL
                    let bit7 = (res & 0x80) != 0;
                    res <<= 1;
                    cf = bit7;
                    of = ((res & 0x80) != 0) ^ cf;
                }
                5 => {
                    // SHR
                    let bit0 = (res & 0x01) != 0;
                    let bit7 = (res & 0x80) != 0;
                    res >>= 1;
                    cf = bit0;
                    of = bit7;
                }
                7 => {
                    // SAR
                    let bit0 = (res & 0x01) != 0;
                    let msb = res & 0x80;
                    res = (res >> 1) | msb;
                    cf = bit0;
                    of = false;
                }
                _ => {}
            }
        }

        if op >= 4 {
            let zf = res == 0;
            let sf = (res & 0x80) != 0;
            let pf = I8086State::parity(res);
            self.set_flags(cf, pf, (self.state.flags & flags::AF) != 0, zf, sf, of);
        } else {
            let mut f = self.state.flags & !flags::CF;
            if cf { f |= flags::CF; }
            if count == 1 {
                f &= !flags::OF;
                if of { f |= flags::OF; }
            }
            self.state.flags = f | 0x0002;
        }

        res
    }

    fn shift_u16(&mut self, val: u16, count: u8, op: u8) -> u16 {
        if count == 0 {
            return val;
        }
        let mut res = val;
        let mut cf = (self.state.flags & flags::CF) != 0;
        let mut of = false;

        for _ in 0..count {
            match op {
                0 => {
                    // ROL
                    let bit15 = (res & 0x8000) != 0;
                    res = (res << 1) | if bit15 { 1 } else { 0 };
                    cf = bit15;
                    of = ((res & 0x8000) != 0) ^ cf;
                }
                1 => {
                    // ROR
                    let bit0 = (res & 0x0001) != 0;
                    res = (res >> 1) | if bit0 { 0x8000 } else { 0 };
                    cf = bit0;
                    of = ((res & 0x8000) != 0) ^ ((res & 0x4000) != 0);
                }
                2 => {
                    // RCL
                    let old_cf = if cf { 1 } else { 0 };
                    let bit15 = (res & 0x8000) != 0;
                    res = (res << 1) | old_cf;
                    cf = bit15;
                    of = ((res & 0x8000) != 0) ^ cf;
                }
                3 => {
                    // RCR
                    let old_cf = if cf { 0x8000 } else { 0 };
                    let bit0 = (res & 0x0001) != 0;
                    res = (res >> 1) | old_cf;
                    cf = bit0;
                    of = ((res & 0x8000) != 0) ^ ((res & 0x4000) != 0);
                }
                4 => {
                    // SHL / SAL
                    let bit15 = (res & 0x8000) != 0;
                    res <<= 1;
                    cf = bit15;
                    of = ((res & 0x8000) != 0) ^ cf;
                }
                5 => {
                    // SHR
                    let bit0 = (res & 0x0001) != 0;
                    let bit15 = (res & 0x8000) != 0;
                    res >>= 1;
                    cf = bit0;
                    of = bit15;
                }
                7 => {
                    // SAR
                    let bit0 = (res & 0x0001) != 0;
                    let msb = res & 0x8000;
                    res = (res >> 1) | msb;
                    cf = bit0;
                    of = false;
                }
                _ => {}
            }
        }

        if op >= 4 {
            let zf = res == 0;
            let sf = (res & 0x8000) != 0;
            let pf = I8086State::parity((res & 0xFF) as u8);
            self.set_flags(cf, pf, (self.state.flags & flags::AF) != 0, zf, sf, of);
        } else {
            let mut f = self.state.flags & !flags::CF;
            if cf { f |= flags::CF; }
            if count == 1 {
                f &= !flags::OF;
                if of { f |= flags::OF; }
            }
            self.state.flags = f | 0x0002;
        }

        res
    }
}

impl CpuEngine for I8086Cpu {
    fn arch(&self) -> Architecture {
        Architecture::I8086
    }

    fn endianness(&self) -> Endianness {
        Endianness::LittleEndian
    }

    fn stack_growth(&self) -> StackGrowth {
        StackGrowth::Downwards
    }

    fn word_size(&self) -> WordSize {
        WordSize::Bytes2
    }

    fn pc(&self) -> u64 {
        I8086State::linear_address(self.state.cs, self.state.ip) as u64
    }

    fn set_pc(&mut self, val: u64) {
        if val > 0xFFFF {
            self.state.cs = ((val >> 4) & 0xFFFF) as u16;
            self.state.ip = (val & 0x000F) as u16;
        } else {
            self.state.cs = 0x0000;
            self.state.ip = (val & 0xFFFF) as u16;
        }
    }

    fn sp(&self) -> u64 {
        I8086State::linear_address(self.state.ss, self.state.sp) as u64
    }

    fn set_sp(&mut self, val: u64) {
        if val > 0xFFFF {
            self.state.ss = ((val >> 4) & 0xFFFF) as u16;
            self.state.sp = (val & 0x000F) as u16;
        } else {
            self.state.ss = 0x0000;
            self.state.sp = (val & 0xFFFF) as u16;
        }
    }

    fn reset(&mut self) {
        self.state = I8086State {
            cs: 0xFFFF,
            ip: 0x0000,
            sp: 0xFFFE,
            flags: 0x0002,
            ..Default::default()
        };
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let mut seg_override = None;
        let mut rep_prefix = None;
        let mut opcode = self.fetch_u8(bus)?;

        // Handle segment prefix overrides, REP prefixes, and LOCK prefix
        loop {
            match opcode {
                0x26 => { seg_override = Some(self.state.es); opcode = self.fetch_u8(bus)?; }
                0x2E => { seg_override = Some(self.state.cs); opcode = self.fetch_u8(bus)?; }
                0x36 => { seg_override = Some(self.state.ss); opcode = self.fetch_u8(bus)?; }
                0x3E => { seg_override = Some(self.state.ds); opcode = self.fetch_u8(bus)?; }
                0xF2 | 0xF3 => { rep_prefix = Some(opcode); opcode = self.fetch_u8(bus)?; }
                0xF0 => { opcode = self.fetch_u8(bus)?; } // LOCK
                _ => break,
            }
        }

        match opcode {
            // NOP
            0x90 => Ok(StepOutcome::Continue { cycles: 3 }),

            // HLT
            0xF4 => {
                self.state.halted = true;
                Ok(StepOutcome::Halted)
            }

            // WAIT
            0x9B => Ok(StepOutcome::Continue { cycles: 4 }),

            // INT 3
            0xCC => Ok(StepOutcome::Breakpoint),

            // INT imm8
            0xCD => {
                let int_num = self.fetch_u8(bus)?;
                self.push_u16(bus, self.state.flags)?;
                self.push_u16(bus, self.state.cs)?;
                self.push_u16(bus, self.state.ip)?;
                let ivt_addr = (int_num as u64) * 4;
                let new_ip = bus.read_u16(ivt_addr, Endianness::LittleEndian)?;
                let new_cs = bus.read_u16(ivt_addr + 2, Endianness::LittleEndian)?;
                if new_ip != 0 || new_cs != 0 {
                    self.state.ip = new_ip;
                    self.state.cs = new_cs;
                }
                Ok(StepOutcome::Interrupt(int_num as u32))
            }

            // INTO
            0xCE => {
                if (self.state.flags & flags::OF) != 0 {
                    self.push_u16(bus, self.state.flags)?;
                    self.push_u16(bus, self.state.cs)?;
                    self.push_u16(bus, self.state.ip)?;
                    let ivt_addr = 4 * 4;
                    let new_ip = bus.read_u16(ivt_addr, Endianness::LittleEndian)?;
                    let new_cs = bus.read_u16(ivt_addr + 2, Endianness::LittleEndian)?;
                    if new_ip != 0 || new_cs != 0 {
                        self.state.ip = new_ip;
                        self.state.cs = new_cs;
                    }
                    Ok(StepOutcome::Interrupt(4))
                } else {
                    Ok(StepOutcome::Continue { cycles: 4 })
                }
            }

            // IRET
            0xCF => {
                self.state.ip = self.pop_u16(bus)?;
                self.state.cs = self.pop_u16(bus)?;
                self.state.flags = self.pop_u16(bus)? | 0x0002;
                Ok(StepOutcome::Continue { cycles: 32 })
            }

            // PUSH / POP 16-bit register (0x50..0x5F)
            0x50..=0x57 => {
                let reg = opcode - 0x50;
                let val = self.get_reg16(reg);
                self.push_u16(bus, val)?;
                Ok(StepOutcome::Continue { cycles: 11 })
            }
            0x58..=0x5F => {
                let reg = opcode - 0x58;
                let val = self.pop_u16(bus)?;
                self.set_reg16(reg, val);
                Ok(StepOutcome::Continue { cycles: 8 })
            }

            // PUSH segment reg
            0x06 => { let v = self.state.es; self.push_u16(bus, v)?; Ok(StepOutcome::Continue { cycles: 10 }) }
            0x0E => { let v = self.state.cs; self.push_u16(bus, v)?; Ok(StepOutcome::Continue { cycles: 10 }) }
            0x16 => { let v = self.state.ss; self.push_u16(bus, v)?; Ok(StepOutcome::Continue { cycles: 10 }) }
            0x1E => { let v = self.state.ds; self.push_u16(bus, v)?; Ok(StepOutcome::Continue { cycles: 10 }) }

            // POP segment reg
            0x07 => { self.state.es = self.pop_u16(bus)?; Ok(StepOutcome::Continue { cycles: 8 }) }
            0x17 => { self.state.ss = self.pop_u16(bus)?; Ok(StepOutcome::Continue { cycles: 8 }) }
            0x1F => { self.state.ds = self.pop_u16(bus)?; Ok(StepOutcome::Continue { cycles: 8 }) }

            // PUSHF / POPF
            0x9C => { let f = self.state.flags; self.push_u16(bus, f)?; Ok(StepOutcome::Continue { cycles: 10 }) }
            0x9D => { self.state.flags = self.pop_u16(bus)? | 0x0002; Ok(StepOutcome::Continue { cycles: 8 }) }

            // POP rm16 (0x8F /0)
            0x8F => {
                let (ea, _) = self.decode_modrm(bus, seg_override)?;
                let val = self.pop_u16(bus)?;
                self.write_ea_u16(bus, ea, val)?;
                Ok(StepOutcome::Continue { cycles: 17 })
            }

            // MOV imm8 -> reg8 (0xB0..0xB7)
            0xB0..=0xB7 => {
                let reg = opcode - 0xB0;
                let imm = self.fetch_u8(bus)?;
                self.set_reg8(reg, imm);
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // MOV imm16 -> reg16 (0xB8..0xBF)
            0xB8..=0xBF => {
                let reg = opcode - 0xB8;
                let imm = self.fetch_u16(bus)?;
                self.set_reg16(reg, imm);
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // MOV rm8 <-> r8 (0x88, 0x8A)
            0x88 => {
                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                let val = self.get_reg8(reg);
                self.write_ea_u8(bus, ea, val)?;
                Ok(StepOutcome::Continue { cycles: 9 })
            }
            0x8A => {
                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                let val = self.read_ea_u8(bus, ea)?;
                self.set_reg8(reg, val);
                Ok(StepOutcome::Continue { cycles: 8 })
            }

            // MOV rm16 <-> r16 (0x89, 0x8B)
            0x89 => {
                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                let val = self.get_reg16(reg);
                self.write_ea_u16(bus, ea, val)?;
                Ok(StepOutcome::Continue { cycles: 9 })
            }
            0x8B => {
                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                let val = self.read_ea_u16(bus, ea)?;
                self.set_reg16(reg, val);
                Ok(StepOutcome::Continue { cycles: 8 })
            }

            // MOV rm16 <-> segreg (0x8C, 0x8E)
            0x8C => {
                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                let val = self.get_seg(reg);
                self.write_ea_u16(bus, ea, val)?;
                Ok(StepOutcome::Continue { cycles: 9 })
            }
            0x8E => {
                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                let val = self.read_ea_u16(bus, ea)?;
                self.set_seg(reg, val);
                Ok(StepOutcome::Continue { cycles: 8 })
            }

            // MOV imm -> rm (0xC6, 0xC7)
            0xC6 => {
                let (ea, _) = self.decode_modrm(bus, seg_override)?;
                let imm = self.fetch_u8(bus)?;
                self.write_ea_u8(bus, ea, imm)?;
                Ok(StepOutcome::Continue { cycles: 10 })
            }
            0xC7 => {
                let (ea, _) = self.decode_modrm(bus, seg_override)?;
                let imm = self.fetch_u16(bus)?;
                self.write_ea_u16(bus, ea, imm)?;
                Ok(StepOutcome::Continue { cycles: 10 })
            }

            // MOV AL/AX <-> [moffs] (0xA0..0xA3)
            0xA0 => {
                let off = self.fetch_u16(bus)?;
                let seg = seg_override.unwrap_or(self.state.ds);
                let addr = I8086State::linear_address(seg, off) as u64;
                let val = bus.read_u8(addr)?;
                self.set_reg8(0, val);
                Ok(StepOutcome::Continue { cycles: 10 })
            }
            0xA1 => {
                let off = self.fetch_u16(bus)?;
                let seg = seg_override.unwrap_or(self.state.ds);
                let addr = I8086State::linear_address(seg, off) as u64;
                let val = bus.read_u16(addr, Endianness::LittleEndian)?;
                self.state.ax = val;
                Ok(StepOutcome::Continue { cycles: 10 })
            }
            0xA2 => {
                let off = self.fetch_u16(bus)?;
                let seg = seg_override.unwrap_or(self.state.ds);
                let addr = I8086State::linear_address(seg, off) as u64;
                let val = (self.state.ax & 0xFF) as u8;
                bus.write_u8(addr, val)?;
                Ok(StepOutcome::Continue { cycles: 10 })
            }
            0xA3 => {
                let off = self.fetch_u16(bus)?;
                let seg = seg_override.unwrap_or(self.state.ds);
                let addr = I8086State::linear_address(seg, off) as u64;
                bus.write_u16(addr, self.state.ax, Endianness::LittleEndian)?;
                Ok(StepOutcome::Continue { cycles: 10 })
            }

            // LEA r16, m16 (0x8D)
            0x8D => {
                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                if let EffectiveAddress::Memory { offset, .. } = ea {
                    self.set_reg16(reg, offset);
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }

            // LES / LDS (0xC4, 0xC5)
            0xC4 => {
                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                if let EffectiveAddress::Memory { seg, offset } = ea {
                    let addr = I8086State::linear_address(seg, offset) as u64;
                    let off_val = bus.read_u16(addr, Endianness::LittleEndian)?;
                    let seg_val = bus.read_u16(addr + 2, Endianness::LittleEndian)?;
                    self.set_reg16(reg, off_val);
                    self.state.es = seg_val;
                }
                Ok(StepOutcome::Continue { cycles: 16 })
            }
            0xC5 => {
                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                if let EffectiveAddress::Memory { seg, offset } = ea {
                    let addr = I8086State::linear_address(seg, offset) as u64;
                    let off_val = bus.read_u16(addr, Endianness::LittleEndian)?;
                    let seg_val = bus.read_u16(addr + 2, Endianness::LittleEndian)?;
                    self.set_reg16(reg, off_val);
                    self.state.ds = seg_val;
                }
                Ok(StepOutcome::Continue { cycles: 16 })
            }

            // XLAT (0xD7)
            0xD7 => {
                let seg = seg_override.unwrap_or(self.state.ds);
                let off = self.state.bx.wrapping_add(self.state.ax & 0xFF);
                let addr = I8086State::linear_address(seg, off) as u64;
                let val = bus.read_u8(addr)?;
                self.set_reg8(0, val);
                Ok(StepOutcome::Continue { cycles: 11 })
            }

            // XCHG AX, r16 (0x91..0x97)
            0x91..=0x97 => {
                let reg = opcode - 0x90;
                let tmp = self.state.ax;
                self.state.ax = self.get_reg16(reg);
                self.set_reg16(reg, tmp);
                Ok(StepOutcome::Continue { cycles: 3 })
            }

            // XCHG rm8/16 <-> r8/16 (0x86, 0x87)
            0x86 => {
                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                let v1 = self.get_reg8(reg);
                let v2 = self.read_ea_u8(bus, ea)?;
                self.set_reg8(reg, v2);
                self.write_ea_u8(bus, ea, v1)?;
                Ok(StepOutcome::Continue { cycles: 17 })
            }
            0x87 => {
                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                let v1 = self.get_reg16(reg);
                let v2 = self.read_ea_u16(bus, ea)?;
                self.set_reg16(reg, v2);
                self.write_ea_u16(bus, ea, v1)?;
                Ok(StepOutcome::Continue { cycles: 17 })
            }

            // TEST r/m, r (0x84, 0x85)
            0x84 => {
                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                let v1 = self.read_ea_u8(bus, ea)?;
                let v2 = self.get_reg8(reg);
                self.alu_and_u8(v1, v2);
                Ok(StepOutcome::Continue { cycles: 9 })
            }
            0x85 => {
                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                let v1 = self.read_ea_u16(bus, ea)?;
                let v2 = self.get_reg16(reg);
                self.alu_and_u16(v1, v2);
                Ok(StepOutcome::Continue { cycles: 9 })
            }

            // TEST AL/AX, imm (0xA8, 0xA9)
            0xA8 => {
                let imm = self.fetch_u8(bus)?;
                let val = (self.state.ax & 0xFF) as u8;
                self.alu_and_u8(val, imm);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0xA9 => {
                let imm = self.fetch_u16(bus)?;
                let val = self.state.ax;
                self.alu_and_u16(val, imm);
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // INC / DEC reg16 (0x40..0x4F)
            0x40..=0x47 => {
                let reg = opcode - 0x40;
                let val = self.get_reg16(reg);
                let cf = (self.state.flags & flags::CF) != 0;
                let res = self.alu_add_u16(val, 1, false);
                if cf { self.state.flags |= flags::CF; } else { self.state.flags &= !flags::CF; }
                self.set_reg16(reg, res);
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x48..=0x4F => {
                let reg = opcode - 0x48;
                let val = self.get_reg16(reg);
                let cf = (self.state.flags & flags::CF) != 0;
                let res = self.alu_sub_u16(val, 1, false);
                if cf { self.state.flags |= flags::CF; } else { self.state.flags &= !flags::CF; }
                self.set_reg16(reg, res);
                Ok(StepOutcome::Continue { cycles: 2 })
            }

            // BCD / Decimal Adjust Instructions (0x27, 0x2F, 0x37, 0x3F)
            0x27 => {
                // DAA
                let mut al = (self.state.ax & 0xFF) as u8;
                let old_al = al;
                let mut cf = (self.state.flags & flags::CF) != 0;
                let mut af = (self.state.flags & flags::AF) != 0;

                if (al & 0x0F) > 9 || af {
                    al = al.wrapping_add(6);
                    af = true;
                }
                if old_al > 0x99 || cf {
                    al = al.wrapping_add(0x60);
                    cf = true;
                }
                self.set_reg8(0, al);
                let zf = al == 0;
                let sf = (al & 0x80) != 0;
                let pf = I8086State::parity(al);
                self.set_flags(cf, pf, af, zf, sf, (self.state.flags & flags::OF) != 0);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x2F => {
                // DAS
                let mut al = (self.state.ax & 0xFF) as u8;
                let old_al = al;
                let mut cf = (self.state.flags & flags::CF) != 0;
                let mut af = (self.state.flags & flags::AF) != 0;

                if (al & 0x0F) > 9 || af {
                    al = al.wrapping_sub(6);
                    af = true;
                }
                if old_al > 0x99 || cf {
                    al = al.wrapping_sub(0x60);
                    cf = true;
                }
                self.set_reg8(0, al);
                let zf = al == 0;
                let sf = (al & 0x80) != 0;
                let pf = I8086State::parity(al);
                self.set_flags(cf, pf, af, zf, sf, (self.state.flags & flags::OF) != 0);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x37 => {
                // AAA
                let mut al = (self.state.ax & 0xFF) as u8;
                let mut ah = (self.state.ax >> 8) as u8;
                let af = (self.state.flags & flags::AF) != 0;

                if (al & 0x0F) > 9 || af {
                    al = al.wrapping_add(6) & 0x0F;
                    ah = ah.wrapping_add(1);
                    self.state.flags |= flags::AF | flags::CF;
                } else {
                    al &= 0x0F;
                    self.state.flags &= !(flags::AF | flags::CF);
                }
                self.state.ax = ((ah as u16) << 8) | (al as u16);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x3F => {
                // AAS
                let mut al = (self.state.ax & 0xFF) as u8;
                let mut ah = (self.state.ax >> 8) as u8;
                let af = (self.state.flags & flags::AF) != 0;

                if (al & 0x0F) > 9 || af {
                    al = al.wrapping_sub(6) & 0x0F;
                    ah = ah.wrapping_sub(1);
                    self.state.flags |= flags::AF | flags::CF;
                } else {
                    al &= 0x0F;
                    self.state.flags &= !(flags::AF | flags::CF);
                }
                self.state.ax = ((ah as u16) << 8) | (al as u16);
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // ADD / OR / ADC / SBB / AND / SUB / XOR / CMP (0x00..0x3F)
            0x00..=0x3F => {
                let op = (opcode >> 3) & 7;
                let is_16 = (opcode & 1) != 0;
                let is_dst_reg = (opcode & 2) != 0;
                let is_imm_accum = (opcode & 4) != 0;

                if is_imm_accum {
                    if is_16 {
                        let imm = self.fetch_u16(bus)?;
                        let val = self.state.ax;
                        let res = match op {
                            0 => self.alu_add_u16(val, imm, false),
                            1 => self.alu_or_u16(val, imm),
                            2 => self.alu_add_u16(val, imm, true),
                            3 => self.alu_sub_u16(val, imm, true),
                            4 => self.alu_and_u16(val, imm),
                            5 | 7 => self.alu_sub_u16(val, imm, false),
                            6 => self.alu_xor_u16(val, imm),
                            _ => val,
                        };
                        if op != 7 { self.state.ax = res; }
                    } else {
                        let imm = self.fetch_u8(bus)?;
                        let val = (self.state.ax & 0xFF) as u8;
                        let res = match op {
                            0 => self.alu_add_u8(val, imm, false),
                            1 => self.alu_or_u8(val, imm),
                            2 => self.alu_add_u8(val, imm, true),
                            3 => self.alu_sub_u8(val, imm, true),
                            4 => self.alu_and_u8(val, imm),
                            5 | 7 => self.alu_sub_u8(val, imm, false),
                            6 => self.alu_xor_u8(val, imm),
                            _ => val,
                        };
                        if op != 7 { self.set_reg8(0, res); }
                    }
                    return Ok(StepOutcome::Continue { cycles: 4 });
                }

                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                if is_16 {
                    let r_val = self.get_reg16(reg);
                    let m_val = self.read_ea_u16(bus, ea)?;
                    let (dst, src) = if is_dst_reg { (r_val, m_val) } else { (m_val, r_val) };
                    let res = match op {
                        0 => self.alu_add_u16(dst, src, false),
                        1 => self.alu_or_u16(dst, src),
                        2 => self.alu_add_u16(dst, src, true),
                        3 => self.alu_sub_u16(dst, src, true),
                        4 => self.alu_and_u16(dst, src),
                        5 | 7 => self.alu_sub_u16(dst, src, false),
                        6 => self.alu_xor_u16(dst, src),
                        _ => dst,
                    };
                    if op != 7 {
                        if is_dst_reg { self.set_reg16(reg, res); } else { self.write_ea_u16(bus, ea, res)?; }
                    }
                } else {
                    let r_val = self.get_reg8(reg);
                    let m_val = self.read_ea_u8(bus, ea)?;
                    let (dst, src) = if is_dst_reg { (r_val, m_val) } else { (m_val, r_val) };
                    let res = match op {
                        0 => self.alu_add_u8(dst, src, false),
                        1 => self.alu_or_u8(dst, src),
                        2 => self.alu_add_u8(dst, src, true),
                        3 => self.alu_sub_u8(dst, src, true),
                        4 => self.alu_and_u8(dst, src),
                        5 | 7 => self.alu_sub_u8(dst, src, false),
                        6 => self.alu_xor_u8(dst, src),
                        _ => dst,
                    };
                    if op != 7 {
                        if is_dst_reg { self.set_reg8(reg, res); } else { self.write_ea_u8(bus, ea, res)?; }
                    }
                }

                Ok(StepOutcome::Continue { cycles: 3 })
            }

            // Group 1: ALU imm -> rm (0x80..0x83)
            0x80..=0x83 => {
                let (ea, op) = self.decode_modrm(bus, seg_override)?;
                let is_16 = (opcode & 1) != 0;
                let sign_extend = opcode == 0x83;

                if is_16 {
                    let imm = if sign_extend {
                        (self.fetch_u8(bus)? as i8) as i16 as u16
                    } else {
                        self.fetch_u16(bus)?
                    };
                    let dst = self.read_ea_u16(bus, ea)?;
                    let res = match op {
                        0 => self.alu_add_u16(dst, imm, false),
                        1 => self.alu_or_u16(dst, imm),
                        2 => self.alu_add_u16(dst, imm, true),
                        3 => self.alu_sub_u16(dst, imm, true),
                        4 => self.alu_and_u16(dst, imm),
                        5 | 7 => self.alu_sub_u16(dst, imm, false),
                        6 => self.alu_xor_u16(dst, imm),
                        _ => dst,
                    };
                    if op != 7 { self.write_ea_u16(bus, ea, res)?; }
                } else {
                    let imm = self.fetch_u8(bus)?;
                    let dst = self.read_ea_u8(bus, ea)?;
                    let res = match op {
                        0 => self.alu_add_u8(dst, imm, false),
                        1 => self.alu_or_u8(dst, imm),
                        2 => self.alu_add_u8(dst, imm, true),
                        3 => self.alu_sub_u8(dst, imm, true),
                        4 => self.alu_and_u8(dst, imm),
                        5 | 7 => self.alu_sub_u8(dst, imm, false),
                        6 => self.alu_xor_u8(dst, imm),
                        _ => dst,
                    };
                    if op != 7 { self.write_ea_u8(bus, ea, res)?; }
                }
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // Group 2: Shifts & Rotates (0xD0..0xD3)
            0xD0..=0xD3 => {
                let (ea, op) = self.decode_modrm(bus, seg_override)?;
                let is_16 = (opcode & 1) != 0;
                let count = if (opcode & 2) != 0 { (self.state.cx & 0xFF) as u8 } else { 1 };

                if is_16 {
                    let val = self.read_ea_u16(bus, ea)?;
                    let res = self.shift_u16(val, count, op);
                    self.write_ea_u16(bus, ea, res)?;
                } else {
                    let val = self.read_ea_u8(bus, ea)?;
                    let res = self.shift_u8(val, count, op);
                    self.write_ea_u8(bus, ea, res)?;
                }
                Ok(StepOutcome::Continue { cycles: 8 })
            }

            // Group 3: NOT / NEG / MUL / IMUL / DIV / IDIV / TEST (0xF6, 0xF7)
            0xF6 | 0xF7 => {
                let (ea, op) = self.decode_modrm(bus, seg_override)?;
                let is_16 = (opcode & 1) != 0;

                if is_16 {
                    let val = self.read_ea_u16(bus, ea)?;
                    match op {
                        0 => {
                            // TEST rm16, imm16
                            let imm = self.fetch_u16(bus)?;
                            self.alu_and_u16(val, imm);
                        }
                        2 => {
                            // NOT rm16
                            self.write_ea_u16(bus, ea, !val)?;
                        }
                        3 => {
                            // NEG rm16
                            let res = self.alu_sub_u16(0, val, false);
                            self.write_ea_u16(bus, ea, res)?;
                        }
                        4 => {
                            // MUL rm16 (AX * rm16 -> DX:AX)
                            let prod = (self.state.ax as u32) * (val as u32);
                            self.state.ax = (prod & 0xFFFF) as u16;
                            self.state.dx = (prod >> 16) as u16;
                            let has_hi = self.state.dx != 0;
                            if has_hi {
                                self.state.flags |= flags::CF | flags::OF;
                            } else {
                                self.state.flags &= !(flags::CF | flags::OF);
                            }
                        }
                        5 => {
                            // IMUL rm16 (signed AX * rm16 -> DX:AX)
                            let prod = (self.state.ax as i16 as i32) * (val as i16 as i32);
                            self.state.ax = (prod as u32 & 0xFFFF) as u16;
                            self.state.dx = ((prod as u32) >> 16) as u16;
                            let has_hi = (prod >> 15) != 0 && (prod >> 15) != -1;
                            if has_hi {
                                self.state.flags |= flags::CF | flags::OF;
                            } else {
                                self.state.flags &= !(flags::CF | flags::OF);
                            }
                        }
                        6 => {
                            // DIV rm16 (DX:AX / rm16 -> AX=quot, DX=rem)
                            if val == 0 {
                                return Ok(StepOutcome::Interrupt(0)); // Divide error
                            }
                            let num = ((self.state.dx as u32) << 16) | (self.state.ax as u32);
                            let quot = num / (val as u32);
                            let rem = num % (val as u32);
                            if quot > 0xFFFF {
                                return Ok(StepOutcome::Interrupt(0));
                            }
                            self.state.ax = quot as u16;
                            self.state.dx = rem as u16;
                        }
                        7 => {
                            // IDIV rm16 (signed DX:AX / rm16 -> AX=quot, DX=rem)
                            if val == 0 {
                                return Ok(StepOutcome::Interrupt(0));
                            }
                            let num = (((self.state.dx as u32) << 16) | (self.state.ax as u32)) as i32;
                            let d = val as i16 as i32;
                            let quot = num / d;
                            let rem = num % d;
                            if !(-32768..=32767).contains(&quot) {
                                return Ok(StepOutcome::Interrupt(0));
                            }
                            self.state.ax = (quot as i16) as u16;
                            self.state.dx = (rem as i16) as u16;
                        }
                        _ => return Err(CpuError::InvalidInstruction { opcode: opcode as u64, pc: self.pc() }),
                    }
                } else {
                    let val = self.read_ea_u8(bus, ea)?;
                    match op {
                        0 => {
                            // TEST rm8, imm8
                            let imm = self.fetch_u8(bus)?;
                            self.alu_and_u8(val, imm);
                        }
                        2 => {
                            // NOT rm8
                            self.write_ea_u8(bus, ea, !val)?;
                        }
                        3 => {
                            // NEG rm8
                            let res = self.alu_sub_u8(0, val, false);
                            self.write_ea_u8(bus, ea, res)?;
                        }
                        4 => {
                            // MUL rm8 (AL * rm8 -> AX)
                            let al = (self.state.ax & 0xFF) as u8;
                            let prod = (al as u16) * (val as u16);
                            self.state.ax = prod;
                            let has_hi = (prod & 0xFF00) != 0;
                            if has_hi {
                                self.state.flags |= flags::CF | flags::OF;
                            } else {
                                self.state.flags &= !(flags::CF | flags::OF);
                            }
                        }
                        5 => {
                            // IMUL rm8 (signed AL * rm8 -> AX)
                            let al = (self.state.ax & 0xFF) as u8 as i8;
                            let prod = (al as i16) * (val as i8 as i16);
                            self.state.ax = prod as u16;
                            let has_hi = (prod >> 7) != 0 && (prod >> 7) != -1;
                            if has_hi {
                                self.state.flags |= flags::CF | flags::OF;
                            } else {
                                self.state.flags &= !(flags::CF | flags::OF);
                            }
                        }
                        6 => {
                            // DIV rm8 (AX / rm8 -> AL=quot, AH=rem)
                            if val == 0 {
                                return Ok(StepOutcome::Interrupt(0));
                            }
                            let num = self.state.ax;
                            let quot = num / (val as u16);
                            let rem = num % (val as u16);
                            if quot > 0xFF {
                                return Ok(StepOutcome::Interrupt(0));
                            }
                            self.state.ax = ((rem & 0xFF) << 8) | (quot & 0xFF);
                        }
                        7 => {
                            // IDIV rm8 (signed AX / rm8 -> AL=quot, AH=rem)
                            if val == 0 {
                                return Ok(StepOutcome::Interrupt(0));
                            }
                            let num = self.state.ax as i16;
                            let d = val as i8 as i16;
                            let quot = num / d;
                            let rem = num % d;
                            if !(-128..=127).contains(&quot) {
                                return Ok(StepOutcome::Interrupt(0));
                            }
                            self.state.ax = (((rem as u16) & 0xFF) << 8) | ((quot as u16) & 0xFF);
                        }
                        _ => return Err(CpuError::InvalidInstruction { opcode: opcode as u64, pc: self.pc() }),
                    }
                }
                Ok(StepOutcome::Continue { cycles: 12 })
            }

            // Group 4: INC / DEC rm8 (0xFE)
            0xFE => {
                let (ea, op) = self.decode_modrm(bus, seg_override)?;
                let val = self.read_ea_u8(bus, ea)?;
                let cf = (self.state.flags & flags::CF) != 0;
                let res = match op {
                    0 => self.alu_add_u8(val, 1, false),
                    1 => self.alu_sub_u8(val, 1, false),
                    _ => return Err(CpuError::InvalidInstruction { opcode: 0xFE, pc: self.pc() }),
                };
                if cf { self.state.flags |= flags::CF; } else { self.state.flags &= !flags::CF; }
                self.write_ea_u8(bus, ea, res)?;
                Ok(StepOutcome::Continue { cycles: 15 })
            }

            // Group 5: INC/DEC/CALL/JMP/PUSH rm16 (0xFF)
            0xFF => {
                let (ea, op) = self.decode_modrm(bus, seg_override)?;
                match op {
                    0 => {
                        // INC rm16
                        let val = self.read_ea_u16(bus, ea)?;
                        let cf = (self.state.flags & flags::CF) != 0;
                        let res = self.alu_add_u16(val, 1, false);
                        if cf { self.state.flags |= flags::CF; } else { self.state.flags &= !flags::CF; }
                        self.write_ea_u16(bus, ea, res)?;
                        Ok(StepOutcome::Continue { cycles: 15 })
                    }
                    1 => {
                        // DEC rm16
                        let val = self.read_ea_u16(bus, ea)?;
                        let cf = (self.state.flags & flags::CF) != 0;
                        let res = self.alu_sub_u16(val, 1, false);
                        if cf { self.state.flags |= flags::CF; } else { self.state.flags &= !flags::CF; }
                        self.write_ea_u16(bus, ea, res)?;
                        Ok(StepOutcome::Continue { cycles: 15 })
                    }
                    2 => {
                        // CALL near indirect
                        let target = self.read_ea_u16(bus, ea)?;
                        let ip = self.state.ip;
                        self.push_u16(bus, ip)?;
                        self.state.ip = target;
                        Ok(StepOutcome::Continue { cycles: 16 })
                    }
                    3 => {
                        // CALL far indirect (m16:16)
                        if let EffectiveAddress::Memory { seg, offset } = ea {
                            let addr = I8086State::linear_address(seg, offset) as u64;
                            let target_ip = bus.read_u16(addr, Endianness::LittleEndian)?;
                            let target_cs = bus.read_u16(addr + 2, Endianness::LittleEndian)?;
                            let cs = self.state.cs;
                            let ip = self.state.ip;
                            self.push_u16(bus, cs)?;
                            self.push_u16(bus, ip)?;
                            self.state.cs = target_cs;
                            self.state.ip = target_ip;
                            Ok(StepOutcome::Continue { cycles: 37 })
                        } else {
                            Err(CpuError::InvalidInstruction { opcode: 0xFF, pc: self.pc() })
                        }
                    }
                    4 => {
                        // JMP near indirect
                        let target = self.read_ea_u16(bus, ea)?;
                        self.state.ip = target;
                        Ok(StepOutcome::Continue { cycles: 11 })
                    }
                    5 => {
                        // JMP far indirect (m16:16)
                        if let EffectiveAddress::Memory { seg, offset } = ea {
                            let addr = I8086State::linear_address(seg, offset) as u64;
                            let target_ip = bus.read_u16(addr, Endianness::LittleEndian)?;
                            let target_cs = bus.read_u16(addr + 2, Endianness::LittleEndian)?;
                            self.state.cs = target_cs;
                            self.state.ip = target_ip;
                            Ok(StepOutcome::Continue { cycles: 24 })
                        } else {
                            Err(CpuError::InvalidInstruction { opcode: 0xFF, pc: self.pc() })
                        }
                    }
                    6 => {
                        // PUSH rm16
                        let val = self.read_ea_u16(bus, ea)?;
                        self.push_u16(bus, val)?;
                        Ok(StepOutcome::Continue { cycles: 16 })
                    }
                    _ => Err(CpuError::InvalidInstruction { opcode: 0xFF, pc: self.pc() }),
                }
            }

            // Far JMP & CALL (0xEA, 0x9A)
            0xEA => {
                let target_ip = self.fetch_u16(bus)?;
                let target_cs = self.fetch_u16(bus)?;
                self.state.cs = target_cs;
                self.state.ip = target_ip;
                Ok(StepOutcome::Continue { cycles: 15 })
            }
            0x9A => {
                let target_ip = self.fetch_u16(bus)?;
                let target_cs = self.fetch_u16(bus)?;
                let cs = self.state.cs;
                let ip = self.state.ip;
                self.push_u16(bus, cs)?;
                self.push_u16(bus, ip)?;
                self.state.cs = target_cs;
                self.state.ip = target_ip;
                Ok(StepOutcome::Continue { cycles: 28 })
            }

            // Far RET (0xCB, 0xCA)
            0xCB => {
                self.state.ip = self.pop_u16(bus)?;
                self.state.cs = self.pop_u16(bus)?;
                Ok(StepOutcome::Continue { cycles: 18 })
            }
            0xCA => {
                let pop_bytes = self.fetch_u16(bus)?;
                self.state.ip = self.pop_u16(bus)?;
                self.state.cs = self.pop_u16(bus)?;
                self.state.sp = self.state.sp.wrapping_add(pop_bytes);
                Ok(StepOutcome::Continue { cycles: 17 })
            }

            // Control flow: JMP rel8 / rel16
            0xEB => {
                let rel = self.fetch_u8(bus)? as i8;
                self.state.ip = (self.state.ip as i16).wrapping_add(rel as i16) as u16;
                Ok(StepOutcome::Continue { cycles: 15 })
            }
            0xE9 => {
                let rel = self.fetch_u16(bus)? as i16;
                self.state.ip = (self.state.ip as i16).wrapping_add(rel) as u16;
                Ok(StepOutcome::Continue { cycles: 15 })
            }

            // CALL rel16 (0xE8)
            0xE8 => {
                let rel = self.fetch_u16(bus)? as i16;
                let ip = self.state.ip;
                self.push_u16(bus, ip)?;
                self.state.ip = (self.state.ip as i16).wrapping_add(rel) as u16;
                Ok(StepOutcome::Continue { cycles: 19 })
            }

            // RET (0xC3, 0xC2)
            0xC3 => {
                self.state.ip = self.pop_u16(bus)?;
                Ok(StepOutcome::Continue { cycles: 10 })
            }
            0xC2 => {
                let pop_bytes = self.fetch_u16(bus)?;
                self.state.ip = self.pop_u16(bus)?;
                self.state.sp = self.state.sp.wrapping_add(pop_bytes);
                Ok(StepOutcome::Continue { cycles: 14 })
            }

            // Conditional jumps (0x70..0x7F)
            0x70..=0x7F => {
                let rel = self.fetch_u8(bus)? as i8;
                let cc = opcode & 0x0F;
                let f = self.state.flags;
                let take = match cc {
                    0x0 => (f & flags::OF) != 0,                                 // JO
                    0x1 => (f & flags::OF) == 0,                                 // JNO
                    0x2 => (f & flags::CF) != 0,                                 // JB / JC
                    0x3 => (f & flags::CF) == 0,                                 // JNB / JNC
                    0x4 => (f & flags::ZF) != 0,                                 // JZ / JE
                    0x5 => (f & flags::ZF) == 0,                                 // JNZ / JNE
                    0x6 => ((f & flags::CF) != 0) || ((f & flags::ZF) != 0),     // JBE
                    0x7 => ((f & flags::CF) == 0) && ((f & flags::ZF) == 0),     // JA
                    0x8 => (f & flags::SF) != 0,                                 // JS
                    0x9 => (f & flags::SF) == 0,                                 // JNS
                    0xA => (f & flags::PF) != 0,                                 // JP
                    0xB => (f & flags::PF) == 0,                                 // JNP
                    0xC => ((f & flags::SF) != 0) != ((f & flags::OF) != 0),     // JL
                    0xD => ((f & flags::SF) != 0) == ((f & flags::OF) != 0),     // JGE
                    0xE => ((f & flags::ZF) != 0) || (((f & flags::SF) != 0) != ((f & flags::OF) != 0)), // JLE
                    0xF => ((f & flags::ZF) == 0) && (((f & flags::SF) != 0) == ((f & flags::OF) != 0)), // JG
                    _ => false,
                };
                if take {
                    self.state.ip = (self.state.ip as i16).wrapping_add(rel as i16) as u16;
                }
                Ok(StepOutcome::Continue { cycles: if take { 16 } else { 4 } })
            }

            // LOOP / LOOPE / LOOPNE / JCXZ (0xE0..0xE3)
            0xE0 => {
                // LOOPNE / LOOPNZ
                let rel = self.fetch_u8(bus)? as i8;
                self.state.cx = self.state.cx.wrapping_sub(1);
                if self.state.cx != 0 && (self.state.flags & flags::ZF) == 0 {
                    self.state.ip = (self.state.ip as i16).wrapping_add(rel as i16) as u16;
                }
                Ok(StepOutcome::Continue { cycles: 17 })
            }
            0xE1 => {
                // LOOPE / LOOPZ
                let rel = self.fetch_u8(bus)? as i8;
                self.state.cx = self.state.cx.wrapping_sub(1);
                if self.state.cx != 0 && (self.state.flags & flags::ZF) != 0 {
                    self.state.ip = (self.state.ip as i16).wrapping_add(rel as i16) as u16;
                }
                Ok(StepOutcome::Continue { cycles: 17 })
            }
            0xE2 => {
                // LOOP
                let rel = self.fetch_u8(bus)? as i8;
                self.state.cx = self.state.cx.wrapping_sub(1);
                if self.state.cx != 0 {
                    self.state.ip = (self.state.ip as i16).wrapping_add(rel as i16) as u16;
                }
                Ok(StepOutcome::Continue { cycles: 17 })
            }
            0xE3 => {
                // JCXZ
                let rel = self.fetch_u8(bus)? as i8;
                if self.state.cx == 0 {
                    self.state.ip = (self.state.ip as i16).wrapping_add(rel as i16) as u16;
                }
                Ok(StepOutcome::Continue { cycles: 6 })
            }

            // String Operations (MOVS, CMPS, STOS, LODS, SCAS)
            0xA4 | 0xA5 => {
                // MOVSB / MOVSW
                let is_16 = opcode == 0xA5;
                let step = if (self.state.flags & flags::DF) != 0 { if is_16 { -2i16 } else { -1i16 } } else { if is_16 { 2i16 } else { 1i16 } };
                let seg_src = seg_override.unwrap_or(self.state.ds);
                let seg_dst = self.state.es;

                let repeat_count = match rep_prefix {
                    Some(_) => self.state.cx,
                    None => 1,
                };

                for _ in 0..repeat_count {
                    let src_addr = I8086State::linear_address(seg_src, self.state.si) as u64;
                    let dst_addr = I8086State::linear_address(seg_dst, self.state.di) as u64;
                    if is_16 {
                        let val = bus.read_u16(src_addr, Endianness::LittleEndian)?;
                        bus.write_u16(dst_addr, val, Endianness::LittleEndian)?;
                    } else {
                        let val = bus.read_u8(src_addr)?;
                        bus.write_u8(dst_addr, val)?;
                    }
                    self.state.si = (self.state.si as i16).wrapping_add(step) as u16;
                    self.state.di = (self.state.di as i16).wrapping_add(step) as u16;
                    if rep_prefix.is_some() {
                        self.state.cx = self.state.cx.wrapping_sub(1);
                    }
                }
                Ok(StepOutcome::Continue { cycles: 18 })
            }
            0xA6 | 0xA7 => {
                // CMPSB / CMPSW
                let is_16 = opcode == 0xA7;
                let step = if (self.state.flags & flags::DF) != 0 { if is_16 { -2i16 } else { -1i16 } } else { if is_16 { 2i16 } else { 1i16 } };
                let seg_src = seg_override.unwrap_or(self.state.ds);
                let seg_dst = self.state.es;

                loop {
                    let src_addr = I8086State::linear_address(seg_src, self.state.si) as u64;
                    let dst_addr = I8086State::linear_address(seg_dst, self.state.di) as u64;
                    if is_16 {
                        let v1 = bus.read_u16(src_addr, Endianness::LittleEndian)?;
                        let v2 = bus.read_u16(dst_addr, Endianness::LittleEndian)?;
                        self.alu_sub_u16(v1, v2, false);
                    } else {
                        let v1 = bus.read_u8(src_addr)?;
                        let v2 = bus.read_u8(dst_addr)?;
                        self.alu_sub_u8(v1, v2, false);
                    }
                    self.state.si = (self.state.si as i16).wrapping_add(step) as u16;
                    self.state.di = (self.state.di as i16).wrapping_add(step) as u16;

                    if let Some(rep) = rep_prefix {
                        self.state.cx = self.state.cx.wrapping_sub(1);
                        let zf = (self.state.flags & flags::ZF) != 0;
                        let stop = (rep == 0xF3 && !zf) || (rep == 0xF2 && zf) || self.state.cx == 0;
                        if stop { break; }
                    } else {
                        break;
                    }
                }
                Ok(StepOutcome::Continue { cycles: 22 })
            }
            0xAA | 0xAB => {
                // STOSB / STOSW
                let is_16 = opcode == 0xAB;
                let step = if (self.state.flags & flags::DF) != 0 { if is_16 { -2i16 } else { -1i16 } } else { if is_16 { 2i16 } else { 1i16 } };
                let seg_dst = self.state.es;

                let repeat_count = match rep_prefix {
                    Some(_) => self.state.cx,
                    None => 1,
                };

                for _ in 0..repeat_count {
                    let dst_addr = I8086State::linear_address(seg_dst, self.state.di) as u64;
                    if is_16 {
                        bus.write_u16(dst_addr, self.state.ax, Endianness::LittleEndian)?;
                    } else {
                        bus.write_u8(dst_addr, (self.state.ax & 0xFF) as u8)?;
                    }
                    self.state.di = (self.state.di as i16).wrapping_add(step) as u16;
                    if rep_prefix.is_some() {
                        self.state.cx = self.state.cx.wrapping_sub(1);
                    }
                }
                Ok(StepOutcome::Continue { cycles: 11 })
            }
            0xAC | 0xAD => {
                // LODSB / LODSW
                let is_16 = opcode == 0xAD;
                let step = if (self.state.flags & flags::DF) != 0 { if is_16 { -2i16 } else { -1i16 } } else { if is_16 { 2i16 } else { 1i16 } };
                let seg_src = seg_override.unwrap_or(self.state.ds);

                let src_addr = I8086State::linear_address(seg_src, self.state.si) as u64;
                if is_16 {
                    self.state.ax = bus.read_u16(src_addr, Endianness::LittleEndian)?;
                } else {
                    self.set_reg8(0, bus.read_u8(src_addr)?);
                }
                self.state.si = (self.state.si as i16).wrapping_add(step) as u16;
                Ok(StepOutcome::Continue { cycles: 12 })
            }
            0xAE | 0xAF => {
                // SCASB / SCASW
                let is_16 = opcode == 0xAF;
                let step = if (self.state.flags & flags::DF) != 0 { if is_16 { -2i16 } else { -1i16 } } else { if is_16 { 2i16 } else { 1i16 } };
                let seg_dst = self.state.es;

                loop {
                    let dst_addr = I8086State::linear_address(seg_dst, self.state.di) as u64;
                    if is_16 {
                        let v = bus.read_u16(dst_addr, Endianness::LittleEndian)?;
                        self.alu_sub_u16(self.state.ax, v, false);
                    } else {
                        let v = bus.read_u8(dst_addr)?;
                        self.alu_sub_u8((self.state.ax & 0xFF) as u8, v, false);
                    }
                    self.state.di = (self.state.di as i16).wrapping_add(step) as u16;

                    if let Some(rep) = rep_prefix {
                        self.state.cx = self.state.cx.wrapping_sub(1);
                        let zf = (self.state.flags & flags::ZF) != 0;
                        let stop = (rep == 0xF3 && !zf) || (rep == 0xF2 && zf) || self.state.cx == 0;
                        if stop { break; }
                    } else {
                        break;
                    }
                }
                Ok(StepOutcome::Continue { cycles: 15 })
            }

            // Port I/O (0xE4..0xE7, 0xEC..0xEF)
            0xE4 => {
                let port = self.fetch_u8(bus)? as u64;
                let val = bus.read_u8(port)?;
                self.set_reg8(0, val);
                Ok(StepOutcome::Continue { cycles: 10 })
            }
            0xE5 => {
                let port = self.fetch_u8(bus)? as u64;
                let val = bus.read_u16(port, Endianness::LittleEndian)?;
                self.state.ax = val;
                Ok(StepOutcome::Continue { cycles: 10 })
            }
            0xE6 => {
                let port = self.fetch_u8(bus)? as u64;
                bus.write_u8(port, (self.state.ax & 0xFF) as u8)?;
                Ok(StepOutcome::Continue { cycles: 10 })
            }
            0xE7 => {
                let port = self.fetch_u8(bus)? as u64;
                bus.write_u16(port, self.state.ax, Endianness::LittleEndian)?;
                Ok(StepOutcome::Continue { cycles: 10 })
            }
            0xEC => {
                let port = self.state.dx as u64;
                let val = bus.read_u8(port)?;
                self.set_reg8(0, val);
                Ok(StepOutcome::Continue { cycles: 8 })
            }
            0xED => {
                let port = self.state.dx as u64;
                let val = bus.read_u16(port, Endianness::LittleEndian)?;
                self.state.ax = val;
                Ok(StepOutcome::Continue { cycles: 8 })
            }
            0xEE => {
                let port = self.state.dx as u64;
                bus.write_u8(port, (self.state.ax & 0xFF) as u8)?;
                Ok(StepOutcome::Continue { cycles: 8 })
            }
            0xEF => {
                let port = self.state.dx as u64;
                bus.write_u16(port, self.state.ax, Endianness::LittleEndian)?;
                Ok(StepOutcome::Continue { cycles: 8 })
            }

            // Sign Extensions (CBW, CWD)
            0x98 => {
                // CBW
                let al = (self.state.ax & 0xFF) as u8 as i8;
                self.state.ax = (al as i16) as u16;
                Ok(StepOutcome::Continue { cycles: 2 })
            }
            0x99 => {
                // CWD
                self.state.dx = if (self.state.ax & 0x8000) != 0 { 0xFFFF } else { 0x0000 };
                Ok(StepOutcome::Continue { cycles: 5 })
            }

            // Flag Transfers (LAHF, SAHF)
            0x9F => {
                // LAHF (AH <- flags lower byte)
                let f = (self.state.flags & 0xFF) as u8;
                self.set_reg8(4, f);
                Ok(StepOutcome::Continue { cycles: 4 })
            }
            0x9E => {
                // SAHF (flags lower byte <- AH)
                let ah = self.get_reg8(4);
                self.state.flags = (self.state.flags & 0xFF00) | (ah as u16) | 0x0002;
                Ok(StepOutcome::Continue { cycles: 4 })
            }

            // BCD / Base Adjust Instructions (AAM, AAD)
            0xD4 => {
                // AAM
                let base = self.fetch_u8(bus)?;
                if base == 0 {
                    return Ok(StepOutcome::Interrupt(0));
                }
                let al = (self.state.ax & 0xFF) as u8;
                let ah = al / base;
                let al_rem = al % base;
                self.state.ax = ((ah as u16) << 8) | (al_rem as u16);
                let zf = al_rem == 0;
                let sf = (al_rem & 0x80) != 0;
                let pf = I8086State::parity(al_rem);
                self.set_flags(false, pf, false, zf, sf, false);
                Ok(StepOutcome::Continue { cycles: 83 })
            }
            0xD5 => {
                // AAD
                let base = self.fetch_u8(bus)?;
                let ah = (self.state.ax >> 8) as u8;
                let al = (self.state.ax & 0xFF) as u8;
                let res = ah.wrapping_mul(base).wrapping_add(al);
                self.state.ax = res as u16;
                let zf = res == 0;
                let sf = (res & 0x80) != 0;
                let pf = I8086State::parity(res);
                self.set_flags(false, pf, false, zf, sf, false);
                Ok(StepOutcome::Continue { cycles: 60 })
            }

            // Flag instructions
            0xF8 => { self.state.flags &= !flags::CF; Ok(StepOutcome::Continue { cycles: 2 }) } // CLC
            0xF9 => { self.state.flags |= flags::CF; Ok(StepOutcome::Continue { cycles: 2 }) }  // STC
            0xF5 => { self.state.flags ^= flags::CF; Ok(StepOutcome::Continue { cycles: 2 }) }  // CMC
            0xFA => { self.state.flags &= !flags::IF; Ok(StepOutcome::Continue { cycles: 2 }) } // CLI
            0xFB => { self.state.flags |= flags::IF; Ok(StepOutcome::Continue { cycles: 2 }) }  // STI
            0xFC => { self.state.flags &= !flags::DF; Ok(StepOutcome::Continue { cycles: 2 }) } // CLD
            0xFD => { self.state.flags |= flags::DF; Ok(StepOutcome::Continue { cycles: 2 }) }  // STD

            _ => Err(CpuError::InvalidInstruction {
                opcode: opcode as u64,
                pc: self.pc().wrapping_sub(1),
            }),
        }
    }

    fn register_count(&self) -> usize {
        14
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0 => Some(RegisterInfo {
                name: "AX",
                value: RegisterValue::U16(self.state.ax),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            1 => Some(RegisterInfo {
                name: "BX",
                value: RegisterValue::U16(self.state.bx),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            2 => Some(RegisterInfo {
                name: "CX",
                value: RegisterValue::U16(self.state.cx),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            3 => Some(RegisterInfo {
                name: "DX",
                value: RegisterValue::U16(self.state.dx),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            4 => Some(RegisterInfo {
                name: "SI",
                value: RegisterValue::U16(self.state.si),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            5 => Some(RegisterInfo {
                name: "DI",
                value: RegisterValue::U16(self.state.di),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            6 => Some(RegisterInfo {
                name: "BP",
                value: RegisterValue::U16(self.state.bp),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            7 => Some(RegisterInfo {
                name: "SP",
                value: RegisterValue::U16(self.state.sp),
                is_pc: false,
                is_sp: true,
                is_flags: false,
            }),
            8 => Some(RegisterInfo {
                name: "CS",
                value: RegisterValue::U16(self.state.cs),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            9 => Some(RegisterInfo {
                name: "DS",
                value: RegisterValue::U16(self.state.ds),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            10 => Some(RegisterInfo {
                name: "SS",
                value: RegisterValue::U16(self.state.ss),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            11 => Some(RegisterInfo {
                name: "ES",
                value: RegisterValue::U16(self.state.es),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            12 => Some(RegisterInfo {
                name: "IP",
                value: RegisterValue::U16(self.state.ip),
                is_pc: true,
                is_sp: false,
                is_flags: false,
            }),
            13 => Some(RegisterInfo {
                name: "FLAGS",
                value: RegisterValue::U16(self.state.flags),
                is_pc: false,
                is_sp: false,
                is_flags: true,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        if name.eq_ignore_ascii_case("AX") {
            Some(self.state.ax as u64)
        } else if name.eq_ignore_ascii_case("BX") {
            Some(self.state.bx as u64)
        } else if name.eq_ignore_ascii_case("CX") {
            Some(self.state.cx as u64)
        } else if name.eq_ignore_ascii_case("DX") {
            Some(self.state.dx as u64)
        } else if name.eq_ignore_ascii_case("SI") {
            Some(self.state.si as u64)
        } else if name.eq_ignore_ascii_case("DI") {
            Some(self.state.di as u64)
        } else if name.eq_ignore_ascii_case("BP") {
            Some(self.state.bp as u64)
        } else if name.eq_ignore_ascii_case("SP") {
            Some(self.state.sp as u64)
        } else if name.eq_ignore_ascii_case("CS") {
            Some(self.state.cs as u64)
        } else if name.eq_ignore_ascii_case("DS") {
            Some(self.state.ds as u64)
        } else if name.eq_ignore_ascii_case("SS") {
            Some(self.state.ss as u64)
        } else if name.eq_ignore_ascii_case("ES") {
            Some(self.state.es as u64)
        } else if name.eq_ignore_ascii_case("IP") || name.eq_ignore_ascii_case("PC") {
            Some(self.state.ip as u64)
        } else if name.eq_ignore_ascii_case("FLAGS") {
            Some(self.state.flags as u64)
        } else {
            None
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        let v16 = (val & 0xFFFF) as u16;
        if name.eq_ignore_ascii_case("AX") {
            self.state.ax = v16;
            Ok(())
        } else if name.eq_ignore_ascii_case("BX") {
            self.state.bx = v16;
            Ok(())
        } else if name.eq_ignore_ascii_case("CX") {
            self.state.cx = v16;
            Ok(())
        } else if name.eq_ignore_ascii_case("DX") {
            self.state.dx = v16;
            Ok(())
        } else if name.eq_ignore_ascii_case("SI") {
            self.state.si = v16;
            Ok(())
        } else if name.eq_ignore_ascii_case("DI") {
            self.state.di = v16;
            Ok(())
        } else if name.eq_ignore_ascii_case("BP") {
            self.state.bp = v16;
            Ok(())
        } else if name.eq_ignore_ascii_case("SP") {
            self.state.sp = v16;
            Ok(())
        } else if name.eq_ignore_ascii_case("CS") {
            self.state.cs = v16;
            Ok(())
        } else if name.eq_ignore_ascii_case("DS") {
            self.state.ds = v16;
            Ok(())
        } else if name.eq_ignore_ascii_case("SS") {
            self.state.ss = v16;
            Ok(())
        } else if name.eq_ignore_ascii_case("ES") {
            self.state.es = v16;
            Ok(())
        } else if name.eq_ignore_ascii_case("IP") {
            self.state.ip = v16;
            Ok(())
        } else if name.eq_ignore_ascii_case("FLAGS") {
            self.state.flags = v16 | 0x0002;
            Ok(())
        } else {
            Err(CpuError::RegisterNotFound)
        }
    }
}
