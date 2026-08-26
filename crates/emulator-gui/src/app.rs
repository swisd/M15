//! Main emulator application state and GUI layout.

use eframe::App;
use egui::{Color32, RichText};
use emulator_core::arch::AnyCpu;
use emulator_core::arch::Architecture;
use emulator_core::bus::DynamicMemory;
use emulator_core::cpu::{CpuEngine, StepOutcome};
use emulator_stack_viewer::{analyze_stack, StackViewOptions, StackViewerWidget};

use crate::demos::load_demo_for_arch;
use crate::panels::code_panel::render_code_panel;
use crate::panels::control_panel::render_control_panel;
use crate::panels::memory_panel::render_memory_panel;
use crate::panels::project_dialog::{render_arch_picker_modal, render_load_dialog};
use crate::panels::registers_panel::render_registers_panel;
use crate::serial::SerialConsole;
use crate::terminal::TerminalScreen;

/// Current execution lifecycle state of the CPU.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionState {
    Stopped,
    Running,
    Paused,
    Halted,
    Error(String),
}

/// The desktop GUI application for the multi-architecture CPU emulator.
pub struct EmulatorApp {
    pub cpu: AnyCpu,
    pub bus: DynamicMemory,
    pub selected_arch: Architecture,
    pub execution_state: ExecutionState,
    pub cycle_count: u64,
    pub step_count: u64,
    pub speed_hz: u32,

    // Stack Viewer state
    pub stack_widget: StackViewerWidget,
    pub stack_options: StackViewOptions,

    // Memory Viewer state
    pub memory_view_addr: u64,
    pub memory_addr_input: String,

    // Register editing state
    pub editing_reg: Option<(&'static str, String)>,

    // Breakpoints
    pub breakpoints: Vec<u64>,

    // Layout Split Ratios
    pub horizontal_split: f32,
    pub left_vertical_split: f32,
    pub right_vertical_split: f32,

    // Independent Window Subsystems
    pub terminal: TerminalScreen,
    pub show_terminal: bool,
    pub serial_console: SerialConsole,
    pub show_serial_console: bool,

    // App status & dialogs
    pub status_message: Option<String>,
    pub show_about: bool,

    // Project / File Loader state
    pub show_load_dialog: bool,
    pub show_arch_picker_modal: bool,
    pub load_path_input: String,
    pub asm_editor_input: String,
    pub editor_arch_selection: Option<Architecture>,
    pub pending_file_content: Option<(String, Option<String>, String)>,
    pub pending_dir_path: Option<String>,
    pub pending_candidates: Vec<Architecture>,
    pub loaded_source_code: Option<String>,
    pub current_project_name: Option<String>,
    pub current_project_path: Option<String>,
}

impl Default for EmulatorApp {
    fn default() -> Self {
        Self::new(Architecture::X86_64)
    }
}

impl EmulatorApp {
    /// Creates a new emulator GUI application initialized to a specific architecture.
    pub fn new(arch: Architecture) -> Self {
        let mut cpu = AnyCpu::new(arch);
        let mut bus = DynamicMemory::new(1024 * 1024); // 1 MB addressable RAM
        load_demo_for_arch(arch, &mut cpu, &mut bus);

        let sp = cpu.sp();

        let terminal = TerminalScreen::new();
        terminal.sync_to_vram(&mut bus);

        let mut serial_console = SerialConsole::new();
        serial_console.sync_with_bus(&mut bus);

        Self {
            cpu,
            bus,
            selected_arch: arch,
            execution_state: ExecutionState::Stopped,
            cycle_count: 0,
            step_count: 0,
            speed_hz: 10,
            stack_widget: StackViewerWidget::new(),
            stack_options: StackViewOptions {
                slot_count: 24,
                ..Default::default()
            },
            memory_view_addr: sp & !0x0F,
            memory_addr_input: format!("{:#X}", sp & !0x0F),
            editing_reg: None,
            breakpoints: Vec::new(),
            horizontal_split: 0.48,
            left_vertical_split: 0.50,
            right_vertical_split: 0.50,
            terminal,
            show_terminal: false,
            serial_console,
            show_serial_console: false,
            status_message: Some(format!("Loaded {} demo.", arch.name())),
            show_about: false,
            show_load_dialog: false,
            show_arch_picker_modal: false,
            load_path_input: String::new(),
            asm_editor_input: r#"; arch: 8086
.org 0x1000
mov ax, 0x0042
mov bx, 0x0010
add ax, bx
push ax
pop cx
hlt
"#
            .to_string(),
            editor_arch_selection: None,
            pending_file_content: None,
            pending_dir_path: None,
            pending_candidates: Vec::new(),
            loaded_source_code: None,
            current_project_name: None,
            current_project_path: None,
        }
    }

