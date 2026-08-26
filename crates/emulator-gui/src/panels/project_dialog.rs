//! Project and ASM file loading dialogs, interactive ASM scratchpad,
//! and architecture fallback selection modal.

use egui::{Color32, RichText, Window};
use emulator_core::arch::Architecture;

use crate::app::EmulatorApp;

/// Renders the Project & File Loader modal window.
pub fn render_load_dialog(ctx: &egui::Context, app: &mut EmulatorApp) {
    if !app.show_load_dialog {
        return;
    }

    let mut open = app.show_load_dialog;

    Window::new("📂 Load ASM Project / File / Scratchpad")
        .open(&mut open)
        .resizable(true)
        .default_width(650.0)
        .default_height(500.0)
        .show(ctx, |ui| {
            ui.heading("Load Assembly Project or File");
            ui.label(
                "Open a directory containing `mconfig.toml` / ASM files, or load an individual source file with automatic architecture detection.",
            );
            ui.separator();

            ui.horizontal(|ui| {
                ui.label(RichText::new("Path:").strong());
                ui.text_edit_singleline(&mut app.load_path_input);
                if ui.button("📂 Load Path").clicked() {
                    let path = app.load_path_input.trim().to_string();
                    if !path.is_empty() {
                        let p = std::path::Path::new(&path);
                        if p.is_dir() {
                            app.load_project_from_dir(&path);
                        } else if p.is_file() {
                            app.load_asm_file(&path, None);
                        } else {
                            app.status_message = Some(format!("Path does not exist: '{}'", path));
                        }
                    }
                }
            });

            ui.add_space(4.0);
            ui.collapsing("ℹ `mconfig.toml` project format info", |ui| {
                ui.label("When loading a folder, `mconfig.toml` in the project root configures the target architecture and entry points:");
                ui.monospace(
                    r#"[project]
name = "My Assembly Project"
arch = "8086"       # e.g. 8086, risc-v, avr, 6502, x86_64, arm32, arm64, etc.
entry_point = "0x1000"
sp = "0xFFF8"
main = "main.asm"   # primary source file
files = ["main.asm"]"#,
                );
            });

            ui.separator();

            // Preset Project / File loader
            ui.horizontal(|ui| {
                ui.label(RichText::new("Quick Preset ASM Projects:").strong());
                egui::ComboBox::from_id_salt("quick_preset_combo")
                    .selected_text("Select Sample Assembly...")
                    .show_ui(ui, |ui| {
                        if ui.button("Intel 8086 Arithmetic & Stack").clicked() {
                            app.asm_editor_input = r#"; arch: 8086
.org 0x1000
mov ax, 0x0042
mov bx, 0x0010
add ax, bx
push ax
pop cx
hlt
"#.to_string();
                        }
                        if ui.button("RISC-V (RV32I) Register Math").clicked() {
                            app.asm_editor_input = r#".arch riscv
.org 0x1000
addi x1, x0, 25
addi x2, x0, 17
add x3, x1, x2
ebreak
"#.to_string();
                        }
                        if ui.button("MOS 6502 Accumulator & Page 1 Stack").clicked() {
                            app.asm_editor_input = r#"processor 6502
.org 0x0600
lda #$12
tax
pha
pla
"#.to_string();
                        }
                        if ui.button("Atmel AVR 8-bit SRAM Stack").clicked() {
                            app.asm_editor_input = r#".device atmega328p
.org 0x0000
ldi r16, 0x30
ldi r17, 0x15
add r16, r17
push r16
pop r18
"#.to_string();
                        }
                    });
            });

            ui.separator();

            // Interactive In-App ASM Editor / Scratchpad
            ui.label(RichText::new("Interactive Assembly Source Editor:").strong());
            ui.label("Write or paste assembly instructions directly below. Supports `.org`, `.entry`, `.byte`, labels, and architecture instructions.");

            egui::ScrollArea::vertical()
                .max_height(220.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut app.asm_editor_input)
                            .font(egui::TextStyle::Monospace)
                            .desired_rows(10)
                            .desired_width(f32::INFINITY),
                    );
                });

            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.label("Target Architecture:");
                egui::ComboBox::from_id_salt("editor_arch_override")
                    .selected_text(
                        app.editor_arch_selection
                            .map(|a| a.name())
                            .unwrap_or("Auto-Detect from Source"),
                    )
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(app.editor_arch_selection.is_none(), "Auto-Detect from Source").clicked() {
                            app.editor_arch_selection = None;
                        }
                        ui.separator();
                        for &arch in Architecture::ALL {
                            if ui.selectable_label(app.editor_arch_selection == Some(arch), arch.name()).clicked() {
                                app.editor_arch_selection = Some(arch);
                            }
                        }
                    });

                ui.add_space(10.0);

                if ui.button(RichText::new("⚡ Assemble & Run Program").color(Color32::from_rgb(80, 220, 100)).strong()).clicked() {
                    let src = app.asm_editor_input.clone();
                    if !src.trim().is_empty() {
                        let manual_override = app.editor_arch_selection;
                        app.load_asm_source(&src, "Scratchpad Program", None, manual_override);
                    }
                }

                if ui.button("Close").clicked() {
                    app.show_load_dialog = false;
                }
            });
        });

    app.show_load_dialog = open;
}

/// Renders the architecture fallback modal window when autodetection is ambiguous or fails.
pub fn render_arch_picker_modal(ctx: &egui::Context, app: &mut EmulatorApp) {
    if !app.show_arch_picker_modal {
        return;
    }

    let mut open = app.show_arch_picker_modal;
    let mut selected_arch = None;
    let mut cancelled = false;

    let candidates = app.pending_candidates.clone();

    Window::new("❓ Select Target CPU Architecture")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(520.0)
        .show(ctx, |ui| {
            ui.heading("Architecture Autodetection Prompt");
            ui.label(RichText::new("Could not automatically determine the target CPU architecture for this program with high confidence.").color(Color32::from_rgb(255, 200, 80)));
            ui.label("Please select the target CPU architecture from the list below to assemble and execute:");

            if !candidates.is_empty() {
                ui.add_space(4.0);
                ui.label(RichText::new("Top Candidates Detected:").strong());
                ui.horizontal_wrapped(|ui| {
                    for &cand in &candidates {
                        let btn = ui.button(RichText::new(format!("⭐ {}", cand.display_name())).strong().color(Color32::from_rgb(100, 220, 255)));
                        if btn.clicked() {
                            selected_arch = Some(cand);
                        }
                    }
                });
            }

            ui.separator();
            ui.label(RichText::new("All Supported Architectures (16):").strong());

            egui::Grid::new("arch_picker_grid")
                .striped(true)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    let archs = Architecture::ALL;
                    for (i, &arch) in archs.iter().enumerate() {
                        let label = format!("{}: {}", arch.name(), arch.display_name());
                        if ui.button(label).clicked() {
                            selected_arch = Some(arch);
                        }
                        if (i + 1) % 2 == 0 {
                            ui.end_row();
                        }
                    }
                });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    cancelled = true;
                }
            });
        });

    if let Some(arch) = selected_arch {
        app.apply_pending_with_arch(arch);
        app.show_arch_picker_modal = false;
    }

    if cancelled {
        app.show_arch_picker_modal = false;
        app.pending_file_content = None;
        app.pending_dir_path = None;
        app.pending_candidates.clear();
    } else {
        app.show_arch_picker_modal = open;
    }
}
