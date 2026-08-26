//! Memory bus abstractions and implementations for `#![no_std]` and hosted environments.

use crate::types::Endianness;

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

/// Memory access errors.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MemoryError {
    OutOfBounds(u64),
    AlignmentFault(u64),
    ReadOnly(u64),
    BusFault(&'static str),
}

/// Abstract memory bus supporting byte, multi-byte, and slice operations.
pub trait MemoryBus {
    /// Reads a single 8-bit byte from memory.
    fn read_u8(&self, addr: u64) -> Result<u8, MemoryError>;

    /// Writes a single 8-bit byte to memory.
    fn write_u8(&mut self, addr: u64, val: u8) -> Result<(), MemoryError>;

    /// Reads a slice of bytes into `buf`.
    fn read_bytes(&self, addr: u64, buf: &mut [u8]) -> Result<(), MemoryError> {
        for (i, b) in buf.iter_mut().enumerate() {
            *b = self.read_u8(addr.wrapping_add(i as u64))?;
        }
        Ok(())
    }

    /// Writes a slice of bytes from `data` into memory.
    fn write_bytes(&mut self, addr: u64, data: &[u8]) -> Result<(), MemoryError> {
        for (i, &b) in data.iter().enumerate() {
            self.write_u8(addr.wrapping_add(i as u64), b)?;
        }
        Ok(())
    }

    /// Reads a 16-bit integer with explicit endianness.
    fn read_u16(&self, addr: u64, endian: Endianness) -> Result<u16, MemoryError> {
        let mut buf = [0u8; 2];
        self.read_bytes(addr, &mut buf)?;
        Ok(match endian {
            Endianness::LittleEndian => u16::from_le_bytes(buf),
            Endianness::BigEndian => u16::from_be_bytes(buf),
        })
    }

    /// Writes a 16-bit integer with explicit endianness.
    fn write_u16(&mut self, addr: u64, val: u16, endian: Endianness) -> Result<(), MemoryError> {
        let bytes = match endian {
            Endianness::LittleEndian => val.to_le_bytes(),
            Endianness::BigEndian => val.to_be_bytes(),
        };
        self.write_bytes(addr, &bytes)
    }

    /// Reads a 32-bit integer with explicit endianness.
    fn read_u32(&self, addr: u64, endian: Endianness) -> Result<u32, MemoryError> {
        let mut buf = [0u8; 4];
        self.read_bytes(addr, &mut buf)?;
        Ok(match endian {
            Endianness::LittleEndian => u32::from_le_bytes(buf),
            Endianness::BigEndian => u32::from_be_bytes(buf),
        })
    }

    /// Writes a 32-bit integer with explicit endianness.
    fn write_u32(&mut self, addr: u64, val: u32, endian: Endianness) -> Result<(), MemoryError> {
        let bytes = match endian {
            Endianness::LittleEndian => val.to_le_bytes(),
            Endianness::BigEndian => val.to_be_bytes(),
        };
        self.write_bytes(addr, &bytes)
    }

    /// Reads a 64-bit integer with explicit endianness.
    fn read_u64(&self, addr: u64, endian: Endianness) -> Result<u64, MemoryError> {
        let mut buf = [0u8; 8];
        self.read_bytes(addr, &mut buf)?;
        Ok(match endian {
            Endianness::LittleEndian => u64::from_le_bytes(buf),
            Endianness::BigEndian => u64::from_be_bytes(buf),
        })
    }

    /// Writes a 64-bit integer with explicit endianness.
    fn write_u64(&mut self, addr: u64, val: u64, endian: Endianness) -> Result<(), MemoryError> {
        let bytes = match endian {
            Endianness::LittleEndian => val.to_le_bytes(),
            Endianness::BigEndian => val.to_be_bytes(),
        };
        self.write_bytes(addr, &bytes)
    }
}

/// A fixed-size array memory bus suitable for `#![no_std]` bare-metal targets without heap allocation.
pub struct ArrayMemory<const SIZE: usize> {
    pub data: [u8; SIZE],
}

impl<const SIZE: usize> ArrayMemory<SIZE> {
    pub const fn new() -> Self {
        Self {
            data: [0u8; SIZE],
        }
    }

    pub fn len(&self) -> usize {
        SIZE
    }

    pub fn is_empty(&self) -> bool {
        SIZE == 0
    }
}

impl<const SIZE: usize> Default for ArrayMemory<SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const SIZE: usize> MemoryBus for ArrayMemory<SIZE> {
    fn read_u8(&self, addr: u64) -> Result<u8, MemoryError> {
        let idx = addr as usize;
        if idx < SIZE {
            Ok(self.data[idx])
        } else {
            Err(MemoryError::OutOfBounds(addr))
        }
    }

    fn write_u8(&mut self, addr: u64, val: u8) -> Result<(), MemoryError> {
        let idx = addr as usize;
        if idx < SIZE {
            self.data[idx] = val;
            Ok(())
        } else {
            Err(MemoryError::OutOfBounds(addr))
        }
    }
}

/// A slice-backed memory bus borrowing a mutable byte buffer.
pub struct SliceMemory<'a> {
    pub data: &'a mut [u8],
}

impl<'a> SliceMemory<'a> {
    pub fn new(data: &'a mut [u8]) -> Self {
        Self { data }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl<'a> MemoryBus for SliceMemory<'a> {
    fn read_u8(&self, addr: u64) -> Result<u8, MemoryError> {
        let idx = addr as usize;
        if idx < self.data.len() {
            Ok(self.data[idx])
        } else {
            Err(MemoryError::OutOfBounds(addr))
        }
    }

    fn write_u8(&mut self, addr: u64, val: u8) -> Result<(), MemoryError> {
        let idx = addr as usize;
        let len = self.data.len();
        if idx < len {
            self.data[idx] = val;
            Ok(())
        } else {
            Err(MemoryError::OutOfBounds(addr))
        }
    }
}

/// A dynamically resizable vector-backed memory bus (available with `alloc` feature).
#[cfg(feature = "alloc")]
pub struct DynamicMemory {
    pub data: Vec<u8>,
}

#[cfg(feature = "alloc")]
impl DynamicMemory {
    pub fn new(size: usize) -> Self {
        Self {
            data: alloc::vec![0u8; size],
        }
    }

    pub fn from_vec(data: Vec<u8>) -> Self {
        Self { data }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[cfg(feature = "alloc")]
impl MemoryBus for DynamicMemory {
    fn read_u8(&self, addr: u64) -> Result<u8, MemoryError> {
        let idx = addr as usize;
        if idx < self.data.len() {
            Ok(self.data[idx])
        } else {
            Err(MemoryError::OutOfBounds(addr))
        }
    }

    fn write_u8(&mut self, addr: u64, val: u8) -> Result<(), MemoryError> {
        let idx = addr as usize;
        if idx < self.data.len() {
            self.data[idx] = val;
            Ok(())
        } else {
            Err(MemoryError::OutOfBounds(addr))
        }
    }
}
