//! Intel IA-64 (Itanium / EPIC) CPU implementation.

use crate::arch::Architecture;
use crate::bus::MemoryBus;
use crate::cpu::{CpuEngine, CpuError, RegisterInfo, RegisterValue, StepOutcome};
use crate::types::{Endianness, StackGrowth, WordSize};

/// IA-64 register state.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Ia64State {
    pub gr: [u64; 128], // General Registers: gr0=0, gr1=gp, gr12=sp, gr13=tp
    pub pr: u64,        // 64 1-bit predicate registers (p0 is hardwired 1)
    pub br: [u64; 8],   // Branch registers b0-b7 (b0 is return pointer)
    pub ip: u64,        // Instruction Pointer
    pub cfm: u64,       // Current Frame Marker
    pub bsp: u64,       // Backing Store Pointer for Register Stack Engine (RSE)
    pub halted: bool,
}

impl Default for Ia64State {
    fn default() -> Self {
        let mut state = Self {
            gr: [0; 128],
            pr: 1, // p0 = 1
            br: [0; 8],
            ip: 0,
            cfm: 0,
            bsp: 0x0000_7FFF_0000_0000,
            halted: false,
        };
        state.gr[12] = 0x0000_7FFF_FFFF_0000; // SP
        state
    }
}

/// IA-64 CPU Engine.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ia64Cpu {
    pub state: Ia64State,
}

impl Ia64Cpu {
    pub fn new() -> Self {
        let mut cpu = Self::default();
        cpu.reset();
        cpu
    }

    pub fn push_u64(&mut self, bus: &mut dyn MemoryBus, val: u64) -> Result<(), CpuError> {
        self.state.gr[12] = self.state.gr[12].wrapping_sub(8);
        bus.write_u64(self.state.gr[12], val, Endianness::LittleEndian)?;
        Ok(())
    }

    pub fn pop_u64(&mut self, bus: &mut dyn MemoryBus) -> Result<u64, CpuError> {
        let val = bus.read_u64(self.state.gr[12], Endianness::LittleEndian)?;
        self.state.gr[12] = self.state.gr[12].wrapping_add(8);
        Ok(val)
    }
}

impl CpuEngine for Ia64Cpu {
    fn arch(&self) -> Architecture {
        Architecture::Ia64
    }

    fn endianness(&self) -> Endianness {
        Endianness::LittleEndian
    }

    fn stack_growth(&self) -> StackGrowth {
        StackGrowth::Downwards
    }

    fn word_size(&self) -> WordSize {
        WordSize::Bytes8
    }

    fn pc(&self) -> u64 {
        self.state.ip
    }

    fn set_pc(&mut self, val: u64) {
        self.state.ip = val;
    }

    fn sp(&self) -> u64 {
        self.state.gr[12]
    }

    fn set_sp(&mut self, val: u64) {
        self.state.gr[12] = val;
    }

    fn reset(&mut self) {
        self.state = Ia64State::default();
    }

    fn step(&mut self, bus: &mut dyn MemoryBus) -> Result<StepOutcome, CpuError> {
        if self.state.halted {
            return Ok(StepOutcome::Halted);
        }

        let ip = self.pc();
        // IA-64 instructions are organized in 128-bit bundles (16 bytes)
        let mut bundle = [0u8; 16];
        bus.read_bytes(ip, &mut bundle)?;
        self.state.ip = self.state.ip.wrapping_add(16);

        // Template slot check / NOP bundle
        if bundle == [0u8; 16] {
            // NOP bundle
            return Ok(StepOutcome::Continue { cycles: 1 });
        }

        // Check if bundle first byte is 0xFF (Halt convention for simulation)
        if bundle[0] == 0xFF {
            self.state.halted = true;
            return Ok(StepOutcome::Halted);
        }

        // Generic execution outcome
        self.state.gr[0] = 0;
        self.state.pr |= 1; // p0 is always 1
        Ok(StepOutcome::Continue { cycles: 1 })
    }

    fn register_count(&self) -> usize {
        // 128 GR + 8 BR + IP + CFM + BSP + PR
        140
    }

