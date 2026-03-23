//! Architecture wrapping helpers (`enrobage`) used by compiler orchestration.
//!
//! # Source provenance (C++)
//! - `compiler/parser/enrobage.hh`
//! - `compiler/parser/enrobage.cpp`
//!
//! # Porting status
//! - Step B (pure helpers): implemented in this module.
//! - Search/open helpers and stream-copy logic are added in subsequent steps.
//!
//! # API mapping status
//! - `fileBasename` => [`file_basename`] (`1:1` semantics on separators).
//! - `fileDirname` => [`file_dirname`] (`1:1` fallback behavior).
//! - `stripEnd` => [`strip_end`] (`1:1` suffix behavior).
//! - `makeOutputFile` => [`make_output_file`] (`adapted` to `PathBuf`).

use std::collections::HashSet;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::{env, fs::File};

/// Returns the basename portion of a path-like string.
///
/// This follows C++ `fileBasename` behavior:
/// - `/` and `\\` are treated as directory separators,
/// - DOS drive prefix (`C:`) is skipped before separator scanning.
#[must_use]
pub fn file_basename(name: &str) -> &str {
    let bytes = name.as_bytes();
    let mut offset = 0usize;
    if bytes.len() > 1 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        offset = 2;
    }

    let mut base = offset;
    for (index, ch) in name[offset..].char_indices() {
        if ch == '/' || ch == '\\' {
            base = offset + index + ch.len_utf8();
        }
    }
    &name[base..]
}

/// Returns the dirname of a path-like string.
///
/// C++ parity behavior:
/// - no dirname => `"."`,
/// - root dirname => `"/"` (or first separator),
/// - otherwise strip one trailing basename component.
#[must_use]
pub fn file_dirname(name: &str) -> String {
    let size = name.len() - file_basename(name).len();
    if size == 0 {
        ".".to_owned()
    } else if size == 1 {
        name.chars().next().unwrap_or('.').to_string()
    } else {
        name[..size - 1].to_owned()
    }
}

/// Removes `ext` when it is present as a trailing suffix.
///
/// This intentionally mirrors the C++ guard `name.length() >= 4` before suffix
/// stripping, because this helper is historically used for extension-style
/// suffixes.
/// Removes the trailing `ext` suffix when present.
#[must_use]
pub fn strip_end(name: &str, ext: &str) -> String {
    if name.len() >= 4 && name.ends_with(ext) {
        name[..name.len() - ext.len()].to_owned()
    } else {
        name.to_owned()
    }
}

/// Builds an output file path from `output_dir` and `file_name`.
///
/// Rust adaptation of C++ `makeOutputFile`:
/// - when `output_dir` is missing/empty, returns `file_name`,
/// - otherwise returns `output_dir/file_name`.
#[must_use]
pub fn make_output_file(output_dir: Option<&Path>, file_name: &str) -> PathBuf {
    match output_dir {
        Some(dir) if !dir.as_os_str().is_empty() => dir.join(file_name),
        _ => PathBuf::from(file_name),
    }
}

/// Result returned by [`fopen_search`].
/// Result of one include or helper file lookup during wrapping.
#[derive(Debug)]
pub struct FileSearchResult {
    /// Opened file handle.
    pub file: File,
    /// Full path assembled with C++-style search semantics.
    pub full_path: PathBuf,
}

/// Opens an architecture file by searching current directory then architecture dirs.
///
/// Search order is deterministic and mirrors C++ `openArchStream`:
/// 1. current directory / direct `filename`,
/// 2. each `architecture_dirs[i]/filename` in declaration order.
pub fn open_arch_stream(filename: &str, architecture_dirs: &[PathBuf]) -> io::Result<File> {
    if let Ok(file) = File::open(filename) {
        return Ok(file);
    }

    for dir in architecture_dirs {
        let candidate = dir.join(filename);
        if let Ok(file) = File::open(&candidate) {
            return Ok(file);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("architecture file not found: {filename}"),
    ))
}

