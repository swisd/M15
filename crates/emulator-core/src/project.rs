//! Project configuration, `mconfig.toml` parsing, architecture autodetection,
//! and multi-architecture assembly loading.

#[cfg(feature = "alloc")]
use alloc::{
    collections::{BTreeMap, BTreeSet},
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::arch::{AnyCpu, Architecture};
use crate::bus::{DynamicMemory, MemoryBus};
use crate::cpu::CpuEngine;
use crate::types::Endianness;

/// Represents a parsed project configuration from `mconfig.toml`.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectConfig {
    /// Human-readable project name.
    pub name: String,
    /// Target CPU architecture.
    pub arch: Option<Architecture>,
    /// Project description or author notes.
    pub description: Option<String>,
    /// Program execution entry point (PC).
    pub entry_point: Option<u64>,
    /// Initial stack pointer (SP).
    pub sp: Option<u64>,
    /// Main entry source file (e.g. `main.asm`).
    pub main: Option<String>,
    /// List of source files included in the project.
    pub files: Vec<String>,
    /// Memory size in bytes.
    pub memory_size: Option<usize>,
    /// Base origin address for assembly loading.
    pub org: Option<u64>,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: "New Project".to_string(),
            arch: None,
            description: None,
            entry_point: None,
            sp: None,
            main: None,
            files: Vec::new(),
            memory_size: None,
            org: None,
        }
    }
}

/// Errors that can occur when parsing or loading project configs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectError {
    ParseError(String),
    ArchDetectionFailed(String),
    AmbiguousArch(Vec<Architecture>),
    FileNotFound(String),
    AssembleError(String),
}

impl core::fmt::Display for ProjectError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ProjectError::ParseError(msg) => write!(f, "Configuration parse error: {}", msg),
            ProjectError::ArchDetectionFailed(msg) => write!(f, "Architecture detection failed: {}", msg),
            ProjectError::AmbiguousArch(archs) => {
                write!(f, "Ambiguous architecture match between {} candidate(s)", archs.len())
            }
            ProjectError::FileNotFound(msg) => write!(f, "File not found: {}", msg),
            ProjectError::AssembleError(msg) => write!(f, "Assembly error: {}", msg),
        }
    }
}

impl ProjectConfig {
    /// Parse `mconfig.toml` text into a `ProjectConfig`.
    pub fn parse(toml_text: &str) -> Result<Self, ProjectError> {
        let mut config = ProjectConfig::default();

        let mut current_section = "";

        for raw_line in toml_text.lines() {
            let mut line = raw_line.trim();
            if let Some(comment_idx) = line.find('#') {
                line = line[..comment_idx].trim();
            }
            if line.is_empty() {
                continue;
            }

            if line.starts_with('[') && line.ends_with(']') {
                current_section = line[1..line.len() - 1].trim();
                continue;
            }

            if let Some((key, value)) = line.split_once('=') {
                let key = key.trim().to_ascii_lowercase();
                let value = value.trim();

                match key.as_str() {
                    "name" | "project_name" => {
                        config.name = strip_quotes(value).to_string();
                    }
                    "arch" | "architecture" | "target_arch" | "cpu" => {
                        let arch_str = strip_quotes(value);
                        config.arch = Architecture::from_name(arch_str);
                        if config.arch.is_none() {
                            return Err(ProjectError::ParseError(format!(
                                "Unknown architecture in mconfig.toml: '{}'",
                                arch_str
                            )));
                        }
                    }
                    "desc" | "description" => {
                        config.description = Some(strip_quotes(value).to_string());
                    }
                    "entry_point" | "entry" | "pc" | "start" => {
                        config.entry_point = parse_num_literal(value);
                    }
                    "sp" | "stack_pointer" | "stack" => {
                        config.sp = parse_num_literal(value);
                    }
                    "main" | "source" | "main_file" => {
                        config.main = Some(strip_quotes(value).to_string());
                    }
                    "org" | "origin" | "base" => {
                        config.org = parse_num_literal(value);
                    }
                    "memory_size" | "ram_size" | "mem_size" => {
                        config.memory_size = parse_num_literal(value).map(|n| n as usize);
                    }
                    "files" | "sources" => {
                        config.files = parse_string_array(value);
                    }
                    _ => {
                        // Section-scoped properties
                        if current_section == "project" || current_section == "package" || current_section == "target" {
                            // Already handled top-level or section keys
                        }
                    }
                }
            }
        }

        Ok(config)
    }

    /// Generate formatted `mconfig.toml` text representation.
    pub fn to_toml_string(&self) -> String {
        let mut out = String::new();
        out.push_str("[project]\n");
        out.push_str(&format!("name = \"{}\"\n", self.name));
        if let Some(arch) = self.arch {
            out.push_str(&format!("arch = \"{}\"\n", arch.name()));
        }
        if let Some(ref desc) = self.description {
            out.push_str(&format!("description = \"{}\"\n", desc));
        }
        if let Some(entry) = self.entry_point {
            out.push_str(&format!("entry_point = \"{:#06X}\"\n", entry));
        }
        if let Some(sp) = self.sp {
            out.push_str(&format!("sp = \"{:#06X}\"\n", sp));
        }
        if let Some(ref main) = self.main {
            out.push_str(&format!("main = \"{}\"\n", main));
        }
        if !self.files.is_empty() {
            out.push_str("files = [");
            for (i, f) in self.files.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("\"{}\"", f));
            }
            out.push_str("]\n");
        }
        out
    }
}

/// Helper to parse decimal or hex string (`"0x1000"` / `"$1000"` / `1000`).
pub fn parse_num_literal(s: &str) -> Option<u64> {
    let clean = strip_quotes(s.trim());
    if let Some(rest) = clean.strip_prefix("0x").or_else(|| clean.strip_prefix("0X")) {
        u64::from_str_radix(rest.trim_start_matches('_'), 16).ok()
    } else if let Some(rest) = clean.strip_prefix('$') {
        u64::from_str_radix(rest.trim_start_matches('_'), 16).ok()
    } else if let Some(rest) = clean.strip_prefix("0b").or_else(|| clean.strip_prefix("0B")) {
        u64::from_str_radix(rest.trim_start_matches('_'), 2).ok()
    } else if clean.ends_with('h') || clean.ends_with('H') {
        let hex_part = &clean[..clean.len() - 1];
        u64::from_str_radix(hex_part, 16).ok()
    } else {
        clean.parse::<u64>().ok()
    }
}

fn strip_quotes(s: &str) -> &str {
    let trimmed = s.trim();
    if (trimmed.starts_with('"') && trimmed.ends_with('"'))
        || (trimmed.starts_with('\'') && trimmed.ends_with('\''))
    {
        if trimmed.len() >= 2 {
            &trimmed[1..trimmed.len() - 1]
        } else {
            ""
        }
    } else {
        trimmed
    }
}

