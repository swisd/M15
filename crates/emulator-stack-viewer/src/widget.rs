//! egui UI widget for interactive stack frame visualization.

#[cfg(feature = "egui")]
use egui::{Color32, RichText, ScrollArea, Ui};

use crate::model::{DisplayFormat, StackAnalysis, StackViewOptions};

/// An interactive egui widget that visualizes CPU stack memory and frames.
#[derive(Default, Clone, Debug)]
pub struct StackViewerWidget {
    /// Filter string for searching specific values in the stack.
    pub filter: alloc::string::String,
}

impl StackViewerWidget {
    /// Creates a new default stack viewer widget.
    pub fn new() -> Self {
        Self::default()
    }

    /// Renders the stack viewer inside the provided `egui::Ui`.
    #[cfg(feature = "egui")]
    pub fn show(&mut self, ui: &mut Ui, analysis: &StackAnalysis, options: &mut StackViewOptions) {
        ui.vertical(|ui| {
            // Top Toolbar / Controls
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Display Format:").strong());
                egui::ComboBox::from_id_salt("stack_display_format")
                    .selected_text(match options.display_format {
                        DisplayFormat::Hex => "Hexadecimal",
                        DisplayFormat::Decimal => "Decimal",
                        DisplayFormat::SignedDecimal => "Signed Decimal",
                        DisplayFormat::Binary => "Binary",
                        DisplayFormat::Ascii => "ASCII",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut options.display_format,
                            DisplayFormat::Hex,
                            "Hexadecimal",
                        );
                        ui.selectable_value(
                            &mut options.display_format,
                            DisplayFormat::Decimal,
                            "Decimal",
                        );
                        ui.selectable_value(
                            &mut options.display_format,
                            DisplayFormat::SignedDecimal,
                            "Signed Decimal",
                        );
                        ui.selectable_value(
                            &mut options.display_format,
                            DisplayFormat::Binary,
                            "Binary",
                        );
                        ui.selectable_value(
                            &mut options.display_format,
                            DisplayFormat::Ascii,
                            "ASCII",
                        );
                    });

                ui.separator();

                ui.label("Slots:");
                ui.add(egui::Slider::new(&mut options.slot_count, 4..=128).text("count"));

                ui.separator();

                ui.checkbox(&mut options.show_raw_bytes, "Raw Bytes");
                ui.checkbox(&mut options.show_ascii, "ASCII");

                if options.custom_base_addr.is_some() && ui.button("Reset to SP").clicked() {
                    options.custom_base_addr = None;
                }
            });

            ui.add_space(4.0);

            // Architecture Metadata & Growth Indicator
            ui.horizontal(|ui| {
                let growth_text = match analysis.growth {
                    emulator_core::types::StackGrowth::Downwards => "Growth: ⬇ Down (High to Low)",
                    emulator_core::types::StackGrowth::Upwards => "Growth: ⬆ Up (Low to High)",
                };
                ui.label(RichText::new(growth_text).color(Color32::from_rgb(100, 200, 255)).strong());

                ui.separator();

                let align_color = if analysis.is_aligned {
                    Color32::from_rgb(120, 220, 120)
                } else {
                    Color32::from_rgb(255, 180, 80)
                };
                let align_text = alloc::format!(
                    "Align: {}-byte ({})",
                    analysis.expected_alignment,
                    if analysis.is_aligned { "OK" } else { "Unaligned" }
                );
                ui.label(RichText::new(align_text).color(align_color));

                ui.separator();

                ui.label(RichText::new(alloc::format!("Word: {}-bit", analysis.word_size * 8)).weak());

                if let Some(fp) = analysis.fp {
                    ui.separator();
                    ui.label(RichText::new(alloc::format!("FP: {:#010X}", fp)).color(Color32::from_rgb(255, 215, 0)));
                }
            });

            ui.separator();

            // Stack Slots Table / List
            ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    egui::Grid::new("stack_viewer_grid")
                        .striped(true)
                        .min_col_width(60.0)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            // Headers
                            ui.label(RichText::new("Offset").strong());
                            ui.label(RichText::new("Address").strong());
                            ui.label(RichText::new("Value").strong());
                            if options.show_raw_bytes {
                                ui.label(RichText::new("Raw Bytes").strong());
                            }
                            if options.show_ascii {
                                ui.label(RichText::new("ASCII").strong());
                            }
                            ui.label(RichText::new("Tag / Annotations").strong());
                            ui.end_row();

                            // Rows
                            for entry in &analysis.entries {
                                let is_top = entry.is_sp;
                                let is_fp = entry.is_fp;

                                let offset_text = if entry.offset_from_sp >= 0 {
                                    alloc::format!("SP+0x{:02X}", entry.offset_from_sp)
                                } else {
                                    alloc::format!("SP-0x{:02X}", -entry.offset_from_sp)
                                };

                                let addr_text = match analysis.word_size {
                                    1 | 2 => alloc::format!("{:#06X}", entry.address),
                                    4 => alloc::format!("{:#010X}", entry.address),
                                    8 => alloc::format!("{:#018X}", entry.address),
                                    _ => alloc::format!("{:#X}", entry.address),
                                };

                                let row_color = if is_top {
                                    Color32::from_rgb(80, 220, 120) // Green highlight for Top of Stack
                                } else if is_fp {
                                    Color32::from_rgb(255, 215, 0) // Gold highlight for Frame Pointer
                                } else {
                                    ui.visuals().text_color()
                                };

                                // Offset column
                                ui.label(
                                    RichText::new(offset_text)
                                        .monospace()
                                        .color(if is_top { Color32::from_rgb(80, 220, 120) } else { ui.visuals().weak_text_color() }),
                                );

                                // Address column
                                ui.label(
                                    RichText::new(addr_text)
                                        .monospace()
                                        .color(row_color),
                                );

                                // Value column
                                let val_text = entry.format_value(options.display_format);
                                ui.label(
                                    RichText::new(val_text)
                                        .monospace()
                                        .strong()
                                        .color(row_color),
                                );

                                // Raw Bytes column
                                if options.show_raw_bytes {
                                    ui.label(
                                        RichText::new(entry.format_raw_bytes())
                                            .monospace()
                                            .color(ui.visuals().weak_text_color()),
                                    );
                                }

                                // ASCII column
                                if options.show_ascii {
                                    ui.label(
                                        RichText::new(entry.ascii_representation())
                                            .monospace()
                                            .color(Color32::from_rgb(200, 200, 140)),
                                    );
                                }

                                // Annotations column
                                if let Some(ref annot) = entry.annotation {
                                    let badge_color = if is_top {
                                        Color32::from_rgb(80, 220, 120)
                                    } else {
                                        Color32::from_rgb(255, 215, 0)
                                    };
                                    ui.label(RichText::new(annot).strong().color(badge_color));
                                } else {
                                    ui.label("");
                                }

                                ui.end_row();
                            }
                        });
                });
        });
    }
}