    /// Switches the active CPU architecture and resets system state.
    pub fn switch_arch(&mut self, arch: Architecture) {
        self.selected_arch = arch;
        self.cpu = AnyCpu::new(arch);
        load_demo_for_arch(arch, &mut self.cpu, &mut self.bus);
        self.terminal.sync_to_vram(&mut self.bus);
        self.serial_console.sync_with_bus(&mut self.bus);
        self.execution_state = ExecutionState::Stopped;
        self.cycle_count = 0;
        self.step_count = 0;
        self.memory_view_addr = self.cpu.sp() & !0x0F;
        self.memory_addr_input = format!("{:#X}", self.memory_view_addr);
        self.status_message = Some(format!("Switched to {} architecture.", arch.name()));
    }

    /// Resets the CPU state and reloads demo memory.
    pub fn reset_cpu(&mut self) {
        load_demo_for_arch(self.selected_arch, &mut self.cpu, &mut self.bus);
        self.terminal.sync_to_vram(&mut self.bus);
        self.serial_console.sync_with_bus(&mut self.bus);
        self.execution_state = ExecutionState::Stopped;
        self.cycle_count = 0;
        self.step_count = 0;
        self.status_message = Some("System reset.".to_string());
    }

    /// Executes a single CPU instruction step.
    pub fn step(&mut self) {
        if self.execution_state == ExecutionState::Halted {
            return;
        }

        match self.cpu.step(&mut self.bus) {
            Ok(outcome) => {
                self.step_count += 1;
                match outcome {
                    StepOutcome::Continue { cycles } => {
                        self.cycle_count += cycles as u64;
                    }
                    StepOutcome::Halted => {
                        self.execution_state = ExecutionState::Halted;
                        self.status_message = Some("CPU Halted.".to_string());
                    }
                    StepOutcome::Breakpoint => {
                        self.execution_state = ExecutionState::Paused;
                        self.status_message = Some("Breakpoint reached.".to_string());
                    }
                    StepOutcome::Interrupt(vec) => {
                        self.status_message = Some(format!("Interrupt raised: vector {:#X}", vec));
                    }
                }
            }
            Err(err) => {
                self.execution_state = ExecutionState::Error(format!("{:?}", err));
                self.status_message = Some(format!("CPU Execution Error: {:?}", err));
            }
        }

        // Synchronize UART MMIO
        self.serial_console.sync_with_bus(&mut self.bus);

        // Check if new PC matches a user breakpoint
        if self.breakpoints.contains(&self.cpu.pc()) {
            self.execution_state = ExecutionState::Paused;
            self.status_message = Some(format!("Hit breakpoint at {:#010X}", self.cpu.pc()));
        }
    }

    /// Loads an assembly project from a directory path (reading `mconfig.toml` if present).
    pub fn load_project_from_dir(&mut self, dir_path: &str) {
        let p = std::path::Path::new(dir_path);
        match emulator_core::project::LoadedProject::load_from_dir(p) {
            Ok(loaded) => {
                self.selected_arch = loaded.arch;
                self.cpu = AnyCpu::new(loaded.arch);
                self.bus = DynamicMemory::new(loaded.config.memory_size.unwrap_or(1024 * 1024));
                loaded.program.load_into(&mut self.cpu, &mut self.bus);
                self.terminal.sync_to_vram(&mut self.bus);
                self.serial_console.sync_with_bus(&mut self.bus);
                self.execution_state = ExecutionState::Stopped;
                self.cycle_count = 0;
                self.step_count = 0;
                self.memory_view_addr = self.cpu.sp() & !0x0F;
                self.memory_addr_input = format!("{:#X}", self.memory_view_addr);
                self.loaded_source_code = Some(loaded.source_code);
                self.current_project_name = Some(loaded.config.name.clone());
                self.current_project_path = Some(dir_path.to_string());
                self.status_message = Some(format!(
                    "Loaded project '{}' for {} (Entry: {:#06X})",
                    loaded.config.name,
                    loaded.arch.name(),
                    self.cpu.pc()
                ));
                self.show_load_dialog = false;
            }
            Err(emulator_core::project::ProjectError::AmbiguousArch(candidates)) => {
                self.pending_dir_path = Some(dir_path.to_string());
                self.pending_candidates = candidates;
                self.show_arch_picker_modal = true;
                self.status_message = Some(format!(
                    "Multiple architectures matched for directory '{}'. Please select target architecture.",
                    dir_path
                ));
            }
            Err(emulator_core::project::ProjectError::ArchDetectionFailed(_)) => {
                self.pending_dir_path = Some(dir_path.to_string());
                self.pending_candidates = Vec::new();
                self.show_arch_picker_modal = true;
                self.status_message = Some(format!(
                    "Architecture detection failed for directory '{}'. Please select target architecture.",
                    dir_path
                ));
            }
            Err(err) => {
                self.status_message = Some(format!("Failed to load project: {}", err));
            }
        }
    }

