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

use std::path::{Path, PathBuf};
use std::{env, fs::File, io};

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
