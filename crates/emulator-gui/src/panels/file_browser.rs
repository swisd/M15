//! Interactive File and Folder Browser modal dialog for selecting and loading
//! assembly source files (*.asm, *.s, *.inc) and project directories (mconfig.toml).

use std::fs;
use std::path::{Path, PathBuf};

use egui::{Color32, Context, RichText, ScrollArea, Window};

use crate::app::EmulatorApp;

/// Interactive File/Folder Browser modal state.
#[derive(Clone, Debug)]
pub struct FileBrowserModal {
    pub is_open: bool,
    pub current_dir: PathBuf,
    pub selected_path: Option<PathBuf>,
    pub search_filter: String,
    pub show_all_files: bool,
    pub status_message: Option<String>,
}

impl Default for FileBrowserModal {
    fn default() -> Self {
        let start_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self {
            is_open: false,
            current_dir: start_dir,
            selected_path: None,
            search_filter: String::new(),
            show_all_files: false,
            status_message: None,
        }
    }
}

impl FileBrowserModal {
    /// Creates a new FileBrowserModal rooted at current directory.
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens the file browser dialog.
    pub fn open(&mut self) {
        self.is_open = true;
        self.status_message = None;
    }

    /// Opens the file browser navigating to an initial path if valid.
    pub fn open_with_path(&mut self, path_str: &str) {
        let trimmed = path_str.trim();
        if !trimmed.is_empty() {
            let p = Path::new(trimmed);
            if p.is_dir() {
                self.current_dir = p.to_path_buf();
                self.selected_path = None;
            } else if p.is_file() {
                if let Some(parent) = p.parent() {
                    self.current_dir = parent.to_path_buf();
                }
                self.selected_path = Some(p.to_path_buf());
            }
        }
        self.open();
    }

    /// Closes the file browser dialog.
    pub fn close(&mut self) {
        self.is_open = false;
        self.status_message = None;
    }

    /// Navigates to target directory.
    pub fn navigate_to(&mut self, dir: PathBuf) {
        if dir.is_dir() {
            self.current_dir = dir;
            self.selected_path = None;
            self.status_message = None;
        }
    }

    /// Navigates to the parent directory.
    pub fn navigate_up(&mut self) {
        if let Some(parent) = self.current_dir.parent() {
            self.current_dir = parent.to_path_buf();
            self.selected_path = None;
            self.status_message = None;
        }
    }

    /// Checks whether a file extension matches supported assembly or project files.
    pub fn is_supported_file(&self, path: &Path) -> bool {
        if self.show_all_files {
            return true;
        }
        let fname = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if fname.eq_ignore_ascii_case("mconfig.toml") {
            return true;
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_ascii_lowercase();
            matches!(
                ext_lower.as_str(),
                "asm" | "s" | "inc" | "toml" | "txt" | "hex" | "bin"
            )
        } else {
            false
        }
    }
}

