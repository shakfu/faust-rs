//! `.fbc` serialization — read/write FBC bytecode in the interpreter text format.
//!
//! # Source provenance (C++)
//! - Write: `interpreter_dsp_factory_aux::write()` in `interpreter_dsp_aux.hh`,
//!   `FBCBasicInstruction::write()` / `FIRBlockStoreRealInstruction::write()` /
//!   `FIRBlockStoreIntInstruction::write()` / `FIRUserInterfaceInstruction::write()` /
//!   `FIRMetaInstruction::write()` in `interpreter_bytecode.hh`.
//! - Read: `interpreter_dsp_factory_aux::read()` in `interpreter_dsp.hh`,
//!   `readCodeBlock()` / `readCodeInstruction()` / `readUIBlock()` /
//!   `readMetaBlock()` in `interpreter_dsp_aux.hh`.
//!
//! # Design notes
//! - Two modes: **normal** (human-readable labels) and **small** (compact tokens).
//!   Both are text-based, line-oriented.
//! - Sub-blocks (for If/Select/Loop instructions) are written/read recursively.
//! - String quoting: labels, keys, and values are wrapped in double quotes
//!   (`quote1`/`unquote1`).
//! - Real values use max precision via `{:.digits$}` formatting where `digits`
//!   is `std::mem::size_of::<R>() * 3 + 1` (matching C++ `digits10 + 1`).

use std::fmt::Write as FmtWrite;
use std::io::{self, BufRead, Write};

use super::bytecode::{
    BlockId, BlockStoreData, FbcBlock, FbcBlockArena, FbcInstruction, FbcMetaInstruction,
    FbcUiInstruction,
};
use super::factory::FbcDspFactory;
use super::opcode::{FBC_INSTRUCTION_NAMES, FbcOpcode, INTERP_FILE_VERSION};
use super::real::FbcReal;

// ── Constants ──────────────────────────────────────────────────────────────

/// Faust version string written into `.fbc` headers.
///
/// This should match the Faust compiler version that generates the bytecode.
/// For the Rust port, we use a fixed version string.
pub const FAUST_VERSION: &str = "2.84.5-rust";

// ── Error type ─────────────────────────────────────────────────────────────

/// Errors that can occur during `.fbc` deserialization.
#[derive(Clone, Debug)]
pub enum FbcSerialError {
    /// Unexpected token in the input stream.
    UnexpectedToken { expected: String, got: String },
    /// File format version mismatch.
    VersionMismatch { expected: u32, got: u32 },
    /// REAL type mismatch (e.g., file says "double" but we're reading as f32).
    TypeMismatch { expected: String, got: String },
    /// Failed to parse an integer.
    ParseInt(String),
    /// Failed to parse a real number.
    ParseReal(String),
    /// I/O error.
    Io(String),
    /// Unexpected end of input.
    UnexpectedEof,
}

impl std::fmt::Display for FbcSerialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedToken { expected, got } => {
                write!(f, "expected token '{expected}', got '{got}'")
            }
            Self::VersionMismatch { expected, got } => {
                write!(f, "file version {got} != compiled version {expected}")
            }
            Self::TypeMismatch { expected, got } => {
                write!(f, "REAL type mismatch: expected '{expected}', got '{got}'")
            }
            Self::ParseInt(s) => write!(f, "failed to parse integer: '{s}'"),
            Self::ParseReal(s) => write!(f, "failed to parse real: '{s}'"),
            Self::Io(s) => write!(f, "I/O error: {s}"),
            Self::UnexpectedEof => write!(f, "unexpected end of input"),
        }
    }
}

