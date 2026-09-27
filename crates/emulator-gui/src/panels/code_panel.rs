//! Instruction Stream and Disassembly Panel with multi-architecture
//! translated assembly (ASM) decoding, breakpoint toggling, and independent traversal.

use std::collections::BTreeMap;

use egui::{Color32, RichText, ScrollArea, Ui};
use emulator_core::arch::Architecture;
use emulator_core::bus::{DynamicMemory, MemoryBus};
use emulator_core::cpu::CpuEngine;

/// Disassembly view configuration and traversal state.
#[derive(Clone, Debug)]
pub struct CodePanelState {
    pub lock_to_pc: bool,
    pub custom_start_addr: Option<u64>,
    pub addr_input: String,
    pub row_count: usize,
    pub scroll_offset_instr: i64,
}

impl Default for CodePanelState {
    fn default() -> Self {
        Self {
            lock_to_pc: true,
            custom_start_addr: None,
            addr_input: String::new(),
            row_count: 32,
            scroll_offset_instr: 0,
        }
    }
}

/// Renders the Disassembly / Instruction Stream panel with translated ASM column.
pub fn render_code_panel(
    ui: &mut Ui,
    cpu: &dyn CpuEngine,
    bus: &DynamicMemory,
    breakpoints: &mut Vec<u64>,
    state: &mut CodePanelState,
    symbols: Option<&BTreeMap<String, u64>>,
) {
    ui.heading("Disassembly / Instruction Stream");
    ui.separator();

    let arch = cpu.arch();
    let pc = cpu.pc();
    let word_size = cpu.word_size().in_bytes();

    // Top Controls Bar - Traversal & Lock to PC
    ui.horizontal_wrapped(|ui| {
        let lock_label = if state.lock_to_pc {
            RichText::new("🔒 Locked to PC").color(Color32::from_rgb(80, 220, 120)).strong()
        } else {
            RichText::new("🔓 Unlocked View").color(Color32::from_rgb(255, 200, 80)).strong()
        };

        if ui.button(lock_label).clicked() {
            state.lock_to_pc = !state.lock_to_pc;
            if state.lock_to_pc {
                state.custom_start_addr = None;
                state.scroll_offset_instr = 0;
            }
        }

        ui.separator();
        ui.label("Traverse:");

        if ui.button("⏮ -32").clicked() {
            state.lock_to_pc = false;
            state.scroll_offset_instr = state.scroll_offset_instr.saturating_sub(32);
        }
        if ui.button("◀ -4").clicked() {
            state.lock_to_pc = false;
            state.scroll_offset_instr = state.scroll_offset_instr.saturating_sub(4);
        }
        if ui.button("▶ +4").clicked() {
            state.lock_to_pc = false;
            state.scroll_offset_instr = state.scroll_offset_instr.saturating_add(4);
        }
        if ui.button("⏭ +32").clicked() {
            state.lock_to_pc = false;
            state.scroll_offset_instr = state.scroll_offset_instr.saturating_add(32);
        }

        if (!state.lock_to_pc || state.scroll_offset_instr != 0 || state.custom_start_addr.is_some())
            && ui.button(RichText::new("🎯 Jump to PC").color(Color32::from_rgb(100, 220, 255))).clicked()
        {
            state.lock_to_pc = true;
            state.custom_start_addr = None;
            state.scroll_offset_instr = 0;
        }

        ui.separator();
        ui.label("Rows:");
        ui.add(egui::Slider::new(&mut state.row_count, 16..=128).text("count"));
    });

    ui.add_space(2.0);

    // Jump to Address Bar
    ui.horizontal(|ui| {
        ui.label("Jump Addr:");
        ui.text_edit_singleline(&mut state.addr_input);
        if ui.button("Go").clicked() {
            let trimmed = state.addr_input.trim();
            let parsed = if let Some(hex_str) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
                u64::from_str_radix(hex_str, 16).ok()
            } else if trimmed.ends_with('h') || trimmed.ends_with('H') {
                u64::from_str_radix(&trimmed[..trimmed.len() - 1], 16).ok()
            } else {
                trimmed.parse::<u64>().ok()
            };

            if let Some(target) = parsed {
                state.lock_to_pc = false;
                state.custom_start_addr = Some(target);
                state.scroll_offset_instr = 0;
            }
        }
    });

    ui.separator();

    // Compute base start address
    let base_anchor = if state.lock_to_pc {
        pc.saturating_sub((6 * word_size) as u64)
    } else {
        state.custom_start_addr.unwrap_or(pc.saturating_sub((6 * word_size) as u64))
    };

    let start_pc = base_anchor.wrapping_add((state.scroll_offset_instr * word_size as i64) as u64);

    // Disassembly Table with independent scrolling
    ScrollArea::vertical()
        .id_salt("code_panel_scroll_area")
        .auto_shrink([false; 2])
        .show(ui, |ui| {
            egui::Grid::new("disasm_grid")
                .striped(true)
                .min_col_width(45.0)
                .spacing([12.0, 4.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("BP").strong());
                    ui.label(RichText::new("PC").strong());
                    ui.label(RichText::new("Address").strong());
                    ui.label(RichText::new("Opcode Bytes").strong());
                    ui.label(RichText::new("Instruction (ASM)").strong().color(Color32::from_rgb(100, 220, 255)));
                    ui.label(RichText::new("Preview / Status").strong());
                    ui.end_row();

                    for i in 0..state.row_count {
                        let addr = start_pc.wrapping_add((i * word_size) as u64);
                        let is_current_pc = addr == pc;
                        let is_breakpoint = breakpoints.contains(&addr);

                        // BP Column
                        let bp_btn = if is_breakpoint {
                            ui.button(RichText::new("🔴").color(Color32::RED))
                        } else {
                            ui.button("⚪")
                        };
                        if bp_btn.clicked() {
                            if is_breakpoint {
                                breakpoints.retain(|&b| b != addr);
                            } else {
                                breakpoints.push(addr);
                            }
                        }

                        // PC Pointer arrow
                        if is_current_pc {
                            ui.label(RichText::new("=>").strong().color(Color32::from_rgb(255, 100, 100)));
                        } else {
                            ui.label("  ");
                        }

                        // Address Column
                        let addr_color = if is_current_pc {
                            Color32::from_rgb(255, 100, 100)
                        } else {
                            Color32::from_rgb(100, 200, 255)
                        };
                        ui.label(RichText::new(format!("{:#010X}", addr)).monospace().color(addr_color));

                        // Opcode Bytes Column
                        let mut bytes = [0u8; 16];
                        let _ = bus.read_bytes(addr, &mut bytes[..word_size.max(4)]);
                        let mut opcode_str = String::new();
                        for (b, &byte) in bytes[..word_size].iter().enumerate() {
                            if b > 0 {
                                opcode_str.push(' ');
                            }
                            opcode_str.push_str(&format!("{:02X}", byte));
                        }
                        ui.label(RichText::new(opcode_str).monospace().color(ui.visuals().strong_text_color()));

                        // Translated Instruction (ASM) Column
                        let translated_asm = disassemble_instruction(arch, addr, &bytes[..word_size], &bytes, symbols);
                        let asm_color = if is_current_pc {
                            Color32::from_rgb(255, 220, 120)
                        } else {
                            Color32::from_rgb(140, 230, 160)
                        };
                        ui.label(RichText::new(translated_asm).monospace().strong().color(asm_color));

                        // Preview / Status Column
                        let label_annotation = symbols.and_then(|syms| {
                            syms.iter().find(|(_, val)| **val == addr).map(|(name, _)| format!("<{}>", name))
                        });

                        let preview_text = if is_current_pc {
                            "CURRENT INSTRUCTION".to_string()
                        } else if let Some(lbl) = label_annotation {
                            lbl
                        } else if addr < pc {
                            "Executed".to_string()
                        } else {
                            "Upcoming".to_string()
                        };
                        ui.label(RichText::new(preview_text).weak().color(Color32::LIGHT_GRAY));

                        ui.end_row();
                    }
                });
        });
}

