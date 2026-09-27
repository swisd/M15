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

/// Helper to parse decimal or hex string (`"0x1000"` / `"$1000"` / `1000` / `100h`).
pub fn parse_num_literal(s: &str) -> Option<u64> {
    let clean = strip_quotes(s.trim());
    if clean.is_empty() {
        return None;
    }
    if let Some(rest) = clean.strip_prefix("0x").or_else(|| clean.strip_prefix("0X")) {
        u64::from_str_radix(rest.trim_start_matches('_'), 16).ok()
    } else if let Some(rest) = clean.strip_prefix('$') {
        u64::from_str_radix(rest.trim_start_matches('_'), 16).ok()
    } else if let Some(rest) = clean.strip_prefix("0b").or_else(|| clean.strip_prefix("0B")) {
        u64::from_str_radix(rest.trim_start_matches('_'), 2).ok()
    } else if let Some(rest) = clean.strip_prefix('%') {
        u64::from_str_radix(rest.trim_start_matches('_'), 2).ok()
    } else if let Some(rest) = clean.strip_prefix("0o").or_else(|| clean.strip_prefix("0O")) {
        u64::from_str_radix(rest.trim_start_matches('_'), 8).ok()
    } else if (clean.ends_with('h') || clean.ends_with('H')) && clean.len() > 1 {
        let hex_part = &clean[..clean.len() - 1];
        u64::from_str_radix(hex_part.trim_start_matches('_'), 16).ok()
    } else if (clean.ends_with('b') || clean.ends_with('B')) && clean.len() > 1 && clean.chars().all(|c| c == '0' || c == '1' || c == 'b' || c == 'B') {
        let bin_part = &clean[..clean.len() - 1];
        u64::from_str_radix(bin_part.trim_start_matches('_'), 2).ok()
    } else if clean.starts_with('\'') && clean.ends_with('\'') && clean.len() == 3 {
        Some(clean.as_bytes()[1] as u64)
    } else {
        clean.parse::<u64>().ok()
    }
}

/// Evaluates an arithmetic / logical expression with label and symbol substitution.
pub fn eval_expression(expr: &str, symbols: &BTreeMap<String, u64>) -> Option<u64> {
    let text = expr.trim();
    if text.is_empty() {
        return None;
    }

    // Direct number literal
    if let Some(val) = parse_num_literal(text) {
        return Some(val);
    }

    // Direct symbol lookup
    if let Some(&val) = symbols.get(text) {
        return Some(val);
    }
    // Case-insensitive lookup
    for (k, &v) in symbols {
        if k.eq_ignore_ascii_case(text) {
            return Some(v);
        }
    }

    // Standard builtin symbols
    match text.to_ascii_lowercase().as_str() {
        "$" => return symbols.get("$").copied().or(Some(0)),
        "@data" => return Some(0x0000),
        "@code" => return Some(0x0000),
        "@stack" => return Some(0x0000),
        "@curseg" => return Some(0x0000),
        "ramend" => return Some(0x08FF),
        "sph" => return Some(0x3E),
        "spl" => return Some(0x3D),
        "sreg" => return Some(0x3F),
        "txen0" => return Some(3),
        "ucsz01" => return Some(2),
        "ucsz00" => return Some(1),
        "udre0" => return Some(5),
        "udr0" => return Some(0xC6),
        "ubrr0l" => return Some(0xC4),
        "ubrr0h" => return Some(0xC5),
        "ucsr0a" => return Some(0xC0),
        "ucsr0b" => return Some(0xC1),
        "ucsr0c" => return Some(0xC2),
        "portb" => return Some(0x25),
        "ddrb" => return Some(0x24),
        "pinb" => return Some(0x23),
        _ => {}
    }

    let lower = text.to_ascii_lowercase();

    // OFFSET / offset prefix or function
    if lower.starts_with("offset ") {
        let actual_rest = text[7..].trim();
        return eval_expression(actual_rest, symbols);
    }
    if lower.starts_with("seg ") {
        let actual_rest = text[4..].trim();
        return eval_expression(actual_rest, symbols).map(|a| a >> 4);
    }
    if (lower.starts_with("offset(") || lower.starts_with("seg(")) && text.ends_with(')') {
        let (func, inner) = text.split_once('(')?;
        let inner_expr = &inner[..inner.len() - 1];
        let val = eval_expression(inner_expr, symbols)?;
        return if func.trim().eq_ignore_ascii_case("seg") {
            Some(val >> 4)
        } else {
            Some(val)
        };
    }

    // Unary < (low byte) and > (high byte) as used in 6502 / AVR
    if let Some(rest) = text.strip_prefix('<') {
        let val = eval_expression(rest, symbols)?;
        return Some(val & 0xFF);
    }
    if let Some(rest) = text.strip_prefix('>') {
        let val = eval_expression(rest, symbols)?;
        return Some((val >> 8) & 0xFF);
    }

    // Unary functions: high(...), low(...), byte2(...), byte3(...), byte4(...)
    if (lower.starts_with("high(") || lower.starts_with("low(") || lower.starts_with("byte2(") || lower.starts_with("byte3(") || lower.starts_with("byte4(") || lower.starts_with("lwb(") || lower.starts_with("hwb(")) && text.ends_with(')') {
        let (func, inner) = text.split_once('(')?;
        let inner_expr = &inner[..inner.len() - 1];
        let inner_val = eval_expression(inner_expr, symbols)?;
        return match func.trim().to_ascii_lowercase().as_str() {
            "high" | "hwb" | "byte2" => Some((inner_val >> 8) & 0xFF),
            "low" | "lwb" => Some(inner_val & 0xFF),
            "byte3" => Some((inner_val >> 16) & 0xFF),
            "byte4" => Some((inner_val >> 24) & 0xFF),
            _ => None,
        };
    }

    // Strip outer matched parentheses
    if text.starts_with('(') && text.ends_with(')') {
        let mut depth = 0;
        let mut fully_enclosed = true;
        for (i, c) in text.chars().enumerate() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
                if depth == 0 && i < text.len() - 1 {
                    fully_enclosed = false;
                    break;
                }
            }
        }
        if fully_enclosed && depth == 0 {
            return eval_expression(&text[1..text.len() - 1], symbols);
        }
    }

    // Binary operators with precedence (evaluated right-to-left to respect left-associativity)
    // Level 1: Bitwise OR |
    if let Some(idx) = find_op_outside_parens(text, &["|"]) {
        let left = eval_expression(&text[..idx], symbols)?;
        let right = eval_expression(&text[idx + 1..], symbols)?;
        return Some(left | right);
    }

    // Level 2: Bitwise XOR ^
    if let Some(idx) = find_op_outside_parens(text, &["^"]) {
        let left = eval_expression(&text[..idx], symbols)?;
        let right = eval_expression(&text[idx + 1..], symbols)?;
        return Some(left ^ right);
    }

    // Level 3: Bitwise AND &
    if let Some(idx) = find_op_outside_parens(text, &["&"]) {
        let left = eval_expression(&text[..idx], symbols)?;
        let right = eval_expression(&text[idx + 1..], symbols)?;
        return Some(left & right);
    }

    // Level 4: Shifts <<, >>
    if let Some(idx) = find_op_outside_parens(text, &["<<", ">>"]) {
        let op_len = 2;
        let is_shl = &text[idx..idx + 2] == "<<";
        let left = eval_expression(&text[..idx], symbols)?;
        let right = eval_expression(&text[idx + op_len..], symbols)?;
        return if is_shl {
            Some(left.wrapping_shl(right as u32))
        } else {
            Some(left.wrapping_shr(right as u32))
        };
    }

    // Level 5: Additive +, -
    if let Some(idx) = find_op_outside_parens(text, &["+", "-"]) {
        // Ensure '-' is binary and not unary at the start
        if idx > 0 {
            let is_add = &text[idx..idx + 1] == "+";
            let left = eval_expression(&text[..idx], symbols)?;
            let right = eval_expression(&text[idx + 1..], symbols)?;
            return if is_add {
                Some(left.wrapping_add(right))
            } else {
                Some(left.wrapping_sub(right))
            };
        }
    }

    // Level 6: Multiplicative *, /, %
    if let Some(idx) = find_op_outside_parens(text, &["*", "/", "%"]) {
        let op = &text[idx..idx + 1];
        let left = eval_expression(&text[..idx], symbols)?;
        let right = eval_expression(&text[idx + 1..], symbols)?;
        return match op {
            "*" => Some(left.wrapping_mul(right)),
            "/" => if right == 0 { None } else { Some(left / right) },
            "%" => if right == 0 { None } else { Some(left % right) },
            _ => None,
        };
    }

    // Level 7: Unary ~, !
    if let Some(rest) = text.strip_prefix('~') {
        let val = eval_expression(rest, symbols)?;
        return Some(!val);
    }
    if let Some(rest) = text.strip_prefix('!') {
        let val = eval_expression(rest, symbols)?;
        return Some(if val == 0 { 1 } else { 0 });
    }

    None
}