fn parse_string_array(s: &str) -> Vec<String> {
    let trimmed = s.trim();
    if !trimmed.starts_with('[') || !trimmed.ends_with(']') {
        return Vec::new();
    }
    let inner = &trimmed[1..trimmed.len() - 1];
    let mut results = Vec::new();
    for part in inner.split(',') {
        let item = strip_quotes(part.trim());
        if !item.is_empty() {
            results.push(item.to_string());
        }
    }
    results
}

/// Result of architecture autodetection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DetectionResult {
    /// Distinct high-confidence detected architecture.
    Detected(Architecture),
    /// Multiple possible architectures tied with high score.
    Ambiguous(Vec<Architecture>),
    /// Unable to identify target architecture from content.
    Unknown,
}

/// Analyzes source code and optional filename to automatically detect CPU architecture.
pub fn detect_architecture(filename: Option<&str>, content: &str) -> DetectionResult {
    // 1. Direct explicit directives in source
    for line in content.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        // e.g. .arch <name>, ; arch: <name>, # arch: <name>, // arch: <name>
        if let Some(rest) = lower
            .strip_prefix(".arch")
            .or_else(|| lower.strip_prefix("; arch:"))
            .or_else(|| lower.strip_prefix("// arch:"))
            .or_else(|| lower.strip_prefix("# arch:"))
            .or_else(|| lower.strip_prefix("; arch ="))
            .or_else(|| lower.strip_prefix("// arch ="))
            .or_else(|| lower.strip_prefix("# arch ="))
        {
            let name = rest.trim().trim_matches(|c| c == ':' || c == '"' || c == '\'' || c == '=');
            if let Some(arch) = Architecture::from_name(name.trim()) {
                return DetectionResult::Detected(arch);
            }
        }
        if lower.starts_with("processor 6502") || lower.starts_with(".processor 6502") {
            return DetectionResult::Detected(Architecture::Mos6502);
        }
        if lower.starts_with(".device atmega") || lower.contains("m328pdef.inc") {
            return DetectionResult::Detected(Architecture::Avr);
        }
        if lower.starts_with(".machine \"powerpc\"") || lower.starts_with(".machine ppc") {
            return DetectionResult::Detected(Architecture::PowerPc);
        }
    }

    // 2. Extension-based hinting
    if let Some(fname) = filename {
        let lower_fname = fname.to_ascii_lowercase();
        if lower_fname.ends_with(".rv32") || lower_fname.ends_with(".rv64") || lower_fname.ends_with(".riscv") {
            return DetectionResult::Detected(Architecture::RiscV);
        }
        if lower_fname.ends_with(".s86") || lower_fname.ends_with(".a86") || lower_fname.ends_with(".8086") {
            return DetectionResult::Detected(Architecture::I8086);
        }
        if lower_fname.ends_with(".s65") || lower_fname.ends_with(".a65") || lower_fname.ends_with(".6502") {
            return DetectionResult::Detected(Architecture::Mos6502);
        }
        if lower_fname.ends_with(".avr") || lower_fname.ends_with(".atmega") {
            return DetectionResult::Detected(Architecture::Avr);
        }
        if lower_fname.ends_with(".m68k") || lower_fname.ends_with(".s68") || lower_fname.ends_with(".68k") {
            return DetectionResult::Detected(Architecture::Motorola68000);
        }
        if lower_fname.ends_with(".ia64") {
            return DetectionResult::Detected(Architecture::Ia64);
        }
    }

    // 3. Keyword and token pattern scoring
    let mut scores = [0i32; 16];

    // Convert source to lowercase words and tokens
    let text_lower = content.to_ascii_lowercase();
    let tokens: BTreeSet<&str> = text_lower
        .split(|c: char| !c.is_alphanumeric() && c != '$' && c != '%' && c != '.' && c != '_')
        .filter(|t| !t.is_empty())
        .collect();

    // Helper to add score to an architecture
    let mut add_score = |arch: Architecture, points: i32| {
        if let Some(idx) = Architecture::ALL.iter().position(|&a| a == arch) {
            scores[idx] += points;
        }
    };

    // Check specific distinct syntax signatures:
    if text_lower.contains(".code16") || text_lower.contains("[bits 16]") || text_lower.contains("bits 16") {
        add_score(Architecture::I8086, 20);
    }
    if text_lower.contains(".code32") || text_lower.contains("[bits 32]") || text_lower.contains("bits 32") {
        add_score(Architecture::X86, 20);
    }
    if text_lower.contains(".code64") || text_lower.contains("[bits 64]") || text_lower.contains("bits 64") {
        add_score(Architecture::X86_64, 20);
    }

    // MOS 6502 signature:
    let mos6502_tokens = ["lda", "sta", "ldx", "stx", "ldy", "sty", "tax", "tay", "txa", "tya", "tsx", "txs", "pha", "pla", "php", "plp", "rts", "rti", "clc", "sec", "cli", "sei", "cld", "sed", "brk", "bpl", "bmi", "bvc", "bvs", "bcc", "bcs"];
    for tok in mos6502_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::Mos6502, 3);
        }
    }
    if text_lower.contains("$0100") || text_lower.contains("#$") {
        add_score(Architecture::Mos6502, 4);
    }

    // AVR signature:
    let avr_tokens = ["ldi", "sreg", "spl", "sph", "rjmp", "rcall", "sbi", "cbi", "sbrs", "sbrc", "brne", "breq", "brcs", "brcc", "lpm", "spm", "r16", "r17", "r18", "r19", "r20", "r24", "r25", "r26", "r27", "r28", "r29", "r30", "r31"];
    for tok in avr_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::Avr, 3);
        }
    }

    // 8086 signature:
    let i8086_tokens = ["mov", "ax", "bx", "cx", "dx", "si", "di", "bp", "sp", "hlt", "int", "iret", "loop", "loopz", "loopnz", "pushf", "popf", "cs", "ds", "ss", "es"];
    for tok in i8086_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::I8086, 2);
        }
    }
    if text_lower.contains("int 21h") || text_lower.contains("int 10h") || text_lower.contains("mov ax") || text_lower.contains("mov bx") {
        add_score(Architecture::I8086, 6);
    }

    // X86 (IA-32) signature:
    let x86_tokens = ["eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp", "pushfd", "popfd", "sysenter", "sysexit"];
    for tok in x86_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::X86, 4);
        }
    }

    // X86_64 signature:
    let x86_64_tokens = ["rax", "rbx", "rcx", "rdx", "rsi", "rdi", "rsp", "rbp", "r8", "r9", "r10", "r11", "r12", "r13", "r14", "r15", "rip", "syscall", "sysret", "cqo", "cdqe"];
    for tok in x86_64_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::X86_64, 4);
        }
    }

    // RISC-V signature:
    let riscv_tokens = ["addi", "slli", "slti", "sltiu", "xori", "srli", "srai", "ori", "andi", "auipc", "lui", "jal", "jalr", "ecall", "ebreak", "wfi", "x0", "x1", "x2", "x3", "x4", "x5", "x6", "x7", "x8", "x9", "x10", "x11", "x12", "x13", "x14", "x15", "zero", "ra", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2"];
    for tok in riscv_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::RiscV, 3);
        }
    }

    // ARM32 signature:
    let arm32_tokens = ["cpsr", "spsr", "stmfd", "ldmfd", "bx", "blx", "push", "pop"];
    for tok in arm32_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::Arm32, 2);
        }
    }
    if text_lower.contains("bx lr") || text_lower.contains("push {") || text_lower.contains("pop {") {
        add_score(Architecture::Arm32, 8);
    }

    // ARM64 signature:
    let arm64_tokens = ["w0", "w1", "w2", "xzr", "wzr", "stp", "ldp", "adrp", "cbnz", "cbz", "sp_el0", "elr_el1"];
    for tok in arm64_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::Arm64, 4);
        }
    }

    // IA-64 signature:
    let ia64_tokens = ["alloc", "br.call", "br.ret", "mov.m", "flushrs", "bsp", "r127", "p63", "b0", "b1", ".mii", ".mmi", ".stop"];
    for tok in ia64_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::Ia64, 5);
        }
    }

    // MIPS signature:
    let mips_tokens = ["$zero", "$at", "$v0", "$v1", "$a0", "$a1", "$t0", "$t1", "$s0", "$s1", "$ra", "$gp", "$sp", "$fp", "addu", "subu", "jr"];
    for tok in mips_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::Mips, 4);
        }
    }

    // PowerPC signature:
    let ppc_tokens = ["mtlr", "mflr", "mtctr", "mfctr", "stwu", "lwz", "blr", "bctr", "cr0", "cr1", "rlwinm"];
    for tok in ppc_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::PowerPc, 4);
        }
    }

    // SPARC signature:
    let sparc_tokens = ["%g0", "%g1", "%o0", "%o1", "%l0", "%l1", "%i0", "%i1", "%psr", "sethi", "save", "restore"];
    for tok in sparc_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::Sparc, 5);
        }
    }

    // SuperH signature:
    let sh_tokens = ["mov.l", "mov.w", "mov.b", "clrmac", "sts.l", "lds.l", "gbr", "vbr", "macl", "mach"];
    for tok in sh_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::SuperH, 4);
        }
    }

    // PA-RISC signature:
    let parisc_tokens = ["b,l", "bv,n", "ldo", "%sr0", "%sr1", "%r30", "%r31", "comb", "comib"];
    for tok in parisc_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::PaRisc, 5);
        }
    }

    // DEC Alpha signature:
    let alpha_tokens = ["ldq", "stq", "ldah", "cmpeq", "cmpult", "zapnot", "$r30", "$r31", "$ra"];
    for tok in alpha_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::DecAlpha, 4);
        }
    }

    // Motorola 68000 signature:
    let m68k_tokens = ["movea", "movem", "moveq", "link", "unlk", "dbra", "pea", "d0", "d1", "d2", "a0", "a1", "a6", "a7"];
    for tok in m68k_tokens {
        if tokens.contains(tok) {
            add_score(Architecture::Motorola68000, 3);
        }
    }

    // Find highest score
    let mut max_score = 0;
    let mut top_candidates = Vec::new();

    for (idx, &score) in scores.iter().enumerate() {
        if score > max_score {
            max_score = score;
            top_candidates.clear();
            top_candidates.push(Architecture::ALL[idx]);
        } else if score == max_score && score > 0 {
            top_candidates.push(Architecture::ALL[idx]);
        }
    }

    if max_score >= 4 {
        if top_candidates.len() == 1 {
            DetectionResult::Detected(top_candidates[0])
        } else {
            DetectionResult::Ambiguous(top_candidates)
        }
    } else {
        DetectionResult::Unknown
    }
}