/// Translates raw opcode bytes into a human-readable assembly instruction representation.
pub fn disassemble_instruction(
    arch: Architecture,
    _addr: u64,
    _word_bytes: &[u8],
    full_bytes: &[u8],
    _symbols: Option<&BTreeMap<String, u64>>,
) -> String {
    let b0 = full_bytes.first().copied().unwrap_or(0);
    let b1 = full_bytes.get(1).copied().unwrap_or(0);
    let b2 = full_bytes.get(2).copied().unwrap_or(0);
    let b3 = full_bytes.get(3).copied().unwrap_or(0);

    match arch {
        Architecture::I8086 | Architecture::X86 | Architecture::X86_64 => {
            disassemble_x86_family(arch, full_bytes)
        }
        Architecture::Mos6502 => disassemble_mos6502(full_bytes),
        Architecture::Avr => disassemble_avr(full_bytes),
        Architecture::RiscV => disassemble_riscv(b0, b1, b2, b3),
        Architecture::Arm32 => disassemble_arm32(b0, b1, b2, b3),
        Architecture::Arm64 => disassemble_arm64(b0, b1, b2, b3),
        Architecture::Mips => disassemble_mips(b0, b1, b2, b3),
        Architecture::PowerPc => disassemble_powerpc(b0, b1, b2, b3),
        Architecture::Sparc => disassemble_sparc(b0, b1, b2, b3),
        Architecture::Motorola68000 => disassemble_m68k(full_bytes),
        Architecture::SuperH => disassemble_superh(b0, b1),
        Architecture::PaRisc => disassemble_parisc(b0, b1, b2, b3),
        Architecture::DecAlpha => disassemble_alpha(b0, b1, b2, b3),
        Architecture::Ia64 => format!("bundle [{:02X} {:02X} {:02X} {:02X}]", b0, b1, b2, b3),
    }
}