/// Searches and opens a source file with C++ `fopenSearch` semantics.
///
/// Behavior:
/// - first tries direct `filename`,
/// - on direct success, appends the discovered file directory to `import_dirs`,
/// - then tries each `import_dirs` entry in-order without additional enrichment.
pub fn fopen_search(
    filename: &str,
    import_dirs: &mut Vec<PathBuf>,
) -> io::Result<FileSearchResult> {
    if let Ok(file) = File::open(filename) {
        let full_path = build_full_pathname(filename)?;
        import_dirs.push(PathBuf::from(file_dirname(
            full_path.to_string_lossy().as_ref(),
        )));
        return Ok(FileSearchResult { file, full_path });
    }

    let cwd = env::current_dir()?;
    for dir in import_dirs.iter() {
        let candidate = dir.join(filename);
        if let Ok(file) = File::open(&candidate) {
            let full_path = if is_absolute_pathname(dir.to_string_lossy().as_ref()) {
                candidate
            } else {
                cwd.join(candidate)
            };
            return Ok(FileSearchResult { file, full_path });
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("file not found in search paths: {filename}"),
    ))
}

/// Configuration for line-oriented stream copying and include injection.
///
/// Used by [`stream_copy_until`] and [`stream_copy_until_end`].
#[derive(Debug, Clone)]
pub struct StreamCopyConfig {
    /// Replacement target for `mydsp` occurrences (forced replacement).
    pub class_name: String,
    /// Replacement target for `dsp` occurrences (word-boundary replacement).
    pub super_class_name: String,
    /// Enables architecture include inlining (`#include <faust/...>`).
    pub inline_arch_switch: bool,
    /// Search path used when resolving injected include files.
    pub architecture_dirs: Vec<PathBuf>,
}

impl Default for StreamCopyConfig {
    fn default() -> Self {
        Self {
            class_name: "mydsp".to_owned(),
            super_class_name: "dsp".to_owned(),
            inline_arch_switch: false,
            architecture_dirs: Vec::new(),
        }
    }
}

/// Mutable state accumulated while copying/wrapping one generated stream.
#[derive(Debug, Default)]
pub struct StreamCopyState {
    /// Tracks architecture includes already injected.
    pub already_included: HashSet<String>,
    /// Last recoverable include-injection error (`ERROR : <file> not found`).
    pub last_error: Option<String>,
}

/// Copies/removes architecture license header from `src` into `dst`.
///
/// C++ parity behavior:
/// - blank leading lines are copied,
/// - first non-blank line must begin a block comment (`/*`) to be treated as
///   license header,
/// - when the header contains `exception_tag`, the header is removed.
pub fn stream_copy_license<R: BufRead, W: Write>(
    src: &mut R,
    dst: &mut W,
    exception_tag: &str,
) -> io::Result<()> {
    let mut line = String::new();

    loop {
        line.clear();
        if src.read_line(&mut line)? == 0 {
            return Ok(());
        }
        trim_single_newline(&mut line);
        if is_blank(&line) {
            writeln!(dst, "{line}")?;
            continue;
        }
        break;
    }

    if !line.contains("/*") {
        writeln!(dst, "{line}")?;
        return Ok(());
    }

    let mut header = vec![line];
    let mut remove = false;
    loop {
        let mut current = String::new();
        if src.read_line(&mut current)? == 0 {
            break;
        }
        trim_single_newline(&mut current);
        if current.contains("*/") {
            if !remove {
                for h in &header {
                    writeln!(dst, "{h}")?;
                }
                writeln!(dst, "{current}")?;
            }
            return Ok(());
        }
        if current.contains(exception_tag) {
            remove = true;
        }
        header.push(current);
    }
    Ok(())
}