/// Assembly load artifact containing assembled bytes, entry point, initial SP, and symbols.
#[derive(Clone, Debug, PartialEq)]
pub struct AsmProgram {
    pub arch: Architecture,
    pub entry_point: u64,
    pub initial_sp: u64,
    pub chunks: Vec<(u64, Vec<u8>)>, // (address, bytes)
    pub labels: BTreeMap<String, u64>,
}

impl AsmProgram {
    /// Loads this program directly into the memory bus and sets up CPU initial registers.
    pub fn load_into(&self, cpu: &mut AnyCpu, bus: &mut DynamicMemory) {
        cpu.reset();
        for (addr, bytes) in &self.chunks {
            for (offset, &b) in bytes.iter().enumerate() {
                let _ = bus.write_u8(addr.wrapping_add(offset as u64), b);
            }
        }
        cpu.set_pc(self.entry_point);
        cpu.set_sp(self.initial_sp);
    }
}

/// Assembles assembly source text into an `AsmProgram` for a specific target architecture.
pub fn assemble_source(
    arch: Architecture,
    source: &str,
    default_entry: Option<u64>,
    default_sp: Option<u64>,
) -> Result<AsmProgram, ProjectError> {
    let mut current_addr: u64 = default_entry.unwrap_or(match arch {
        Architecture::Mos6502 => 0x0600,
        Architecture::Avr => 0x0000,
        Architecture::I8086 => 0x1000,
        Architecture::X86 => 0x00400000,
        Architecture::X86_64 => 0x00400000,
        _ => 0x1000,
    });

    let default_stack: u64 = default_sp.unwrap_or(match arch {
        Architecture::Mos6502 => 0x01FF,
        Architecture::Avr => 0x08FF,
        Architecture::I8086 => 0xFFF8,
        Architecture::X86 => 0x0007FFF0,
        Architecture::X86_64 => 0x00080000,
        Architecture::Arm32 => 0x00020000,
        Architecture::Arm64 => 0x00070000,
        Architecture::RiscV => 0x00010000,
        Architecture::PaRisc => 0x00001000,
        _ => 0x00010000,
    });

    let mut entry_point = current_addr;
    let initial_sp = default_stack;
    let mut labels = BTreeMap::new();
    let mut chunks: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut current_chunk: (u64, Vec<u8>) = (current_addr, Vec::new());

    // First pass: collect labels and directives
    for line in source.lines() {
        let mut text = line.trim();
        // Remove comments: ';' and '//' are always comments.
        if let Some(idx) = text.find(';').or_else(|| text.find("//")) {
            text = text[..idx].trim();
        }
        // '#' is a comment only if it's not an immediate operand (e.g. not #$12, #0x12, #10)
        if let Some(idx) = text.find('#') {
            let after = &text[idx + 1..];
            let is_immediate = after.starts_with('$')
                || after.starts_with('%')
                || after.starts_with('0')
                || after.starts_with('1')
                || after.starts_with('2')
                || after.starts_with('3')
                || after.starts_with('4')
                || after.starts_with('5')
                || after.starts_with('6')
                || after.starts_with('7')
                || after.starts_with('8')
                || after.starts_with('9')
                || after.starts_with('-')
                || after.starts_with('+')
                || after.starts_with('\'')
                || after.starts_with('"')
                || after.starts_with('<')
                || after.starts_with('>');
            if !is_immediate {
                text = text[..idx].trim();
            }
        }
        if text.is_empty() {
            continue;
        }

        // Handle label definition
        if let Some((label_name, rest)) = text.split_once(':') {
            let label = label_name.trim();
            if !label.is_empty() {
                labels.insert(label.to_string(), current_addr + current_chunk.1.len() as u64);
            }
            text = rest.trim();
            if text.is_empty() {
                continue;
            }
        }

        // Parse directives
        let lower = text.to_ascii_lowercase();
        if lower.starts_with(".org") || lower.starts_with("org ") {
            let val_str = lower.trim_start_matches(".org").trim_start_matches("org").trim();
            if let Some(addr) = parse_num_literal(val_str) {
                if !current_chunk.1.is_empty() {
                    chunks.push(current_chunk);
                }
                current_addr = addr;
                current_chunk = (current_addr, Vec::new());
                if default_entry.is_none() {
                    entry_point = addr;
                }
                continue;
            }
        } else if lower.starts_with(".entry") || lower.starts_with("entry ") {
            let val_str = lower.trim_start_matches(".entry").trim_start_matches("entry").trim();
            if let Some(addr) = parse_num_literal(val_str) {
                entry_point = addr;
                continue;
            }
        } else if lower.starts_with(".byte") || lower.starts_with("db ") {
            let val_str = if lower.starts_with(".byte") { &text[5..] } else { &text[3..] };
            for part in val_str.split(',') {
                let p = part.trim();
                if (p.starts_with('"') && p.ends_with('"')) || (p.starts_with('\'') && p.ends_with('\'')) {
                    let s = strip_quotes(p);
                    current_chunk.1.extend_from_slice(s.as_bytes());
                } else if let Some(n) = parse_num_literal(p) {
                    current_chunk.1.push(n as u8);
                }
            }
            continue;
        } else if lower.starts_with(".word") || lower.starts_with("dw ") {
            let val_str = if lower.starts_with(".word") { &text[5..] } else { &text[3..] };
            for part in val_str.split(',') {
                if let Some(n) = parse_num_literal(part.trim()) {
                    let w = n as u16;
                    if arch.endianness() == Endianness::LittleEndian {
                        current_chunk.1.extend_from_slice(&w.to_le_bytes());
                    } else {
                        current_chunk.1.extend_from_slice(&w.to_be_bytes());
                    }
                }
            }
            continue;
        } else if lower.starts_with(".dword") || lower.starts_with("dd ") {
            let val_str = if lower.starts_with(".dword") { &text[6..] } else { &text[3..] };
            for part in val_str.split(',') {
                if let Some(n) = parse_num_literal(part.trim()) {
                    let dw = n as u32;
                    if arch.endianness() == Endianness::LittleEndian {
                        current_chunk.1.extend_from_slice(&dw.to_le_bytes());
                    } else {
                        current_chunk.1.extend_from_slice(&dw.to_be_bytes());
                    }
                }
            }
            continue;
        } else if lower.starts_with(".qword") || lower.starts_with("dq ") {
            let val_str = if lower.starts_with(".qword") { &text[6..] } else { &text[3..] };
            for part in val_str.split(',') {
                if let Some(n) = parse_num_literal(part.trim()) {
                    let qw = n;
                    if arch.endianness() == Endianness::LittleEndian {
                        current_chunk.1.extend_from_slice(&qw.to_le_bytes());
                    } else {
                        current_chunk.1.extend_from_slice(&qw.to_be_bytes());
                    }
                }
            }
            continue;
        } else if lower.starts_with(".asciiz") || lower.starts_with(".string") {
            let val_str = &text[7..];
            let s = strip_quotes(val_str.trim());
            current_chunk.1.extend_from_slice(s.as_bytes());
            current_chunk.1.push(0);
            continue;
        } else if lower.starts_with(".ascii") {
            let s = strip_quotes(text[6..].trim());
            current_chunk.1.extend_from_slice(s.as_bytes());
            continue;
        } else if lower.starts_with(".arch")
            || lower.starts_with(".global")
            || lower.starts_with(".globl")
            || lower.starts_with(".device")
            || lower.starts_with(".processor")
            || lower.starts_with("processor")
            || lower.starts_with(".include")
            || lower.starts_with(".syntax")
            || lower.starts_with(".code16")
            || lower.starts_with(".code32")
            || lower.starts_with(".code64")
            || lower.starts_with("bits 16")
            || lower.starts_with("bits 32")
            || lower.starts_with("bits 64")
            || lower.starts_with("[bits")
            || lower.starts_with(".text")
            || lower.starts_with(".data")
            || lower.starts_with(".bss")
            || lower.starts_with(".section")
            || lower.starts_with(".align")
            || lower.starts_with(".balign")
            || lower.starts_with(".p2align")
            || lower.starts_with(".type")
            || lower.starts_with(".size")
            || lower.starts_with(".set")
            || lower.starts_with(".option")
        {
            continue;
        }

        // Instruction encoding based on architecture
        let bytes = encode_instruction(arch, text, current_addr + current_chunk.1.len() as u64, &labels)?;
        current_chunk.1.extend_from_slice(&bytes);
    }

    if !current_chunk.1.is_empty() {
        chunks.push(current_chunk);
    }

    Ok(AsmProgram {
        arch,
        entry_point,
        initial_sp,
        chunks,
        labels,
    })
}