fn disassemble_x86_family(arch: Architecture, bytes: &[u8]) -> String {
    let b0 = bytes.first().copied().unwrap_or(0);
    let b1 = bytes.get(1).copied().unwrap_or(0);
    let b2 = bytes.get(2).copied().unwrap_or(0);
    let b3 = bytes.get(3).copied().unwrap_or(0);

    let reg_names_16 = ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"];
    let reg_names_32 = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
    let reg_names_64 = ["rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi"];

    let reg_names = match arch {
        Architecture::X86_64 => &reg_names_64,
        Architecture::X86 => &reg_names_32,
        _ => &reg_names_16,
    };

    match b0 {
        0x90 => "nop".to_string(),
        0xF4 => "hlt".to_string(),
        0xCC => "int 3".to_string(),
        0xCD => format!("int {:#04X}", b1),
        0xC3 => "ret".to_string(),
        0xCB => "retf".to_string(),
        0xFA => "cli".to_string(),
        0xFB => "sti".to_string(),
        0xFC => "cld".to_string(),
        0xFD => "std".to_string(),
        0xF8 => "clc".to_string(),
        0xF9 => "stc".to_string(),
        0x50..=0x57 => format!("push {}", reg_names[(b0 - 0x50) as usize]),
        0x58..=0x5F => format!("pop {}", reg_names[(b0 - 0x58) as usize]),
        0x40..=0x47 if arch != Architecture::X86_64 => format!("inc {}", reg_names[(b0 - 0x40) as usize]),
        0x48..=0x4F if arch != Architecture::X86_64 => format!("dec {}", reg_names[(b0 - 0x48) as usize]),
        0xB8..=0xBF => {
            let reg = reg_names[(b0 - 0xB8) as usize];
            let imm = (b1 as u16) | ((b2 as u16) << 8);
            format!("mov {}, {:#06X}", reg, imm)
        }
        0xB0..=0xB7 => {
            let r8_names = ["al", "cl", "dl", "bl", "ah", "ch", "dh", "bh"];
            format!("mov {}, {:#04X}", r8_names[(b0 - 0xB0) as usize], b1)
        }
        0x88 => format!("mov [r/m8], r8 (modrm={:#04X})", b1),
        0x89 => format!("mov [r/m], r (modrm={:#04X})", b1),
        0x8A => format!("mov r8, [r/m8] (modrm={:#04X})", b1),
        0x8B => format!("mov r, [r/m] (modrm={:#04X})", b1),
        0x8D => format!("lea reg, [r/m] (modrm={:#04X})", b1),
        0x00 | 0x01 => format!("add r/m, r (modrm={:#04X})", b1),
        0x02 | 0x03 => format!("add r, r/m (modrm={:#04X})", b1),
        0x04 | 0x05 => format!("add {}, {:#06X}", reg_names[0], (b1 as u16) | ((b2 as u16) << 8)),
        0x28 | 0x29 => format!("sub r/m, r (modrm={:#04X})", b1),
        0x2A | 0x2B => format!("sub r, r/m (modrm={:#04X})", b1),
        0x30 | 0x31 => format!("xor r/m, r (modrm={:#04X})", b1),
        0x32 | 0x33 => format!("xor r, r/m (modrm={:#04X})", b1),
        0x38 | 0x39 => format!("cmp r/m, r (modrm={:#04X})", b1),
        0x3A | 0x3B => format!("cmp r, r/m (modrm={:#04X})", b1),
        0x08 | 0x09 => format!("or r/m, r (modrm={:#04X})", b1),
        0x20 | 0x21 => format!("and r/m, r (modrm={:#04X})", b1),
        0xE8 => {
            let rel = (b1 as i16) | ((b2 as i16) << 8);
            format!("call {:#06X}", rel)
        }
        0xE9 => {
            let rel = (b1 as i16) | ((b2 as i16) << 8);
            format!("jmp {:#06X}", rel)
        }
        0xEB => format!("jmp short {:#04X}", b1 as i8),
        0x74 => format!("jz / je {:#04X}", b1 as i8),
        0x75 => format!("jnz / jne {:#04X}", b1 as i8),
        0x72 => format!("jb / jc {:#04X}", b1 as i8),
        0x73 => format!("jnb / jnc {:#04X}", b1 as i8),
        0x7C => format!("jl {:#04X}", b1 as i8),
        0x7D => format!("jge {:#04X}", b1 as i8),
        0x7E => format!("jle {:#04X}", b1 as i8),
        0x7F => format!("jg {:#04X}", b1 as i8),
        0xE4 | 0xE5 => format!("in al, {:#04X}", b1),
        0xE6 | 0xE7 => format!("out {:#04X}, al", b1),
        0xEC | 0xED => "in al, dx".to_string(),
        0xEE | 0xEF => "out dx, al".to_string(),
        0xC6 | 0xC7 => format!("mov [r/m], imm (modrm={:#04X}, imm={:#04X})", b1, b2),
        _ => format!("db {:#04X}, {:#04X}, {:#04X}, {:#04X}", b0, b1, b2, b3),
    }
}