fn find_op_outside_parens(text: &str, ops: &[&str]) -> Option<usize> {
    let mut depth = 0;
    let bytes = text.as_bytes();
    let mut last_match = None;

    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'(' {
            depth += 1;
            i += 1;
        } else if b == b')' {
            if depth > 0 {
                depth -= 1;
            }
            i += 1;
        } else if depth == 0 {
            let mut matched = false;
            for &op in ops {
                if text[i..].starts_with(op) {
                    // Check if it's not a leading unary +/-
                    if (op == "+" || op == "-") && i == 0 {
                        continue;
                    }
                    last_match = Some(i);
                    i += op.len();
                    matched = true;
                    break;
                }
            }
            if !matched {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    last_match
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
    if text_lower.contains(".model") || text_lower.contains(".stack") || text_lower.contains("int 21h") || text_lower.contains("@data") {
        add_score(Architecture::I8086, 25);
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
        // Ensure bus has enough memory for all chunks across all segments
        let mut max_end_addr = 0u64;
        for (addr, bytes) in &self.chunks {
            let end = addr.wrapping_add(bytes.len() as u64);
            if end > max_end_addr {
                max_end_addr = end;
            }
        }
        if max_end_addr as usize > bus.len() {
            bus.data.resize(max_end_addr as usize, 0);
        }
        for (addr, bytes) in &self.chunks {
            for (offset, &b) in bytes.iter().enumerate() {
                let _ = bus.write_u8(addr.wrapping_add(offset as u64), b);
            }
        }
        cpu.set_pc(self.entry_point);
        cpu.set_sp(self.initial_sp);
    }
}

/// Supported assembly section kinds.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SectionKind {
    Text,
    Data,
    RoData,
    Bss,
}

/// Returns the default starting base address for a section under a target architecture.
pub fn default_section_base(arch: Architecture, section: SectionKind, default_entry: Option<u64>) -> u64 {
    match section {
        SectionKind::Text => default_entry.unwrap_or(match arch {
            Architecture::Mos6502 => 0x0600,
            Architecture::Avr => 0x0000,
            Architecture::I8086 => 0x1000,
            Architecture::X86 | Architecture::X86_64 => 0x00400000,
            _ => 0x00010000,
        }),
        SectionKind::Data => match arch {
            Architecture::Mos6502 => 0x0200,
            Architecture::Avr => 0x0100, // SRAM start for AVR ATmega
            Architecture::I8086 => 0x2000,
            Architecture::X86 => 0x00402000,
            Architecture::X86_64 => 0x00402000,
            _ => 0x00020000,
        },
        SectionKind::RoData => match arch {
            Architecture::Mos6502 => 0x0400,
            Architecture::Avr => 0x0000,
            Architecture::I8086 => 0x2800,
            Architecture::X86 => 0x00401800,
            Architecture::X86_64 => 0x00401800,
            _ => 0x00018000,
        },
        SectionKind::Bss => match arch {
            Architecture::Mos6502 => 0x0300,
            Architecture::Avr => 0x0200,
            Architecture::I8086 => 0x3000,
            Architecture::X86 => 0x00403000,
            Architecture::X86_64 => 0x00403000,
            _ => 0x00028000,
        },
    }
}

/// Identifies if a source line is a section / segment switching directive.
pub fn parse_section_directive(line: &str) -> Option<SectionKind> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();

    // Direct section keywords
    if lower == ".text"
        || lower == ".code"
        || lower == ".cseg"
        || lower == "text"
        || lower == "code"
        || lower == "cseg"
        || lower == "code segment"
        || lower == "text segment"
        || lower == "segment code"
        || lower == "segment text"
        || lower.starts_with(".code16")
        || lower.starts_with(".code32")
        || lower.starts_with(".code64")
        || lower.starts_with("bits 16")
        || lower.starts_with("bits 32")
        || lower.starts_with("bits 64")
        || lower.starts_with("[bits")
    {
        return Some(SectionKind::Text);
    }

    if lower == ".data"
        || lower == ".dseg"
        || lower == "data"
        || lower == "dseg"
        || lower == "data segment"
        || lower == "segment data"
        || lower == ".ram"
        || lower == "ram"
        || lower == "seg .data"
        || lower == "seg data"
    {
        return Some(SectionKind::Data);
    }

    if lower == ".rodata"
        || lower == ".rdata"
        || lower == "rodata"
        || lower == "rdata"
        || lower == ".rom"
        || lower == "rom"
        || lower == ".eeprom"
        || lower == ".eseg"
        || lower == "eseg"
        || lower == "seg .rodata"
        || lower == "seg rodata"
    {
        return Some(SectionKind::RoData);
    }

    if lower == ".bss"
        || lower == "bss"
        || lower == "bss segment"
        || lower == "segment bss"
        || lower == ".udata"
        || lower == "udata"
        || lower == "seg .bss"
        || lower == "seg bss"
    {
        return Some(SectionKind::Bss);
    }

    // Check .section / section / seg directives with arguments
    if lower.starts_with(".section") || lower.starts_with("section ") || lower.starts_with("seg ") {
        let sec_args = if lower.starts_with(".section") {
            lower.trim_start_matches(".section").trim()
        } else if lower.starts_with("section ") {
            lower.trim_start_matches("section").trim()
        } else {
            lower.trim_start_matches("seg").trim()
        };
        let first_token = sec_args.split([',', ' ', '\t']).next().unwrap_or("").trim();
        if first_token.starts_with(".text") || first_token.starts_with("text") || first_token.starts_with(".code") || first_token.starts_with("code") || first_token.contains("__text") {
            return Some(SectionKind::Text);
        }
        if first_token.starts_with(".rodata") || first_token.starts_with("rodata") || first_token.starts_with(".rdata") || first_token.starts_with("rdata") || first_token.contains("__const") {
            return Some(SectionKind::RoData);
        }
        if first_token.starts_with(".bss") || first_token.starts_with("bss") || first_token.contains("__bss") {
            return Some(SectionKind::Bss);
        }
        if first_token.starts_with(".data") || first_token.starts_with("data") || first_token.contains("__data") {
            return Some(SectionKind::Data);
        }
    }

    None
}