/// Encodes a single instruction mnemonic and operands into binary machine code.
fn encode_instruction(
    arch: Architecture,
    line: &str,
    pc: u64,
    labels: &BTreeMap<String, u64>,
) -> Result<Vec<u8>, ProjectError> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.is_empty() {
        return Ok(Vec::new());
    }

    let mnemonic = parts[0].to_ascii_lowercase();
    let operand_str = if line.len() > parts[0].len() {
        line[parts[0].len()..].trim()
    } else {
        ""
    };

    match arch {
        Architecture::Mos6502 => encode_mos6502(&mnemonic, operand_str, pc, labels),
        Architecture::I8086 => encode_i8086(&mnemonic, operand_str, pc, labels),
        Architecture::RiscV => encode_riscv(&mnemonic, operand_str, pc, labels),
        Architecture::Avr => encode_avr(&mnemonic, operand_str, pc, labels),
        _ => {
            // Generic fallback for architectures without full assembler: emit NOP or hex bytes
            if mnemonic == "nop" {
                match arch.word_size() {
                    crate::types::WordSize::Bytes1 => Ok(vec![0x00]),
                    crate::types::WordSize::Bytes2 => Ok(vec![0x90, 0x90]),
                    crate::types::WordSize::Bytes4 => Ok(vec![0x00, 0x00, 0x00, 0x00]),
                    crate::types::WordSize::Bytes8 => Ok(vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]),
                }
            } else if let Some(hex_val) = parse_num_literal(&mnemonic) {
                Ok(vec![hex_val as u8])
            } else {
                // If it's a raw hex string or instruction we don't encode, emit 4 NOP bytes or fail
                Ok(vec![0x00, 0x00, 0x00, 0x00])
            }
        }
    }
}