fn disassemble_mos6502(bytes: &[u8]) -> String {
    let b0 = bytes.first().copied().unwrap_or(0);
    let b1 = bytes.get(1).copied().unwrap_or(0);
    let b2 = bytes.get(2).copied().unwrap_or(0);

    match b0 {
        0x00 => "brk".to_string(),
        0xEA => "nop".to_string(),
        0x60 => "rts".to_string(),
        0x40 => "rti".to_string(),
        0x48 => "pha".to_string(),
        0x68 => "pla".to_string(),
        0x08 => "php".to_string(),
        0x28 => "plp".to_string(),
        0xAA => "tax".to_string(),
        0x8A => "txa".to_string(),
        0xA8 => "tay".to_string(),
        0x98 => "tya".to_string(),
        0xBA => "tsx".to_string(),
        0x9A => "txs".to_string(),
        0xE8 => "inx".to_string(),
        0xCA => "dex".to_string(),
        0xC8 => "iny".to_string(),
        0x88 => "dey".to_string(),
        0x18 => "clc".to_string(),
        0x38 => "sec".to_string(),
        0x58 => "cli".to_string(),
        0x78 => "sei".to_string(),
        0xD8 => "cld".to_string(),
        0xF8 => "sed".to_string(),
        0xB8 => "clv".to_string(),
        0xA9 => format!("lda #${:02X}", b1),
        0xA2 => format!("ldx #${:02X}", b1),
        0xA0 => format!("ldy #${:02X}", b1),
        0x85 => format!("sta ${:02X}", b1),
        0x8D => format!("sta ${:02X}{:02X}", b2, b1),
        0x86 => format!("stx ${:02X}", b1),
        0x8E => format!("stx ${:02X}{:02X}", b2, b1),
        0x84 => format!("sty ${:02X}", b1),
        0x8C => format!("sty ${:02X}{:02X}", b2, b1),
        0x4C => format!("jmp ${:02X}{:02X}", b2, b1),
        0x6C => format!("jmp (${:02X}{:02X})", b2, b1),
        0x20 => format!("jsr ${:02X}{:02X}", b2, b1),
        0xF0 => format!("beq ${:02X}", b1 as i8),
        0xD0 => format!("bne ${:02X}", b1 as i8),
        0x90 => format!("bcc ${:02X}", b1 as i8),
        0xB0 => format!("bcs ${:02X}", b1 as i8),
        0x10 => format!("bpl ${:02X}", b1 as i8),
        0x30 => format!("bmi ${:02X}", b1 as i8),
        0x69 => format!("adc #${:02X}", b1),
        0xE9 => format!("sbc #${:02X}", b1),
        0xC9 => format!("cmp #${:02X}", b1),
        0xE0 => format!("cpx #${:02X}", b1),
        0xC0 => format!("cpy #${:02X}", b1),
        _ => format!(".byte ${:02X}", b0),
    }
}

