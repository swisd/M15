use egui::{Color32, RichText, Ui};
use emulator_core::arch::Architecture;

use crate::app::ExecutionState;
use crate::demos::DEMOS;

#[allow(clippy::too_many_arguments)]
pub fn render_control_panel(
    ui: &mut Ui,
    selected_arch: &mut Architecture,
    execution_state: &mut ExecutionState,
    step_count: u64,
    cycle_count: u64,
    speed_hz: &mut u32,
    show_terminal: &mut bool,
    show_serial_console: &mut bool,
    show_breadboard_view: &mut bool,
    on_step_1: &mut bool,
    on_step_10: &mut bool,
    on_reset: &mut bool,
    on_load_demo: &mut bool,
    on_open_load_dialog: &mut bool,
    on_open_file_browser: &mut bool,
) {
    ui.horizontal_wrapped(|ui| {
        // Load Project / File button
        if ui.button(RichText::new("📂 Load Project / File").color(Color32::from_rgb(255, 215, 0)).strong()).clicked() {
            *on_open_load_dialog = true;
        }

        if ui.button(RichText::new("📁 Browse...").color(Color32::from_rgb(100, 220, 255)).strong()).clicked() {
            *on_open_file_browser = true;
        }

        ui.separator();

        // Architecture Selector
        ui.label(RichText::new("Architecture:").strong());
        let current_arch_name = selected_arch.name();
        egui::ComboBox::from_id_salt("arch_selector_combo")
            .selected_text(current_arch_name)
            .show_ui(ui, |ui| {
                for &arch in Architecture::ALL {
                    if ui.selectable_value(selected_arch, arch, arch.name()).clicked() {
                        *on_load_demo = true;
                    }
                }
            });

        ui.separator();

        // Demo Selector
        egui::ComboBox::from_id_salt("demo_selector_combo")
            .selected_text("Load Demo Program...")
            .show_ui(ui, |ui| {
                for demo in DEMOS {
                    if ui.button(format!("{}: {}", demo.arch.name(), demo.name)).clicked() {
                        *selected_arch = demo.arch;
                        *on_load_demo = true;
                    }
                }
            });

        ui.separator();

        // Stepping / Running Controls
        match *execution_state {
            ExecutionState::Running => {
                if ui.button(RichText::new("⏸ Pause").color(Color32::from_rgb(255, 200, 80))).clicked() {
                    *execution_state = ExecutionState::Paused;
                }
            }
            _ => {
                if ui.button(RichText::new("▶ Run").color(Color32::from_rgb(80, 220, 100))).clicked() {
                    *execution_state = ExecutionState::Running;
                }
            }
        }

        if ui.button("⏭ Step 1").clicked() {
            *on_step_1 = true;
        }

        if ui.button("⏭ Step 10").clicked() {
            *on_step_10 = true;
        }

        if ui.button("🔄 Reset").clicked() {
            *on_reset = true;
        }

        ui.separator();

        // Speed Slider
        ui.label("Speed:");
        ui.add(egui::Slider::new(speed_hz, 1..=1000).text("Hz"));

        ui.separator();

        // Window Toggles for Terminal and Serial Console
        let term_btn_text = if *show_terminal {
            RichText::new("🖥 Terminal (80x25)").color(Color32::from_rgb(100, 220, 255))
        } else {
            RichText::new("🖥 Terminal (80x25)").weak()
        };
        if ui.button(term_btn_text).clicked() {
            *show_terminal = !*show_terminal;
        }

        let serial_btn_text = if *show_serial_console {
            RichText::new("📟 Serial Console").color(Color32::from_rgb(120, 230, 140))
        } else {
            RichText::new("📟 Serial Console").weak()
        };
        if ui.button(serial_btn_text).clicked() {
            *show_serial_console = !*show_serial_console;
        }

        let breadboard_btn_text = if *show_breadboard_view {
            RichText::new("🍞 Breadboard").color(Color32::from_rgb(120, 230, 140)) // I couldn't find an icon for breadboard
        } else {
            RichText::new("🍞 Breadboard").weak()
        };
        if ui.button(breadboard_btn_text).clicked() {
            *show_breadboard_view = !*show_breadboard_view;
        }

        ui.separator();

        // State indicator
        let (status_text, status_color) = match *execution_state {
            ExecutionState::Stopped => ("Stopped", Color32::GRAY),
            ExecutionState::Running => ("Running", Color32::from_rgb(80, 220, 100)),
            ExecutionState::Paused => ("Paused", Color32::from_rgb(255, 200, 80)),
            ExecutionState::Halted => ("Halted (HLT)", Color32::from_rgb(255, 100, 100)),
            ExecutionState::Error(_) => ("Error", Color32::RED),
        };

        ui.label(RichText::new(format!("Status: {}", status_text)).strong().color(status_color));

        ui.separator();

        ui.label(RichText::new(format!("Cycles: {} | Steps: {}", cycle_count, step_count)).weak());
    });
}