/// Copies lines from `src` into `dst` until `remove_spaces(line) == until`.
///
/// While copying:
/// - class names are rewritten with `mydsp`/`dsp` C++ rules,
/// - when `inline_arch_switch` is enabled, recognized `faust` includes are
///   injected exactly once and removed from output.
pub fn stream_copy_until<R: BufRead, W: Write>(
    src: &mut R,
    dst: &mut W,
    until: &str,
    config: &StreamCopyConfig,
    state: &mut StreamCopyState,
) -> io::Result<()> {
    let mut line = String::new();
    loop {
        line.clear();
        if src.read_line(&mut line)? == 0 {
            return Ok(());
        }
        trim_single_newline(&mut line);
        if remove_spaces(&line) == until {
            return Ok(());
        }

        if config.inline_arch_switch
            && let Some(fname) = is_faust_include(&line)
        {
            inject(dst, &fname, config, state)?;
            continue;
        }

        let replaced = replace_class_name(&line, &config.class_name, &config.super_class_name);
        writeln!(dst, "{replaced}")?;
    }
}

/// Copies `src` into `dst` until stream end.
///
/// This is a thin wrapper using the C++ sentinel convention.
pub fn stream_copy_until_end<R: BufRead, W: Write>(
    src: &mut R,
    dst: &mut W,
    config: &StreamCopyConfig,
    state: &mut StreamCopyState,
) -> io::Result<()> {
    stream_copy_until(
        src,
        dst,
        "<<<FORBIDDEN LINE IN A FAUST ARCHITECTURE FILE>>>",
        config,
        state,
    )
}

/// Options controlling the C++ wrapper/enrobage step.
///
/// Used to wrap generated C++ class text with an architecture template.
#[derive(Debug, Clone)]
pub struct EnrobageOptions {
    /// Architecture template filename/path.
    pub architecture_file: PathBuf,
    /// Additional architecture search directories.
    pub architecture_dirs: Vec<PathBuf>,
    /// Replacement target for `mydsp`.
    pub class_name: String,
    /// Replacement target for `dsp`.
    pub super_class_name: String,
    /// Enables inline architecture include injection.
    pub inline_arch_files: bool,
}

impl EnrobageOptions {
    /// Creates default wrapping options for one architecture file.
    #[must_use]
    pub fn new(architecture_file: PathBuf) -> Self {
        Self {
            architecture_file,
            architecture_dirs: Vec::new(),
            class_name: "mydsp".to_owned(),
            super_class_name: "dsp".to_owned(),
            inline_arch_files: false,
        }
    }
}

/// Result of [`wrap_cpp_with_architecture`]: generated C++ wrapped in the architecture scaffold.
#[derive(Debug)]
pub struct WrappedCppCode {
    /// Final wrapped C++ output text.
    pub code: String,
    /// Last recoverable include-injection error, if any.
    pub recoverable_error: Option<String>,
}

/// Wraps generated C++ class text into an architecture template.
///
/// This follows the C++ `generateCodeAux1` assembly shape:
/// - copy architecture prologue until `<<includeIntrinsic>>`,
/// - copy intermediate section until `<<includeclass>>`,
/// - inject generated class/module text,
/// - copy architecture epilogue to end.
pub fn wrap_cpp_with_architecture(
    generated_cpp: &str,
    options: &EnrobageOptions,
) -> io::Result<WrappedCppCode> {
    let architecture_name = options.architecture_file.to_string_lossy();
    let file = open_arch_stream(architecture_name.as_ref(), &options.architecture_dirs)?;
    let mut src = BufReader::new(file);
    let mut out = Vec::<u8>::new();
    let mut state = StreamCopyState::default();
    let cfg = StreamCopyConfig {
        class_name: options.class_name.clone(),
        super_class_name: options.super_class_name.clone(),
        inline_arch_switch: options.inline_arch_files,
        architecture_dirs: options.architecture_dirs.clone(),
    };

    stream_copy_until(&mut src, &mut out, "<<includeIntrinsic>>", &cfg, &mut state)?;
    stream_copy_until(&mut src, &mut out, "<<includeclass>>", &cfg, &mut state)?;
    write!(out, "{generated_cpp}")?;
    if !generated_cpp.ends_with('\n') {
        writeln!(out)?;
    }
    stream_copy_until_end(&mut src, &mut out, &cfg, &mut state)?;

    Ok(WrappedCppCode {
        code: String::from_utf8(out)
            .expect("architecture wrapping output is expected to stay UTF-8 text"),
        recoverable_error: state.last_error,
    })
}