    fn register_info(&self, index: usize) -> Option<RegisterInfo> {
        match index {
            0..=127 => {
                // Return named info for GR0-GR127
                const SPECIAL_GR: [&str; 16] = [
                    "gr0", "gr1(gp)", "gr2", "gr3", "gr4", "gr5", "gr6", "gr7", "gr8", "gr9",
                    "gr10", "gr11", "gr12(sp)", "gr13(tp)", "gr14", "gr15",
                ];
                let name = if index < 16 {
                    SPECIAL_GR[index]
                } else {
                    "gr"
                };
                Some(RegisterInfo {
                    name,
                    value: RegisterValue::U64(self.state.gr[index]),
                    is_pc: false,
                    is_sp: index == 12,
                    is_flags: false,
                })
            }
            128..=135 => {
                const BR_NAMES: [&str; 8] = ["b0", "b1", "b2", "b3", "b4", "b5", "b6", "b7"];
                Some(RegisterInfo {
                    name: BR_NAMES[index - 128],
                    value: RegisterValue::U64(self.state.br[index - 128]),
                    is_pc: false,
                    is_sp: false,
                    is_flags: false,
                })
            }
            136 => Some(RegisterInfo {
                name: "ip",
                value: RegisterValue::U64(self.state.ip),
                is_pc: true,
                is_sp: false,
                is_flags: false,
            }),
            137 => Some(RegisterInfo {
                name: "cfm",
                value: RegisterValue::U64(self.state.cfm),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            138 => Some(RegisterInfo {
                name: "bsp",
                value: RegisterValue::U64(self.state.bsp),
                is_pc: false,
                is_sp: false,
                is_flags: false,
            }),
            139 => Some(RegisterInfo {
                name: "pr",
                value: RegisterValue::U64(self.state.pr),
                is_pc: false,
                is_sp: false,
                is_flags: true,
            }),
            _ => None,
        }
    }

    fn get_register(&self, name: &str) -> Option<u64> {
        if let Some(num_str) = name
            .strip_prefix("gr")
            .or_else(|| name.strip_prefix("GR"))
            .or_else(|| name.strip_prefix('r'))
            .or_else(|| name.strip_prefix('R'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 128 {
                    return Some(self.state.gr[idx]);
                }
        if let Some(num_str) = name.strip_prefix('b').or_else(|| name.strip_prefix('B'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    return Some(self.state.br[idx]);
                }
        match name {
            s if s.eq_ignore_ascii_case("sp") || s.eq_ignore_ascii_case("r12") => {
                Some(self.state.gr[12])
            }
            s if s.eq_ignore_ascii_case("gp") || s.eq_ignore_ascii_case("r1") => {
                Some(self.state.gr[1])
            }
            s if s.eq_ignore_ascii_case("tp") || s.eq_ignore_ascii_case("r13") => {
                Some(self.state.gr[13])
            }
            s if s.eq_ignore_ascii_case("ip") || s.eq_ignore_ascii_case("pc") => {
                Some(self.state.ip)
            }
            s if s.eq_ignore_ascii_case("cfm") => Some(self.state.cfm),
            s if s.eq_ignore_ascii_case("bsp") => Some(self.state.bsp),
            s if s.eq_ignore_ascii_case("pr") => Some(self.state.pr),
            _ => None,
        }
    }

    fn set_register(&mut self, name: &str, val: u64) -> Result<(), CpuError> {
        if let Some(num_str) = name
            .strip_prefix("gr")
            .or_else(|| name.strip_prefix("GR"))
            .or_else(|| name.strip_prefix('r'))
            .or_else(|| name.strip_prefix('R'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 128 {
                    if idx != 0 {
                        self.state.gr[idx] = val;
                    }
                    return Ok(());
                }
        if let Some(num_str) = name.strip_prefix('b').or_else(|| name.strip_prefix('B'))
            && let Ok(idx) = num_str.parse::<usize>()
                && idx < 8 {
                    self.state.br[idx] = val;
                    return Ok(());
                }
        match name {
            s if s.eq_ignore_ascii_case("sp") || s.eq_ignore_ascii_case("r12") => {
                self.state.gr[12] = val
            }
            s if s.eq_ignore_ascii_case("gp") || s.eq_ignore_ascii_case("r1") => {
                self.state.gr[1] = val
            }
            s if s.eq_ignore_ascii_case("tp") || s.eq_ignore_ascii_case("r13") => {
                self.state.gr[13] = val
            }
            s if s.eq_ignore_ascii_case("ip") || s.eq_ignore_ascii_case("pc") => {
                self.state.ip = val
            }
            s if s.eq_ignore_ascii_case("cfm") => self.state.cfm = val,
            s if s.eq_ignore_ascii_case("bsp") => self.state.bsp = val,
            s if s.eq_ignore_ascii_case("pr") => self.state.pr = val | 1,
            _ => return Err(CpuError::RegisterNotFound),
        }
        self.state.gr[0] = 0;
        self.state.pr |= 1;
        Ok(())
    }
}
