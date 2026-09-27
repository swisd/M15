use egui::{Color32, RichText, ScrollArea, Ui};
use emulator_core::bus::{DynamicMemory, MemoryBus};
use emulator_core::cpu::CpuEngine;

pub fn render_memory_panel(
    ui: &mut Ui,
    bus: &mut DynamicMemory,
    cpu: &dyn CpuEngine,
    view_addr: &mut u64,
    addr_input: &mut String,
) {
    ui.heading("Memory Hex Viewer");
    ui.separator();

    ui.horizontal(|ui| {
        ui.label("Address:");
        let response = ui.text_edit_singleline(addr_input);
        let parse_and_jump = |input: &str, target: &mut u64| {
            if let Ok(addr) = u64::from_str_radix(input.trim_start_matches("0x").trim_start_matches("0X"), 16) {
                *target = addr;
            }
        };

        if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            parse_and_jump(addr_input, view_addr);
        }
        if ui.button("Go").clicked() {
            parse_and_jump(addr_input, view_addr);
        }
        if ui.button("Jump to PC").clicked() {
            *view_addr = cpu.pc();
            *addr_input = format!("{:#X}", cpu.pc());
        }
        if ui.button("Jump to SP").clicked() {
            *view_addr = cpu.sp();
            *addr_input = format!("{:#X}", cpu.sp());
        }
    });

    ui.add_space(4.0);

    let base_addr = *view_addr & !0x0F;
    let rows = 32;
    let bytes_per_row = 16;

    ScrollArea::vertical()
        .id_salt("memory_panel_scroll")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            egui::Grid::new("memory_hex_grid")
                .striped(true)
                .spacing([10.0, 3.0])
                .show(ui, |ui| {
                    // Header
                    ui.label(RichText::new("Offset").strong());
                    for b in 0..bytes_per_row {
                        ui.label(RichText::new(format!("{:02X}", b)).strong());
                    }
                    ui.label(RichText::new("ASCII").strong());
                    ui.end_row();

                    for row in 0..rows {
                        let row_addr = base_addr.wrapping_add((row * bytes_per_row) as u64);
                        ui.label(RichText::new(format!("{:#010X}", row_addr)).monospace().color(Color32::from_rgb(100, 200, 255)));

                        let mut ascii_repr = String::new();

                        for b in 0..bytes_per_row {
                            let cell_addr = row_addr.wrapping_add(b as u64);
                            let byte_val = bus.read_u8(cell_addr).unwrap_or(0);

                            let is_pc = cell_addr == cpu.pc();
                            let is_sp = cell_addr == cpu.sp();

                            let cell_color = if is_pc {
                                Color32::from_rgb(255, 100, 100)
                            } else if is_sp {
                                Color32::from_rgb(100, 220, 100)
                            } else if byte_val != 0 {
                                Color32::WHITE
                            } else {
                                Color32::from_rgb(120, 120, 120)
                            };

                            ui.label(
                                RichText::new(format!("{:02X}", byte_val))
                                    .monospace()
                                    .color(cell_color),
                            );

                            if byte_val.is_ascii_graphic() || byte_val == b' ' {
                                ascii_repr.push(byte_val as char);
                            } else {
                                ascii_repr.push('.');
                            }
                        }

                        ui.label(RichText::new(ascii_repr).monospace().color(Color32::from_rgb(180, 180, 120)));
                        ui.end_row();
                    }
                });
        });
}
