//! Preloaded demo programs showcasing stack frames and instruction execution for all 16 architectures.

use emulator_core::arch::AnyCpu;
use emulator_core::arch::Architecture;
use emulator_core::bus::{DynamicMemory, MemoryBus};
use emulator_core::cpu::CpuEngine;
use emulator_core::types::Endianness;

/// Metadata for a preloaded demo program.
#[derive(Copy, Clone, Debug)]
pub struct ArchDemo {
    pub arch: Architecture,
    pub name: &'static str,
    pub description: &'static str,
}

pub const DEMOS: &[ArchDemo] = &[
    ArchDemo {
        arch: Architecture::I8086,
        name: "8086 Real Mode Execution & Stack",
        description: "Initializes SP at 0xFFF8, executes MOV, PUSH AX, ADD AX, BX, POP BX, and observes register and stack changes.",
    },
    ArchDemo {
        arch: Architecture::X86,
        name: "IA-32 Standard Frame",
        description: "Sets ESP to 0x0007FFF0, pushes parameters, enters frame with EBP, performs arithmetic, and returns.",
    },
    ArchDemo {
        arch: Architecture::X86_64,
        name: "AMD64 Fastcall & Stack",
        description: "Sets RSP to 0x00080000, aligns stack to 16 bytes, pushes RBP frame, executes math, and cleans stack.",
    },
    ArchDemo {
        arch: Architecture::Arm32,
        name: "ARM32 Push/Pop Link Register",
        description: "Sets SP (R13) to 0x20000, pushes {R4, LR}, executes subroutine, and restores {R4, PC}.",
    },
    ArchDemo {
        arch: Architecture::Arm64,
        name: "AArch64 Frame Record (X29/X30)",
        description: "Sets SP to 0x70000, stores pair [X29, X30], updates frame pointer X29, and returns.",
    },
    ArchDemo {
        arch: Architecture::RiscV,
        name: "RISC-V RV32I Arithmetic & Stack",
        description: "Executes ADDI, ADD, SW to stack, LW from stack, verifying register file and downward stack inspection.",
    },
    ArchDemo {
        arch: Architecture::Ia64,
        name: "IA-64 Stack & Backing Store",
        description: "Initializes r12 (memory stack) and BSP (backing store), sets register window, and returns.",
    },
    ArchDemo {
        arch: Architecture::Mips,
        name: "MIPS32 Subroutine Frame",
        description: "Adjusts $sp ($29), preserves return address $ra ($31) and $fp ($30), loads args, and jumps via jr.",
    },
    ArchDemo {
        arch: Architecture::PowerPc,
        name: "PowerPC ABI Backchain Frame",
        description: "Stores link register LR, updates r1 stack backchain, performs calculations, and restores LR.",
    },
    ArchDemo {
        arch: Architecture::Sparc,
        name: "SPARC Register Window & Stack",
        description: "Allocates window frame via SAVE %sp, uses %i0-%i7/%o0-%o7, restores window via RESTORE, and returns.",
    },
    ArchDemo {
        arch: Architecture::Avr,
        name: "AVR Harvard RISC & Hardware Stack",
        description: "Executes LDI, ADD, PUSH, and POP on SRAM Data stack with SPL/SPH pointer tracking and SREG flags.",
    },
    ArchDemo {
        arch: Architecture::SuperH,
        name: "SuperH (SH-4) Stack Linkage",
        description: "Pre-decrements R15 with PR link register, sets up R14 frame pointer, and restores via @R15+.",
    },
    ArchDemo {
        arch: Architecture::PaRisc,
        name: "PA-RISC Upward Growing Stack",
        description: "Demonstrates PA-RISC upwards stack growth towards higher memory addresses using r30 (SP) and r3 (FP).",
    },
    ArchDemo {
        arch: Architecture::DecAlpha,
        name: "DEC Alpha 64-bit Procedure Frame",
        description: "Decrements $sp ($30) by 64-bit quadwords, saves return address $26, and returns via ret $31, ($26).",
    },
    ArchDemo {
        arch: Architecture::Motorola68000,
        name: "Motorola 68000 LINK/UNLK Frame",
        description: "Sets A7 (SP), creates stack frame using A6 with LINK #-$10, pushes data registers, and calls UNLK.",
    },
    ArchDemo {
        arch: Architecture::Mos6502,
        name: "MOS 6502 Accumulator & Page 1 Stack",
        description: "Executes LDA, ADC, PHA, TAX, PLA in 6502 Zero Page/Page 1 ($0100-$01FF) architecture.",
    },
];