/// Assembles assembly source text into an `AsmProgram` for a specific target architecture.
pub fn assemble_source(
    arch: Architecture,
    source: &str,
    default_entry: Option<u64>,
    default_sp: Option<u64>,
) -> Result<AsmProgram, ProjectError> {
    let default_entry_addr: u64 = default_entry.unwrap_or(match arch {
        Architecture::Mos6502 => 0x0600,
        Architecture::Avr => 0x0000,
        Architecture::I8086 => 0x1000,
        Architecture::X86 => 0x00400000,
        Architecture::X86_64 => 0x00400000,
        _ => 0x1000,
    });

    let mut initial_sp: u64 = default_sp.unwrap_or(match arch {
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

    // Expand includes (e.g. .include "m328pdef.inc" / #include "m328pdef.inc")
    let expanded_source = expand_includes(source, 0);

    let mut symbols: BTreeMap<String, u64> = BTreeMap::new();
    let mut entry_point = default_entry_addr;

    // Helper to clean lines
    let sanitize_line = |line: &str| -> String {
        let mut text = line.trim();
        // C preprocessor ignore
        if text.starts_with("#ifndef")
            || text.starts_with("#ifdef")
            || text.starts_with("#else")
            || text.starts_with("#elif")
            || text.starts_with("#endif")
            || text.starts_with("#pragma")
        {
            return String::new();
        }

        if let Some(idx) = text.find(';').or_else(|| text.find("//")) {
            text = text[..idx].trim();
        }
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
        text.to_string()
    };

    // Equates pass: collect .equ, equ, =, #define, .set
    for line in expanded_source.lines() {
        let text = sanitize_line(line);
        if text.is_empty() {
            continue;
        }
        parse_equate_definition(&text, &mut symbols);
    }
    // Re-evaluate symbols once more in case of forward dependencies in equates
    for line in expanded_source.lines() {
        let text = sanitize_line(line);
        if text.is_empty() {
            continue;
        }
        parse_equate_definition(&text, &mut symbols);
    }

    // Pass 1: compute addresses for labels and handle multi-section layout
    let mut current_section = SectionKind::Text;
    let mut section_cursors: BTreeMap<SectionKind, u64> = BTreeMap::new();
    let mut section_origins: BTreeMap<SectionKind, u64> = BTreeMap::new();

    section_cursors.insert(SectionKind::Text, default_entry_addr);
    section_origins.insert(SectionKind::Text, default_entry_addr);

    let mut custom_entry_set = default_entry.is_some();

    for line in expanded_source.lines() {
        let text = sanitize_line(line);
        if text.is_empty() {
            continue;
        }

        // Equate definitions
        if parse_equate_definition(&text, &mut symbols) {
            continue;
        }

        // Check section switching
        if let Some(new_sec) = parse_section_directive(&text) {
            current_section = new_sec;
            if !section_cursors.contains_key(&current_section) {
                let base = default_section_base(arch, current_section, default_entry);
                section_cursors.insert(current_section, base);
                section_origins.insert(current_section, base);
            }
            continue;
        }

        let lower = text.to_ascii_lowercase();

        // Check section ends e.g. "data ends", "code ends"
        if lower.ends_with(" ends") || lower == "ends" || lower == "endseg" {
            continue;
        }

        // Check .org directive
        if lower.starts_with(".org") || lower.starts_with("org ") {
            let val_str = lower.trim_start_matches(".org").trim_start_matches("org").trim();
            if let Some(addr) = eval_expression(val_str, &symbols) {
                section_cursors.insert(current_section, addr);
                section_origins.insert(current_section, addr);
                if current_section == SectionKind::Text && !custom_entry_set {
                    entry_point = addr;
                    custom_entry_set = true;
                }
            }
            continue;
        }

        // Check .entry directive
        if lower.starts_with(".entry") || lower.starts_with("entry ") {
            let val_str = lower.trim_start_matches(".entry").trim_start_matches("entry").trim();
            if let Some(addr) = eval_expression(val_str, &symbols) {
                entry_point = addr;
                custom_entry_set = true;
            }
            continue;
        }

        let cur_addr = *section_cursors.entry(current_section).or_insert_with(|| {
            let base = default_section_base(arch, current_section, default_entry);
            section_origins.insert(current_section, base);
            base
        });

        // Check proc definition (e.g. main proc)
        if let Some(rest) = lower.strip_suffix(" proc") {
            let label = text[..rest.len()].trim().trim_end_matches(':').trim();
            if !label.is_empty() {
                let label_val = if arch == Architecture::Avr && current_section == SectionKind::Text { cur_addr / 2 } else { cur_addr };
                symbols.insert(label.to_string(), label_val);
                if !custom_entry_set && default_entry.is_none() && current_section == SectionKind::Text {
                    entry_point = cur_addr;
                }
            }
            continue;
        }
        if lower.ends_with(" proc near") || lower.ends_with(" proc far") {
            let idx = lower.find(" proc").unwrap();
            let label = text[..idx].trim().trim_end_matches(':').trim();
            if !label.is_empty() {
                let label_val = if arch == Architecture::Avr && current_section == SectionKind::Text { cur_addr / 2 } else { cur_addr };
                symbols.insert(label.to_string(), label_val);
                if !custom_entry_set && default_entry.is_none() && current_section == SectionKind::Text {
                    entry_point = cur_addr;
                }
            }
            continue;
        }

        // Check end directive (e.g. end main or end)
        if lower.starts_with("end ") || lower == "end" {
            let rest = text.trim_start_matches(|c: char| c.is_ascii_alphabetic() || c.is_whitespace()).trim();
            if !rest.is_empty() {
                if let Some(target) = eval_expression(rest, &symbols) {
                    entry_point = target;
                    custom_entry_set = true;
                }
            }
            continue;
        }

        // Check .stack directive (e.g. .stack 100h)
        if lower.starts_with(".stack") || lower.starts_with("stack ") {
            let val_str = lower.trim_start_matches(".stack").trim_start_matches("stack").trim();
            if !val_str.is_empty() {
                if let Some(size) = eval_expression(val_str, &symbols) {
                    if default_sp.is_none() && arch == Architecture::I8086 {
                        initial_sp = 0x1000u64.wrapping_add(size);
                    }
                }
            }
            continue;
        }

        // Check .model / main endp / other metadata directives
        if lower.starts_with(".model")
            || lower.ends_with(" endp")
            || lower.starts_with(".arch")
            || lower.starts_with(".global")
            || lower.starts_with(".globl")
            || lower.starts_with(".device")
            || lower.starts_with(".processor")
            || lower.starts_with("processor")
            || lower.starts_with(".include")
            || lower.starts_with(".syntax")
            || lower.starts_with(".type")
            || lower.starts_with(".size")
            || lower.starts_with(".option")
        {
            continue;
        }

        // Variable definitions with optional leading label (e.g. num1 dw 10, largest dw ?)
        let (var_label, data_rest) = parse_variable_definition(&text);
        if let Some(lbl) = var_label {
            let label_val = if arch == Architecture::Avr && current_section == SectionKind::Text { cur_addr / 2 } else { cur_addr };
            symbols.insert(lbl.to_string(), label_val);
        }

        // Handle standalone label with colon (e.g. `loop:` or `start: lda #0`)
        let instruction_or_data_text = if var_label.is_some() {
            data_rest.unwrap_or(&text)
        } else if let Some((lbl_name, rest)) = text.split_once(':') {
            let lbl = lbl_name.trim();
            if !lbl.is_empty() {
                let label_val = if arch == Architecture::Avr && current_section == SectionKind::Text { cur_addr / 2 } else { cur_addr };
                symbols.insert(lbl.to_string(), label_val);
                if (lbl.eq_ignore_ascii_case("main") || lbl.eq_ignore_ascii_case("_start"))
                    && !custom_entry_set && default_entry.is_none() && current_section == SectionKind::Text {
                    entry_point = cur_addr;
                }
            }
            rest.trim()
        } else {
            text.as_str()
        };

        if instruction_or_data_text.is_empty() {
            continue;
        }

        // Calculate size of data or instruction
        let byte_count = estimate_line_bytes(arch, instruction_or_data_text, cur_addr, &symbols);
        section_cursors.insert(current_section, cur_addr.wrapping_add(byte_count));
    }

    // Pass 2: assemble chunks with multi-section placement
    current_section = SectionKind::Text;
    let mut pass2_cursors = section_origins.clone();
    let initial_cursor = *pass2_cursors.get(&SectionKind::Text).unwrap_or(&default_entry_addr);

    let mut chunks: Vec<(u64, Vec<u8>)> = Vec::new();
    let mut current_chunk: (u64, Vec<u8>) = (initial_cursor, Vec::new());

    for line in expanded_source.lines() {
        let text = sanitize_line(line);
        if text.is_empty() {
            continue;
        }

        // Skip equate definitions
        if parse_equate_definition(&text, &mut symbols) {
            continue;
        }

        // Check if line switches section
        if let Some(new_sec) = parse_section_directive(&text) {
            if new_sec != current_section {
                if !current_chunk.1.is_empty() {
                    chunks.push((current_chunk.0, core::mem::take(&mut current_chunk.1)));
                }
                current_section = new_sec;
                let cur = *pass2_cursors.entry(current_section).or_insert_with(|| {
                    default_section_base(arch, current_section, default_entry)
                });
                current_chunk.0 = cur;
            }
            continue;
        }

        let lower = text.to_ascii_lowercase();

        // Check section ends
        if lower.ends_with(" ends") || lower == "ends" || lower == "endseg" {
            continue;
        }

        // Handle .org in pass 2
        if lower.starts_with(".org") || lower.starts_with("org ") {
            let val_str = lower.trim_start_matches(".org").trim_start_matches("org").trim();
            if let Some(addr) = eval_expression(val_str, &symbols) {
                if !current_chunk.1.is_empty() {
                    chunks.push((current_chunk.0, core::mem::take(&mut current_chunk.1)));
                }
                pass2_cursors.insert(current_section, addr);
                current_chunk.0 = addr;
            }
            continue;
        }

        // Skip non-code / metadata directives
        if lower.ends_with(" proc")
            || lower.ends_with(" proc near")
            || lower.ends_with(" proc far")
            || lower.ends_with(" endp")
            || lower.starts_with(".model")
            || lower.starts_with(".stack")
            || lower.starts_with("stack ")
            || lower.starts_with("end ")
            || lower == "end"
            || lower.starts_with(".entry")
            || lower.starts_with("entry ")
            || lower.starts_with(".arch")
            || lower.starts_with(".global")
            || lower.starts_with(".globl")
            || lower.starts_with(".device")
            || lower.starts_with(".processor")
            || lower.starts_with("processor")
            || lower.starts_with(".include")
            || lower.starts_with(".syntax")
            || lower.starts_with(".type")
            || lower.starts_with(".size")
            || lower.starts_with(".option")
        {
            continue;
        }

        // Variable definitions (num1 dw 10, etc.)
        let (var_label, data_rest) = parse_variable_definition(&text);

        // Strip colon label if not a variable definition
        let actual_text = if var_label.is_some() {
            data_rest.unwrap_or(&text)
        } else if let Some((_, rest)) = text.split_once(':') {
            rest.trim()
        } else {
            text.as_str()
        };

        if actual_text.is_empty() {
            continue;
        }

        let cur = *pass2_cursors.get(&current_section).unwrap_or(&current_chunk.0);

        // Check data directives
        if let Some(data_bytes) = emit_data_directive(arch, actual_text, cur, &symbols) {
            let byte_len = data_bytes.len() as u64;
            current_chunk.1.extend_from_slice(&data_bytes);
            pass2_cursors.insert(current_section, cur.wrapping_add(byte_len));
            continue;
        }

        // Encode instruction
        let bytes = encode_instruction(arch, actual_text, cur, &symbols)?;
        let byte_len = bytes.len() as u64;
        current_chunk.1.extend_from_slice(&bytes);
        pass2_cursors.insert(current_section, cur.wrapping_add(byte_len));
    }

    if !current_chunk.1.is_empty() {
        chunks.push(current_chunk);
    }

    Ok(AsmProgram {
        arch,
        entry_point,
        initial_sp,
        chunks,
        labels: symbols,
    })
}

fn parse_equate_definition(text: &str, symbols: &mut BTreeMap<String, u64>) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();

    // Check directives: .equ, .set, #define, .def
    for prefix in &[".equ", ".set", "#define", ".def"] {
        if lower.starts_with(prefix) {
            let after = &trimmed[prefix.len()..];
            if after.is_empty() || after.starts_with(char::is_whitespace) {
                let r = after.trim();
                if let Some((name, val_part)) = r
                    .split_once('=')
                    .or_else(|| r.split_once(','))
                    .or_else(|| r.split_once(char::is_whitespace))
                {
                    let name = name.trim().trim_end_matches(':').trim();
                    let val_part = val_part.trim().trim_start_matches('=').trim();
                    if !name.is_empty() {
                        if let Some(val) = eval_expression(val_part, symbols) {
                            symbols.insert(name.to_string(), val);
                            return true;
                        }
                    }
                }
            }
        }
    }

    // NAME .equ EXPR or NAME equ EXPR or NAME = EXPR or NAME .set EXPR or NAME set EXPR
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() >= 3 {
        let name = parts[0].trim_end_matches(':').trim();
        let op = parts[1].to_ascii_lowercase();
        if op == "=" || op == ":=" || op == "equ" || op == ".equ" || op == "set" || op == ".set" {
            let after_first = &trimmed[parts[0].len()..];
            if let Some(idx) = after_first.find(parts[1]) {
                let expr_str = after_first[idx + parts[1].len()..].trim();
                let expr = expr_str.trim_start_matches('=').trim();
                if !name.is_empty() {
                    if let Some(val) = eval_expression(expr, symbols) {
                        symbols.insert(name.to_string(), val);
                        return true;
                    }
                }
            }
        }
    } else if let Some((name, expr)) = trimmed.split_once('=') {
        let name = name.trim().trim_end_matches(':').trim();
        let expr = expr.trim();
        if !name.contains(' ') && !name.contains('\t') && !name.is_empty() {
            if let Some(val) = eval_expression(expr, symbols) {
                symbols.insert(name.to_string(), val);
                return true;
            }
        }
    }

    false
}

fn is_data_directive_keyword(kw: &str) -> bool {
    matches!(
        kw.to_ascii_lowercase().as_str(),
        "db" | ".db" | "byte" | ".byte" | ".2byte" | ".4byte" | ".8byte" | "defb" | "dc.b" | "fcb" | ".d8" | ".string8"
        | "dw" | ".dw" | "word" | ".word" | ".short" | ".hword" | "defw" | "dc.w" | "fdb" | ".d16" | ".string16"
        | "dd" | ".dd" | "dword" | ".dword" | ".long" | ".float" | ".single" | "dc.l" | ".d32"
        | "dq" | ".dq" | "qword" | ".qword" | ".quad" | ".double" | "dc.q" | ".d64"
        | "dt" | ".dt" | "tbyte" | ".octa"
        | ".ascii" | ".asciz" | ".asciiz" | ".string" | "defm" | "fcc"
        | ".space" | ".skip" | ".fill" | ".zero" | "defs" | "rmb" | "ds.b" | "ds.w" | "ds.l" | "ds.q" | "ds"
        | "resb" | "resw" | "resd" | "resq" | "rest" | "reso" | "resy" | "resz"
    )
}

fn parse_variable_definition<'a>(text: &'a str) -> (Option<&'a str>, Option<&'a str>) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return (None, None);
    }

    let mut iter = trimmed.split_whitespace();
    let first = iter.next().unwrap_or("");
    let first_clean = first.trim_end_matches(':');

    // Case 1: First token is directly a data directive (e.g. `db 1, 2`)
    if is_data_directive_keyword(first) {
        return (None, Some(trimmed));
    }

    // Case 2: Second token is a data directive (e.g. `num1 dw 20` or `num1: dw 20`)
    if let Some(second) = iter.next() {
        if is_data_directive_keyword(second) {
            let after_first = &trimmed[first.len()..];
            let offset_in_after = after_first.find(second).unwrap_or(0);
            let rest_idx = first.len() + offset_in_after;
            return (Some(first_clean), Some(&trimmed[rest_idx..]));
        }
    }

    (None, None)
}