/// Renders the File & Folder Browser window.
pub fn render_file_browser(ctx: &Context, app: &mut EmulatorApp) {
    if !app.file_browser.is_open {
        return;
    }

    let mut open = app.file_browser.is_open;
    let mut to_load_file: Option<PathBuf> = None;
    let mut to_load_dir: Option<PathBuf> = None;
    let mut new_dir: Option<PathBuf> = None;

    Window::new("📁 File & Folder Browser")
        .open(&mut open)
        .resizable(true)
        .default_width(700.0)
        .default_height(520.0)
        .show(ctx, |ui| {
            ui.heading("Browse File System");
            ui.label("Navigate directories to select an assembly file (*.asm, *.s, *.inc) or open an entire project folder containing `mconfig.toml`.");
            ui.separator();

            // Quick Navigation Shortcuts Bar
            ui.horizontal_wrapped(|ui| {
                if ui.button("⬆ Parent Directory (..)").clicked() {
                    app.file_browser.navigate_up();
                }

                if ui.button("🏠 Current Directory").clicked() {
                    if let Ok(cur) = std::env::current_dir() {
                        app.file_browser.navigate_to(cur);
                    }
                }

                let test_proj = PathBuf::from("test-projects");
                if test_proj.is_dir() && ui.button("📂 test-projects").clicked() {
                    app.file_browser.navigate_to(test_proj);
                }

                // Windows drive roots
                #[cfg(windows)]
                {
                    for drive_letter in b'A'..=b'Z' {
                        let drive_path = format!("{}:\\", drive_letter as char);
                        if Path::new(&drive_path).exists() {
                            if ui.button(format!("💾 {}", drive_path)).clicked() {
                                app.file_browser.navigate_to(PathBuf::from(drive_path));
                            }
                        }
                    }
                }
            });

            ui.add_space(4.0);

            // Current Directory Path Bar
            ui.horizontal(|ui| {
                ui.label(RichText::new("Location:").strong());
                let mut dir_str = app.file_browser.current_dir.to_string_lossy().to_string();
                if ui.text_edit_singleline(&mut dir_str).lost_focus() {
                    let p = PathBuf::from(dir_str.trim());
                    if p.is_dir() {
                        app.file_browser.navigate_to(p);
                    }
                }
                if ui.button("Go").clicked() {
                    let p = PathBuf::from(dir_str.trim());
                    if p.is_dir() {
                        app.file_browser.navigate_to(p);
                    }
                }
            });

            ui.add_space(4.0);

            // Search Filter & File Type Options Bar
            ui.horizontal(|ui| {
                ui.label("🔍 Filter:");
                ui.text_edit_singleline(&mut app.file_browser.search_filter);
                if !app.file_browser.search_filter.is_empty() && ui.button("✖").clicked() {
                    app.file_browser.search_filter.clear();
                }

                ui.separator();
                ui.checkbox(&mut app.file_browser.show_all_files, "Show All Files (*.*)");
            });

            ui.separator();

            // Directory Contents Table with independent scroll
            ScrollArea::vertical()
                .id_salt("file_browser_entries_scroll")
                .max_height(280.0)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    let mut subdirs = Vec::new();
                    let mut files = Vec::new();

                    if let Ok(entries) = fs::read_dir(&app.file_browser.current_dir) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            let name = p
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("")
                                .to_string();

                            // Search filter
                            if !app.file_browser.search_filter.is_empty()
                                && !name.to_ascii_lowercase().contains(&app.file_browser.search_filter.to_ascii_lowercase())
                            {
                                continue;
                            }

                            if p.is_dir() {
                                subdirs.push((name, p));
                            } else if p.is_file() && app.file_browser.is_supported_file(&p) {
                                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                                files.push((name, p, size));
                            }
                        }
                    }

                    subdirs.sort_by(|a, b| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()));
                    files.sort_by(|a, b| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()));

                    egui::Grid::new("file_browser_grid")
                        .striped(true)
                        .min_col_width(80.0)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            ui.label(RichText::new("Name").strong());
                            ui.label(RichText::new("Type").strong());
                            ui.label(RichText::new("Size").strong());
                            ui.label(RichText::new("Action").strong());
                            ui.end_row();

                            // Render subdirectories
                            for (name, path) in subdirs {
                                let is_selected = app.file_browser.selected_path.as_ref() == Some(&path);
                                let label = RichText::new(format!("📁 {}", name))
                                    .strong()
                                    .color(if is_selected {
                                        Color32::from_rgb(100, 220, 255)
                                    } else {
                                        Color32::from_rgb(255, 215, 0)
                                    });

                                if ui.selectable_label(is_selected, label).clicked() {
                                    app.file_browser.selected_path = Some(path.clone());
                                }

                                ui.label(RichText::new("Directory").weak());
                                ui.label("-");

                                ui.horizontal(|ui| {
                                    if ui.button("Enter ➡").clicked() {
                                        new_dir = Some(path.clone());
                                    }
                                    if ui.button("Select Folder").clicked() {
                                        to_load_dir = Some(path);
                                    }
                                });

                                ui.end_row();
                            }

                            // Render files
                            for (name, path, size) in files {
                                let is_selected = app.file_browser.selected_path.as_ref() == Some(&path);
                                let icon = if name.ends_with(".toml") {
                                    "⚙"
                                } else if name.ends_with(".asm") || name.ends_with(".s") || name.ends_with(".inc") {
                                    "📝"
                                } else {
                                    "📄"
                                };

                                let label = RichText::new(format!("{} {}", icon, name))
                                    .color(if is_selected {
                                        Color32::from_rgb(80, 220, 100)
                                    } else {
                                        ui.visuals().text_color()
                                    });

                                if ui.selectable_label(is_selected, label).clicked() {
                                    app.file_browser.selected_path = Some(path.clone());
                                }

                                let ext = path
                                    .extension()
                                    .and_then(|e| e.to_str())
                                    .unwrap_or("file")
                                    .to_uppercase();
                                ui.label(RichText::new(ext).weak());

                                let size_str = if size < 1024 {
                                    format!("{} B", size)
                                } else if size < 1024 * 1024 {
                                    format!("{:.1} KB", size as f64 / 1024.0)
                                } else {
                                    format!("{:.1} MB", size as f64 / (1024.0 * 1024.0))
                                };
                                ui.label(RichText::new(size_str).monospace().weak());

                                if ui.button(RichText::new("Open File").color(Color32::from_rgb(80, 220, 100))).clicked() {
                                    to_load_file = Some(path);
                                }

                                ui.end_row();
                            }
                        });
                });

            ui.separator();

            // Bottom Selection and Confirmation Controls Bar
            ui.horizontal(|ui| {
                if let Some(ref sel) = app.file_browser.selected_path {
                    ui.label(RichText::new(format!("Selected: {}", sel.file_name().unwrap_or_default().to_string_lossy())).strong());
                } else {
                    ui.label(RichText::new(format!("Current Directory: {}", app.file_browser.current_dir.file_name().unwrap_or_default().to_string_lossy())).weak());
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Cancel").clicked() {
                        app.file_browser.close();
                    }

                    if ui.button(RichText::new("📂 Open Current Folder as Project").strong()).clicked() {
                        to_load_dir = Some(app.file_browser.current_dir.clone());
                    }

                    if let Some(ref sel) = app.file_browser.selected_path.clone() {
                        if sel.is_file() {
                            if ui.button(RichText::new("📄 Load Selected File").color(Color32::from_rgb(80, 220, 100)).strong()).clicked() {
                                to_load_file = Some(sel.clone());
                            }
                        } else if sel.is_dir() {
                            if ui.button(RichText::new("📂 Open Selected Folder").color(Color32::from_rgb(255, 215, 0)).strong()).clicked() {
                                to_load_dir = Some(sel.clone());
                            }
                        }
                    }
                });
            });

            if let Some(ref msg) = app.file_browser.status_message {
                ui.add_space(4.0);
                ui.label(RichText::new(msg).color(Color32::from_rgb(255, 100, 100)));
            }
        });

    if let Some(dir) = new_dir {
        app.file_browser.navigate_to(dir);
    }

    if let Some(file_path) = to_load_file {
        let path_str = file_path.to_string_lossy().to_string();
        app.load_asm_file(&path_str, None);
        app.file_browser.close();
    }

    if let Some(dir_path) = to_load_dir {
        let path_str = dir_path.to_string_lossy().to_string();
        app.load_project_from_dir(&path_str);
        app.file_browser.close();
    }

    app.file_browser.is_open = open;
}
