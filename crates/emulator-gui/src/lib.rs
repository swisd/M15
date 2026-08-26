//! Desktop GUI application and debugging environment for M15 Multi-Architecture CPU Emulator.

pub mod app;
pub mod demos;
pub mod panels;
pub mod serial;
pub mod terminal;

pub use app::{EmulatorApp, ExecutionState};
pub use demos::{load_demo_for_arch, ArchDemo, DEMOS};
pub use serial::{SerialConsole, SerialLineEnding, SerialParity};
pub use terminal::{
    TerminalCell, TerminalScreen, DEFAULT_VRAM_BASE_ADDR, TERMINAL_COLS, TERMINAL_ROWS,
};