fn encode_mos6502(
    mnemonic: &str,
    operands: &str,
    pc: u64,
    labels: &BTreeMap<String, u64>,
) -> Result<Vec<u8>, ProjectError> {
    match mnemonic {
        "nop" => Ok(vec![0xEA]),
        "tax" => Ok(vec![0xAA]),
        "tay" => Ok(vec![0xA8]),
        "txa" => Ok(vec![0x8A]),
        "tya" => Ok(vec![0x98]),
        "tsx" => Ok(vec![0xBA]),
        "txs" => Ok(vec![0x9A]),
        "pha" => Ok(vec![0x48]),
        "pla" => Ok(vec![0x68]),
        "php" => Ok(vec![0x08]),
        "plp" => Ok(vec![0x28]),
        "clc" => Ok(vec![0x18]),
        "sec" => Ok(vec![0x38]),
        "cli" => Ok(vec![0x58]),
        "sei" => Ok(vec![0x78]),
        "cld" => Ok(vec![0xD8]),
        "sed" => Ok(vec![0xF8]),
        "clv" => Ok(vec![0xB8]),
        "rts" => Ok(vec![0x60]),
        "rti" => Ok(vec![0x40]),
        "brk" => Ok(vec![0x00]),
        "inx" => Ok(vec![0xE8]),
        "dex" => Ok(vec![0xCA]),
        "iny" => Ok(vec![0xC8]),
        "dey" => Ok(vec![0x88]),

        "lda" => encode_6502_alu(0xA9, 0xA5, 0xAD, operands, labels),
        "ldx" => encode_6502_alu(0xA2, 0xA6, 0xAE, operands, labels),
        "ldy" => encode_6502_alu(0xA0, 0xA4, 0xAC, operands, labels),
        "sta" => encode_6502_mem(0x85, 0x8D, operands, labels),
        "stx" => encode_6502_mem(0x86, 0x8E, operands, labels),
        "sty" => encode_6502_mem(0x84, 0x8C, operands, labels),
        "adc" => encode_6502_alu(0x69, 0x65, 0x6D, operands, labels),
        "sbc" => encode_6502_alu(0xE9, 0xE5, 0xED, operands, labels),
        "and" => encode_6502_alu(0x29, 0x25, 0x2D, operands, labels),
        "ora" => encode_6502_alu(0x09, 0x05, 0x0D, operands, labels),
        "eor" => encode_6502_alu(0x49, 0x45, 0x4D, operands, labels),
        "cmp" => encode_6502_alu(0xC9, 0xC5, 0xCD, operands, labels),
        "cpx" => encode_6502_alu(0xE0, 0xE4, 0xEC, operands, labels),
        "cpy" => encode_6502_alu(0xC0, 0xC4, 0xCC, operands, labels),

        "jmp" => {
            let target = resolve_operand_val(operands, labels).unwrap_or(0);
            let lo = (target & 0xFF) as u8;
            let hi = ((target >> 8) & 0xFF) as u8;
            Ok(vec![0x4C, lo, hi])
        }
        "jsr" => {
            let target = resolve_operand_val(operands, labels).unwrap_or(0);
            let lo = (target & 0xFF) as u8;
            let hi = ((target >> 8) & 0xFF) as u8;
            Ok(vec![0x20, lo, hi])
        }

        "bne" | "beq" | "bpl" | "bmi" | "bcc" | "bcs" | "bvc" | "bvs" => {
            let op = match mnemonic {
                "bpl" => 0x10,
                "bmi" => 0x30,
                "bvc" => 0x50,
                "bvs" => 0x70,
                "bcc" => 0x90,
                "bcs" => 0xB0,
                "bne" => 0xD0,
                "beq" => 0xF0,
                _ => 0xD0,
            };
            let target = resolve_operand_val(operands, labels).unwrap_or(pc + 2);
            let rel = (target as i64) - ((pc + 2) as i64);
            Ok(vec![op, (rel as i8) as u8])
        }

        _ => Err(ProjectError::AssembleError(format!(
            "Unsupported 6502 instruction: '{}'",
            mnemonic
        ))),
    }
}

fn encode_6502_alu(
    imm_op: u8,
    zp_op: u8,
    abs_op: u8,
    operands: &str,
    labels: &BTreeMap<String, u64>,
) -> Result<Vec<u8>, ProjectError> {
    let op = operands.trim();
    if let Some(imm_str) = op.strip_prefix('#') {
        let val = parse_num_literal(imm_str).unwrap_or(0) as u8;
        Ok(vec![imm_op, val])
    } else if let Some(target) = resolve_operand_val(op, labels) {
        if target <= 0xFF {
            Ok(vec![zp_op, target as u8])
        } else {
            Ok(vec![abs_op, (target & 0xFF) as u8, ((target >> 8) & 0xFF) as u8])
        }
    } else {
        Ok(vec![imm_op, 0x00])
    }
}

fn encode_6502_mem(
    zp_op: u8,
    abs_op: u8,
    operands: &str,
    labels: &BTreeMap<String, u64>,
) -> Result<Vec<u8>, ProjectError> {
    let target = resolve_operand_val(operands.trim(), labels).unwrap_or(0);
    if target <= 0xFF {
        Ok(vec![zp_op, target as u8])
    } else {
        Ok(vec![abs_op, (target & 0xFF) as u8, ((target >> 8) & 0xFF) as u8])
    }
}

