//! Architecture-aware Stack Viewer and Frame Inspector.
//!
//! Provides data models, stack analysis, frame pointer identification,
//! text formatting, and egui widget rendering for all 15 supported CPU architectures.

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "alloc")]
pub mod analyzer;
#[cfg(feature = "alloc")]
pub mod formatter;
#[cfg(feature = "alloc")]
pub mod model;

#[cfg(all(feature = "egui", feature = "alloc"))]
pub mod widget;

#[cfg(feature = "alloc")]
pub use analyzer::{analyze_stack, detect_frame_pointer, standard_stack_alignment};
#[cfg(feature = "alloc")]
pub use formatter::format_stack_table;
#[cfg(feature = "alloc")]
pub use model::{DisplayFormat, StackAnalysis, StackEntry, StackViewOptions};

#[cfg(all(feature = "egui", feature = "alloc"))]
pub use widget::StackViewerWidget;
