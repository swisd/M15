//! CPU Engine abstractions and execution outcomes.

use crate::arch::Architecture;
use crate::bus::{MemoryBus, MemoryError};
use crate::types::{Endianness, StackGrowth, WordSize};

/// Result of executing a single CPU instruction or step.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// Instruction executed successfully, consuming cycles.
    Continue { cycles: u32 },
    /// CPU entered a halted or sleep state.
    Halted,
    /// Hit a software or hardware breakpoint.
    Breakpoint,
    /// Raised an interrupt or exception vector.
    Interrupt(u32),
}

/// Errors occurring during CPU instruction decoding or execution.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CpuError {
    /// Bus error during instruction fetch or data access.
    Memory(MemoryError),
    /// Unrecognized or illegal instruction opcode.
    InvalidInstruction { opcode: u64, pc: u64 },
    /// Arithmetic division by zero.
    DivisionByZero,
    /// CPU is in halted state and cannot step without interrupt/reset.
    Halted,
    /// Operation or addressing mode is unsupported.
    UnsupportedOperation(&'static str),
    /// Register name or index was not found on this architecture.
    RegisterNotFound,
    /// CPU trap or fault.
    Trap(u32),
}

impl From<MemoryError> for CpuError {
    fn from(err: MemoryError) -> Self {
        CpuError::Memory(err)
    }
}

/// Value of a CPU register across various word widths.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RegisterValue {
    U8(u8),
    U16(u16),
    U32(u32),
    U64(u64),
}

impl RegisterValue {
    /// Returns the register value converted to `u64`.
    #[inline]
    pub fn as_u64(self) -> u64 {
        match self {
            RegisterValue::U8(v) => v as u64,
            RegisterValue::U16(v) => v as u64,
            RegisterValue::U32(v) => v as u64,
            RegisterValue::U64(v) => v,
        }
    }
}

/// Metadata and current value of a register for inspection/GUI.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RegisterInfo {
    /// Register name (e.g., "RAX", "r13", "sp", "PC").
    pub name: &'static str,
    /// Current register value.
    pub value: RegisterValue,
    /// True if this register serves as the Program Counter.
    pub is_pc: bool,
    /// True if this register serves as the Stack Pointer.
    pub is_sp: bool,
    /// True if this register holds flags / status register.
    pub is_flags: bool,
}

/// Core CPU engine interface implemented by all emulated architectures.
pub trait CpuEngine {
    /// Returns the target architecture identifier.
    fn arch(&self) -> Architecture;

    /// Returns the native endianness.
    fn endianness(&self) -> Endianness;

    /// Returns the stack growth direction.
    fn stack_growth(&self) -> StackGrowth;

    /// Returns the default word size.
    fn word_size(&self) -> WordSize;

    /// Returns the current Program Counter (PC/IP/EIP/RIP).
    fn pc(&self) -> u64;

    /// Sets the current Program Counter.
    fn set_pc(&mut self, val: u64);

    /// Returns the current Stack Pointer (SP/ESP/RSP).
    fn sp(&self) -> u64;

    /// Sets the current Stack Pointer.
    fn set_sp(&mut self, val: u64);

    /// Executes a single instruction or cycle on the given memory bus.
    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError>;

    /// Resets CPU registers and internal state to initial values.
    fn reset(&mut self);

    /// Total number of inspectable registers.
    fn register_count(&self) -> usize;

    /// Gets register metadata by index.
    fn register_info(&self, index: usize) -> Option<RegisterInfo>;

    /// Gets register value by case-insensitive name.
    fn get_register(&self, name: &str) -> Option<u64>;

    /// Sets register value by case-insensitive name.
    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError>;
}