fn disassemble_avr(bytes: &[u8]) -> String {
    let b0 = bytes.first().copied().unwrap_or(0);
    let b1 = bytes.get(1).copied().unwrap_or(0);
    let word = (b0 as u16) | ((b1 as u16) << 8);

    match word {
        0x0000 => "nop".to_string(),
        0x9508 => "ret".to_string(),
        0x9518 => "reti".to_string(),
        0x95F8 => "cli".to_string(),
        0x9478 => "sei".to_string(),
        0x9588 => "sleep".to_string(),
        0x95A8 => "wdr".to_string(),
        _ if (word & 0xFE0F) == 0x920F => {
            let r = ((word >> 4) & 0x1F) as u8;
            format!("push r{}", r)
        }
        _ if (word & 0xFE0F) == 0x900F => {
            let r = ((word >> 4) & 0x1F) as u8;
            format!("pop r{}", r)
        }
        _ if (word & 0xF000) == 0xE000 => {
            let r = 16 + (((word >> 4) & 0x0F) as u8);
            let imm = (((word >> 8) & 0x0F) << 4) | (word & 0x0F);
            format!("ldi r{}, {:#04X}", r, imm)
        }
        _ if (word & 0xFC00) == 0x0C00 => {
            let rd = ((word >> 4) & 0x1F) as u8;
            let rr = ((((word >> 9) & 0x01) << 4) | (word & 0x0F)) as u8;
            format!("add r{}, r{}", rd, rr)
        }
        _ if (word & 0xFC00) == 0x1800 => {
            let rd = ((word >> 4) & 0x1F) as u8;
            let rr = ((((word >> 9) & 0x01) << 4) | (word & 0x0F)) as u8;
            format!("sub r{}, r{}", rd, rr)
        }
        _ if (word & 0xFC00) == 0x2400 => {
            let rd = ((word >> 4) & 0x1F) as u8;
            let rr = ((((word >> 9) & 0x01) << 4) | (word & 0x0F)) as u8;
            if rd == rr {
                format!("clr r{}", rd)
            } else {
                format!("eor r{}, r{}", rd, rr)
            }
        }
        _ if (word & 0xF800) == 0xB000 => {
            let r = ((word >> 4) & 0x1F) as u8;
            let port = (((word >> 9) & 0x03) << 4) | (word & 0x0F);
            format!("in r{}, {:#04X}", r, port)
        }
        _ if (word & 0xF800) == 0xB800 => {
            let r = ((word >> 4) & 0x1F) as u8;
            let port = (((word >> 9) & 0x03) << 4) | (word & 0x0F);
            format!("out {:#04X}, r{}", port, r)
        }
        _ if (word & 0xF000) == 0xC000 => {
            let rel = (word & 0x0FFF) as i16;
            format!("rjmp {:#06X}", rel)
        }
        _ if (word & 0xF000) == 0xD000 => {
            let rel = (word & 0x0FFF) as i16;
            format!("rcall {:#06X}", rel)
        }
        _ if (word & 0xFC00) == 0xF000 => {
            let rel = ((word >> 3) & 0x7F) as i8;
            format!("breq {:#04X}", rel)
        }
        _ if (word & 0xFC00) == 0xF400 => {
            let rel = ((word >> 3) & 0x7F) as i8;
            format!("brne {:#04X}", rel)
        }
        _ => format!(".word {:#06X}", word),
    }
}

