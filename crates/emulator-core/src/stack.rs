//! Universal stack inspection helpers.

use crate::bus::MemoryBus;
use crate::cpu::CpuEngine;
use crate::types::{Endianness, StackGrowth};

#[cfg(feature = "alloc")]
use alloc::vec::Vec;

/// Represents an individual decoded stack slot.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StackSlot {
    /// Memory address where this stack item resides.
    pub address: u64,
    /// Raw bytes of the stack slot.
    pub raw_bytes: [u8; 8],
    /// Size of the slot in bytes (1, 2, 4, or 8).
    pub size: usize,
    /// Numeric value decoded with architecture endianness.
    pub value: u64,
    /// Offset relative to current Stack Pointer (SP).
    pub offset_from_sp: i64,
}

/// Inspects stack slots starting from the current Stack Pointer.
///
/// Fills the provided `slots` buffer without heap allocation. Returns the number of slots successfully read.
pub fn inspect_stack<B: MemoryBus + ?Sized>(
    cpu: &dyn CpuEngine,
    bus: &B,
    slots: &mut [StackSlot],
) -> usize {
    let sp = cpu.sp();
    let word_size = cpu.word_size().in_bytes();
    let endian = cpu.endianness();
    let growth = cpu.stack_growth();

    let mut successful_slots = 0;
    for (i, slot) in slots.iter_mut().enumerate() {
        let (addr, offset_from_sp) = match growth {
            StackGrowth::Downwards => {
                // Downward stack: values pushed at SP, SP+word_size, SP+2*word_size, ...
                let offset = (i * word_size) as u64;
                (sp.wrapping_add(offset), offset as i64)
            }
            StackGrowth::Upwards => {
                // Upward stack (e.g. PA-RISC): values pushed at SP, SP-word_size, SP-2*word_size, ...
                let offset = (i * word_size) as u64;
                (sp.wrapping_sub(offset), -(offset as i64))
            }
        };

        let mut raw = [0u8; 8];
        if bus.read_bytes(addr, &mut raw[..word_size]).is_err() {
            return successful_slots;
        }

        let val = match word_size {
            1 => raw[0] as u64,
            2 => match endian {
                Endianness::LittleEndian => u16::from_le_bytes([raw[0], raw[1]]) as u64,
                Endianness::BigEndian => u16::from_be_bytes([raw[0], raw[1]]) as u64,
            },
            4 => match endian {
                Endianness::LittleEndian => {
                    u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as u64
                }
                Endianness::BigEndian => {
                    u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as u64
                }
            },
            8 => match endian {
                Endianness::LittleEndian => u64::from_le_bytes(raw),
                Endianness::BigEndian => u64::from_be_bytes(raw),
            },
            _ => 0,
        };

        *slot = StackSlot {
            address: addr,
            raw_bytes: raw,
            size: word_size,
            value: val,
            offset_from_sp,
        };
        successful_slots += 1;
    }

    successful_slots
}

/// Inspects stack slots into a dynamically allocated vector (requires `alloc` feature).
#[cfg(feature = "alloc")]
pub fn inspect_stack_vec<B: MemoryBus + ?Sized>(
    cpu: &dyn CpuEngine,
    bus: &B,
    slot_count: usize,
) -> Vec<StackSlot> {
    let mut slots = alloc::vec![
        StackSlot {
            address: 0,
            raw_bytes: [0u8; 8],
            size: 0,
            value: 0,
            offset_from_sp: 0,
        };
        slot_count
    ];
    let read_count = inspect_stack(cpu, bus, &mut slots);
    slots.truncate(read_count);
    slots
}