fn encode_i8086(
    mnemonic: &str,
    operands: &str,
    pc: u64,
    labels: &BTreeMap<String, u64>,
) -> Result<Vec<u8>, ProjectError> {
    match mnemonic {
        "nop" => Ok(vec![0x90]),
        "hlt" => Ok(vec![0xF4]),
        "ret" => Ok(vec![0xC3]),
        "cli" => Ok(vec![0xFA]),
        "sti" => Ok(vec![0xFB]),
        "cld" => Ok(vec![0xFC]),
        "std" => Ok(vec![0xFD]),
        "clc" => Ok(vec![0xF8]),
        "stc" => Ok(vec![0xF9]),
        "pushf" => Ok(vec![0x9C]),
        "popf" => Ok(vec![0x9D]),

        "push" => {
            let reg = operands.trim().to_ascii_lowercase();
            match reg.as_str() {
                "ax" => Ok(vec![0x50]),
                "cx" => Ok(vec![0x51]),
                "dx" => Ok(vec![0x52]),
                "bx" => Ok(vec![0x53]),
                "sp" => Ok(vec![0x54]),
                "bp" => Ok(vec![0x55]),
                "si" => Ok(vec![0x56]),
                "di" => Ok(vec![0x57]),
                _ => Ok(vec![0x50]),
            }
        }
        "pop" => {
            let reg = operands.trim().to_ascii_lowercase();
            match reg.as_str() {
                "ax" => Ok(vec![0x58]),
                "cx" => Ok(vec![0x59]),
                "dx" => Ok(vec![0x5A]),
                "bx" => Ok(vec![0x5B]),
                "sp" => Ok(vec![0x5C]),
                "bp" => Ok(vec![0x5D]),
                "si" => Ok(vec![0x5E]),
                "di" => Ok(vec![0x5F]),
                _ => Ok(vec![0x58]),
            }
        }

        "inc" => {
            let reg = operands.trim().to_ascii_lowercase();
            let reg_code = i8086_reg_code(&reg).unwrap_or(0);
            Ok(vec![0x40 + reg_code])
        }
        "dec" => {
            let reg = operands.trim().to_ascii_lowercase();
            let reg_code = i8086_reg_code(&reg).unwrap_or(0);
            Ok(vec![0x48 + reg_code])
        }

        "mov" => {
            if let Some((dst, src)) = operands.split_once(',') {
                let dst = dst.trim().to_ascii_lowercase();
                let src = src.trim().to_ascii_lowercase();

                if let Some(dst_code) = i8086_reg_code(&dst) {
                    if let Some(src_code) = i8086_reg_code(&src) {
                        // Register-to-register: MOV r/m16, r16 (0x89)
                        let modrm = 0xC0 | (src_code << 3) | dst_code;
                        return Ok(vec![0x89, modrm]);
                    } else if let Some(imm) = resolve_operand_val(&src, labels) {
                        // MOV reg, imm16 (0xB8 + reg)
                        let lo = (imm & 0xFF) as u8;
                        let hi = ((imm >> 8) & 0xFF) as u8;
                        return Ok(vec![0xB8 + dst_code, lo, hi]);
                    }
                }
            }
            Ok(vec![0x90])
        }

        "add" | "sub" | "and" | "or" | "xor" | "cmp" => {
            let op_code = match mnemonic {
                "add" => 0x01,
                "or" => 0x09,
                "and" => 0x21,
                "sub" => 0x29,
                "xor" => 0x31,
                "cmp" => 0x39,
                _ => 0x01,
            };
            if let Some((dst, src)) = operands.split_once(',') {
                let dst = dst.trim().to_ascii_lowercase();
                let src = src.trim().to_ascii_lowercase();
                let dst_code = i8086_reg_code(&dst).unwrap_or(0);
                let src_code = i8086_reg_code(&src).unwrap_or(0);
                let modrm = 0xC0 | (src_code << 3) | dst_code;
                Ok(vec![op_code, modrm])
            } else {
                Ok(vec![0x90])
            }
        }

        "jmp" => {
            let target = resolve_operand_val(operands.trim(), labels).unwrap_or(pc + 3);
            let rel = (target as i64) - ((pc + 3) as i64);
            let rel16 = rel as i16;
            let lo = (rel16 & 0xFF) as u8;
            let hi = ((rel16 >> 8) & 0xFF) as u8;
            Ok(vec![0xE9, lo, hi])
        }
        "call" => {
            let target = resolve_operand_val(operands.trim(), labels).unwrap_or(pc + 3);
            let rel = (target as i64) - ((pc + 3) as i64);
            let rel16 = rel as i16;
            let lo = (rel16 & 0xFF) as u8;
            let hi = ((rel16 >> 8) & 0xFF) as u8;
            Ok(vec![0xE8, lo, hi])
        }

        "jz" | "je" | "jnz" | "jne" | "jc" | "jb" | "jnc" | "jnb" | "js" | "jns" => {
            let op = match mnemonic {
                "jb" | "jc" => 0x72,
                "jnb" | "jnc" => 0x73,
                "je" | "jz" => 0x74,
                "jne" | "jnz" => 0x75,
                "js" => 0x78,
                "jns" => 0x79,
                _ => 0x74,
            };
            let target = resolve_operand_val(operands.trim(), labels).unwrap_or(pc + 2);
            let rel = (target as i64) - ((pc + 2) as i64);
            Ok(vec![op, (rel as i8) as u8])
        }

        _ => Err(ProjectError::AssembleError(format!(
            "Unsupported 8086 instruction: '{}'",
            mnemonic
        ))),
    }
}

fn i8086_reg_code(reg: &str) -> Option<u8> {
    match reg {
        "ax" => Some(0),
        "cx" => Some(1),
        "dx" => Some(2),
        "bx" => Some(3),
        "sp" => Some(4),
        "bp" => Some(5),
        "si" => Some(6),
        "di" => Some(7),
        _ => None,
    }
}

