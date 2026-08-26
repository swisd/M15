use egui::{Color32, RichText, ScrollArea, Ui};
use emulator_core::bus::{DynamicMemory, MemoryBus};
use emulator_core::cpu::CpuEngine;

pub fn render_code_panel(
    ui: &mut Ui,
    cpu: &dyn CpuEngine,
    bus: &DynamicMemory,
    breakpoints: &mut Vec<u64>,
) {
    ui.heading("Disassembly / Instruction Stream");
    ui.separator();

    let pc = cpu.pc();
    let word_size = cpu.word_size().in_bytes();
    let instr_count = 32;
    let start_pc = pc.saturating_sub((6 * word_size) as u64);

    ScrollArea::vertical()
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            egui::Grid::new("disasm_grid")
                .striped(true)
                .min_col_width(50.0)
                .spacing([14.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("BP").strong());
                    ui.label(RichText::new("PC").strong());
                    ui.label(RichText::new("Address").strong());
                    ui.label(RichText::new("Opcode Bytes").strong());
                    ui.label(RichText::new("Preview / Status").strong());
                    ui.end_row();

                    for i in 0..instr_count {
                        let addr = start_pc.wrapping_add((i * word_size) as u64);
                        let is_current_pc = addr == pc;
                        let is_breakpoint = breakpoints.contains(&addr);

                        // BP Column
                        let bp_btn = if is_breakpoint {
                            ui.button(RichText::new("🔴").color(Color32::RED))
                        } else {
                            ui.button("⚪")
                        };
                        if bp_btn.clicked() {
                            if is_breakpoint {
                                breakpoints.retain(|&b| b != addr);
                            } else {
                                breakpoints.push(addr);
                            }
                        }

                        // PC Pointer arrow
                        if is_current_pc {
                            ui.label(RichText::new("=>").strong().color(Color32::from_rgb(255, 100, 100)));
                        } else {
                            ui.label("  ");
                        }

                        // Address
                        let addr_color = if is_current_pc {
                            Color32::from_rgb(255, 100, 100)
                        } else {
                            Color32::from_rgb(100, 200, 255)
                        };
                        ui.label(RichText::new(format!("{:#010X}", addr)).monospace().color(addr_color));

                        // Opcode Bytes
                        let mut bytes = [0u8; 8];
                        let _ = bus.read_bytes(addr, &mut bytes[..word_size]);
                        let mut opcode_str = String::new();
                        for (b, &byte) in bytes[..word_size].iter().enumerate() {
                            if b > 0 {
                                opcode_str.push(' ');
                            }
                            opcode_str.push_str(&format!("{:02X}", byte));
                        }
                        ui.label(RichText::new(opcode_str).monospace().color(ui.visuals().weak_text_color()));

                        // Preview
                        let preview_text = if is_current_pc {
                            "CURRENT INSTRUCTION"
                        } else if addr < pc {
                            "Executed"
                        } else {
                            "Upcoming"
                        };
                        ui.label(RichText::new(preview_text).weak());

                        ui.end_row();
                    }
                });
        });
}