fn estimate_line_bytes(arch: Architecture, text: &str, pc: u64, symbols: &BTreeMap<String, u64>) -> u64 {
    let trimmed = text.trim();
    if let Some(bytes) = emit_data_directive(arch, trimmed, pc, symbols) {
        return bytes.len() as u64;
    }
    match encode_instruction(arch, trimmed, pc, symbols) {
        Ok(b) => b.len() as u64,
        Err(_) => match arch {
            Architecture::Avr => {
                let lower = trimmed.to_ascii_lowercase();
                if lower.starts_with("lds ") || lower.starts_with("sts ") || lower.starts_with("call ") || lower.starts_with("jmp ") {
                    4
                } else {
                    2
                }
            }
            Architecture::Mos6502 => 2,
            Architecture::I8086 => 2,
            _ => 4,
        },
    }
}

fn emit_data_directive(
    arch: Architecture,
    text: &str,
    current_pc: u64,
    symbols: &BTreeMap<String, u64>,
) -> Option<Vec<u8>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let (directive, raw_args) = if let Some((d, a)) = trimmed.split_once(char::is_whitespace) {
        (d.trim().to_ascii_lowercase(), a.trim())
    } else {
        (trimmed.to_ascii_lowercase(), "")
    };

    let endian = arch.endianness();

    match directive.as_str() {
        // --- 1-byte Directives ---
        "db" | ".db" | "byte" | ".byte" | "defb" | "dc.b" | "fcb" | ".d8" | ".string8" => {
            let mut out = Vec::new();
            for item in split_data_args(raw_args) {
                emit_single_data_arg(1, &item, endian, current_pc, symbols, &mut out);
            }
            Some(out)
        }

        // --- 2-byte Directives ---
        "dw" | ".dw" | "word" | ".word" | ".short" | ".hword" | ".2byte" | "defw" | "dc.w" | "fdb" | ".d16" | ".string16" => {
            let mut out = Vec::new();
            for item in split_data_args(raw_args) {
                emit_single_data_arg(2, &item, endian, current_pc, symbols, &mut out);
            }
            Some(out)
        }

        // --- 4-byte Directives ---
        "dd" | ".dd" | "dword" | ".dword" | ".long" | ".4byte" | ".float" | ".single" | "dc.l" | ".d32" => {
            let mut out = Vec::new();
            for item in split_data_args(raw_args) {
                emit_single_data_arg(4, &item, endian, current_pc, symbols, &mut out);
            }
            Some(out)
        }

        // --- 8-byte Directives ---
        "dq" | ".dq" | "qword" | ".qword" | ".quad" | ".8byte" | ".double" | "dc.q" | ".d64" => {
            let mut out = Vec::new();
            for item in split_data_args(raw_args) {
                emit_single_data_arg(8, &item, endian, current_pc, symbols, &mut out);
            }
            Some(out)
        }

        // --- 10-byte / 16-byte Directives ---
        "dt" | ".dt" | "tbyte" => {
            let mut out = Vec::new();
            for item in split_data_args(raw_args) {
                emit_single_data_arg(10, &item, endian, current_pc, symbols, &mut out);
            }
            Some(out)
        }
        ".octa" => {
            let mut out = Vec::new();
            for item in split_data_args(raw_args) {
                emit_single_data_arg(16, &item, endian, current_pc, symbols, &mut out);
            }
            Some(out)
        }

        // --- String Directives ---
        ".asciiz" | ".asciz" | ".string" => {
            let mut out = Vec::new();
            for item in split_data_args(raw_args) {
                let s = parse_string_or_chars(&item);
                out.extend_from_slice(&s);
            }
            out.push(0);
            Some(out)
        }
        ".ascii" | "defm" | "fcc" => {
            let mut out = Vec::new();
            for item in split_data_args(raw_args) {
                let s = parse_string_or_chars(&item);
                out.extend_from_slice(&s);
            }
            Some(out)
        }

        // --- Space / Fill / Zero Directives ---
        ".space" | ".skip" | "defs" | "rmb" | "ds.b" | "ds" => {
            let args = split_data_args(raw_args);
            let count = args.get(0).and_then(|a| eval_expression(a, symbols)).unwrap_or(0) as usize;
            let val = args.get(1).and_then(|a| eval_expression(a, symbols)).unwrap_or(0) as u8;
            Some(alloc::vec![val; count])
        }
        "ds.w" => {
            let args = split_data_args(raw_args);
            let count = args.get(0).and_then(|a| eval_expression(a, symbols)).unwrap_or(0) as usize;
            Some(alloc::vec![0u8; count * 2])
        }
        "ds.l" => {
            let args = split_data_args(raw_args);
            let count = args.get(0).and_then(|a| eval_expression(a, symbols)).unwrap_or(0) as usize;
            Some(alloc::vec![0u8; count * 4])
        }
        "ds.q" => {
            let args = split_data_args(raw_args);
            let count = args.get(0).and_then(|a| eval_expression(a, symbols)).unwrap_or(0) as usize;
            Some(alloc::vec![0u8; count * 8])
        }
        ".fill" => {
            let args = split_data_args(raw_args);
            let count = args.get(0).and_then(|a| eval_expression(a, symbols)).unwrap_or(0) as usize;
            let size = args.get(1).and_then(|a| eval_expression(a, symbols)).unwrap_or(1) as usize;
            let val = args.get(2).and_then(|a| eval_expression(a, symbols)).unwrap_or(0) as u8;
            Some(alloc::vec![val; count * size])
        }
        ".zero" => {
            let count = eval_expression(raw_args, symbols).unwrap_or(0) as usize;
            Some(alloc::vec![0u8; count])
        }

        // --- Uninitialized (NASM / BSS) Directives ---
        "resb" => {
            let count = eval_expression(raw_args, symbols).unwrap_or(0) as usize;
            Some(alloc::vec![0u8; count])
        }
        "resw" => {
            let count = eval_expression(raw_args, symbols).unwrap_or(0) as usize;
            Some(alloc::vec![0u8; count * 2])
        }
        "resd" => {
            let count = eval_expression(raw_args, symbols).unwrap_or(0) as usize;
            Some(alloc::vec![0u8; count * 4])
        }
        "resq" => {
            let count = eval_expression(raw_args, symbols).unwrap_or(0) as usize;
            Some(alloc::vec![0u8; count * 8])
        }
        "rest" => {
            let count = eval_expression(raw_args, symbols).unwrap_or(0) as usize;
            Some(alloc::vec![0u8; count * 10])
        }
        "reso" | "resy" | "resz" => {
            let count = eval_expression(raw_args, symbols).unwrap_or(0) as usize;
            Some(alloc::vec![0u8; count * 16])
        }

        _ => None,
    }
}

fn emit_single_data_arg(
    unit_size: usize,
    arg: &str,
    endian: Endianness,
    current_pc: u64,
    symbols: &BTreeMap<String, u64>,
    out: &mut Vec<u8>,
) {
    let p = arg.trim();
    if p.is_empty() {
        return;
    }

    // Check MASM dup syntax: e.g. "10 dup(0)", "5 dup(?)", "20 dup('A')"
    if let Some(open_idx) = p.find("dup(").or_else(|| p.find("DUP(")) {
        if p.ends_with(')') {
            let count_str = p[..open_idx].trim();
            let inner_str = &p[open_idx + 4..p.len() - 1].trim();
            let count = eval_expression(count_str, symbols).unwrap_or(1) as usize;
            let mut pattern = Vec::new();
            for sub_arg in split_data_args(inner_str) {
                emit_single_data_arg(unit_size, &sub_arg, endian, current_pc, symbols, &mut pattern);
            }
            if pattern.is_empty() {
                pattern.resize(unit_size, 0);
            }
            for _ in 0..count {
                out.extend_from_slice(&pattern);
            }
            return;
        }
    }

    // String literal in 1-byte directive: "Hello" or 'Hello'
    if unit_size == 1 && ((p.starts_with('"') && p.ends_with('"')) || (p.starts_with('\'') && p.ends_with('\''))) && p.len() > 3 {
        let s = unescape_string(strip_quotes(p));
        out.extend_from_slice(s.as_bytes());
        return;
    }

    // Uninitialized '?'
    if p == "?" || p == "(?)" {
        out.resize(out.len() + unit_size, 0);
        return;
    }

    // Single character literal e.g. 'A' or '\n'
    if (p.starts_with('\'') && p.ends_with('\'')) || (p.starts_with('"') && p.ends_with('"')) {
        let s = unescape_string(strip_quotes(p));
        if unit_size == 1 {
            out.extend_from_slice(s.as_bytes());
            return;
        } else if let Some(first_char) = s.chars().next() {
            let val = first_char as u64;
            emit_numeric_value(unit_size, val, endian, out);
            return;
        }
    }

    // Floating point number (if dd / dq and contains '.')
    if (unit_size == 4 || unit_size == 8) && p.contains('.') && !p.starts_with('.') {
        if unit_size == 4 {
            if let Ok(f) = p.parse::<f32>() {
                let bits = f.to_bits();
                let bytes = if endian == Endianness::LittleEndian { bits.to_le_bytes() } else { bits.to_be_bytes() };
                out.extend_from_slice(&bytes);
                return;
            }
        } else if unit_size == 8 {
            if let Ok(f) = p.parse::<f64>() {
                let bits = f.to_bits();
                let bytes = if endian == Endianness::LittleEndian { bits.to_le_bytes() } else { bits.to_be_bytes() };
                out.extend_from_slice(&bytes);
                return;
            }
        }
    }

    // Evaluate arithmetic expression or symbol
    if let Some(val) = eval_expression(p, symbols) {
        emit_numeric_value(unit_size, val, endian, out);
    } else {
        // Fallback: 0
        out.resize(out.len() + unit_size, 0);
    }
}

