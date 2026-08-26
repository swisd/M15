//! Text formatting and table generation for stack frames.

use alloc::format;
use alloc::string::String;

use crate::model::{StackAnalysis, StackViewOptions};

/// Formats a complete stack analysis into a readable plain text table.
pub fn format_stack_table(analysis: &StackAnalysis, options: &StackViewOptions) -> String {
    let mut out = String::new();

    let growth_str = match analysis.growth {
        emulator_core::types::StackGrowth::Downwards => "Downwards (High to Low)",
        emulator_core::types::StackGrowth::Upwards => "Upwards (Low to High)",
    };

    let align_str = if analysis.is_aligned {
        "Aligned"
    } else {
        "UNALIGNED"
    };

    out.push_str(&format!(
        "--- Stack Viewer [{:?}] ---\nSP: {:#010X} | Growth: {} | Alignment: {}-byte ({})\n",
        analysis.arch, analysis.sp, growth_str, analysis.expected_alignment, align_str
    ));

    if let Some(fp) = analysis.fp {
        out.push_str(&format!("FP: {:#010X}\n", fp));
    }

    out.push_str(
        "+---------+--------------------+--------------------+-------------------------+----------+----------------------+\n",
    );
    out.push_str(
        "| Offset  | Address            | Value              | Raw Bytes               | ASCII    | Annotations          |\n",
    );
    out.push_str(
        "+---------+--------------------+--------------------+-------------------------+----------+----------------------+\n",
    );

    for entry in &analysis.entries {
        let offset_str = if entry.offset_from_sp >= 0 {
            format!("SP+0x{:02X}", entry.offset_from_sp)
        } else {
            format!("SP-0x{:02X}", -entry.offset_from_sp)
        };

        let addr_str = match analysis.word_size {
            1 | 2 => format!("{:#06X}", entry.address),
            4 => format!("{:#010X}", entry.address),
            8 => format!("{:#018X}", entry.address),
            _ => format!("{:#X}", entry.address),
        };

        let val_str = entry.format_value(options.display_format);
        let raw_str = entry.format_raw_bytes();
        let ascii_str = entry.ascii_representation();
        let annot_str = entry.annotation.as_deref().unwrap_or("");

        out.push_str(&format!(
            "| {:<7} | {:<18} | {:<18} | {:<23} | {:<8} | {:<20} |\n",
            offset_str, addr_str, val_str, raw_str, ascii_str, annot_str
        ));
    }

    out.push_str(
        "+---------+--------------------+--------------------+-------------------------+----------+----------------------+\n",
    );

    out
}