impl From<io::Error> for FbcSerialError {
    fn from(e: io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

// ── String quoting (C++ quote1 / unquote1) ─────────────────────────────────

/// Wraps a string in double quotes.
///
/// # Source provenance (C++)
/// - `quote1()` in `interpreter_bytecode.hh`.
fn quote1(s: &str) -> String {
    format!("\"{s}\"")
}

/// Removes surrounding double quotes from a string, if present.
///
/// # Source provenance (C++)
/// - `unquote1()` in `interpreter_bytecode.hh`.
#[allow(dead_code)]
fn unquote1(s: &str) -> String {
    if s.starts_with('"') && s.len() >= 2 {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

// ── Real precision ─────────────────────────────────────────────────────────

/// Returns the number of significant digits for REAL output.
///
/// Matches C++ `std::numeric_limits<REAL>::digits10 + 1`:
/// - `f32`: 6 + 1 = 7
/// - `f64`: 15 + 1 = 16
fn real_precision<R: FbcReal>() -> usize {
    if std::mem::size_of::<R>() == 4 {
        7 // f32: digits10 = 6, +1
    } else {
        16 // f64: digits10 = 15, +1
    }
}

/// Returns "float" or "double" for the REAL type.
fn real_type_name<R: FbcReal>() -> &'static str {
    if std::mem::size_of::<R>() == 4 {
        "float"
    } else {
        "double"
    }
}

/// Formats a REAL value with full precision.
fn fmt_real<R: FbcReal>(v: R) -> String {
    let prec = real_precision::<R>();
    format!("{:.prec$}", v)
}

// ═══════════════════════════════════════════════════════════════════════════
// WRITE PATH
// ═══════════════════════════════════════════════════════════════════════════

/// Serializes an [`FbcDspFactory`] to `.fbc` text format.
///
/// # Arguments
/// - `factory`: the factory to serialize.
/// - `writer`: output stream.
/// - `small`: if `true`, use compact (small) format; otherwise, normal format.
///
/// # Source provenance (C++)
/// - `interpreter_dsp_factory_aux::write()` in `interpreter_dsp_aux.hh`.
pub fn write_fbc<R: FbcReal>(
    factory: &FbcDspFactory<R>,
    writer: &mut dyn Write,
    small: bool,
) -> io::Result<()> {
    if small {
        writeln!(writer, "i {}", real_type_name::<R>())?;
        writeln!(writer, "f {INTERP_FILE_VERSION}")?;
        writeln!(writer, "v {FAUST_VERSION}")?;
        writeln!(writer, "c {}", factory.compile_options)?;
        writeln!(writer, "n {}", factory.name)?;
        writeln!(writer, "s {}", factory.sha_key)?;
        writeln!(writer, "o {}", factory.opt_level)?;
        writeln!(writer, "i {} o {}", factory.num_inputs, factory.num_outputs)?;
        writeln!(
            writer,
            "i {} r {} s {} c {} i {}",
            factory.int_heap_size,
            factory.real_heap_size,
            factory.sr_offset,
            factory.count_offset,
            factory.iota_offset
        )?;

        writeln!(writer, "m")?;
        write_meta_block(&factory.meta_block, writer, small)?;

        writeln!(writer, "u")?;
        write_ui_block(&factory.ui_block, writer, small)?;

        writeln!(writer, "s")?;
        write_code_block(&factory.arena, factory.static_init_block, writer, small)?;

        writeln!(writer, "i")?;
        write_code_block(&factory.arena, factory.init_block, writer, small)?;

        writeln!(writer, "c")?;
        write_code_block(&factory.arena, factory.reset_ui_block, writer, small)?;

        writeln!(writer, "c")?;
        write_code_block(&factory.arena, factory.clear_block, writer, small)?;

        writeln!(writer, "c")?;
        write_code_block(&factory.arena, factory.compute_block, writer, small)?;

        writeln!(writer, "d")?;
        write_code_block(&factory.arena, factory.compute_dsp_block, writer, small)?;
    } else {
        writeln!(writer, "interpreter_dsp_factory {}", real_type_name::<R>())?;
        writeln!(writer, "file_version {INTERP_FILE_VERSION}")?;
        writeln!(writer, "Faust version {FAUST_VERSION}")?;
        writeln!(writer, "compile_options {}", factory.compile_options)?;
        writeln!(writer, "name {}", factory.name)?;
        writeln!(writer, "sha_key {}", factory.sha_key)?;
        writeln!(writer, "opt_level {}", factory.opt_level)?;
        writeln!(
            writer,
            "inputs {} outputs {}",
            factory.num_inputs, factory.num_outputs
        )?;
        writeln!(
            writer,
            "int_heap_size {} real_heap_size {} sr_offset {} count_offset {} iota_offset {}",
            factory.int_heap_size,
            factory.real_heap_size,
            factory.sr_offset,
            factory.count_offset,
            factory.iota_offset
        )?;

        writeln!(writer, "meta_block")?;
        write_meta_block(&factory.meta_block, writer, small)?;

        writeln!(writer, "user_interface_block")?;
        write_ui_block(&factory.ui_block, writer, small)?;

        writeln!(writer, "static_init_block")?;
        write_code_block(&factory.arena, factory.static_init_block, writer, small)?;

        writeln!(writer, "constants_block")?;
        write_code_block(&factory.arena, factory.init_block, writer, small)?;

        writeln!(writer, "reset_ui")?;
        write_code_block(&factory.arena, factory.reset_ui_block, writer, small)?;

        writeln!(writer, "clear_block")?;
        write_code_block(&factory.arena, factory.clear_block, writer, small)?;

        writeln!(writer, "control_block")?;
        write_code_block(&factory.arena, factory.compute_block, writer, small)?;

        writeln!(writer, "dsp_block")?;
        write_code_block(&factory.arena, factory.compute_dsp_block, writer, small)?;
    }

    Ok(())
}

/// Writes a meta block (block_size + meta instructions).
fn write_meta_block(
    meta: &[FbcMetaInstruction],
    writer: &mut dyn Write,
    small: bool,
) -> io::Result<()> {
    writeln!(writer, "block_size {}", meta.len())?;
    for m in meta {
        write_meta_instruction(m, writer, small)?;
    }
    Ok(())
}

/// Writes a single meta instruction.
///
/// # Source provenance (C++)
/// - `FIRMetaInstruction::write()` in `interpreter_bytecode.hh`.
fn write_meta_instruction(
    m: &FbcMetaInstruction,
    writer: &mut dyn Write,
    small: bool,
) -> io::Result<()> {
    if small {
        writeln!(writer, "m k {} v {}", quote1(&m.key), quote1(&m.value))
    } else {
        writeln!(
            writer,
            "meta key {} value {}",
            quote1(&m.key),
            quote1(&m.value)
        )
    }
}

/// Writes a UI block (block_size + UI instructions).
fn write_ui_block<R: FbcReal>(
    ui: &[FbcUiInstruction<R>],
    writer: &mut dyn Write,
    small: bool,
) -> io::Result<()> {
    writeln!(writer, "block_size {}", ui.len())?;
    for u in ui {
        write_ui_instruction(u, writer, small)?;
    }
    Ok(())
}

/// Writes a single UI instruction.
///
/// # Source provenance (C++)
/// - `FIRUserInterfaceInstruction::write()` in `interpreter_bytecode.hh`.
fn write_ui_instruction<R: FbcReal>(
    u: &FbcUiInstruction<R>,
    writer: &mut dyn Write,
    small: bool,
) -> io::Result<()> {
    let opcode_num = u.opcode as u16;
    if small {
        writeln!(
            writer,
            "o {} k  o {} l {} k {} v {} i {} m {} m {} s {}",
            opcode_num,
            u.offset,
            quote1(&u.label),
            quote1(&u.key),
            quote1(&u.value),
            fmt_real(u.init),
            fmt_real(u.min),
            fmt_real(u.max),
            fmt_real(u.step),
        )
    } else {
        let opcode_name = FBC_INSTRUCTION_NAMES
            .get(opcode_num as usize)
            .unwrap_or(&"unknown");
        writeln!(
            writer,
            "opcode {} {} offset {} label {} key {} value {} init {} min {} max {} step {}",
            opcode_num,
            opcode_name,
            u.offset,
            quote1(&u.label),
            quote1(&u.key),
            quote1(&u.value),
            fmt_real(u.init),
            fmt_real(u.min),
            fmt_real(u.max),
            fmt_real(u.step),
        )
    }
}

/// Writes a code block (block_size + instructions, with recursive sub-blocks).
///
/// # Source provenance (C++)
/// - `FBCBlockInstruction::write()` in `interpreter_bytecode.hh`.
fn write_code_block<R: FbcReal>(
    arena: &FbcBlockArena<R>,
    block_id: BlockId,
    writer: &mut dyn Write,
    small: bool,
) -> io::Result<()> {
    let block = arena.get(block_id);
    writeln!(writer, "block_size {}", block.instructions.len())?;
    for (idx, instr) in block.instructions.iter().enumerate() {
        // Check for block-store data at this instruction index.
        let store_data = block
            .block_store_data
            .iter()
            .find(|(i, _)| *i == idx)
            .map(|(_, d)| d);

        if let Some(data) = store_data {
            write_block_store_instruction(instr, data, writer, small)?;
        } else {
            write_instruction(instr, writer, small)?;
        }

        // Write sub-blocks for branching instructions.
        if instr.get_branch1().is_some() {
            write_code_block(arena, instr.get_branch1().unwrap(), writer, small)?;
        }
        if instr.get_branch2().is_some() {
            write_code_block(arena, instr.get_branch2().unwrap(), writer, small)?;
        }
    }
    Ok(())
}

/// Writes a single regular instruction.
///
/// # Source provenance (C++)
/// - `FBCBasicInstruction::write()` in `interpreter_bytecode.hh`.
fn write_instruction<R: FbcReal>(
    instr: &FbcInstruction<R>,
    writer: &mut dyn Write,
    small: bool,
) -> io::Result<()> {
    let opcode_num = instr.opcode as u16;
    if small {
        let mut line = format!(
            "o {} k  i {} r {} o {} o {}",
            opcode_num,
            instr.int_value,
            fmt_real(instr.real_value),
            instr.offset1,
            instr.offset2,
        );
        if !instr.name.is_empty() {
            write!(line, " n {}", instr.name).unwrap();
        }
        writeln!(writer, "{line}")
    } else {
        let opcode_name = FBC_INSTRUCTION_NAMES
            .get(opcode_num as usize)
            .unwrap_or(&"unknown");
        let mut line = format!(
            "opcode {} {} int {} real {} offset1 {} offset2 {}",
            opcode_num,
            opcode_name,
            instr.int_value,
            fmt_real(instr.real_value),
            instr.offset1,
            instr.offset2,
        );
        if !instr.name.is_empty() {
            write!(line, " name {}", instr.name).unwrap();
        }
        writeln!(writer, "{line}")
    }
}

/// Writes a BlockStoreReal or BlockStoreInt instruction with its data line.
///
/// # Source provenance (C++)
/// - `FIRBlockStoreRealInstruction::write()` and
///   `FIRBlockStoreIntInstruction::write()` in `interpreter_bytecode.hh`.
fn write_block_store_instruction<R: FbcReal>(
    instr: &FbcInstruction<R>,
    data: &BlockStoreData<R>,
    writer: &mut dyn Write,
    small: bool,
) -> io::Result<()> {
    let opcode_num = instr.opcode as u16;
    match data {
        BlockStoreData::Real(values) => {
            if small {
                writeln!(
                    writer,
                    "o {} k  o {} o {} s {}",
                    opcode_num,
                    instr.offset1,
                    instr.offset2,
                    values.len()
                )?;
            } else {
                let opcode_name = FBC_INSTRUCTION_NAMES
                    .get(opcode_num as usize)
                    .unwrap_or(&"unknown");
                writeln!(
                    writer,
                    "opcode {} {} offset1 {} offset2 {} size {}",
                    opcode_num,
                    opcode_name,
                    instr.offset1,
                    instr.offset2,
                    values.len()
                )?;
            }
            // Write data values on a single line.
            let mut line = String::new();
            for (i, v) in values.iter().enumerate() {
                if i > 0 {
                    line.push(' ');
                }
                write!(line, "{}", fmt_real(*v)).unwrap();
            }
            writeln!(writer, "{line}")?;
        }
        BlockStoreData::Int(values) => {
            if small {
                writeln!(
                    writer,
                    "o {} k  o {} o {} s {}",
                    opcode_num,
                    instr.offset1,
                    instr.offset2,
                    values.len()
                )?;
            } else {
                let opcode_name = FBC_INSTRUCTION_NAMES
                    .get(opcode_num as usize)
                    .unwrap_or(&"unknown");
                writeln!(
                    writer,
                    "opcode {} {} offset1 {} offset2 {} size {}",
                    opcode_num,
                    opcode_name,
                    instr.offset1,
                    instr.offset2,
                    values.len()
                )?;
            }
            // Write data values on a single line.
            let mut line = String::new();
            for (i, v) in values.iter().enumerate() {
                if i > 0 {
                    line.push(' ');
                }
                write!(line, "{v}").unwrap();
            }
            writeln!(writer, "{line}")?;
        }
    }
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// READ PATH
// ═══════════════════════════════════════════════════════════════════════════

/// Deserializes an [`FbcDspFactory`] from `.fbc` text format.
///
/// The reader must be positioned at the start of the `.fbc` content
/// (the `"interpreter_dsp_factory float|double"` line).
///
/// # Source provenance (C++)
/// - `read_real_type()` in `interpreter_dsp_aux.cpp` (header line).
/// - `interpreter_dsp_factory_aux::read()` in `interpreter_dsp.hh` (body).
pub fn read_fbc<R: FbcReal>(reader: &mut dyn BufRead) -> Result<FbcDspFactory<R>, FbcSerialError> {
    // ── Header line: "interpreter_dsp_factory float|double" ────────────
    let header_line = read_line(reader)?;
    let mut header_tokens = header_line.split_whitespace();
    check_token(header_tokens.next(), "interpreter_dsp_factory")?;
    let real_type = header_tokens
        .next()
        .ok_or(FbcSerialError::UnexpectedEof)?
        .to_string();
    let expected_type = real_type_name::<R>();
    if real_type != expected_type {
        return Err(FbcSerialError::TypeMismatch {
            expected: expected_type.to_string(),
            got: real_type,
        });
    }

    // ── file_version ───────────────────────────────────────────────────
    let line = read_line(reader)?;
    let mut tokens = line.split_whitespace();
    check_token(tokens.next(), "file_version")?;
    let file_num = parse_i32(tokens.next())?;
    if file_num as u32 != INTERP_FILE_VERSION {
        return Err(FbcSerialError::VersionMismatch {
            expected: INTERP_FILE_VERSION,
            got: file_num as u32,
        });
    }

    // ── Faust version (read and discard) ───────────────────────────────
    let line = read_line(reader)?;
    let mut tokens = line.split_whitespace();
    check_token(tokens.next(), "Faust")?;
    check_token(tokens.next(), "version")?;
    // Version string is read but not stored.

    // ── compile_options ────────────────────────────────────────────────
    let line = read_line(reader)?;
    let compile_options = line
        .strip_prefix("compile_options ")
        .unwrap_or("")
        .to_string();

    // ── name ───────────────────────────────────────────────────────────
    let line = read_line(reader)?;
    let mut tokens = line.split_whitespace();
    check_token(tokens.next(), "name")?;
    let name = tokens.next().unwrap_or("").to_string();

    // ── sha_key ────────────────────────────────────────────────────────
    let line = read_line(reader)?;
    let mut tokens = line.split_whitespace();
    check_token(tokens.next(), "sha_key")?;
    let sha_key = tokens.next().unwrap_or("").to_string();

    // ── opt_level ──────────────────────────────────────────────────────
    let line = read_line(reader)?;
    let mut tokens = line.split_whitespace();
    check_token(tokens.next(), "opt_level")?;
    let opt_level = parse_i32(tokens.next())?;

    // ── inputs / outputs ───────────────────────────────────────────────
    let line = read_line(reader)?;
    let mut tokens = line.split_whitespace();
    check_token(tokens.next(), "inputs")?;
    let num_inputs = parse_i32(tokens.next())?;
    check_token(tokens.next(), "outputs")?;
    let num_outputs = parse_i32(tokens.next())?;

    // ── heap sizes / offsets ───────────────────────────────────────────
    let line = read_line(reader)?;
    let mut tokens = line.split_whitespace();
    check_token(tokens.next(), "int_heap_size")?;
    let int_heap_size = parse_i32(tokens.next())?;
    check_token(tokens.next(), "real_heap_size")?;
    let real_heap_size = parse_i32(tokens.next())?;
    check_token(tokens.next(), "sr_offset")?;
    let sr_offset = parse_i32(tokens.next())?;
    check_token(tokens.next(), "count_offset")?;
    let count_offset = parse_i32(tokens.next())?;
    check_token(tokens.next(), "iota_offset")?;
    let iota_offset = parse_i32(tokens.next())?;

    // ── meta_block ─────────────────────────────────────────────────────
    let _label = read_line(reader)?; // "meta_block"
    let meta_block = read_meta_block(reader)?;

    // ── user_interface_block ───────────────────────────────────────────
    let _label = read_line(reader)?; // "user_interface_block"
    let ui_block = read_ui_block::<R>(reader)?;

    // ── 6 code blocks ──────────────────────────────────────────────────
    let mut arena = FbcBlockArena::<R>::new();

    let _label = read_line(reader)?; // "static_init_block"
    let static_init_block = read_code_block(reader, &mut arena)?;

    let _label = read_line(reader)?; // "constants_block"
    let init_block = read_code_block(reader, &mut arena)?;

    let _label = read_line(reader)?; // "reset_ui"
    let reset_ui_block = read_code_block(reader, &mut arena)?;

    let _label = read_line(reader)?; // "clear_block"
    let clear_block = read_code_block(reader, &mut arena)?;

    let _label = read_line(reader)?; // "control_block"
    let compute_block = read_code_block(reader, &mut arena)?;

    let _label = read_line(reader)?; // "dsp_block"
    let compute_dsp_block = read_code_block(reader, &mut arena)?;

    Ok(FbcDspFactory::new(
        name,
        sha_key,
        compile_options,
        file_num as u32,
        num_inputs,
        num_outputs,
        int_heap_size,
        real_heap_size,
        sr_offset,
        count_offset,
        iota_offset,
        opt_level,
        arena,
        meta_block,
        ui_block,
        static_init_block,
        init_block,
        reset_ui_block,
        clear_block,
        compute_block,
        compute_dsp_block,
    ))
}

// ── Read helpers ───────────────────────────────────────────────────────────

/// Reads a single line from the reader, trimming the trailing newline.
fn read_line(reader: &mut dyn BufRead) -> Result<String, FbcSerialError> {
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Err(FbcSerialError::UnexpectedEof);
    }
    // Trim trailing newline/carriage return.
    while line.ends_with('\n') || line.ends_with('\r') {
        line.pop();
    }
    Ok(line)
}

/// Checks that a token matches the expected string.
fn check_token(token: Option<&str>, expected: &str) -> Result<(), FbcSerialError> {
    match token {
        Some(t) if t == expected => Ok(()),
        Some(t) => Err(FbcSerialError::UnexpectedToken {
            expected: expected.to_string(),
            got: t.to_string(),
        }),
        None => Err(FbcSerialError::UnexpectedToken {
            expected: expected.to_string(),
            got: String::new(),
        }),
    }
}

/// Parses an i32 from an optional token.
fn parse_i32(token: Option<&str>) -> Result<i32, FbcSerialError> {
    match token {
        Some(s) => s
            .parse::<i32>()
            .map_err(|_| FbcSerialError::ParseInt(s.to_string())),
        None => Err(FbcSerialError::UnexpectedEof),
    }
}

/// Parses a REAL value from an optional token.
fn parse_real<R: FbcReal>(token: Option<&str>) -> Result<R, FbcSerialError> {
    match token {
        Some(s) => s
            .parse::<R>()
            .map_err(|_| FbcSerialError::ParseReal(s.to_string())),
        None => Err(FbcSerialError::UnexpectedEof),
    }
}

/// Parses a quoted string token from a line.
///
/// # Source provenance (C++)
/// - `parseStringToken()` in `interpreter_dsp_aux.hh`.
///
/// The token is expected to be a double-quoted string. Returns the content
/// without quotes.
#[allow(dead_code)]
fn parse_string_token(s: &str) -> String {
    // Find the content between double quotes.
    if let Some(start) = s.find('"')
        && let Some(end) = s[start + 1..].find('"')
    {
        return s[start + 1..start + 1 + end].to_string();
    }
    s.to_string()
}

/// Reads a meta block: `block_size N` followed by N meta instructions.
///
/// # Source provenance (C++)
/// - `readMetaBlock()` in `interpreter_dsp_aux.hh`.
fn read_meta_block(reader: &mut dyn BufRead) -> Result<Vec<FbcMetaInstruction>, FbcSerialError> {
    let line = read_line(reader)?;
    let mut tokens = line.split_whitespace();
    let _label = tokens.next(); // "block_size"
    let size = parse_i32(tokens.next())?;

    let mut result = Vec::with_capacity(size as usize);
    for _ in 0..size {
        let line = read_line(reader)?;
        result.push(read_meta_instruction(&line)?);
    }
    Ok(result)
}

/// Reads a single meta instruction from a line.
///
/// Format: `meta key "K" value "V"`
///
/// # Source provenance (C++)
/// - `readMetaInstruction()` in `interpreter_dsp_aux.hh`.
fn read_meta_instruction(line: &str) -> Result<FbcMetaInstruction, FbcSerialError> {
    // Split by quotes to extract key and value strings.
    // Format: meta key "KEY" value "VALUE"
    let rest = line.trim();

    // Find key content between first pair of quotes.
    let key = extract_nth_quoted(rest, 0).unwrap_or_default();
    let value = extract_nth_quoted(rest, 1).unwrap_or_default();

    Ok(FbcMetaInstruction::new(key, value))
}

/// Reads a UI block: `block_size N` followed by N UI instructions.
///
/// # Source provenance (C++)
/// - `readUIBlock()` in `interpreter_dsp_aux.hh`.
fn read_ui_block<R: FbcReal>(
    reader: &mut dyn BufRead,
) -> Result<Vec<FbcUiInstruction<R>>, FbcSerialError> {
    let line = read_line(reader)?;
    let mut tokens = line.split_whitespace();
    let _label = tokens.next(); // "block_size"
    let size = parse_i32(tokens.next())?;

    let mut result = Vec::with_capacity(size as usize);
    for _ in 0..size {
        let line = read_line(reader)?;
        result.push(read_ui_instruction::<R>(&line)?);
    }
    Ok(result)
}

/// Reads a single UI instruction from a line.
///
/// Format (normal):
/// `opcode NUM kName offset O label "L" key "K" value "V" init I min MN max MX step ST`
///
/// # Source provenance (C++)
/// - `readUIInstruction()` in `interpreter_dsp_aux.hh`.
fn read_ui_instruction<R: FbcReal>(line: &str) -> Result<FbcUiInstruction<R>, FbcSerialError> {
    let mut tokens = line.split_whitespace();

    let _opcode_label = tokens.next(); // "opcode"
    let opcode_num = parse_i32(tokens.next())?;
    let opcode = FbcOpcode::from_u16(opcode_num as u16)
        .ok_or_else(|| FbcSerialError::ParseInt(format!("invalid opcode {opcode_num}")))?;
    let _opcode_name = tokens.next(); // opcode string representation (unused)

    let _offset_label = tokens.next(); // "offset"
    let offset = parse_i32(tokens.next())?;

    let _label_label = tokens.next(); // "label"
    // tokens is consumed by collect; we re-parse using extract_nth_quoted below.
    // Since quoted strings can contain spaces, we need to find the tokens
    // after each quoted field.
    let label_str = extract_nth_quoted(line, 0).unwrap_or_default();
    let key_str = extract_nth_quoted(line, 1).unwrap_or_default();
    let value_str = extract_nth_quoted(line, 2).unwrap_or_default();

    // Parse numeric fields after the last quoted string.
    let after_last_quote = after_nth_quote_pair(line, 3);
    let mut num_tokens = after_last_quote.split_whitespace();

    let _init_label = num_tokens.next(); // "init"
    let init = parse_real::<R>(num_tokens.next())?;
    let _min_label = num_tokens.next(); // "min"
    let min = parse_real::<R>(num_tokens.next())?;
    let _max_label = num_tokens.next(); // "max"
    let max = parse_real::<R>(num_tokens.next())?;
    let _step_label = num_tokens.next(); // "step"
    let step = parse_real::<R>(num_tokens.next())?;

    Ok(FbcUiInstruction {
        opcode,
        offset,
        label: label_str,
        key: key_str,
        value: value_str,
        init,
        min,
        max,
        step,
    })
}

/// Reads a code block: `block_size N` followed by N instructions.
///
/// # Source provenance (C++)
/// - `readCodeBlock()` in `interpreter_dsp_aux.hh`.
fn read_code_block<R: FbcReal>(
    reader: &mut dyn BufRead,
    arena: &mut FbcBlockArena<R>,
) -> Result<BlockId, FbcSerialError> {
    let line = read_line(reader)?;
    let mut tokens = line.split_whitespace();
    let _label = tokens.next(); // "block_size"
    let size = parse_i32(tokens.next())?;

    let mut block = FbcBlock::<R>::new();

    for _ in 0..size {
        let line = read_line(reader)?;
        let (instr, store_data) = read_code_instruction::<R>(&line, reader, arena)?;

        // Special case for loops: CondBranch's branch1 is set to the
        // containing block (loop-back pointer). We'll fix this up after
        // the block is allocated.
        let is_cond_branch = instr.opcode == FbcOpcode::CondBranch;

        if let Some(data) = store_data {
            block.push_block_store(instr, data);
        } else {
            block.push(instr);
        }

        // For CondBranch, branch1 will be set to this block's ID after allocation.
        if is_cond_branch {
            // We'll fix this up below.
        }
    }

    let block_id = arena.alloc(block);

    // Fix up CondBranch loop-back pointers.
    let block_ref = arena.get_mut(block_id);
    for instr in &mut block_ref.instructions {
        if instr.opcode == FbcOpcode::CondBranch {
            instr.branch1 = Some(block_id);
        }
    }

    Ok(block_id)
}

/// Reads a single code instruction from a line, potentially consuming
/// additional lines for sub-blocks or block-store data.
///
/// # Source provenance (C++)
/// - `readCodeInstruction()` in `interpreter_dsp_aux.hh`.
fn read_code_instruction<R: FbcReal>(
    line: &str,
    reader: &mut dyn BufRead,
    arena: &mut FbcBlockArena<R>,
) -> Result<(FbcInstruction<R>, Option<BlockStoreData<R>>), FbcSerialError> {
    let mut tokens = line.split_whitespace();

    let _opcode_label = tokens.next(); // "opcode"
    let opcode_num = parse_i32(tokens.next())?;
    let opcode = FbcOpcode::from_u16(opcode_num as u16)
        .ok_or_else(|| FbcSerialError::ParseInt(format!("invalid opcode {opcode_num}")))?;
    let _opcode_name = tokens.next(); // opcode string representation (unused)

    if opcode == FbcOpcode::BlockStoreReal {
        // Format: opcode NUM kBlockStoreReal offset1 O1 offset2 O2 size S
        let _offset1_label = tokens.next(); // "offset1"
        let offset1 = parse_i32(tokens.next())?;
        let _offset2_label = tokens.next(); // "offset2"
        let offset2 = parse_i32(tokens.next())?;
        let _size_label = tokens.next(); // "size"
        let block_size = parse_i32(tokens.next())?;

        // Read data values from next line.
        let data_line = read_line(reader)?;
        let mut values = Vec::with_capacity(block_size as usize);
        for token in data_line.split_whitespace() {
            let v = token
                .parse::<R>()
                .map_err(|_| FbcSerialError::ParseReal(token.to_string()))?;
            values.push(v);
        }

        let instr =
            FbcInstruction::with_values_and_offsets(opcode, 0, R::default(), offset1, offset2);
        Ok((instr, Some(BlockStoreData::Real(values))))
    } else if opcode == FbcOpcode::BlockStoreInt {
        // Format: opcode NUM kBlockStoreInt offset1 O1 offset2 O2 size S
        let _offset1_label = tokens.next(); // "offset1"
        let offset1 = parse_i32(tokens.next())?;
        let _offset2_label = tokens.next(); // "offset2"
        let offset2 = parse_i32(tokens.next())?;
        let _size_label = tokens.next(); // "size"
        let block_size = parse_i32(tokens.next())?;

        // Read data values from next line.
        let data_line = read_line(reader)?;
        let mut values = Vec::with_capacity(block_size as usize);
        for token in data_line.split_whitespace() {
            let v = token
                .parse::<i32>()
                .map_err(|_| FbcSerialError::ParseInt(token.to_string()))?;
            values.push(v);
        }

        let instr =
            FbcInstruction::with_values_and_offsets(opcode, 0, R::default(), offset1, offset2);
        Ok((instr, Some(BlockStoreData::Int(values))))
    } else {
        // General instruction format:
        // opcode NUM kName int V real R offset1 O1 offset2 O2 [name N]
        let _int_label = tokens.next(); // "int"
        let int_value = parse_i32(tokens.next())?;
        let _real_label = tokens.next(); // "real"
        let real_value = parse_real::<R>(tokens.next())?;
        let _offset1_label = tokens.next(); // "offset1"
        let offset1 = parse_i32(tokens.next())?;
        let _offset2_label = tokens.next(); // "offset2"
        let offset2 = parse_i32(tokens.next())?;

        // Optional "name" field.
        let name = if let Some(label) = tokens.next() {
            if label == "name" {
                tokens.next().unwrap_or("").to_string()
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        // Possibly read sub-blocks for branching instructions.
        let mut branch1 = None;
        let mut branch2 = None;

        if opcode.is_choice() || opcode == FbcOpcode::Loop {
            branch1 = Some(read_code_block(reader, arena)?);
            branch2 = Some(read_code_block(reader, arena)?);
        }

        let instr = FbcInstruction::full(
            opcode, name, int_value, real_value, offset1, offset2, branch1, branch2,
        );
        Ok((instr, None))
    }
}

// ── String extraction helpers ──────────────────────────────────────────────

/// Extracts the content of the Nth double-quoted string in the input.
///
/// Returns `None` if there are fewer than `n+1` quoted strings.
fn extract_nth_quoted(s: &str, n: usize) -> Option<String> {
    let mut count = 0;
    let mut chars = s.chars().enumerate();
    while let Some((i, c)) = chars.next() {
        if c == '"' {
            // Find the closing quote.
            let start = i + 1;
            for (j, c2) in chars.by_ref() {
                if c2 == '"' {
                    if count == n {
                        return Some(s[start..j].to_string());
                    }
                    count += 1;
                    break;
                }
            }
        }
    }
    None
}

/// Returns the portion of the string after the Nth pair of double quotes.
///
/// Used to find numeric fields that follow quoted string fields.
fn after_nth_quote_pair(s: &str, n: usize) -> &str {
    let mut count = 0;
    let mut in_quote = false;
    for (i, c) in s.char_indices() {
        if c == '"' {
            if in_quote {
                count += 1;
                if count == n {
                    return &s[i + 1..];
                }
                in_quote = false;
            } else {
                in_quote = true;
            }
        }
    }
    ""
}

// ═══════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::interp::bytecode::FbcBlock;

    /// Helper: creates a trivial factory for round-trip testing.
    fn make_test_factory() -> FbcDspFactory<f32> {
        let mut arena = FbcBlockArena::<f32>::new();

        // static_init_block: StoreRealValue(0.0) at offset 0, Return
        let mut b1 = FbcBlock::new();
        b1.push(FbcInstruction::with_values_and_offsets(
            FbcOpcode::StoreRealValue,
            0,
            0.0,
            0,
            -1,
        ));
        b1.push(FbcInstruction::new(FbcOpcode::Return));
        let static_init = arena.alloc(b1);

        // init_block: Return only
        let mut b2 = FbcBlock::new();
        b2.push(FbcInstruction::new(FbcOpcode::Return));
        let init = arena.alloc(b2);

        // reset_ui_block: Return only
        let mut b3 = FbcBlock::new();
        b3.push(FbcInstruction::new(FbcOpcode::Return));
        let reset_ui = arena.alloc(b3);

        // clear_block: Return only
        let mut b4 = FbcBlock::new();
        b4.push(FbcInstruction::new(FbcOpcode::Return));
        let clear = arena.alloc(b4);

        // compute_block (control): Return only
        let mut b5 = FbcBlock::new();
        b5.push(FbcInstruction::new(FbcOpcode::Return));
        let compute = arena.alloc(b5);

        // compute_dsp_block: Return only
        let mut b6 = FbcBlock::new();
        b6.push(FbcInstruction::new(FbcOpcode::Return));
        let compute_dsp = arena.alloc(b6);

        FbcDspFactory::new(
            "test_dsp",
            "abc123",
            "-lang interp -ct 1 -es 1 -mcd 16",
            INTERP_FILE_VERSION,
            2,  // inputs
            2,  // outputs
            32, // int_heap_size
            64, // real_heap_size
            0,  // sr_offset
            1,  // count_offset
            2,  // iota_offset
            4,  // opt_level
            arena,
            vec![
                FbcMetaInstruction::new("name", "test_dsp"),
                FbcMetaInstruction::new("author", "Faust"),
            ],
            vec![FbcUiInstruction::widget(
                FbcOpcode::AddHorizontalSlider,
                5,
                "gain",
                0.5,
                0.0,
                1.0,
                0.01,
            )],
            static_init,
            init,
            reset_ui,
            clear,
            compute,
            compute_dsp,
        )
    }

    #[test]
    fn test_quote_unquote() {
        assert_eq!(quote1("hello"), "\"hello\"");
        assert_eq!(unquote1("\"hello\""), "hello");
        assert_eq!(unquote1("hello"), "hello");
        assert_eq!(unquote1("\"\""), "");
    }

    #[test]
    fn test_extract_nth_quoted() {
        let s = r#"label "gain" key "freq" value "Hz""#;
        assert_eq!(extract_nth_quoted(s, 0), Some("gain".to_string()));
        assert_eq!(extract_nth_quoted(s, 1), Some("freq".to_string()));
        assert_eq!(extract_nth_quoted(s, 2), Some("Hz".to_string()));
        assert_eq!(extract_nth_quoted(s, 3), None);
    }

    #[test]
    fn test_write_normal_header() {
        let factory = make_test_factory();
        let mut buf = Vec::new();
        write_fbc(&factory, &mut buf, false).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.starts_with("interpreter_dsp_factory float\n"));
        assert!(output.contains("file_version 8\n"));
        assert!(output.contains("name test_dsp\n"));
        assert!(output.contains("sha_key abc123\n"));
        assert!(output.contains("opt_level 4\n"));
        assert!(output.contains("inputs 2 outputs 2\n"));
        assert!(output.contains(
            "int_heap_size 32 real_heap_size 64 sr_offset 0 count_offset 1 iota_offset 2\n"
        ));
        assert!(output.contains("meta_block\n"));
        assert!(output.contains("user_interface_block\n"));
        assert!(output.contains("static_init_block\n"));
        assert!(output.contains("constants_block\n"));
        assert!(output.contains("reset_ui\n"));
        assert!(output.contains("clear_block\n"));
        assert!(output.contains("control_block\n"));
        assert!(output.contains("dsp_block\n"));
    }

    #[test]
    fn test_write_small_header() {
        let factory = make_test_factory();
        let mut buf = Vec::new();
        write_fbc(&factory, &mut buf, true).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.starts_with("i float\n"));
        assert!(output.contains("f 8\n"));
        assert!(output.contains("n test_dsp\n"));
    }

    #[test]
    fn test_write_meta_block() {
        let meta = vec![
            FbcMetaInstruction::new("name", "test"),
            FbcMetaInstruction::new("author", "Faust"),
        ];
        let mut buf = Vec::new();
        write_meta_block(&meta, &mut buf, false).unwrap();
        let output = String::from_utf8(buf).unwrap();

        assert!(output.starts_with("block_size 2\n"));
        assert!(output.contains(r#"meta key "name" value "test""#));
        assert!(output.contains(r#"meta key "author" value "Faust""#));
    }

    #[test]
    fn test_write_read_roundtrip() {
        let factory = make_test_factory();

        // Write.
        let mut buf = Vec::new();
        write_fbc(&factory, &mut buf, false).unwrap();
        let serialized = String::from_utf8(buf).unwrap();

        // Read back.
        let mut cursor = io::Cursor::new(serialized.as_bytes());
        let factory2: FbcDspFactory<f32> = read_fbc(&mut cursor).unwrap();

        // Verify fields match.
        assert_eq!(factory2.name, "test_dsp");
        assert_eq!(factory2.sha_key, "abc123");
        assert_eq!(factory2.num_inputs, 2);
        assert_eq!(factory2.num_outputs, 2);
        assert_eq!(factory2.int_heap_size, 32);
        assert_eq!(factory2.real_heap_size, 64);
        assert_eq!(factory2.sr_offset, 0);
        assert_eq!(factory2.count_offset, 1);
        assert_eq!(factory2.iota_offset, 2);
        assert_eq!(factory2.opt_level, 4);
        assert_eq!(factory2.version, INTERP_FILE_VERSION);

        // Verify meta block.
        assert_eq!(factory2.meta_block.len(), 2);
        assert_eq!(factory2.meta_block[0].key, "name");
        assert_eq!(factory2.meta_block[0].value, "test_dsp");
        assert_eq!(factory2.meta_block[1].key, "author");
        assert_eq!(factory2.meta_block[1].value, "Faust");

        // Verify UI block.
        assert_eq!(factory2.ui_block.len(), 1);
        assert_eq!(factory2.ui_block[0].opcode, FbcOpcode::AddHorizontalSlider);
        assert_eq!(factory2.ui_block[0].offset, 5);
        assert_eq!(factory2.ui_block[0].label, "gain");

        // Verify code blocks exist and have correct sizes.
        assert_eq!(factory2.arena.get(factory2.static_init_block).len(), 2);
        assert_eq!(factory2.arena.get(factory2.init_block).len(), 1);
    }

    #[test]
    fn test_version_check() {
        // Build a .fbc string with wrong version.
        let bad_fbc = "interpreter_dsp_factory float\nfile_version 99\n";
        let mut cursor = io::Cursor::new(bad_fbc.as_bytes());
        let result = read_fbc::<f32>(&mut cursor);
        assert!(result.is_err());
        match result.unwrap_err() {
            FbcSerialError::VersionMismatch { expected, got } => {
                assert_eq!(expected, INTERP_FILE_VERSION);
                assert_eq!(got, 99);
            }
            other => panic!("expected VersionMismatch, got {:?}", other),
        }
    }

    #[test]
    fn test_type_mismatch() {
        let bad_fbc = "interpreter_dsp_factory double\nfile_version 8\n";
        let mut cursor = io::Cursor::new(bad_fbc.as_bytes());
        let result = read_fbc::<f32>(&mut cursor);
        assert!(result.is_err());
        match result.unwrap_err() {
            FbcSerialError::TypeMismatch { expected, got } => {
                assert_eq!(expected, "float");
                assert_eq!(got, "double");
            }
            other => panic!("expected TypeMismatch, got {:?}", other),
        }
    }

    #[test]
    fn test_read_meta_block() {
        let input =
            "block_size 2\nmeta key \"name\" value \"sine\"\nmeta key \"author\" value \"Faust\"\n";
        let mut cursor = io::Cursor::new(input.as_bytes());
        let meta = read_meta_block(&mut cursor).unwrap();
        assert_eq!(meta.len(), 2);
        assert_eq!(meta[0].key, "name");
        assert_eq!(meta[0].value, "sine");
        assert_eq!(meta[1].key, "author");
        assert_eq!(meta[1].value, "Faust");
    }

    #[test]
    fn test_roundtrip_with_branching() {
        // Build a factory with an If instruction (has sub-blocks).
        let mut arena = FbcBlockArena::<f32>::new();

        // Branch blocks for the If instruction.
        let mut then_block = FbcBlock::new();
        then_block.push(FbcInstruction::with_values(FbcOpcode::RealValue, 0, 1.0));
        then_block.push(FbcInstruction::new(FbcOpcode::Return));
        let then_id = arena.alloc(then_block);

        let mut else_block = FbcBlock::new();
        else_block.push(FbcInstruction::with_values(FbcOpcode::RealValue, 0, 0.0));
        else_block.push(FbcInstruction::new(FbcOpcode::Return));
        let else_id = arena.alloc(else_block);

        // Main block with If instruction.
        let mut main_block = FbcBlock::new();
        main_block.push(FbcInstruction::full(
            FbcOpcode::If,
            "",
            0,
            0.0,
            -1,
            -1,
            Some(then_id),
            Some(else_id),
        ));
        main_block.push(FbcInstruction::new(FbcOpcode::Return));
        let main_id = arena.alloc(main_block);

        // Trivial blocks for other slots.
        let mut trivials = Vec::new();
        for _ in 0..5 {
            let mut b = FbcBlock::new();
            b.push(FbcInstruction::new(FbcOpcode::Return));
            trivials.push(arena.alloc(b));
        }

        let factory = FbcDspFactory::new(
            "if_test",
            "",
            "",
            INTERP_FILE_VERSION,
            0,
            0,
            4,
            4,
            0,
            1,
            -1,
            0,
            arena,
            vec![],
            vec![],
            main_id, // static_init has the If instruction
            trivials[0],
            trivials[1],
            trivials[2],
            trivials[3],
            trivials[4],
        );

        // Write.
        let mut buf = Vec::new();
        write_fbc(&factory, &mut buf, false).unwrap();
        let serialized = String::from_utf8(buf).unwrap();

        // Read back.
        let mut cursor = io::Cursor::new(serialized.as_bytes());
        let factory2: FbcDspFactory<f32> = read_fbc(&mut cursor).unwrap();

        // Verify the If instruction's sub-blocks survived round-trip.
        let static_block = factory2.arena.get(factory2.static_init_block);
        assert_eq!(static_block.len(), 2);
        assert_eq!(static_block.instructions[0].opcode, FbcOpcode::If);
        assert!(static_block.instructions[0].branch1.is_some());
        assert!(static_block.instructions[0].branch2.is_some());

        // Verify sub-block contents.
        let b1 = factory2
            .arena
            .get(static_block.instructions[0].branch1.unwrap());
        assert_eq!(b1.len(), 2);
        assert_eq!(b1.instructions[0].opcode, FbcOpcode::RealValue);
        assert!((b1.instructions[0].real_value - 1.0).abs() < 1e-6);

        let b2 = factory2
            .arena
            .get(static_block.instructions[0].branch2.unwrap());
        assert_eq!(b2.len(), 2);
        assert_eq!(b2.instructions[0].opcode, FbcOpcode::RealValue);
        assert!((b2.instructions[0].real_value - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_roundtrip_block_store_real() {
        let mut arena = FbcBlockArena::<f32>::new();

        let mut block = FbcBlock::new();
        let instr =
            FbcInstruction::with_values_and_offsets(FbcOpcode::BlockStoreReal, 0, 0.0, 0, 4);
        let data = BlockStoreData::Real(vec![1.0, 2.0, 3.0, 4.0]);
        block.push_block_store(instr, data);
        block.push(FbcInstruction::new(FbcOpcode::Return));
        let block_id = arena.alloc(block);

        // Create trivial blocks for other slots.
        let mut trivials = Vec::new();
        for _ in 0..5 {
            let mut b = FbcBlock::new();
            b.push(FbcInstruction::new(FbcOpcode::Return));
            trivials.push(arena.alloc(b));
        }

        let factory = FbcDspFactory::new(
            "blockstore",
            "",
            "",
            INTERP_FILE_VERSION,
            0,
            0,
            4,
            8,
            0,
            1,
            -1,
            0,
            arena,
            vec![],
            vec![],
            block_id,
            trivials[0],
            trivials[1],
            trivials[2],
            trivials[3],
            trivials[4],
        );

        // Write.
        let mut buf = Vec::new();
        write_fbc(&factory, &mut buf, false).unwrap();
        let serialized = String::from_utf8(buf).unwrap();

        // Read back.
        let mut cursor = io::Cursor::new(serialized.as_bytes());
        let factory2: FbcDspFactory<f32> = read_fbc(&mut cursor).unwrap();

        // Verify block-store data survived.
        let block = factory2.arena.get(factory2.static_init_block);
        assert_eq!(block.len(), 2);
        assert_eq!(block.instructions[0].opcode, FbcOpcode::BlockStoreReal);
        assert_eq!(block.block_store_data.len(), 1);
        match &block.block_store_data[0].1 {
            BlockStoreData::Real(v) => {
                assert_eq!(v.len(), 4);
                assert!((v[0] - 1.0).abs() < 1e-6);
                assert!((v[1] - 2.0).abs() < 1e-6);
                assert!((v[2] - 3.0).abs() < 1e-6);
                assert!((v[3] - 4.0).abs() < 1e-6);
            }
            BlockStoreData::Int(_) => panic!("expected Real data"),
        }
    }

    #[test]
    fn test_roundtrip_double() {
        // Test with f64 to verify type-specific serialization.
        let mut arena = FbcBlockArena::<f64>::new();

        let mut b = FbcBlock::new();
        b.push(FbcInstruction::with_values(
            FbcOpcode::RealValue,
            0,
            std::f64::consts::PI,
        ));
        b.push(FbcInstruction::new(FbcOpcode::Return));
        let block_id = arena.alloc(b);

        let mut trivials = Vec::new();
        for _ in 0..5 {
            let mut b = FbcBlock::new();
            b.push(FbcInstruction::new(FbcOpcode::Return));
            trivials.push(arena.alloc(b));
        }

        let factory = FbcDspFactory::new(
            "pi_test",
            "",
            "",
            INTERP_FILE_VERSION,
            0,
            0,
            4,
            4,
            0,
            1,
            -1,
            0,
            arena,
            vec![],
            vec![],
            block_id,
            trivials[0],
            trivials[1],
            trivials[2],
            trivials[3],
            trivials[4],
        );

        // Write.
        let mut buf = Vec::new();
        write_fbc(&factory, &mut buf, false).unwrap();
        let serialized = String::from_utf8(buf).unwrap();

        // Verify header says "double".
        assert!(serialized.starts_with("interpreter_dsp_factory double\n"));

        // Read back.
        let mut cursor = io::Cursor::new(serialized.as_bytes());
        let factory2: FbcDspFactory<f64> = read_fbc(&mut cursor).unwrap();

        // Verify PI survived round-trip with full f64 precision.
        let block = factory2.arena.get(factory2.static_init_block);
        let val = block.instructions[0].real_value;
        assert!(
            (val - std::f64::consts::PI).abs() < 1e-14,
            "PI round-trip: got {val}, expected {}",
            std::f64::consts::PI
        );
    }
}