fn emit_numeric_value(unit_size: usize, val: u64, endian: Endianness, out: &mut Vec<u8>) {
    match unit_size {
        1 => out.push(val as u8),
        2 => {
            let w = val as u16;
            let bytes = if endian == Endianness::LittleEndian { w.to_le_bytes() } else { w.to_be_bytes() };
            out.extend_from_slice(&bytes);
        }
        4 => {
            let dw = val as u32;
            let bytes = if endian == Endianness::LittleEndian { dw.to_le_bytes() } else { dw.to_be_bytes() };
            out.extend_from_slice(&bytes);
        }
        8 => {
            let qw = val;
            let bytes = if endian == Endianness::LittleEndian { qw.to_le_bytes() } else { qw.to_be_bytes() };
            out.extend_from_slice(&bytes);
        }
        10 => {
            let mut bytes = [0u8; 10];
            let qw_bytes = val.to_le_bytes();
            bytes[..8].copy_from_slice(&qw_bytes);
            if endian == Endianness::BigEndian {
                bytes.reverse();
            }
            out.extend_from_slice(&bytes);
        }
        16 => {
            let mut bytes = [0u8; 16];
            let qw_bytes = val.to_le_bytes();
            bytes[..8].copy_from_slice(&qw_bytes);
            if endian == Endianness::BigEndian {
                bytes.reverse();
            }
            out.extend_from_slice(&bytes);
        }
        _ => out.push(val as u8),
    }
}

fn parse_string_or_chars(s: &str) -> Vec<u8> {
    let clean = unescape_string(strip_quotes(s.trim()));
    clean.into_bytes()
}

