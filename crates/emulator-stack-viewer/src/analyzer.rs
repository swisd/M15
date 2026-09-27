//! Stack analyzer logic inspecting CPU engine states and memory bus contents.

use alloc::string::String;
use alloc::vec::Vec;
use emulator_core::arch::Architecture;
use emulator_core::bus::MemoryBus;
use emulator_core::cpu::CpuEngine;
use emulator_core::types::{Endianness, StackGrowth};

use crate::model::{StackAnalysis, StackEntry, StackViewOptions};

/// Detects the likely frame pointer / base pointer register for the given architecture.
pub fn detect_frame_pointer(cpu: &dyn CpuEngine) -> Option<u64> {
    let fp_names: &[&str] = match cpu.arch() {
        Architecture::I8086 => &["BP"],
        Architecture::X86 => &["EBP", "BP"],
        Architecture::X86_64 => &["RBP", "EBP"],
        Architecture::Arm32 => &["R11", "FP", "r11"],
        Architecture::Arm64 => &["X29", "FP", "x29"],
        Architecture::RiscV => &["s0", "x8", "fp"],
        Architecture::Ia64 => &["r12", "bsp", "r1"],
        Architecture::Mips => &["$30", "$fp", "fp", "r30"],
        Architecture::PowerPc => &["r31", "r1"],
        Architecture::Sparc => &["%i6", "%fp", "i6", "fp"],
        Architecture::Avr => &["Y", "r28", "r29"],
        Architecture::SuperH => &["r14", "R14"],
        Architecture::PaRisc => &["r3", "r30"],
        Architecture::DecAlpha => &["r15", "$15", "fp"],
        Architecture::Motorola68000 => &["A6", "a6"],
        Architecture::Mos6502 => &["SP", "S"],
    };

    for &name in fp_names {
        if let Some(val) = cpu.get_register(name) {
            return Some(val);
        }
    }
    None
}

/// Returns the standard ABI stack alignment for an architecture in bytes.
pub fn standard_stack_alignment(arch: Architecture) -> usize {
    match arch {
        Architecture::X86_64
        | Architecture::Arm64
        | Architecture::DecAlpha
        | Architecture::Ia64 => 16,
        Architecture::Arm32
        | Architecture::RiscV
        | Architecture::Mips
        | Architecture::PowerPc
        | Architecture::Sparc
        | Architecture::SuperH
        | Architecture::PaRisc => 8,
        Architecture::X86 | Architecture::Motorola68000 => 4,
        Architecture::I8086 => 2,
        Architecture::Avr | Architecture::Mos6502 => 1,
    }
}

/// Analyzes the stack for a given CPU and memory bus according to the specified options.
pub fn analyze_stack<B: MemoryBus + ?Sized>(
    cpu: &dyn CpuEngine,
    bus: &B,
    options: &StackViewOptions,
) -> StackAnalysis {
    let arch = cpu.arch();
    let sp = cpu.sp();
    let fp = detect_frame_pointer(cpu);
    let growth = cpu.stack_growth();
    let endianness = cpu.endianness();

    let word_size = options
        .word_size_override
        .map(|w| w.in_bytes())
        .unwrap_or_else(|| cpu.word_size().in_bytes());

    let expected_alignment = standard_stack_alignment(arch);
    let is_aligned = (sp as usize).is_multiple_of(expected_alignment);

    let anchor = if options.lock_to_sp {
        sp
    } else {
        options.custom_base_addr.unwrap_or(sp)
    };

    let base_addr = match growth {
        StackGrowth::Downwards => {
            anchor.wrapping_add((options.scroll_offset_slots * word_size as i64) as u64)
        }
        StackGrowth::Upwards => {
            anchor.wrapping_sub((options.scroll_offset_slots * word_size as i64) as u64)
        }
    };

    let mut entries = Vec::with_capacity(options.slot_count);
    let above_sp_count = options.slots_above_sp.min(options.slot_count);

    for i in 0..options.slot_count {
        let slot_idx = (i as isize) - (above_sp_count as isize);
        let (addr, offset_from_sp) = match growth {
            StackGrowth::Downwards => {
                let offset_bytes = slot_idx * (word_size as isize);
                let a = base_addr.wrapping_add(offset_bytes as u64);
                (a, (a as i64) - (sp as i64))
            }
            StackGrowth::Upwards => {
                let offset_bytes = slot_idx * (word_size as isize);
                let a = base_addr.wrapping_sub(offset_bytes as u64);
                (a, (sp as i64) - (a as i64))
            }
        };

        let mut raw = [0u8; 8];
        let read_res = bus.read_bytes(addr, &mut raw[..word_size]);
        if read_res.is_err() {
            // Unmapped memory - record 0-byte entry or continue
            raw = [0u8; 8];
        }

        let value = match word_size {
            1 => raw[0] as u64,
            2 => match endianness {
                Endianness::LittleEndian => u16::from_le_bytes([raw[0], raw[1]]) as u64,
                Endianness::BigEndian => u16::from_be_bytes([raw[0], raw[1]]) as u64,
            },
            4 => match endianness {
                Endianness::LittleEndian => {
                    u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as u64
                }
                Endianness::BigEndian => {
                    u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]) as u64
                }
            },
            8 => match endianness {
                Endianness::LittleEndian => u64::from_le_bytes(raw),
                Endianness::BigEndian => u64::from_be_bytes(raw),
            },
            _ => 0,
        };

        let is_sp = addr == sp;
        let is_fp = fp.is_some_and(|f| f == addr);

        let mut annotations = Vec::new();
        if is_sp {
            annotations.push(String::from("[SP] Top of Stack"));
        }
        if is_fp {
            annotations.push(String::from("[FP] Frame Pointer"));
        }
        if !is_sp && !is_fp {
            match growth {
                StackGrowth::Downwards if (addr as i64) < (sp as i64) => {
                    annotations.push(String::from("[Above SP / Free]"));
                }
                StackGrowth::Upwards if (addr as i64) > (sp as i64) => {
                    annotations.push(String::from("[Above SP / Free]"));
                }
                _ => {}
            }
        }

        let annotation = if annotations.is_empty() {
            None
        } else {
            Some(annotations.join(" | "))
        };

        entries.push(StackEntry {
            address: addr,
            raw_bytes: raw,
            size: word_size,
            value,
            offset_from_sp,
            slot_index: i,
            is_sp,
            is_fp,
            annotation,
        });
    }

    StackAnalysis {
        arch,
        sp,
        fp,
        growth,
        word_size,
        endianness,
        entries,
        expected_alignment,
        is_aligned,
    }
}
