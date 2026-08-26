//! Entry point for desktop GUI application on Windows and other platforms.

use emulator_core::arch::Architecture;
use emulator_gui::EmulatorApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([850.0, 550.0])
            .with_title("M15 Multi-Architecture CPU Emulator & Stack Viewer"),
        ..Default::default()
    };

    eframe::run_native(
        "M15 CPU Emulator",
        native_options,
        Box::new(|_cc| Ok(Box::new(EmulatorApp::new(Architecture::X86_64)))),
    )
}