fn unescape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('0') => out.push('\0'),
                Some('\\') => out.push('\\'),
                Some('\'') => out.push('\''),
                Some('"') => out.push('"'),
                Some('x') => {
                    let mut hex = String::new();
                    if let Some(&h1) = chars.peek() {
                        if h1.is_ascii_hexdigit() {
                            hex.push(chars.next().unwrap());
                            if let Some(&h2) = chars.peek() {
                                if h2.is_ascii_hexdigit() {
                                    hex.push(chars.next().unwrap());
                                }
                            }
                        }
                    }
                    if let Ok(b) = u8::from_str_radix(&hex, 16) {
                        out.push(b as char);
                    }
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn split_data_args(s: &str) -> Vec<String> {
    let mut results = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut quote_char = '"';

    for c in s.chars() {
        if (c == '"' || c == '\'') && !in_quotes {
            in_quotes = true;
            quote_char = c;
            current.push(c);
        } else if in_quotes && c == quote_char {
            in_quotes = false;
            current.push(c);
        } else if c == ',' && !in_quotes {
            results.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(c);
        }
    }
    if !current.trim().is_empty() {
        results.push(current.trim().to_string());
    }
    results
}

fn expand_includes(source: &str, depth: usize) -> String {
    if depth > 10 {
        return source.to_string();
    }
    let mut result = String::new();
    for line in source.lines() {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.starts_with(".include ") || lower.starts_with("#include ") {
            let file_part = if lower.starts_with(".include ") {
                &trimmed[9..]
            } else {
                &trimmed[9..]
            };
            let clean_fname = strip_quotes(file_part.trim()).trim_matches(|c| c == '<' || c == '>');
            
            #[cfg(feature = "std")]
            {
                let mut loaded = None;
                // Try direct path
                let path = std::path::Path::new(clean_fname);
                if path.exists() {
                    loaded = std::fs::read_to_string(path).ok();
                }
                // Try case-insensitive and test-projects directories
                if loaded.is_none() {
                    let candidates = [
                        format!("test-projects/avr/m328p/{}", clean_fname),
                        format!("test-projects/avr/m328p/m328Pdef.inc"),
                        format!("test-projects/avr/m328p/m328pdef.inc"),
                        format!("test-projects/i8086/{}", clean_fname),
                    ];
                    for c in &candidates {
                        let cp = std::path::Path::new(c);
                        if cp.exists() {
                            if let Ok(content) = std::fs::read_to_string(cp) {
                                loaded = Some(content);
                                break;
                            }
                        }
                    }
                }
                if let Some(content) = loaded {
                    result.push_str(&expand_includes(&content, depth + 1));
                    result.push('\n');
                    continue;
                }
            }
        }
        result.push_str(line);
        result.push('\n');
    }
    result
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

        "asl" => {
            let op = operands.trim().to_ascii_lowercase();
            if op.is_empty() || op == "a" {
                Ok(vec![0x0A])
            } else if let Some(target) = resolve_operand_val(&op, labels) {
                if target <= 0xFF {
                    Ok(vec![0x06, target as u8])
                } else {
                    Ok(vec![0x0E, (target & 0xFF) as u8, ((target >> 8) & 0xFF) as u8])
                }
            } else {
                Ok(vec![0x0A])
            }
        }
        "lsr" => {
            let op = operands.trim().to_ascii_lowercase();
            if op.is_empty() || op == "a" {
                Ok(vec![0x4A])
            } else if let Some(target) = resolve_operand_val(&op, labels) {
                if target <= 0xFF {
                    Ok(vec![0x46, target as u8])
                } else {
                    Ok(vec![0x4E, (target & 0xFF) as u8, ((target >> 8) & 0xFF) as u8])
                }
            } else {
                Ok(vec![0x4A])
            }
        }
        "rol" => {
            let op = operands.trim().to_ascii_lowercase();
            if op.is_empty() || op == "a" {
                Ok(vec![0x2A])
            } else if let Some(target) = resolve_operand_val(&op, labels) {
                if target <= 0xFF {
                    Ok(vec![0x26, target as u8])
                } else {
                    Ok(vec![0x2E, (target & 0xFF) as u8, ((target >> 8) & 0xFF) as u8])
                }
            } else {
                Ok(vec![0x2A])
            }
        }
        "ror" => {
            let op = operands.trim().to_ascii_lowercase();
            if op.is_empty() || op == "a" {
                Ok(vec![0x6A])
            } else if let Some(target) = resolve_operand_val(&op, labels) {
                if target <= 0xFF {
                    Ok(vec![0x66, target as u8])
                } else {
                    Ok(vec![0x6E, (target & 0xFF) as u8, ((target >> 8) & 0xFF) as u8])
                }
            } else {
                Ok(vec![0x6A])
            }
        }
        "inc" => {
            let op = operands.trim();
            if let Some(target) = resolve_operand_val(op, labels) {
                if target <= 0xFF {
                    Ok(vec![0xE6, target as u8])
                } else {
                    Ok(vec![0xEE, (target & 0xFF) as u8, ((target >> 8) & 0xFF) as u8])
                }
            } else {
                Ok(vec![0xEA])
            }
        }
        "dec" => {
            let op = operands.trim();
            if let Some(target) = resolve_operand_val(op, labels) {
                if target <= 0xFF {
                    Ok(vec![0xC6, target as u8])
                } else {
                    Ok(vec![0xCE, (target & 0xFF) as u8, ((target >> 8) & 0xFF) as u8])
                }
            } else {
                Ok(vec![0xEA])
            }
        }
        "bit" => {
            let op = operands.trim();
            if let Some(target) = resolve_operand_val(op, labels) {
                if target <= 0xFF {
                    Ok(vec![0x24, target as u8])
                } else {
                    Ok(vec![0x2C, (target & 0xFF) as u8, ((target >> 8) & 0xFF) as u8])
                }
            } else {
                Ok(vec![0xEA])
            }
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
        let val = eval_expression(imm_str, labels).unwrap_or(0) as u8;
        Ok(vec![imm_op, val])
    } else if let Some((base, idx)) = op.split_once(',') {
        let target = resolve_operand_val(base.trim(), labels).unwrap_or(0);
        let idx = idx.trim().to_ascii_lowercase();
        let idx_op = if idx == "x" { abs_op + 0x10 } else { abs_op + 0x0C };
        Ok(vec![idx_op, (target & 0xFF) as u8, ((target >> 8) & 0xFF) as u8])
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
    let op = operands.trim();
    if let Some((base, idx)) = op.split_once(',') {
        let target = resolve_operand_val(base.trim(), labels).unwrap_or(0);
        let idx = idx.trim().to_ascii_lowercase();
        let idx_op = if idx == "x" { abs_op + 0x10 } else { abs_op + 0x0C };
        Ok(vec![idx_op, (target & 0xFF) as u8, ((target >> 8) & 0xFF) as u8])
    } else {
        let target = resolve_operand_val(op, labels).unwrap_or(0);
        if target <= 0xFF {
            Ok(vec![zp_op, target as u8])
        } else {
            Ok(vec![abs_op, (target & 0xFF) as u8, ((target >> 8) & 0xFF) as u8])
        }
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
        "ret" => {
            let clean = operands.trim();
            if !clean.is_empty() {
                if let Some(imm) = resolve_operand_val(clean, labels) {
                    let lo = (imm & 0xFF) as u8;
                    let hi = ((imm >> 8) & 0xFF) as u8;
                    return Ok(vec![0xC2, lo, hi]);
                }
            }
            Ok(vec![0xC3])
        }
        "cli" => Ok(vec![0xFA]),
        "sti" => Ok(vec![0xFB]),
        "cld" => Ok(vec![0xFC]),
        "std" => Ok(vec![0xFD]),
        "clc" => Ok(vec![0xF8]),
        "stc" => Ok(vec![0xF9]),
        "pushf" => Ok(vec![0x9C]),
        "popf" => Ok(vec![0x9D]),
        "cbw" => Ok(vec![0x98]),
        "cwd" => Ok(vec![0x99]),
        "into" => Ok(vec![0xCE]),
        "iret" => Ok(vec![0xCF]),

        "int" => {
            let int_str = operands.trim();
            if int_str == "3" {
                return Ok(vec![0xCC]);
            }
            let int_num = resolve_operand_val(int_str, labels).unwrap_or(0x21) as u8;
            Ok(vec![0xCD, int_num])
        }

        "push" => {
            let reg = operands.trim().to_ascii_lowercase();
            if let Some(r) = i8086_reg_code(&reg) {
                Ok(vec![0x50 + r])
            } else if let Some(sr) = i8086_sreg_code(&reg) {
                Ok(vec![match sr {
                    0 => 0x06, // ES
                    1 => 0x0E, // CS
                    2 => 0x16, // SS
                    3 => 0x1E, // DS
                    _ => 0x1E,
                }])
            } else if let Some(addr) = resolve_operand_val(&reg, labels) {
                let lo = (addr & 0xFF) as u8;
                let hi = ((addr >> 8) & 0xFF) as u8;
                Ok(vec![0xFF, 0x36, lo, hi])
            } else {
                Ok(vec![0x50])
            }
        }

        "pop" => {
            let reg = operands.trim().to_ascii_lowercase();
            if let Some(r) = i8086_reg_code(&reg) {
                Ok(vec![0x58 + r])
            } else if let Some(sr) = i8086_sreg_code(&reg) {
                Ok(vec![match sr {
                    0 => 0x07, // ES
                    2 => 0x17, // SS
                    3 => 0x1F, // DS
                    _ => 0x1F,
                }])
            } else if let Some(addr) = resolve_operand_val(&reg, labels) {
                let lo = (addr & 0xFF) as u8;
                let hi = ((addr >> 8) & 0xFF) as u8;
                Ok(vec![0x8F, 0x06, lo, hi])
            } else {
                Ok(vec![0x58])
            }
        }

        "inc" => {
            let op = operands.trim().to_ascii_lowercase();
            if let Some(r) = i8086_reg_code(&op) {
                Ok(vec![0x40 + r])
            } else if let Some(r8) = i8086_reg8_code(&op) {
                Ok(vec![0xFE, 0xC0 | r8])
            } else if let Some(addr) = resolve_operand_val(&op, labels) {
                let lo = (addr & 0xFF) as u8;
                let hi = ((addr >> 8) & 0xFF) as u8;
                Ok(vec![0xFF, 0x06, lo, hi])
            } else {
                Ok(vec![0x40])
            }
        }

        "dec" => {
            let op = operands.trim().to_ascii_lowercase();
            if let Some(r) = i8086_reg_code(&op) {
                Ok(vec![0x48 + r])
            } else if let Some(r8) = i8086_reg8_code(&op) {
                Ok(vec![0xFE, 0xC8 | r8])
            } else if let Some(addr) = resolve_operand_val(&op, labels) {
                let lo = (addr & 0xFF) as u8;
                let hi = ((addr >> 8) & 0xFF) as u8;
                Ok(vec![0xFF, 0x0E, lo, hi])
            } else {
                Ok(vec![0x48])
            }
        }

        "lea" => {
            if let Some((dst, src)) = operands.split_once(',') {
                let dst = dst.trim().to_ascii_lowercase();
                let dst_code = i8086_reg_code(&dst).unwrap_or(0);
                let clean_src = src.trim().trim_start_matches('[').trim_end_matches(']');
                let addr = resolve_operand_val(clean_src, labels).unwrap_or(0);
                let lo = (addr & 0xFF) as u8;
                let hi = ((addr >> 8) & 0xFF) as u8;
                return Ok(vec![0x8D, (dst_code << 3) | 0x06, lo, hi]);
            }
            Ok(vec![0x90])
        }

        "mov" => {
            if let Some((dst, src)) = operands.split_once(',') {
                let dst = dst.trim().to_ascii_lowercase();
                let src = src.trim().to_ascii_lowercase();

                // Segment register to general register / General to segment
                if let Some(sreg) = i8086_sreg_code(&dst) {
                    let src_code = i8086_reg_code(&src).unwrap_or(0);
                    let modrm = 0xC0 | (sreg << 3) | src_code;
                    return Ok(vec![0x8E, modrm]);
                }
                if let Some(sreg) = i8086_sreg_code(&src) {
                    let dst_code = i8086_reg_code(&dst).unwrap_or(0);
                    let modrm = 0xC0 | (sreg << 3) | dst_code;
                    return Ok(vec![0x8C, modrm]);
                }

                // 16-bit register destination
                if let Some(dst_code) = i8086_reg_code(&dst) {
                    if let Some(src_code) = i8086_reg_code(&src) {
                        let modrm = 0xC0 | (src_code << 3) | dst_code;
                        return Ok(vec![0x89, modrm]);
                    } else if let Some(imm) = resolve_operand_val(&src, labels) {
                        let is_symbol = labels.contains_key(src.as_str()) || labels.keys().any(|k| k.eq_ignore_ascii_case(&src)) || src.starts_with('[');
                        if is_symbol && src != "@data" {
                            let lo = (imm & 0xFF) as u8;
                            let hi = ((imm >> 8) & 0xFF) as u8;
                            let modrm = (dst_code << 3) | 0x06;
                            return Ok(vec![0x8B, modrm, lo, hi]);
                        }
                        let lo = (imm & 0xFF) as u8;
                        let hi = ((imm >> 8) & 0xFF) as u8;
                        return Ok(vec![0xB8 + dst_code, lo, hi]);
                    }
                }

                // 8-bit register destination
                if let Some(dst_code) = i8086_reg8_code(&dst) {
                    if let Some(src_code) = i8086_reg8_code(&src) {
                        let modrm = 0xC0 | (src_code << 3) | dst_code;
                        return Ok(vec![0x88, modrm]);
                    } else if let Some(imm) = resolve_operand_val(&src, labels) {
                        return Ok(vec![0xB0 + dst_code, (imm & 0xFF) as u8]);
                    }
                }

                // Memory destination (e.g. mov largest, ax or mov [0x1000], ax)
                let clean_dst = dst.trim_start_matches('[').trim_end_matches(']');
                if let Some(addr) = resolve_operand_val(clean_dst, labels) {
                    let lo = (addr & 0xFF) as u8;
                    let hi = ((addr >> 8) & 0xFF) as u8;
                    if let Some(src_code) = i8086_reg_code(&src) {
                        let modrm = (src_code << 3) | 0x06;
                        return Ok(vec![0x89, modrm, lo, hi]);
                    } else if let Some(src_code) = i8086_reg8_code(&src) {
                        let modrm = (src_code << 3) | 0x06;
                        return Ok(vec![0x88, modrm, lo, hi]);
                    } else if let Some(imm) = resolve_operand_val(&src, labels) {
                        let imm_lo = (imm & 0xFF) as u8;
                        let imm_hi = ((imm >> 8) & 0xFF) as u8;
                        return Ok(vec![0xC7, 0x06, lo, hi, imm_lo, imm_hi]);
                    }
                }
            }
            Ok(vec![0x90])
        }

        "add" | "sub" | "and" | "or" | "xor" | "cmp" | "adc" | "sbb" | "test" => {
            let (reg_op, imm_subop, default_op) = match mnemonic {
                "add" => (0x01, 0, 0x01),
                "or"  => (0x09, 1, 0x09),
                "adc" => (0x11, 2, 0x11),
                "sbb" => (0x19, 3, 0x19),
                "and" => (0x21, 4, 0x21),
                "sub" => (0x29, 5, 0x29),
                "xor" => (0x31, 6, 0x31),
                "cmp" => (0x39, 7, 0x39),
                "test" => (0x85, 0, 0x85),
                _ => (0x01, 0, 0x01),
            };

            if let Some((dst, src)) = operands.split_once(',') {
                let dst = dst.trim().to_ascii_lowercase();
                let src = src.trim().to_ascii_lowercase();

                if let Some(dst_code) = i8086_reg_code(&dst) {
                    if let Some(src_code) = i8086_reg_code(&src) {
                        let modrm = 0xC0 | (src_code << 3) | dst_code;
                        return Ok(vec![reg_op, modrm]);
                    } else if let Some(src_addr) = resolve_operand_val(src.trim_start_matches('[').trim_end_matches(']'), labels) {
                        let is_symbol = labels.contains_key(src.as_str()) || labels.keys().any(|k| k.eq_ignore_ascii_case(&src)) || src.starts_with('[');
                        if is_symbol && src != "@data" {
                            let lo = (src_addr & 0xFF) as u8;
                            let hi = ((src_addr >> 8) & 0xFF) as u8;
                            let modrm = (dst_code << 3) | 0x06;
                            return Ok(vec![reg_op + 2, modrm, lo, hi]);
                        } else {
                            let lo = (src_addr & 0xFF) as u8;
                            let hi = ((src_addr >> 8) & 0xFF) as u8;
                            let modrm = 0xC0 | (imm_subop << 3) | dst_code;
                            return Ok(vec![0x81, modrm, lo, hi]);
                        }
                    }
                }

                // Memory destination: ALU [mem16], reg16
                let clean_dst = dst.trim_start_matches('[').trim_end_matches(']');
                if let Some(addr) = resolve_operand_val(clean_dst, labels) {
                    let lo = (addr & 0xFF) as u8;
                    let hi = ((addr >> 8) & 0xFF) as u8;
                    if let Some(src_code) = i8086_reg_code(&src) {
                        let modrm = (src_code << 3) | 0x06;
                        return Ok(vec![reg_op, modrm, lo, hi]);
                    } else if let Some(imm) = resolve_operand_val(&src, labels) {
                        let imm_lo = (imm & 0xFF) as u8;
                        let imm_hi = ((imm >> 8) & 0xFF) as u8;
                        let modrm = (imm_subop << 3) | 0x06;
                        return Ok(vec![0x81, modrm, lo, hi, imm_lo, imm_hi]);
                    }
                }
            }
            Ok(vec![default_op, 0xC0])
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

        "jo" | "jno" | "jb" | "jc" | "jnae" | "jnb" | "jnc" | "jae" | "jz" | "je" | "jnz" | "jne"
        | "jbe" | "jna" | "ja" | "jnbe" | "js" | "jns" | "jpe" | "jp" | "jpo" | "jnp"
        | "jl" | "jnge" | "jge" | "jnl" | "jle" | "jng" | "jg" | "jnle" | "loop" | "loope"
        | "loopz" | "loopne" | "loopnz" | "jcxz" => {
            let op = match mnemonic {
                "jo" => 0x70,
                "jno" => 0x71,
                "jb" | "jc" | "jnae" => 0x72,
                "jnb" | "jnc" | "jae" => 0x73,
                "je" | "jz" => 0x74,
                "jne" | "jnz" => 0x75,
                "jbe" | "jna" => 0x76,
                "ja" | "jnbe" => 0x77,
                "js" => 0x78,
                "jns" => 0x79,
                "jpe" | "jp" => 0x7A,
                "jpo" | "jnp" => 0x7B,
                "jl" | "jnge" => 0x7C,
                "jge" | "jnl" => 0x7D,
                "jle" | "jng" => 0x7E,
                "jg" | "jnle" => 0x7F,
                "loopne" | "loopnz" => 0xE0,
                "loope" | "loopz" => 0xE1,
                "loop" => 0xE2,
                "jcxz" => 0xE3,
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
    match reg.to_ascii_lowercase().as_str() {
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

fn i8086_reg8_code(reg: &str) -> Option<u8> {
    match reg.to_ascii_lowercase().as_str() {
        "al" => Some(0),
        "cl" => Some(1),
        "dl" => Some(2),
        "bl" => Some(3),
        "ah" => Some(4),
        "ch" => Some(5),
        "dh" => Some(6),
        "bh" => Some(7),
        _ => None,
    }
}

fn i8086_sreg_code(reg: &str) -> Option<u8> {
    match reg.to_ascii_lowercase().as_str() {
        "es" => Some(0),
        "cs" => Some(1),
        "ss" => Some(2),
        "ds" => Some(3),
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

        "addi" | "slti" | "sltiu" | "xori" | "ori" | "andi" | "slli" | "srli" | "srai" => {
            let parts: Vec<&str> = operands.split(',').collect();
            if parts.len() == 3 {
                let rd = riscv_reg(parts[0].trim()).unwrap_or(0);
                let rs1 = riscv_reg(parts[1].trim()).unwrap_or(0);
                let imm = resolve_operand_val(parts[2].trim(), labels).unwrap_or(0) as u32;
                let (funct3, funct7) = match mnemonic {
                    "addi" => (0b000, 0),
                    "slli" => (0b001, 0),
                    "slti" => (0b010, 0),
                    "sltiu" => (0b011, 0),
                    "xori" => (0b100, 0),
                    "srli" => (0b101, 0),
                    "srai" => (0b101, 0b0100000),
                    "ori" => (0b110, 0),
                    "andi" => (0b111, 0),
                    _ => (0, 0),
                };
                let imm_val = if mnemonic == "srai" { (funct7 << 5) | (imm & 0x1F) } else { imm & 0xFFF };
                let ins = (imm_val << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | 0x13;
                return Ok(ins.to_le_bytes().to_vec());
            }
            Ok(0x00000013u32.to_le_bytes().to_vec())
        }

        "add" | "sub" | "sll" | "slt" | "sltu" | "xor" | "srl" | "sra" | "or" | "and" | "mul" | "mulh" | "mulhsu" | "mulhu" | "div" | "divu" | "rem" | "remu" => {
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

                    // RV32M
                    "mul" => (0b000, 0b0000001),
                    "mulh" => (0b001, 0b0000001),
                    "mulhsu" => (0b010, 0b0000001),
                    "mulhu" => (0b011, 0b0000001),
                    "div" => (0b100, 0b0000001),
                    "divu" => (0b101, 0b0000001),
                    "rem" => (0b110, 0b0000001),
                    "remu" => (0b111, 0b0000001),
                    _ => (0, 0),
                };
                let ins = (funct7 << 25) | (rs2 << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | 0x33;
                return Ok(ins.to_le_bytes().to_vec());
            }
            Ok(0x00000013u32.to_le_bytes().to_vec())
        }

        "lw" | "lb" | "lbu" | "lh" | "lhu" => {
            let parts: Vec<&str> = operands.split(',').collect();
            if parts.len() == 2 {
                let rd = riscv_reg(parts[0].trim()).unwrap_or(0);
                let mem_str = parts[1].trim();
                let (imm_val, rs1) = parse_riscv_mem(mem_str, labels);
                let funct3 = match mnemonic {
                    "lb" => 0b000,
                    "lh" => 0b001,
                    "lw" => 0b010,
                    "lbu" => 0b100,
                    "lhu" => 0b101,
                    _ => 0b010,
                };
                let ins = ((imm_val & 0xFFF) << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | 0x03;
                return Ok(ins.to_le_bytes().to_vec());
            }
            Ok(0x00000013u32.to_le_bytes().to_vec())
        }

        "sw" | "sb" | "sh" => {
            let parts: Vec<&str> = operands.split(',').collect();
            if parts.len() == 2 {
                let rs2 = riscv_reg(parts[0].trim()).unwrap_or(0);
                let mem_str = parts[1].trim();
                let (imm_val, rs1) = parse_riscv_mem(mem_str, labels);
                let funct3 = match mnemonic {
                    "sb" => 0b000,
                    "sh" => 0b001,
                    "sw" => 0b010,
                    _ => 0b010,
                };
                let imm_hi = (imm_val >> 5) & 0x7F;
                let imm_lo = imm_val & 0x1F;
                let ins = (imm_hi << 25) | (rs2 << 20) | (rs1 << 15) | (funct3 << 12) | (imm_lo << 7) | 0x23;
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

fn parse_riscv_mem(mem_str: &str, labels: &BTreeMap<String, u64>) -> (u32, u32) {
    if let Some((offset_str, reg_part)) = mem_str.split_once('(') {
        let imm_val = resolve_operand_val(offset_str.trim(), labels).unwrap_or(0) as u32;
        let reg_str = reg_part.trim_end_matches(')').trim();
        let rs1 = riscv_reg(reg_str).unwrap_or(0);
        (imm_val, rs1)
    } else {
        (0, riscv_reg(mem_str).unwrap_or(0))
    }
}

fn encode_avr(
    mnemonic: &str,
    operands: &str,
    pc: u64,
    labels: &BTreeMap<String, u64>,
) -> Result<Vec<u8>, ProjectError> {
    match mnemonic {
        "nop" => Ok(vec![0x00, 0x00]),
        "ret" => Ok(vec![0x08, 0x95]),
        "reti" => Ok(vec![0x18, 0x95]),
        "sleep" => Ok(vec![0x88, 0x95]),
        "break" => Ok(vec![0x98, 0x95]),
        "wdr" => Ok(vec![0xA8, 0x95]),

        "sec" => Ok(vec![0x08, 0x94]),
        "clc" => Ok(vec![0x88, 0x94]),
        "sez" => Ok(vec![0x18, 0x94]),
        "clz" => Ok(vec![0x98, 0x94]),
        "sen" => Ok(vec![0x28, 0x94]),
        "cln" => Ok(vec![0xA8, 0x94]),
        "sev" => Ok(vec![0x38, 0x94]),
        "clv" => Ok(vec![0xB8, 0x94]),
        "ses" => Ok(vec![0x48, 0x94]),
        "cls" => Ok(vec![0xC8, 0x94]),
        "seh" => Ok(vec![0x58, 0x94]),
        "clh" => Ok(vec![0xD8, 0x94]),
        "set" => Ok(vec![0x68, 0x94]),
        "clt" => Ok(vec![0xE8, 0x94]),
        "sei" => Ok(vec![0x78, 0x94]),
        "cli" => Ok(vec![0xF8, 0x94]),

        "clr" => {
            let rd = avr_reg(operands.trim()).unwrap_or(0);
            let ins = 0x2400 | (((rd & 0x10) as u16) << 5) | ((rd as u16) << 4) | ((rd & 0x0F) as u16);
            Ok(ins.to_le_bytes().to_vec())
        }
        "ser" => {
            let rd = avr_reg(operands.trim()).unwrap_or(16);
            let d = rd.saturating_sub(16);
            let ins = 0xEF0F | ((d as u16) << 4);
            Ok(ins.to_le_bytes().to_vec())
        }
        "tst" => {
            let rd = avr_reg(operands.trim()).unwrap_or(0);
            let ins = 0x2000 | (((rd & 0x10) as u16) << 5) | ((rd as u16) << 4) | ((rd & 0x0F) as u16);
            Ok(ins.to_le_bytes().to_vec())
        }

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

        "subi" | "sbci" | "ori" | "andi" | "cpi" | "sbr" | "cbr" => {
            if let Some((rd_str, imm_str)) = operands.split_once(',') {
                let rd = avr_reg(rd_str.trim()).unwrap_or(16);
                let mut imm = resolve_operand_val(imm_str.trim(), labels).unwrap_or(0) as u8;
                if mnemonic == "cbr" {
                    imm = !imm;
                }
                if (16..=31).contains(&rd) {
                    let d = rd - 16;
                    let k_high = (imm >> 4) & 0x0F;
                    let k_low = imm & 0x0F;
                    let base_op = match mnemonic {
                        "cpi" => 0x3000,
                        "sbci" => 0x4000,
                        "subi" => 0x5000,
                        "ori" | "sbr" => 0x6000,
                        "andi" | "cbr" => 0x7000,
                        _ => 0x3000,
                    };
                    let ins = base_op | ((k_high as u16) << 8) | ((d as u16) << 4) | (k_low as u16);
                    return Ok(ins.to_le_bytes().to_vec());
                }
            }
            Ok(vec![0x00, 0x00])
        }

        "mov" | "add" | "adc" | "sub" | "sbc" | "and" | "or" | "eor" | "cp" | "cpc" => {
            if let Some((rd_str, rr_str)) = operands.split_once(',') {
                let rd = avr_reg(rd_str.trim()).unwrap_or(0);
                let rr = avr_reg(rr_str.trim()).unwrap_or(0);
                let base_op = match mnemonic {
                    "cpc" => 0x0400,
                    "sbc" => 0x0800,
                    "add" => 0x0C00,
                    "cp" => 0x1400,
                    "sub" => 0x1800,
                    "adc" => 0x1C00,
                    "and" => 0x2000,
                    "eor" => 0x2400,
                    "or" => 0x2800,
                    "mov" => 0x2C00,
                    _ => 0x2C00,
                };
                let ins = base_op | (((rr & 0x10) as u16) << 5) | ((rd as u16) << 4) | ((rr & 0x0F) as u16);
                return Ok(ins.to_le_bytes().to_vec());
            }
            Ok(vec![0x00, 0x00])
        }

        "movw" => {
            if let Some((rd_str, rr_str)) = operands.split_once(',') {
                let rd = avr_reg(rd_str.trim()).unwrap_or(0);
                let rr = avr_reg(rr_str.trim()).unwrap_or(0);
                let ins = 0x0100 | (((rd as u16) >> 1) << 4) | ((rr as u16) >> 1);
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

        "com" | "neg" | "swap" | "inc" | "asr" | "lsr" | "ror" | "dec" => {
            let rd = avr_reg(operands.trim()).unwrap_or(0);
            let sub_op = match mnemonic {
                "com" => 0x9400,
                "neg" => 0x9401,
                "swap" => 0x9402,
                "inc" => 0x9403,
                "asr" => 0x9405,
                "lsr" => 0x9406,
                "ror" => 0x9407,
                "dec" => 0x940A,
                _ => 0x9400,
            };
            let ins = sub_op | ((rd as u16) << 4);
            Ok(ins.to_le_bytes().to_vec())
        }

        "rjmp" => {
            let target = resolve_operand_val(operands.trim(), labels).unwrap_or((pc / 2) + 1);
            let k = (target as i64) - ((pc / 2) as i64 + 1);
            let ins = 0xC000 | ((k as u16) & 0x0FFF);
            Ok(ins.to_le_bytes().to_vec())
        }
        "rcall" => {
            let target = resolve_operand_val(operands.trim(), labels).unwrap_or((pc / 2) + 1);
            let k = (target as i64) - ((pc / 2) as i64 + 1);
            let ins = 0xD000 | ((k as u16) & 0x0FFF);
            Ok(ins.to_le_bytes().to_vec())
        }

        "breq" | "brne" | "brcs" | "brlo" | "brcc" | "brsh" | "brmi" | "brpl" | "brlt" | "brge"
        | "brhs" | "brhc" | "brts" | "brtc" | "brvs" | "brvc" | "brie" | "brid" => {
            let (bit, is_clear) = match mnemonic {
                "brcs" | "brlo" => (0, false),
                "brcc" | "brsh" => (0, true),
                "breq" => (1, false),
                "brne" => (1, true),
                "brmi" => (2, false),
                "brpl" => (2, true),
                "brvs" => (3, false),
                "brvc" => (3, true),
                "brlt" => (4, false),
                "brge" => (4, true),
                "brhs" => (5, false),
                "brhc" => (5, true),
                "brts" => (6, false),
                "brtc" => (6, true),
                "brie" => (7, false),
                "brid" => (7, true),
                _ => (1, false),
            };
            let target = resolve_operand_val(operands.trim(), labels).unwrap_or((pc / 2) + 1);
            let k = (target as i64) - ((pc / 2) as i64 + 1);
            let ins = 0xF000 | (if is_clear { 0x0400 } else { 0 }) | (((k as u16) & 0x7F) << 3) | (bit as u16);
            Ok(ins.to_le_bytes().to_vec())
        }

        "in" => {
            if let Some((rd_str, a_str)) = operands.split_once(',') {
                let rd = avr_reg(rd_str.trim()).unwrap_or(0);
                let a = (resolve_operand_val(a_str.trim(), labels).unwrap_or(0) & 0x3F) as u16;
                let ins = 0xB000 | ((a & 0x30) << 5) | ((rd as u16) << 4) | (a & 0x0F);
                return Ok(ins.to_le_bytes().to_vec());
            }
            Ok(vec![0x00, 0x00])
        }

        "out" => {
            if let Some((a_str, rr_str)) = operands.split_once(',') {
                let a = (resolve_operand_val(a_str.trim(), labels).unwrap_or(0) & 0x3F) as u16;
                let rr = avr_reg(rr_str.trim()).unwrap_or(0);
                let ins = 0xB800 | ((a & 0x30) << 5) | ((rr as u16) << 4) | (a & 0x0F);
                return Ok(ins.to_le_bytes().to_vec());
            }
            Ok(vec![0x00, 0x00])
        }

        "sbrs" | "sbrc" => {
            let is_set = mnemonic == "sbrs";
            if let Some((rr_str, bit_str)) = operands.split_once(',') {
                let rr = avr_reg(rr_str.trim()).unwrap_or(0);
                let bit = (resolve_operand_val(bit_str.trim(), labels).unwrap_or(0) & 7) as u16;
                let base = if is_set { 0xFE00 } else { 0xFC00 };
                let ins = base | (((rr as u16) & 0x1F) << 4) | bit;
                return Ok(ins.to_le_bytes().to_vec());
            }
            Ok(vec![0x00, 0x00])
        }

        "sbi" | "cbi" | "sbis" | "sbic" => {
            if let Some((a_str, bit_str)) = operands.split_once(',') {
                let a = (resolve_operand_val(a_str.trim(), labels).unwrap_or(0) & 0x1F) as u16;
                let bit = (resolve_operand_val(bit_str.trim(), labels).unwrap_or(0) & 7) as u16;
                let base = match mnemonic {
                    "cbi" => 0x9800,
                    "sbic" => 0x9900,
                    "sbi" => 0x9A00,
                    "sbis" => 0x9B00,
                    _ => 0x9A00,
                };
                let ins = base | (a << 3) | bit;
                return Ok(ins.to_le_bytes().to_vec());
            }
            Ok(vec![0x00, 0x00])
        }

        "lpm" => {
            let clean = operands.trim().to_ascii_lowercase();
            if clean.is_empty() || clean == "r0, z" || clean == "r0,z" {
                Ok(vec![0xC8, 0x95]) // 0x95C8
            } else if let Some((rd_str, z_str)) = clean.split_once(',') {
                let rd = avr_reg(rd_str.trim()).unwrap_or(0);
                let z = z_str.trim();
                let op = if z == "z+" { 0x9005 } else { 0x9004 };
                let ins = op | (((rd as u16) & 0x1F) << 4);
                Ok(ins.to_le_bytes().to_vec())
            } else {
                Ok(vec![0xC8, 0x95])
            }
        }

        "lds" => {
            if let Some((rd_str, k_str)) = operands.split_once(',') {
                let rd = avr_reg(rd_str.trim()).unwrap_or(0);
                let k = resolve_operand_val(k_str.trim(), labels).unwrap_or(0) as u16;
                let ins1 = 0x9000 | (((rd as u16) & 0x1F) << 4);
                let ins2 = k;
                let mut out = Vec::with_capacity(4);
                out.extend_from_slice(&ins1.to_le_bytes());
                out.extend_from_slice(&ins2.to_le_bytes());
                return Ok(out);
            }
            Ok(vec![0x00, 0x00, 0x00, 0x00])
        }

        "sts" => {
            if let Some((k_str, rr_str)) = operands.split_once(',') {
                let rr = avr_reg(rr_str.trim()).unwrap_or(0);
                let k = resolve_operand_val(k_str.trim(), labels).unwrap_or(0) as u16;
                let ins1 = 0x9200 | (((rr as u16) & 0x1F) << 4);
                let ins2 = k;
                let mut out = Vec::with_capacity(4);
                out.extend_from_slice(&ins1.to_le_bytes());
                out.extend_from_slice(&ins2.to_le_bytes());
                return Ok(out);
            }
            Ok(vec![0x00, 0x00, 0x00, 0x00])
        }

        _ => Err(ProjectError::AssembleError(format!(
            "Unsupported AVR instruction: '{}'",
            mnemonic
        ))),
    }
}

fn avr_reg(reg: &str) -> Option<u8> {
    let lower = reg.to_ascii_lowercase();
    match lower.as_str() {
        "zh" | "r31" => Some(31),
        "zl" | "r30" => Some(30),
        "yh" | "r29" => Some(29),
        "yl" | "r28" => Some(28),
        "xh" | "r27" => Some(27),
        "xl" | "r26" => Some(26),
        _ => {
            if let Some(num_str) = lower.strip_prefix('r') {
                num_str.parse::<u8>().ok().filter(|&n| n < 32)
            } else {
                None
            }
        }
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
    eval_expression(op, labels)
}
