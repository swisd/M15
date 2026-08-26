//! # emulator-core
//!
//! A `#![no_std]` multi-architecture CPU emulator core supporting 15 CPU architectures:
//! Intel 8086, x86 (IA-32), x86-64, ARM32, ARM64, RISC-V, IA-64, MIPS, PowerPC,
//! SPARC, AVR, SuperH, PA-RISC, DEC Alpha, and Motorola 68000.

#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub mod arch;
pub mod bus;
pub mod cpu;
#[cfg(feature = "alloc")]
pub mod project;
pub mod stack;
pub mod types;

pub use arch::{AnyCpu, Architecture, create_cpu};
pub use bus::{ArrayMemory, MemoryBus, MemoryError, SliceMemory};
#[cfg(feature = "alloc")]
pub use bus::DynamicMemory;
pub use cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
#[cfg(feature = "alloc")]
pub use project::{
    AsmProgram, DetectionResult, LoadedProject, ProjectConfig, ProjectError, assemble_source,
    detect_architecture,
};
pub use stack::{StackSlot, inspect_stack};
#[cfg(feature = "alloc")]
pub use stack::inspect_stack_vec;
pub use types::{Endianness, StackGrowth, WordSize};