fn disassemble_riscv(b0: u8, b1: u8, b2: u8, b3: u8) -> String {
    let word = (b0 as u32) | ((b1 as u32) << 8) | ((b2 as u32) << 16) | ((b3 as u32) << 24);
    let opcode = word & 0x7F;
    let rd = (word >> 7) & 0x1F;
    let funct3 = (word >> 12) & 0x07;
    let rs1 = (word >> 15) & 0x1F;
    let rs2 = (word >> 20) & 0x1F;
    let funct7 = (word >> 25) & 0x7F;

    let reg_name = |r: u32| match r {
        0 => "zero", 1 => "ra", 2 => "sp", 3 => "gp", 4 => "tp",
        5 => "t0", 6 => "t1", 7 => "t2", 8 => "s0/fp", 9 => "s1",
        10 => "a0", 11 => "a1", 12 => "a2", 13 => "a3", 14 => "a4", 15 => "a5",
        16 => "a6", 17 => "a7", 18 => "s2", 19 => "s3", 20 => "s4", 21 => "s5",
        22 => "s6", 23 => "s7", 24 => "s8", 25 => "s9", 26 => "s10", 27 => "s11",
        28 => "t3", 29 => "t4", 30 => "t5", 31 => "t6",
        _ => "x?",
    };

    match opcode {
        0x13 => {
            let imm = (word as i32) >> 20;
            match funct3 {
                0 => {
                    if rs1 == 0 && imm == 0 && rd == 0 {
                        "nop".to_string()
                    } else if rs1 == 0 {
                        format!("li {}, {}", reg_name(rd), imm)
                    } else {
                        format!("addi {}, {}, {}", reg_name(rd), reg_name(rs1), imm)
                    }
                }
                1 => format!("slli {}, {}, {}", reg_name(rd), reg_name(rs1), imm & 0x1F),
                2 => format!("slti {}, {}, {}", reg_name(rd), reg_name(rs1), imm),
                4 => format!("xori {}, {}, {}", reg_name(rd), reg_name(rs1), imm),
                6 => format!("ori {}, {}, {}", reg_name(rd), reg_name(rs1), imm),
                7 => format!("andi {}, {}, {}", reg_name(rd), reg_name(rs1), imm),
                _ => format!("op-imm f3={} {}, {}, {}", funct3, reg_name(rd), reg_name(rs1), imm),
            }
        }
        0x33 => match (funct3, funct7) {
            (0, 0x00) => format!("add {}, {}, {}", reg_name(rd), reg_name(rs1), reg_name(rs2)),
            (0, 0x20) => format!("sub {}, {}, {}", reg_name(rd), reg_name(rs1), reg_name(rs2)),
            (1, 0x00) => format!("sll {}, {}, {}", reg_name(rd), reg_name(rs1), reg_name(rs2)),
            (2, 0x00) => format!("slt {}, {}, {}", reg_name(rd), reg_name(rs1), reg_name(rs2)),
            (4, 0x00) => format!("xor {}, {}, {}", reg_name(rd), reg_name(rs1), reg_name(rs2)),
            (6, 0x00) => format!("or {}, {}, {}", reg_name(rd), reg_name(rs1), reg_name(rs2)),
            (7, 0x00) => format!("and {}, {}, {}", reg_name(rd), reg_name(rs1), reg_name(rs2)),
            _ => format!("op f3={}, f7={} {}, {}, {}", funct3, funct7, reg_name(rd), reg_name(rs1), reg_name(rs2)),
        },
        0x03 => {
            let imm = (word as i32) >> 20;
            let mnem = match funct3 {
                0 => "lb", 1 => "lh", 2 => "lw", 4 => "lbu", 5 => "lhu",
                _ => "load",
            };
            format!("{} {}, {}({})", mnem, reg_name(rd), imm, reg_name(rs1))
        }
        0x23 => {
            let imm = (((word as i32) >> 25) << 5) | (((word >> 7) & 0x1F) as i32);
            let mnem = match funct3 {
                0 => "sb", 1 => "sh", 2 => "sw",
                _ => "store",
            };
            format!("{} {}, {}({})", mnem, reg_name(rs2), imm, reg_name(rs1))
        }
        0x63 => {
            let mnem = match funct3 {
                0 => "beq", 1 => "bne", 4 => "blt", 5 => "bge", 6 => "bltu", 7 => "bgeu",
                _ => "branch",
            };
            format!("{} {}, {}, <target>", mnem, reg_name(rs1), reg_name(rs2))
        }
        0x6F => format!("jal {}, <target>", reg_name(rd)),
        0x67 => format!("jalr {}, {}({})", reg_name(rd), (word as i32) >> 20, reg_name(rs1)),
        0x37 => format!("lui {}, {:#06X}", reg_name(rd), word >> 12),
        0x17 => format!("auipc {}, {:#06X}", reg_name(rd), word >> 12),
        0x73 => match word {
            0x00000073 => "ecall".to_string(),
            0x00100073 => "ebreak".to_string(),
            _ => format!("system {:#010X}", word),
        },
        _ => format!(".word {:#010X}", word),
    }
}