    /// Loads an individual assembly file from disk.
    pub fn load_asm_file(&mut self, file_path: &str, manual_arch_override: Option<Architecture>) {
        let p = std::path::Path::new(file_path);
        let fname = p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "File".to_string());
        match std::fs::read_to_string(p) {
            Ok(content) => {
                self.load_asm_source(&content, &fname, Some(file_path.to_string()), manual_arch_override);
            }
            Err(e) => {
                self.status_message = Some(format!("Failed to read file '{}': {}", file_path, e));
            }
        }
    }

    /// Assembles and loads assembly source text into the emulator.
    pub fn load_asm_source(
        &mut self,
        source_code: &str,
        name: &str,
        file_path: Option<String>,
        manual_arch_override: Option<Architecture>,
    ) {
        let arch = if let Some(a) = manual_arch_override {
            a
        } else {
            let fname_ref = file_path.as_deref().or(Some(name));
            match emulator_core::project::detect_architecture(fname_ref, source_code) {
                emulator_core::project::DetectionResult::Detected(a) => a,
                emulator_core::project::DetectionResult::Ambiguous(candidates) => {
                    self.pending_file_content =
                        Some((source_code.to_string(), file_path, name.to_string()));
                    self.pending_candidates = candidates;
                    self.show_arch_picker_modal = true;
                    self.status_message = Some(format!(
                        "Ambiguous architecture for '{}'. Please select target CPU architecture.",
                        name
                    ));
                    return;
                }
                emulator_core::project::DetectionResult::Unknown => {
                    self.pending_file_content =
                        Some((source_code.to_string(), file_path, name.to_string()));
                    self.pending_candidates = Vec::new();
                    self.show_arch_picker_modal = true;
                    self.status_message = Some(format!(
                        "Could not detect architecture for '{}'. Please select target CPU architecture.",
                        name
                    ));
                    return;
                }
            }
        };

        match emulator_core::project::assemble_source(arch, source_code, None, None) {
            Ok(program) => {
                self.selected_arch = arch;
                self.cpu = AnyCpu::new(arch);
                self.bus = DynamicMemory::new(1024 * 1024);
                program.load_into(&mut self.cpu, &mut self.bus);
                self.terminal.sync_to_vram(&mut self.bus);
                self.serial_console.sync_with_bus(&mut self.bus);
                self.execution_state = ExecutionState::Stopped;
                self.cycle_count = 0;
                self.step_count = 0;
                self.memory_view_addr = self.cpu.sp() & !0x0F;
                self.memory_addr_input = format!("{:#X}", self.memory_view_addr);
                self.loaded_source_code = Some(source_code.to_string());
                self.current_project_name = Some(name.to_string());
                self.current_project_path = file_path;
                self.status_message = Some(format!(
                    "Loaded '{}' for {} (Entry: {:#06X}, SP: {:#06X})",
                    name,
                    arch.name(),
                    self.cpu.pc(),
                    self.cpu.sp()
                ));
                self.show_load_dialog = false;
                self.show_arch_picker_modal = false;
            }
            Err(e) => {
                self.status_message = Some(format!("Assemble error: {}", e));
            }
        }
    }