fn encode_riscv(
    mnemonic: &str,
    operands: &str,
    pc: u64,
    labels: &BTreeMap<String, u64>,
) -> Result<Vec<u8>, ProjectError> {
    match mnemonic {
        "nop" => {
            let ins = 0x00000013u32; // addi x0, x0, 0
            Ok(ins.to_le_bytes().to_vec())
        }
        "ecall" => Ok(0x00000073u32.to_le_bytes().to_vec()),
        "ebreak" => Ok(0x00100073u32.to_le_bytes().to_vec()),
        "wfi" => Ok(0x10500073u32.to_le_bytes().to_vec()),

        "addi" => {
            let parts: Vec<&str> = operands.split(',').collect();
            if parts.len() == 3 {
                let rd = riscv_reg(parts[0].trim()).unwrap_or(0);
                let rs1 = riscv_reg(parts[1].trim()).unwrap_or(0);
                let imm = resolve_operand_val(parts[2].trim(), labels).unwrap_or(0) as u32;
                let ins = ((imm & 0xFFF) << 20) | (rs1 << 15) | (rd << 7) | 0x13;
                return Ok(ins.to_le_bytes().to_vec());
            }
            Ok(0x00000013u32.to_le_bytes().to_vec())
        }

        "add" | "sub" | "sll" | "slt" | "sltu" | "xor" | "srl" | "sra" | "or" | "and" => {
            let parts: Vec<&str> = operands.split(',').collect();
            if parts.len() == 3 {
                let rd = riscv_reg(parts[0].trim()).unwrap_or(0);
                let rs1 = riscv_reg(parts[1].trim()).unwrap_or(0);
                let rs2 = riscv_reg(parts[2].trim()).unwrap_or(0);

                let (funct3, funct7) = match mnemonic {
                    "add" => (0b000, 0b0000000),
                    "sub" => (0b000, 0b0100000),
                    "sll" => (0b001, 0b0000000),
                    "slt" => (0b010, 0b0000000),
                    "sltu" => (0b011, 0b0000000),
                    "xor" => (0b100, 0b0000000),
                    "srl" => (0b101, 0b0000000),
                    "sra" => (0b101, 0b0100000),
                    "or" => (0b110, 0b0000000),
                    "and" => (0b111, 0b0000000),
                    _ => (0, 0),
                };
                let ins = (funct7 << 25) | (rs2 << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | 0x33;
                return Ok(ins.to_le_bytes().to_vec());
            }
            Ok(0x00000013u32.to_le_bytes().to_vec())
        }

        "lui" => {
            let parts: Vec<&str> = operands.split(',').collect();
            if parts.len() == 2 {
                let rd = riscv_reg(parts[0].trim()).unwrap_or(0);
                let imm = resolve_operand_val(parts[1].trim(), labels).unwrap_or(0) as u32;
                let ins = (imm & 0xFFFFF000) | (rd << 7) | 0x37;
                return Ok(ins.to_le_bytes().to_vec());
            }
            Ok(0x00000013u32.to_le_bytes().to_vec())
        }

        "jal" => {
            let parts: Vec<&str> = operands.split(',').collect();
            let (rd, target_str) = if parts.len() == 2 {
                (riscv_reg(parts[0].trim()).unwrap_or(1), parts[1].trim())
            } else {
                (1, parts[0].trim())
            };
            let target = resolve_operand_val(target_str, labels).unwrap_or(pc + 4);
            let offset = (target as i64) - (pc as i64);
            let imm = offset as u32;
            let bit20 = (imm >> 20) & 1;
            let bit10_1 = (imm >> 1) & 0x3FF;
            let bit11 = (imm >> 11) & 1;
            let bit19_12 = (imm >> 12) & 0xFF;
            let j_imm = (bit20 << 31) | (bit19_12 << 12) | (bit11 << 20) | (bit10_1 << 21);
            let ins = j_imm | (rd << 7) | 0x6F;
            Ok(ins.to_le_bytes().to_vec())
        }

        "beq" | "bne" | "blt" | "bge" => {
            let parts: Vec<&str> = operands.split(',').collect();
            if parts.len() == 3 {
                let rs1 = riscv_reg(parts[0].trim()).unwrap_or(0);
                let rs2 = riscv_reg(parts[1].trim()).unwrap_or(0);
                let target = resolve_operand_val(parts[2].trim(), labels).unwrap_or(pc + 4);
                let offset = (target as i64) - (pc as i64);
                let imm = offset as u32;
                let funct3 = match mnemonic {
                    "beq" => 0b000,
                    "bne" => 0b001,
                    "blt" => 0b100,
                    "bge" => 0b101,
                    _ => 0,
                };
                let bit12 = (imm >> 12) & 1;
                let bit10_5 = (imm >> 5) & 0x3F;
                let bit4_1 = (imm >> 1) & 0xF;
                let bit11 = (imm >> 11) & 1;
                let b_imm_high = (bit12 << 6) | bit10_5;
                let b_imm_low = (bit4_1 << 1) | bit11;
                let ins = (b_imm_high << 25) | (rs2 << 20) | (rs1 << 15) | (funct3 << 12) | (b_imm_low << 7) | 0x63;
                return Ok(ins.to_le_bytes().to_vec());
            }
            Ok(0x00000013u32.to_le_bytes().to_vec())
        }

        _ => Err(ProjectError::AssembleError(format!(
            "Unsupported RISC-V instruction: '{}'",
            mnemonic
        ))),
    }
}

fn riscv_reg(reg: &str) -> Option<u32> {
    let lower = reg.to_ascii_lowercase();
    if let Some(num_str) = lower.strip_prefix('x') {
        return num_str.parse::<u32>().ok().filter(|&n| n < 32);
    }
    match lower.as_str() {
        "zero" => Some(0),
        "ra" => Some(1),
        "sp" => Some(2),
        "gp" => Some(3),
        "tp" => Some(4),
        "t0" => Some(5),
        "t1" => Some(6),
        "t2" => Some(7),
        "s0" | "fp" => Some(8),
        "s1" => Some(9),
        "a0" => Some(10),
        "a1" => Some(11),
        "a2" => Some(12),
        "a3" => Some(13),
        "a4" => Some(14),
        "a5" => Some(15),
        "a6" => Some(16),
        "a7" => Some(17),
        "s2" => Some(18),
        "s3" => Some(19),
        "s4" => Some(20),
        "s5" => Some(21),
        "s6" => Some(22),
        "s7" => Some(23),
        "s8" => Some(24),
        "s9" => Some(25),
        "s10" => Some(26),
        "s11" => Some(27),
        "t3" => Some(28),
        "t4" => Some(29),
        "t5" => Some(30),
        "t6" => Some(31),
        _ => None,
    }
}

fn encode_avr(
    mnemonic: &str,
    operands: &str,
    _pc: u64,
    labels: &BTreeMap<String, u64>,
) -> Result<Vec<u8>, ProjectError> {
    match mnemonic {
        "nop" => Ok(vec![0x00, 0x00]),
        "ret" => Ok(vec![0x08, 0x95]),

        "ldi" => {
            if let Some((rd_str, imm_str)) = operands.split_once(',') {
                let rd = avr_reg(rd_str.trim()).unwrap_or(16);
                let imm = resolve_operand_val(imm_str.trim(), labels).unwrap_or(0) as u8;
                if (16..=31).contains(&rd) {
                    let d = rd - 16;
                    let k_high = (imm >> 4) & 0x0F;
                    let k_low = imm & 0x0F;
                    let ins = 0xE000 | ((k_high as u16) << 8) | ((d as u16) << 4) | (k_low as u16);
                    return Ok(ins.to_le_bytes().to_vec());
                }
            }
            Ok(vec![0x00, 0x00])
        }

        "mov" => {
            if let Some((rd_str, rr_str)) = operands.split_once(',') {
                let rd = avr_reg(rd_str.trim()).unwrap_or(0);
                let rr = avr_reg(rr_str.trim()).unwrap_or(0);
                let ins = 0x2C00 | (((rr & 0x10) as u16) << 5) | ((rd as u16) << 4) | ((rr & 0x0F) as u16);
                return Ok(ins.to_le_bytes().to_vec());
            }
            Ok(vec![0x00, 0x00])
        }

        "add" => {
            if let Some((rd_str, rr_str)) = operands.split_once(',') {
                let rd = avr_reg(rd_str.trim()).unwrap_or(0);
                let rr = avr_reg(rr_str.trim()).unwrap_or(0);
                let ins = 0x0C00 | (((rr & 0x10) as u16) << 5) | ((rd as u16) << 4) | ((rr & 0x0F) as u16);
                return Ok(ins.to_le_bytes().to_vec());
            }
            Ok(vec![0x00, 0x00])
        }

        "push" => {
            let rd = avr_reg(operands.trim()).unwrap_or(0);
            let ins = 0x920F | ((rd as u16) << 4);
            Ok(ins.to_le_bytes().to_vec())
        }

        "pop" => {
            let rd = avr_reg(operands.trim()).unwrap_or(0);
            let ins = 0x900F | ((rd as u16) << 4);
            Ok(ins.to_le_bytes().to_vec())
        }

        _ => Err(ProjectError::AssembleError(format!(
            "Unsupported AVR instruction: '{}'",
            mnemonic
        ))),
    }
}

fn avr_reg(reg: &str) -> Option<u8> {
    let lower = reg.to_ascii_lowercase();
    if let Some(num_str) = lower.strip_prefix('r') {
        num_str.parse::<u8>().ok().filter(|&n| n < 32)
    } else {
        None
    }
}

#[cfg(feature = "std")]
impl ProjectConfig {
    /// Load and parse `mconfig.toml` from a directory.
    pub fn load_from_directory(dir: &std::path::Path) -> Result<Self, ProjectError> {
        let toml_path = dir.join("mconfig.toml");
        if !toml_path.exists() {
            return Err(ProjectError::FileNotFound("mconfig.toml not found in directory".to_string()));
        }
        let content = std::fs::read_to_string(&toml_path)
            .map_err(|e| ProjectError::FileNotFound(format!("Failed to read mconfig.toml: {}", e)))?;
        Self::parse(&content)
    }
}

/// Loaded project bundle ready for CPU execution.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedProject {
    pub config: ProjectConfig,
    pub arch: Architecture,
    pub program: AsmProgram,
    pub source_code: String,
    pub file_path: Option<String>,
}