fn disassemble_arm32(b0: u8, b1: u8, b2: u8, b3: u8) -> String {
    let word = (b0 as u32) | ((b1 as u32) << 8) | ((b2 as u32) << 16) | ((b3 as u32) << 24);
    if word == 0xE1A00000 {
        return "nop".to_string();
    }
    if (word & 0x0F000000) == 0x0F000000 {
        return format!("svc {:#08X}", word & 0x00FFFFFF);
    }
    if (word & 0x0F000000) == 0x0A000000 {
        return format!("b {:#08X}", (word & 0x00FFFFFF) << 2);
    }
    if (word & 0x0F000000) == 0x0B000000 {
        return format!("bl {:#08X}", (word & 0x00FFFFFF) << 2);
    }
    if (word & 0x0FFFFFF0) == 0x012FFF10 {
        return format!("bx r{}", word & 0x0F);
    }
    if (word & 0x0FE00000) == 0x02800000 {
        let rd = (word >> 12) & 0x0F;
        let rn = (word >> 16) & 0x0F;
        let imm = word & 0xFF;
        return format!("add r{}, r{}, #{:#04X}", rd, rn, imm);
    }
    if (word & 0x0FE00000) == 0x02400000 {
        let rd = (word >> 12) & 0x0F;
        let rn = (word >> 16) & 0x0F;
        let imm = word & 0xFF;
        return format!("sub r{}, r{}, #{:#04X}", rd, rn, imm);
    }
    if (word & 0x0FE00000) == 0x03A00000 {
        let rd = (word >> 12) & 0x0F;
        let imm = word & 0xFF;
        return format!("mov r{}, #{:#04X}", rd, imm);
    }
    format!("arm32 {:#010X}", word)
}

fn disassemble_arm64(b0: u8, b1: u8, b2: u8, b3: u8) -> String {
    let word = (b0 as u32) | ((b1 as u32) << 8) | ((b2 as u32) << 16) | ((b3 as u32) << 24);
    if word == 0xD503201F {
        return "nop".to_string();
    }
    if word == 0xD65F03C0 {
        return "ret".to_string();
    }
    if (word & 0xFF000000) == 0xD4000000 {
        return format!("svc #{:#06X}", (word >> 5) & 0xFFFF);
    }
    if (word & 0x7F800000) == 0x11000000 {
        let rd = word & 0x1F;
        let rn = (word >> 5) & 0x1F;
        let imm = (word >> 10) & 0x0FFF;
        return format!("add w{}, w{}, #{:#X}", rd, rn, imm);
    }
    if (word & 0x7F800000) == 0x51000000 {
        let rd = word & 0x1F;
        let rn = (word >> 5) & 0x1F;
        let imm = (word >> 10) & 0x0FFF;
        return format!("sub w{}, w{}, #{:#X}", rd, rn, imm);
    }
    if (word & 0x7F800000) == 0x52800000 {
        let rd = word & 0x1F;
        let imm = (word >> 5) & 0xFFFF;
        return format!("movz w{}, #{:#06X}", rd, imm);
    }
    format!("aarch64 {:#010X}", word)
}

fn disassemble_mips(b0: u8, b1: u8, b2: u8, b3: u8) -> String {
    let word = ((b0 as u32) << 24) | ((b1 as u32) << 16) | ((b2 as u32) << 8) | (b3 as u32);
    if word == 0 {
        return "nop".to_string();
    }
    let opcode = (word >> 26) & 0x3F;
    let rs = (word >> 21) & 0x1F;
    let rt = (word >> 16) & 0x1F;
    let imm = (word & 0xFFFF) as i16;

    match opcode {
        0x08 | 0x09 => format!("addi ${}, ${}, {}", rt, rs, imm),
        0x0F => format!("lui ${}, {:#06X}", rt, imm as u16),
        0x23 => format!("lw ${}, {}(${})", rt, imm, rs),
        0x2B => format!("sw ${}, {}(${})", rt, imm, rs),
        0x02 => format!("j {:#010X}", (word & 0x03FFFFFF) << 2),
        0x03 => format!("jal {:#010X}", (word & 0x03FFFFFF) << 2),
        0x04 => format!("beq ${}, ${}, {}", rs, rt, imm),
        0x05 => format!("bne ${}, ${}, {}", rs, rt, imm),
        _ => format!("mips {:#010X}", word),
    }
}