fn is_absolute_pathname(filename: &str) -> bool {
    let bytes = filename.as_bytes();
    if bytes.len() > 1 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic() {
        return true;
    }
    if filename.starts_with('/') {
        return true;
    }
    Path::new(filename).is_absolute()
}

fn build_full_pathname(filename: &str) -> io::Result<PathBuf> {
    if is_absolute_pathname(filename) {
        Ok(PathBuf::from(filename))
    } else {
        Ok(env::current_dir()?.join(filename))
    }
}

fn trim_single_newline(line: &mut String) {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
}

fn is_blank(s: &str) -> bool {
    s.chars().all(|c| c == ' ' || c == '\t')
}

fn remove_spaces(s: &str) -> String {
    s.chars().filter(|c| *c != ' ').collect()
}

fn word_boundaries(s: &str, pos: usize, len: usize) -> bool {
    let before = if pos == 0 {
        None
    } else {
        s[..pos].chars().next_back()
    };
    if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }

    let after_index = pos + len;
    let after = if after_index >= s.len() {
        None
    } else {
        s[after_index..].chars().next()
    };
    if after.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    true
}

fn replace_occurrences(mut s: String, old: &str, new: &str, force: bool) -> String {
    if old.is_empty() {
        return s;
    }
    let mut pos = 0usize;
    while let Some(found) = s[pos..].find(old) {
        let at = pos + found;
        let replace = force || word_boundaries(&s, at, old.len());
        if replace {
            s.replace_range(at..(at + old.len()), new);
            pos = at + new.len();
        } else {
            pos = at + old.len();
        }
    }
    s
}

fn replace_class_name(line: &str, class_name: &str, super_class_name: &str) -> String {
    let line = replace_occurrences(line.to_owned(), "mydsp", class_name, true);
    replace_occurrences(line, "dsp", super_class_name, false)
}

fn parse_include_filename(s: &str) -> Option<String> {
    let mut chars = s.chars();
    let start = chars.next()?;
    if start != '<' && start != '"' {
        return None;
    }
    let end = if start == '<' { '>' } else { '"' };
    let mut out = String::new();
    for ch in chars {
        if ch == end {
            return Some(out);
        }
        out.push(ch);
    }
    None
}

fn is_faust_include(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if let Some(rest) = trimmed.strip_prefix("#include") {
        let file = parse_include_filename(rest.trim_start())?;
        if file.starts_with("faust/") {
            return Some(file);
        }
    } else if let Some(rest) = trimmed.strip_prefix("include(") {
        let file = parse_include_filename(rest.trim_start())?;
        if file.starts_with("/usr/local/share/faust/julia") {
            return Some(file);
        }
    }
    None
}

fn inject<W: Write>(
    dst: &mut W,
    fname: &str,
    config: &StreamCopyConfig,
    state: &mut StreamCopyState,
) -> io::Result<()> {
    if state.already_included.contains(fname) {
        return Ok(());
    }
    state.already_included.insert(fname.to_owned());

    match open_arch_stream(fname, &config.architecture_dirs) {
        Ok(file) => {
            let mut src = BufReader::new(file);
            stream_copy_until_end(&mut src, dst, config, state)
        }
        Err(_) => {
            state.last_error = Some(format!("ERROR : {fname} not found\n"));
            Ok(())
        }
    }
}
