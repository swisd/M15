use egui::{Color32, RichText, ScrollArea, Ui};
use emulator_core::cpu::{CpuEngine, RegisterValue};

pub fn render_registers_panel(
    ui: &mut Ui,
    cpu: &mut dyn CpuEngine,
    editing_reg: &mut Option<(&'static str, String)>,
) {
    ui.heading("Registers");
    ui.separator();

    ScrollArea::vertical()
        .id_salt("registers_panel_scroll")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            egui::Grid::new("registers_grid")
                .striped(true)
                .min_col_width(50.0)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Name").strong());
                    ui.label(RichText::new("Hex").strong());
                    ui.label(RichText::new("Decimal").strong());
                    ui.label(RichText::new("Role").strong());
                    ui.end_row();

                    let count = cpu.register_count();
                    for i in 0..count {
                        if let Some(info) = cpu.register_info(i) {
                            let name = info.name;
                            let val_u64 = info.value.as_u64();

                            let is_pc = info.is_pc;
                            let is_sp = info.is_sp;
                            let is_flags = info.is_flags;

                            let (hex_str, dec_str) = match info.value {
                                RegisterValue::U8(v) => (format!("{:#04X}", v), format!("{}", v)),
                                RegisterValue::U16(v) => (format!("{:#06X}", v), format!("{}", v)),
                                RegisterValue::U32(v) => {
                                    (format!("{:#010X}", v), format!("{}", v))
                                }
                                RegisterValue::U64(v) => {
                                    (format!("{:#018X}", v), format!("{}", v))
                                }
                            };

                            let name_color = if is_pc {
                                Color32::from_rgb(255, 100, 100)
                            } else if is_sp {
                                Color32::from_rgb(100, 220, 100)
                            } else if is_flags {
                                Color32::from_rgb(255, 200, 80)
                            } else {
                                ui.visuals().text_color()
                            };

                            ui.label(RichText::new(name).strong().color(name_color));

                            // Hex column with click-to-edit
                            let mut val_btn = ui.link(RichText::new(hex_str).monospace());
                            if is_pc {
                                val_btn = val_btn.highlight();
                            }
                            if val_btn.clicked() {
                                *editing_reg = Some((name, format!("{:#X}", val_u64)));
                            }

                            // Decimal column
                            ui.label(
                                RichText::new(dec_str)
                                    .monospace()
                                    .color(ui.visuals().weak_text_color()),
                            );

                            // Role tags
                            let role_badge = if is_pc {
                                "[PC]"
                            } else if is_sp {
                                "[SP]"
                            } else if is_flags {
                                "[FLAGS]"
                            } else {
                                ""
                            };
                            ui.label(RichText::new(role_badge).strong().color(name_color));

                            ui.end_row();
                        }
                    }
                });
        });

    // Handle modal / popup editing of register
    if let Some((reg_name, buffer)) = editing_reg {
        let mut close = false;
        let title = format!("Edit Register {}", *reg_name);
        let name_str = *reg_name;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .show(ui.ctx(), |ui| {
                ui.label(format!("Enter new value for {} (e.g. 0x1234 or 4660):", name_str));
                ui.text_edit_singleline(buffer);
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        let parsed = if buffer.starts_with("0x") || buffer.starts_with("0X") {
                            u64::from_str_radix(&buffer[2..], 16).ok()
                        } else {
                            buffer.parse::<u64>().ok()
                        };

                        if let Some(val) = parsed {
                            let _ = cpu.set_register(name_str, val);
                        }
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });

        if close {
            *editing_reg = None;
        }
    }
}