fn disassemble_powerpc(b0: u8, b1: u8, b2: u8, b3: u8) -> String {
    let word = ((b0 as u32) << 24) | ((b1 as u32) << 16) | ((b2 as u32) << 8) | (b3 as u32);
    if word == 0x60000000 {
        return "nop".to_string();
    }
    if word == 0x4E800020 {
        return "blr".to_string();
    }
    let op = (word >> 26) & 0x3F;
    let rd = (word >> 21) & 0x1F;
    let ra = (word >> 16) & 0x1F;
    let imm = (word & 0xFFFF) as i16;

    match op {
        14 => {
            if ra == 0 {
                format!("li r{}, {}", rd, imm)
            } else {
                format!("addi r{}, r{}, {}", rd, ra, imm)
            }
        }
        15 => format!("lis r{}, {:#06X}", rd, imm as u16),
        32 => format!("lwz r{}, {}(r{})", rd, imm, ra),
        36 => format!("stw r{}, {}(r{})", rd, imm, ra),
        18 => format!("b {:#010X}", word & 0x03FFFFFC),
        _ => format!("ppc {:#010X}", word),
    }
}

fn disassemble_sparc(b0: u8, b1: u8, b2: u8, b3: u8) -> String {
    let word = ((b0 as u32) << 24) | ((b1 as u32) << 16) | ((b2 as u32) << 8) | (b3 as u32);
    if word == 0x01000000 {
        return "nop".to_string();
    }
    if word == 0x81C7E008 {
        return "retl".to_string();
    }
    if word == 0x81C3E008 {
        return "ret".to_string();
    }
    format!("sparc {:#010X}", word)
}

fn disassemble_m68k(bytes: &[u8]) -> String {
    let b0 = bytes.first().copied().unwrap_or(0);
    let b1 = bytes.get(1).copied().unwrap_or(0);
    let word = ((b0 as u16) << 8) | (b1 as u16);

    match word {
        0x4E71 => "nop".to_string(),
        0x4E75 => "rts".to_string(),
        _ if (word & 0xF100) == 0x7000 => {
            let reg = (word >> 9) & 0x07;
            let val = (word & 0xFF) as i8;
            format!("moveq #{}, d{}", val, reg)
        }
        _ if (word & 0xFFF0) == 0x4E40 => {
            let vec = word & 0x0F;
            format!("trap #{}", vec)
        }
        _ => format!("dc.w ${:04X}", word),
    }
}

fn disassemble_superh(b0: u8, b1: u8) -> String {
    let word = ((b0 as u16) << 8) | (b1 as u16);
    match word {
        0x0009 => "nop".to_string(),
        0x000B => "rts".to_string(),
        _ if (word & 0xF000) == 0xE000 => {
            let rn = (word >> 8) & 0x0F;
            let imm = (word & 0xFF) as i8;
            format!("mov #{}, r{}", imm, rn)
        }
        _ => format!("sh {:#06X}", word),
    }
}

fn disassemble_parisc(b0: u8, b1: u8, b2: u8, b3: u8) -> String {
    let word = ((b0 as u32) << 24) | ((b1 as u32) << 16) | ((b2 as u32) << 8) | (b3 as u32);
    if word == 0x08000240 {
        return "nop".to_string();
    }
    if word == 0xE840C000 {
        return "bv 0(r2)".to_string();
    }
    format!("parisc {:#010X}", word)
}

fn disassemble_alpha(b0: u8, b1: u8, b2: u8, b3: u8) -> String {
    let word = (b0 as u32) | ((b1 as u32) << 8) | ((b2 as u32) << 16) | ((b3 as u32) << 24);
    if word == 0x47FF041F {
        return "nop".to_string();
    }
    if word == 0x6BFA8001 {
        return "ret".to_string();
    }
    format!("alpha {:#010X}", word)
}
