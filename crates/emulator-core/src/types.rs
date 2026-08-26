//! Fundamental types for architecture metadata, endianness, word size, and stack growth.

/// Byte order for instruction and data words.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Endianness {
    LittleEndian,
    BigEndian,
}

/// Direction in which the stack pointer advances as new frames are pushed.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum StackGrowth {
    /// Stack pointer decreases as items are pushed (e.g. x86, ARM, RISC-V, MIPS, SPARC, AVR, 68000, SuperH, Alpha, IA-64).
    Downwards,
    /// Stack pointer increases as items are pushed (e.g. PA-RISC).
    Upwards,
}

/// Standard native register or address word size.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum WordSize {
    Bytes1,
    Bytes2,
    Bytes4,
    Bytes8,
}

impl WordSize {
    /// Size in bytes.
    #[inline]
    pub const fn in_bytes(self) -> usize {
        match self {
            WordSize::Bytes1 => 1,
            WordSize::Bytes2 => 2,
            WordSize::Bytes4 => 4,
            WordSize::Bytes8 => 8,
        }
    }

    /// Size in bits.
    #[inline]
    pub const fn in_bits(self) -> usize {
        self.in_bytes() * 8
    }
}