    /// Applies pending file or directory load with user-selected architecture.
    pub fn apply_pending_with_arch(&mut self, arch: Architecture) {
        if let Some((src, file_path, name)) = self.pending_file_content.take() {
            self.load_asm_source(&src, &name, file_path, Some(arch));
        } else if let Some(dir) = self.pending_dir_path.take() {
            let p = std::path::Path::new(&dir);
            if let Ok(mut config) = emulator_core::project::ProjectConfig::load_from_directory(p) {
                config.arch = Some(arch);
                let main_file = config.main.clone().unwrap_or_else(|| "main.asm".to_string());
                let main_path = p.join(&main_file);
                if let Ok(source) = std::fs::read_to_string(&main_path) {
                    self.load_asm_source(
                        &source,
                        &config.name,
                        Some(main_path.to_string_lossy().to_string()),
                        Some(arch),
                    );
                    return;
                }
            }
            if let Ok(entries) = std::fs::read_dir(p) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Ok(source) = std::fs::read_to_string(&path) {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Source".to_string());
                        self.load_asm_source(
                            &source,
                            &name,
                            Some(path.to_string_lossy().to_string()),
                            Some(arch),
                        );
                        return;
                    }
                }
            }
        }
    }
}

impl App for EmulatorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut on_step_1 = false;
        let mut on_step_10 = false;
        let mut on_reset = false;
        let mut on_load_demo = false;
        let mut on_open_load_dialog = false;

        // Execute running instructions if in Running state
        if self.execution_state == ExecutionState::Running {
            self.step();
            ctx.request_repaint();
        }

        // Top Menu Bar
        egui::TopBottomPanel::top("top_menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("📂 Open Project / File...").clicked() {
                        on_open_load_dialog = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Reset CPU").clicked() {
                        on_reset = true;
                        ui.close_menu();
                    }
                });

                ui.menu_button("View", |ui| {
                    ui.checkbox(&mut self.show_terminal, "🖥 80x25 Text Terminal");
                    ui.checkbox(&mut self.show_serial_console, "📟 Serial Console (UART)");
                    ui.separator();
                    ui.label(RichText::new("Layout Presets:").strong());
                    if ui.button("Balanced (50 / 50)").clicked() {
                        self.horizontal_split = 0.48;
                        self.left_vertical_split = 0.50;
                        self.right_vertical_split = 0.50;
                        ui.close_menu();
                    }
                    if ui.button("Wide Stack & Memory (40 / 60)").clicked() {
                        self.horizontal_split = 0.40;
                        self.left_vertical_split = 0.50;
                        self.right_vertical_split = 0.50;
                        ui.close_menu();
                    }
                    if ui.button("Wide Code & Registers (60 / 40)").clicked() {
                        self.horizontal_split = 0.60;
                        self.left_vertical_split = 0.50;
                        self.right_vertical_split = 0.50;
                        ui.close_menu();
                    }
                });

                ui.menu_button("Architectures", |ui| {
                    for &arch in Architecture::ALL {
                        if ui.button(arch.name()).clicked() {
                            self.switch_arch(arch);
                            ui.close_menu();
                        }
                    }
                });

                ui.menu_button("Help", |ui| {
                    if ui.button("About M15 Multi-Arch Emulator").clicked() {
                        self.show_about = true;
                        ui.close_menu();
                    }
                });
            });
        });

        // Top Control Panel (Toolbar)
        egui::TopBottomPanel::top("control_toolbar").show(ctx, |ui| {
            render_control_panel(
                ui,
                &mut self.selected_arch,
                &mut self.execution_state,
                self.step_count,
                self.cycle_count,
                &mut self.speed_hz,
                &mut self.show_terminal,
                &mut self.show_serial_console,
                &mut on_step_1,
                &mut on_step_10,
                &mut on_reset,
                &mut on_load_demo,
                &mut on_open_load_dialog,
            );
        });

        // Bottom Status Bar
        egui::TopBottomPanel::bottom("bottom_status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Arch: {}", self.selected_arch.name())).strong());
                ui.separator();
                ui.label(format!("Endianness: {:?}", self.cpu.endianness()));
                ui.separator();
                ui.label(format!("Stack Growth: {:?}", self.cpu.stack_growth()));
                ui.separator();
                ui.label(format!("PC: {:#010X} | SP: {:#010X}", self.cpu.pc(), self.cpu.sp()));

                if let Some(ref msg) = self.status_message {
                    ui.separator();
                    ui.label(RichText::new(msg).color(Color32::from_rgb(100, 200, 255)));
                }
            });
        });

        // Central Panel with Multi-Pane Debugger Layout
        egui::CentralPanel::default().show(ctx, |ui| {
            let total_width = ui.available_width();
            let total_height = ui.available_height();
            let spacing = ui.spacing().item_spacing;

            let min_col_w = 260.0;
            let left_width = (total_width * self.horizontal_split)
                .clamp(min_col_w, (total_width - min_col_w).max(min_col_w));
            let right_width = (total_width - left_width - spacing.x - 4.0).max(min_col_w);

            ui.horizontal(|ui| {
                // Left Column: Registers (top) + Disassembly (bottom)
                ui.allocate_ui_with_layout(
                    egui::vec2(left_width, total_height),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        let top_h = (total_height * self.left_vertical_split - 4.0)
                            .clamp(120.0, (total_height - 120.0).max(120.0));
                        let bot_h = (total_height - top_h - 12.0).max(120.0);

                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_size(egui::vec2(left_width - 8.0, top_h));
                            ui.set_max_size(egui::vec2(left_width - 8.0, top_h));
                            render_registers_panel(ui, &mut self.cpu, &mut self.editing_reg);
                        });

                        ui.add_space(4.0);

                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_size(egui::vec2(left_width - 8.0, bot_h));
                            ui.set_max_size(egui::vec2(left_width - 8.0, bot_h));
                            render_code_panel(ui, &self.cpu, &self.bus, &mut self.breakpoints);
                        });
                    },
                );

                ui.separator();

                // Right Column: Stack Viewer (top) + Memory Hex View (bottom)
                ui.allocate_ui_with_layout(
                    egui::vec2(right_width, total_height),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        let top_h = (total_height * self.right_vertical_split - 4.0)
                            .clamp(140.0, (total_height - 140.0).max(140.0));
                        let bot_h = (total_height - top_h - 12.0).max(120.0);

                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_size(egui::vec2(right_width - 8.0, top_h));
                            ui.set_max_size(egui::vec2(right_width - 8.0, top_h));

                            ui.heading("Interactive Stack Viewer");
                            ui.separator();

                            let analysis = analyze_stack(&self.cpu, &self.bus, &self.stack_options);
                            self.stack_widget.show(ui, &analysis, &mut self.stack_options);
                        });

                        ui.add_space(4.0);

                        egui::Frame::group(ui.style()).show(ui, |ui| {
                            ui.set_min_size(egui::vec2(right_width - 8.0, bot_h));
                            ui.set_max_size(egui::vec2(right_width - 8.0, bot_h));
                            render_memory_panel(
                                ui,
                                &mut self.bus,
                                &self.cpu,
                                &mut self.memory_view_addr,
                                &mut self.memory_addr_input,
                            );
                        });
                    },
                );
            });
        });

        // Handle trigger events
        if on_step_1 {
            self.step();
        }
        if on_step_10 {
            for _ in 0..10 {
                self.step();
                if self.execution_state == ExecutionState::Halted
                    || matches!(self.execution_state, ExecutionState::Paused)
                {
                    break;
                }
            }
        }
        if on_reset {
            self.reset_cpu();
        }
        if on_load_demo {
            self.switch_arch(self.selected_arch);
        }
        if on_open_load_dialog {
            self.show_load_dialog = true;
        }

        // Project / File Loader Modals
        render_load_dialog(ctx, self);
        render_arch_picker_modal(ctx, self);

        // Independent Peripheral Windows (80x25 Terminal & Serial Console)
        self.terminal.render_window(ctx, &mut self.show_terminal, &mut self.bus);
        self.serial_console.render_window(ctx, &mut self.show_serial_console);

        // About Window Modal
        if self.show_about {
            egui::Window::new("About M15 Multi-Arch Emulator")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.heading("M15 Multi-Architecture CPU Emulator");
                    ui.label("A pure no_std CPU core with a rich desktop GUI & Stack Viewer.");
                    ui.add_space(4.0);
                    ui.label(RichText::new("Supported Architectures (16 total):").strong());
                    ui.label("8086, IA-32 (x86), AMD64 (x86_64), ARM32, ARM64, RISC-V, IA-64 (Itanium), MIPS, PowerPC, SPARC, AVR, SuperH, PA-RISC, DEC Alpha, Motorola 68000, MOS 6502");
                    ui.add_space(8.0);
                    if ui.button("Close").clicked() {
                        self.show_about = false;
                    }
                });
        }
    }
}