#[cfg(feature = "std")]
impl LoadedProject {
    /// Loads a project from a directory containing `mconfig.toml` or asm files.
    pub fn load_from_dir(dir: &std::path::Path) -> Result<Self, ProjectError> {
        let config = ProjectConfig::load_from_directory(dir).unwrap_or_else(|_| ProjectConfig::default());

        // Locate main source file
        let mut source_file = None;
        if let Some(ref main_name) = config.main {
            let p = dir.join(main_name);
            if p.exists() {
                source_file = Some(p);
            }
        }

        if source_file.is_none() {
            // Find first asm file in dir
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(ext) = path.extension() {
                        let ext_str = ext.to_string_lossy().to_ascii_lowercase();
                        if ext_str == "asm" || ext_str == "s" || ext_str == "s86" || ext_str == "s65" || ext_str == "rv32" || ext_str == "avr" {
                            source_file = Some(path);
                            break;
                        }
                    }
                }
            }
        }

        let source_path = source_file.ok_or_else(|| {
            ProjectError::FileNotFound("No assembly source files (.asm, .s) found in project folder".to_string())
        })?;

        let source_code = std::fs::read_to_string(&source_path)
            .map_err(|e| ProjectError::FileNotFound(format!("Failed to read source file: {}", e)))?;

        let arch = if let Some(a) = config.arch {
            a
        } else {
            match detect_architecture(source_path.to_str(), &source_code) {
                DetectionResult::Detected(a) => a,
                DetectionResult::Ambiguous(candidates) => return Err(ProjectError::AmbiguousArch(candidates)),
                DetectionResult::Unknown => {
                    return Err(ProjectError::ArchDetectionFailed(format!(
                        "Could not detect architecture for source file '{}'",
                        source_path.display()
                    )));
                }
            }
        };

        let program = assemble_source(arch, &source_code, config.entry_point, config.sp)?;

        Ok(LoadedProject {
            config,
            arch,
            program,
            source_code,
            file_path: Some(source_path.to_string_lossy().to_string()),
        })
    }

    /// Loads an individual assembly file, autodetecting architecture if possible.
    pub fn load_from_file(file_path: &std::path::Path) -> Result<Self, ProjectError> {
        let source_code = std::fs::read_to_string(file_path)
            .map_err(|e| ProjectError::FileNotFound(format!("Failed to read file: {}", e)))?;

        let arch = match detect_architecture(file_path.to_str(), &source_code) {
            DetectionResult::Detected(a) => a,
            DetectionResult::Ambiguous(candidates) => return Err(ProjectError::AmbiguousArch(candidates)),
            DetectionResult::Unknown => {
                return Err(ProjectError::ArchDetectionFailed(format!(
                    "Could not detect architecture for '{}'",
                    file_path.display()
                )));
            }
        };

        let fname = file_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "File".to_string());
        let config = ProjectConfig {
            name: fname,
            arch: Some(arch),
            ..Default::default()
        };

        let program = assemble_source(arch, &source_code, config.entry_point, config.sp)?;

        Ok(LoadedProject {
            config,
            arch,
            program,
            source_code,
            file_path: Some(file_path.to_string_lossy().to_string()),
        })
    }
}

fn resolve_operand_val(op: &str, labels: &BTreeMap<String, u64>) -> Option<u64> {
    let clean = op.trim();
    if let Some(val) = parse_num_literal(clean) {
        return Some(val);
    }
    if let Some(&addr) = labels.get(clean) {
        return Some(addr);
    }
    None
}
