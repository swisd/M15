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
        let mut opcode = self.fetch_u8(bus)?;

        // Handle segment prefix overrides
        loop {
            match opcode {
                0x26 => { seg_override = Some(self.state.es); opcode = self.fetch_u8(bus)?; }
                0x2E => { seg_override = Some(self.state.cs); opcode = self.fetch_u8(bus)?; }
                0x36 => { seg_override = Some(self.state.ss); opcode = self.fetch_u8(bus)?; }
                0x3E => { seg_override = Some(self.state.ds); opcode = self.fetch_u8(bus)?; }
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

            // LEA r16, m16 (0x8D)
            0x8D => {
                let (ea, reg) = self.decode_modrm(bus, seg_override)?;
                if let EffectiveAddress::Memory { offset, .. } = ea {
                    self.set_reg16(reg, offset);
                }
                Ok(StepOutcome::Continue { cycles: 2 })
            }

            // XCHG AX, r16 (0x91..0x97)
            0x91..=0x97 => {
                let reg = opcode - 0x90;
                let tmp = self.state.ax;
                self.state.ax = self.get_reg16(reg);
                self.set_reg16(reg, tmp);
                Ok(StepOutcome::Continue { cycles: 3 })
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
                    4 => {
                        // JMP near indirect
                        let target = self.read_ea_u16(bus, ea)?;
                        self.state.ip = target;
                        Ok(StepOutcome::Continue { cycles: 11 })
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