/// Loads the default demo program and initial state for a given architecture.
pub fn load_demo_for_arch(arch: Architecture, cpu: &mut AnyCpu, bus: &mut DynamicMemory) {
    // Reset CPU registers and clear memory
    cpu.reset();
    for b in bus.data.iter_mut() {
        *b = 0;
    }

    match arch {
        Architecture::I8086 => {
            cpu.set_pc(0x1000);
            cpu.set_sp(0xFFF8);
            let _ = cpu.set_register("CS", 0x0000);
            let _ = cpu.set_register("AX", 0x1234);
            let _ = cpu.set_register("BX", 0x000A);
            let _ = cpu.set_register("BP", 0xFFFE);

            // Instructions at 0x1000:
            // 0x1000: 50          PUSH AX
            // 0x1001: 01 D8       ADD AX, BX
            // 0x1003: 5B          POP BX
            // 0x1004: F4          HLT
            let _ = bus.write_u8(0x1000, 0x50);
            let _ = bus.write_u8(0x1001, 0x01);
            let _ = bus.write_u8(0x1002, 0xD8);
            let _ = bus.write_u8(0x1003, 0x5B);
            let _ = bus.write_u8(0x1004, 0xF4);

            // Mock stack memory content
            let _ = bus.write_u16(0xFFFC, 0x1008, Endianness::LittleEndian); // Return Address
            let _ = bus.write_u16(0xFFFA, 0xFFFE, Endianness::LittleEndian); // Saved BP
            let _ = bus.write_u16(0xFFF8, 0x002A, Endianness::LittleEndian); // Local var (42)
        }
        Architecture::X86 => {
            cpu.set_pc(0x00010000);
            cpu.set_sp(0x0007FFF0);
            let _ = cpu.set_register("EAX", 0xCAFEBABE);
            let _ = cpu.set_register("EBP", 0x0007FFF8);

            // Instructions at 0x00010000: NOP; NOP; NOP; HLT
            let _ = bus.write_u8(0x00010000, 0x90);
            let _ = bus.write_u8(0x00010001, 0x90);
            let _ = bus.write_u8(0x00010002, 0x90);
            let _ = bus.write_u8(0x00010003, 0xF4);

            let _ = bus.write_u32(0x0007FFF8, 0x00080000, Endianness::LittleEndian); // Saved EBP
            let _ = bus.write_u32(0x0007FFF4, 0x00010050, Endianness::LittleEndian); // Return Addr
            let _ = bus.write_u32(0x0007FFF0, 0x00000100, Endianness::LittleEndian); // Local buffer
        }
        Architecture::X86_64 => {
            cpu.set_pc(0x00002000);
            cpu.set_sp(0x00080000);
            let _ = cpu.set_register("RAX", 0x01234567_89ABCDEF);
            let _ = cpu.set_register("RBP", 0x00080010);

            // Instructions at PC: NOP; NOP; NOP; HLT
            let _ = bus.write_u8(0x00002000, 0x90);
            let _ = bus.write_u8(0x00002001, 0x90);
            let _ = bus.write_u8(0x00002002, 0x90);
            let _ = bus.write_u8(0x00002003, 0xF4);

            let _ = bus.write_u64(0x00080010, 0x00080030, Endianness::LittleEndian);
            let _ = bus.write_u64(0x00080008, 0x00002200, Endianness::LittleEndian);
            let _ = bus.write_u64(0x00080000, 0xDEADBEEF_CAFEBABE, Endianness::LittleEndian);
        }
        Architecture::Arm32 => {
            cpu.set_pc(0x00003000);
            cpu.set_sp(0x00020000);
            let _ = cpu.set_register("R0", 0x00000007);
            let _ = cpu.set_register("R11", 0x00020008);
            let _ = cpu.set_register("LR", 0x00003040);

            // Instructions at 0x3000: NOP
            let _ = bus.write_u32(0x00003000, 0xE1A00000, Endianness::LittleEndian);
            let _ = bus.write_u32(0x00003004, 0xE1A00000, Endianness::LittleEndian);

            let _ = bus.write_u32(0x00020008, 0x00020020, Endianness::LittleEndian);
            let _ = bus.write_u32(0x00020004, 0x00003040, Endianness::LittleEndian);
            let _ = bus.write_u32(0x00020000, 0x0000002A, Endianness::LittleEndian);
        }
        Architecture::Arm64 => {
            cpu.set_pc(0x00004000);
            cpu.set_sp(0x00070000);
            let _ = cpu.set_register("X0", 0x00000000_00000064);
            let _ = cpu.set_register("X29", 0x00070010);
            let _ = cpu.set_register("X30", 0x00004120);

            // Instructions at PC: NOP (0xD503201F)
            let _ = bus.write_u32(0x00004000, 0xD503201F, Endianness::LittleEndian);
            let _ = bus.write_u32(0x00004004, 0xD503201F, Endianness::LittleEndian);

            let _ = bus.write_u64(0x00070010, 0x00070040, Endianness::LittleEndian);
            let _ = bus.write_u64(0x00070008, 0x00004120, Endianness::LittleEndian);
            let _ = bus.write_u64(0x00070000, 0x00000000_12345678, Endianness::LittleEndian);
        }
        Architecture::RiscV => {
            cpu.set_pc(0x00005000);
            cpu.set_sp(0x00060000);
            let _ = cpu.set_register("s0", 0x00060010);
            let _ = cpu.set_register("ra", 0x00005080);

            // Instructions at 0x5000:
            // 0x5000: ADDI a0, zero, 15 (0x00F00513)
            // 0x5004: ADDI a1, zero, 27 (0x01B00593)
            // 0x5008: ADD  a0, a0, a1   (0x00B50533) -> a0 = 42
            // 0x500C: SW   a0, 0(sp)    (0x00A12023)
            // 0x5010: EBREAK            (0x00100073)
            let _ = bus.write_u32(0x00005000, 0x00F00513, Endianness::LittleEndian);
            let _ = bus.write_u32(0x00005004, 0x01B00593, Endianness::LittleEndian);
            let _ = bus.write_u32(0x00005008, 0x00B50533, Endianness::LittleEndian);
            let _ = bus.write_u32(0x0000500C, 0x00A12023, Endianness::LittleEndian);
            let _ = bus.write_u32(0x00005010, 0x00100073, Endianness::LittleEndian);

            let _ = bus.write_u64(0x00060010, 0x00060030, Endianness::LittleEndian);
            let _ = bus.write_u64(0x00060008, 0x00005080, Endianness::LittleEndian);
            let _ = bus.write_u64(0x00060000, 0x0000002A, Endianness::LittleEndian);
        }
        Architecture::Ia64 => {
            cpu.set_pc(0x00006000);
            cpu.set_sp(0x00050000);
            let _ = cpu.set_register("r32", 100);

            // Bundle at PC: 16 zeros is NOP bundle
            let _ = bus.write_bytes(0x00006000, &[0u8; 16]);

            let _ = bus.write_u64(0x00050000, 0x00006080, Endianness::LittleEndian);
            let _ = bus.write_u64(0x00050008, 0x00000000_00000001, Endianness::LittleEndian);
        }
        Architecture::Mips => {
            cpu.set_pc(0x00007000);
            cpu.set_sp(0x00040000);
            let _ = cpu.set_register("$v0", 1);
            let _ = cpu.set_register("$30", 0x00040010);
            let _ = cpu.set_register("$ra", 0x00007040);

            // Instructions at PC: NOP (0x00000000)
            let _ = bus.write_u32(0x00007000, 0x00000000, Endianness::BigEndian);
            let _ = bus.write_u32(0x00007004, 0x00000000, Endianness::BigEndian);

            let _ = bus.write_u32(0x00040010, 0x00040030, Endianness::BigEndian);
            let _ = bus.write_u32(0x00040008, 0x00007040, Endianness::BigEndian);
            let _ = bus.write_u32(0x00040000, 0x000000FF, Endianness::BigEndian);
        }
        Architecture::PowerPc => {
            cpu.set_pc(0x00008000);
            cpu.set_sp(0x00030000);
            let _ = cpu.set_register("r3", 10);
            let _ = cpu.set_register("LR", 0x00008050);

            // Instructions at PC: NOP (0x60000000)
            let _ = bus.write_u32(0x00008000, 0x60000000, Endianness::BigEndian);
            let _ = bus.write_u32(0x00008004, 0x60000000, Endianness::BigEndian);

            let _ = bus.write_u32(0x00030000, 0x00030020, Endianness::BigEndian); // Backchain pointer
            let _ = bus.write_u32(0x00030004, 0x00008050, Endianness::BigEndian); // Saved LR
        }
        Architecture::Sparc => {
            cpu.set_pc(0x00009000);
            cpu.set_sp(0x00020000);
            let _ = cpu.set_register("%o0", 55);
            let _ = cpu.set_register("%i6", 0x00020060);

            // Instructions at PC: NOP (0x01000000)
            let _ = bus.write_u32(0x00009000, 0x01000000, Endianness::BigEndian);
            let _ = bus.write_u32(0x00009004, 0x01000000, Endianness::BigEndian);

            let _ = bus.write_u32(0x00020000, 0x00009040, Endianness::BigEndian);
            let _ = bus.write_u32(0x00020004, 0x00020060, Endianness::BigEndian);
        }
        Architecture::Avr => {
            cpu.set_pc(0x0000);
            cpu.set_sp(0x08FD);
            let _ = cpu.set_register("r28", 0xFD);
            let _ = cpu.set_register("r29", 0x08);

            // Instructions at PC:
            // 0x0000: LDI r16, 20 (0xE104)
            // 0x0002: LDI r17, 22 (0xE116)
            // 0x0004: ADD r16, r17 (0x0F01) -> r16 = 42
            // 0x0006: PUSH r16 (0x930F)
            // 0x0008: POP r18 (0x912F)
            // 0x000A: SLEEP (0x9588)
            let _ = bus.write_u16(0x0000, 0xE104, Endianness::LittleEndian);
            let _ = bus.write_u16(0x0002, 0xE116, Endianness::LittleEndian);
            let _ = bus.write_u16(0x0004, 0x0F01, Endianness::LittleEndian);
            let _ = bus.write_u16(0x0006, 0x930F, Endianness::LittleEndian);
            let _ = bus.write_u16(0x0008, 0x912F, Endianness::LittleEndian);
            let _ = bus.write_u16(0x000A, 0x9588, Endianness::LittleEndian);

            let _ = bus.write_u8(0x08FF, 0x10); // Return PC high
            let _ = bus.write_u8(0x08FE, 0x00); // Return PC low
        }
        Architecture::SuperH => {
            cpu.set_pc(0x0000A000);
            cpu.set_sp(0x00018000);
            let _ = cpu.set_register("r0", 0x33);
            let _ = cpu.set_register("r14", 0x00018008);
            let _ = cpu.set_register("PR", 0x0000A080);

            // Instructions at PC: NOP (0x0009)
            let _ = bus.write_u16(0x0000A000, 0x0009, Endianness::BigEndian);
            let _ = bus.write_u16(0x0000A002, 0x0009, Endianness::BigEndian);

            let _ = bus.write_u32(0x00018000, 0x0000A080, Endianness::BigEndian);
            let _ = bus.write_u32(0x00018004, 0x00018020, Endianness::BigEndian);
        }
        Architecture::PaRisc => {
            // Upward stack growth
            cpu.set_pc(0x0000B000);
            cpu.set_sp(0x00008040); // SP points to current top
            let _ = cpu.set_register("r26", 0x100);
            let _ = cpu.set_register("r3", 0x00008000); // Frame pointer

            // Instructions at PC: NOP (0x08000240)
            let _ = bus.write_u32(0x0000B000, 0x08000240, Endianness::BigEndian);
            let _ = bus.write_u32(0x0000B004, 0x08000240, Endianness::BigEndian);

            // In PA-RISC upward growth, stack slots grow from 0x8000 upwards to 0x8040
            let _ = bus.write_u32(0x00008040, 0x12345678, Endianness::BigEndian);
            let _ = bus.write_u32(0x0000803C, 0x0000B080, Endianness::BigEndian);
            let _ = bus.write_u32(0x00008000, 0x00007FC0, Endianness::BigEndian);
        }
        Architecture::DecAlpha => {
            cpu.set_pc(0x0000C000);
            cpu.set_sp(0x00010000);
            let _ = cpu.set_register("r0", 99);
            let _ = cpu.set_register("r15", 0x00010020);
            let _ = cpu.set_register("r26", 0x0000C100);

            // Instructions at PC: NOP (0x47FF041F)
            let _ = bus.write_u32(0x0000C000, 0x47FF041F, Endianness::LittleEndian);
            let _ = bus.write_u32(0x0000C004, 0x47FF041F, Endianness::LittleEndian);

            let _ = bus.write_u64(0x00010000, 0x0000C100, Endianness::LittleEndian);
            let _ = bus.write_u64(0x00010008, 0x00010040, Endianness::LittleEndian);
        }
        Architecture::Motorola68000 => {
            cpu.set_pc(0x0000D000);
            cpu.set_sp(0x0001C000);
            let _ = cpu.set_register("D0", 0x00001234);
            let _ = cpu.set_register("A6", 0x0001C010);

            // Instructions at PC: NOP (0x4E71)
            let _ = bus.write_u16(0x0000D000, 0x4E71, Endianness::BigEndian);
            let _ = bus.write_u16(0x0000D002, 0x4E71, Endianness::BigEndian);

            let _ = bus.write_u32(0x0001C000, 0x0000D050, Endianness::BigEndian);
            let _ = bus.write_u32(0x0001C004, 0x0001C030, Endianness::BigEndian);
            let _ = bus.write_u32(0x0001C008, 0x0000DEAD, Endianness::BigEndian);
        }
        Architecture::Mos6502 => {
            cpu.set_pc(0x0600);
            cpu.set_sp(0x01FD);
            let _ = cpu.set_register("A", 0);
            let _ = cpu.set_register("X", 0);

            // Instructions at 0x0600:
            // 0x0600: LDA #$1A (0xA9, 0x1A = 26)
            // 0x0602: CLC     (0x18)
            // 0x0603: ADC #$10 (0x69, 0x10 = 16 -> A = 42)
            // 0x0605: PHA     (0x48) -> pushes 42 to 0x01FD
            // 0x0606: TAX     (0xAA) -> X = 42
            // 0x0607: PLA     (0x68) -> pulls 42
            // 0x0608: EA      (NOP)
            let _ = bus.write_u8(0x0600, 0xA9);
            let _ = bus.write_u8(0x0601, 0x1A);
            let _ = bus.write_u8(0x0602, 0x18);
            let _ = bus.write_u8(0x0603, 0x69);
            let _ = bus.write_u8(0x0604, 0x10);
            let _ = bus.write_u8(0x0605, 0x48);
            let _ = bus.write_u8(0x0606, 0xAA);
            let _ = bus.write_u8(0x0607, 0x68);
            let _ = bus.write_u8(0x0608, 0xEA);

            let _ = bus.write_u8(0x01FF, 0x06); // Top of stack mock frame
            let _ = bus.write_u8(0x01FE, 0x50);
            let _ = bus.write_u8(0x01FD, 0x2A);
        }
    }
}
