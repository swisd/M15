//! Data models and configurations for stack viewing and frame analysis.

use emulator_core::arch::Architecture;
use emulator_core::types::{Endianness, StackGrowth, WordSize};

#[cfg(feature = "alloc")]
use alloc::string::String;
#[cfg(feature = "alloc")]
use alloc::vec::Vec;

/// Numeric display formatting options for stack slot values.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum DisplayFormat {
    /// Hexadecimal formatting with prefix (e.g. `0x1234_5678`).
    #[default]
    Hex,
    /// Unsigned decimal formatting.
    Decimal,
    /// Signed decimal formatting.
    SignedDecimal,
    /// Binary formatting (e.g. `0b0010_1010`).
    Binary,
    /// Printable ASCII character representation.
    Ascii,
}

/// A decoded stack slot entry with metadata, offsets, and tags.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackEntry {
    /// Absolute memory address of this stack slot.
    pub address: u64,
    /// Raw byte buffer.
    pub raw_bytes: [u8; 8],
    /// Native slot size in bytes (1, 2, 4, or 8).
    pub size: usize,
    /// Decoded numeric integer value.
    pub value: u64,
    /// Byte offset relative to the current Stack Pointer (e.g. 0, +8, -8).
    pub offset_from_sp: i64,
    /// Sequential slot index relative to stack pointer (0 = top of stack).
    pub slot_index: usize,
    /// Whether this slot coincides with the current Stack Pointer.
    pub is_sp: bool,
    /// Whether this slot coincides with the current Frame/Base Pointer.
    pub is_fp: bool,
    /// Contextual annotation (e.g., "[SP] Top of Stack", "[FP] Saved Frame", "[RET]").
    pub annotation: Option<String>,
}

impl StackEntry {
    /// Formats the slot value according to the chosen `DisplayFormat`.
    pub fn format_value(&self, format: DisplayFormat) -> String {
        #[cfg(feature = "alloc")]
        {
            use alloc::format;
            match format {
                DisplayFormat::Hex => match self.size {
                    1 => format!("{:#04X}", self.value as u8),
                    2 => format!("{:#06X}", self.value as u16),
                    4 => format!("{:#010X}", self.value as u32),
                    8 => format!("{:#018X}", self.value),
                    _ => format!("{:#X}", self.value),
                },
                DisplayFormat::Decimal => match self.size {
                    1 => format!("{}", self.value as u8),
                    2 => format!("{}", self.value as u16),
                    4 => format!("{}", self.value as u32),
                    8 => format!("{}", self.value),
                    _ => format!("{}", self.value),
                },
                DisplayFormat::SignedDecimal => match self.size {
                    1 => format!("{}", (self.value as u8) as i8),
                    2 => format!("{}", (self.value as u16) as i16),
                    4 => format!("{}", (self.value as u32) as i32),
                    8 => format!("{}", self.value as i64),
                    _ => format!("{}", self.value as i64),
                },
                DisplayFormat::Binary => match self.size {
                    1 => format!("0b{:08b}", self.value as u8),
                    2 => format!("0b{:016b}", self.value as u16),
                    4 => format!("0b{:032b}", self.value as u32),
                    8 => format!("0b{:064b}", self.value),
                    _ => format!("0b{:b}", self.value),
                },
                DisplayFormat::Ascii => self.ascii_representation(),
            }
        }
    }

    /// Formats raw bytes as space-separated hex bytes.
    pub fn format_raw_bytes(&self) -> String {
        #[cfg(feature = "alloc")]
        {
            use alloc::format;
            let mut s = String::new();
            for i in 0..self.size {
                if i > 0 {
                    s.push(' ');
                }
                s.push_str(&format!("{:02X}", self.raw_bytes[i]));
            }
            s
        }
    }

    /// Returns a printable ASCII string representation of the slot bytes.
    pub fn ascii_representation(&self) -> String {
        #[cfg(feature = "alloc")]
        {
            let mut s = String::new();
            for &b in &self.raw_bytes[..self.size] {
                if b.is_ascii_graphic() || b == b' ' {
                    s.push(b as char);
                } else {
                    s.push('.');
                }
            }
            s
        }
    }
}

/// Comprehensive analysis of the active stack frame and memory contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackAnalysis {
    /// Architecture being inspected.
    pub arch: Architecture,
    /// Current Stack Pointer register value.
    pub sp: u64,
    /// Frame Pointer / Base Pointer value if identifiable.
    pub fp: Option<u64>,
    /// Direction in which stack grows.
    pub growth: StackGrowth,
    /// Slot word size in bytes.
    pub word_size: usize,
    /// Endianness used for decoding.
    pub endianness: Endianness,
    /// Decoded stack slots.
    pub entries: Vec<StackEntry>,
    /// Required or standard ABI stack alignment for this architecture.
    pub expected_alignment: usize,
    /// Whether SP is currently aligned to `expected_alignment`.
    pub is_aligned: bool,
}

/// User-configurable options for stack inspection and UI presentation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackViewOptions {
    /// Number of stack slots to inspect and display (default: 16).
    pub slot_count: usize,
    /// Active numeric display format (default: Hex).
    pub display_format: DisplayFormat,
    /// Whether to display ASCII column.
    pub show_ascii: bool,
    /// Whether to display raw bytes column.
    pub show_raw_bytes: bool,
    /// Whether to show relative offset strings (e.g. `SP+0x00`).
    pub show_relative_offset: bool,
    /// Optional manual base address override (default: uses CPU SP).
    pub custom_base_addr: Option<u64>,
    /// Optional word size override (e.g., viewing 16-bit slots as 32-bit).
    pub word_size_override: Option<WordSize>,
}

impl Default for StackViewOptions {
    fn default() -> Self {
        Self {
            slot_count: 16,
            display_format: DisplayFormat::Hex,
            show_ascii: true,
            show_raw_bytes: true,
            show_relative_offset: true,
            custom_base_addr: None,
            word_size_override: None,
        }
    }
}
