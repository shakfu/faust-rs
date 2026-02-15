//! Source reader prototype for parser migration.
//!
//! # Source provenance (C++)
//! - `compiler/parser/sourcereader.hh`
//! - `compiler/parser/sourcereader.cpp`
//!
//! # Scope
//! - Search-path based import resolution.
//! - Recursive import expansion with cycle detection.
//! - Read cache and used-file tracking for deterministic parser runs.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Errors returned by [`SourceReader`].
#[derive(Debug)]
pub enum SourceReaderError {
    Io { path: PathBuf, message: Box<str> },
    UnresolvedImport { name: Box<str>, from: PathBuf },
    ImportCycle { path: PathBuf },
}

impl fmt::Display for SourceReaderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, message } => {
                write!(f, "I/O error while reading {}: {message}", path.display())
            }
            Self::UnresolvedImport { name, from } => {
                write!(f, "cannot resolve import `{name}` from {}", from.display())
            }
            Self::ImportCycle { path } => {
                write!(f, "import cycle detected at {}", path.display())
            }
        }
    }
}

impl std::error::Error for SourceReaderError {}

/// Reads Faust sources and expands `import("...");` directives recursively.
#[derive(Debug, Default)]
pub struct SourceReader {
    file_cache: HashMap<PathBuf, Box<str>>,
    search_paths: Vec<PathBuf>,
    used_files: Vec<PathBuf>,
    visiting: HashSet<PathBuf>,
}

impl SourceReader {
    /// Creates a source reader using the provided import search paths.
    #[must_use]
    pub fn new(search_paths: Vec<PathBuf>) -> Self {
        Self {
            file_cache: HashMap::new(),
            search_paths,
            used_files: Vec::new(),
            visiting: HashSet::new(),
        }
    }

    /// Returns search paths used by this reader.
    #[must_use]
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    /// Returns files used during the last/ongoing recursive read.
    #[must_use]
    pub fn used_files(&self) -> &[PathBuf] {
        &self.used_files
    }

    /// Resolves one import name using current search paths.
    #[must_use]
    pub fn resolve_import(&self, name: &str) -> Option<PathBuf> {
        self.resolve_import_from(name, None)
    }

    /// Reads one source file and recursively expands imports.
    pub fn read_file(&mut self, path: &Path) -> Result<String, SourceReaderError> {
        let canonical = canonicalize_path(path)?;
        self.read_file_impl(&canonical).map(|text| text.into())
    }

    fn read_file_impl(&mut self, path: &Path) -> Result<Box<str>, SourceReaderError> {
        if let Some(cached) = self.file_cache.get(path) {
            return Ok(cached.clone());
        }

        if self.visiting.contains(path) {
            return Err(SourceReaderError::ImportCycle {
                path: path.to_path_buf(),
            });
        }

        self.visiting.insert(path.to_path_buf());
        if !self.used_files.iter().any(|p| p == path) {
            self.used_files.push(path.to_path_buf());
        }

        let source = fs::read_to_string(path).map_err(|err| SourceReaderError::Io {
            path: path.to_path_buf(),
            message: err.to_string().into_boxed_str(),
        })?;

        let mut expanded = String::new();
        for line in source.lines() {
            if let Some(import_name) = parse_import_line(line) {
                let from_dir = path.parent();
                let Some(import_path) = self.resolve_import_from(&import_name, from_dir) else {
                    self.visiting.remove(path);
                    return Err(SourceReaderError::UnresolvedImport {
                        name: import_name.into_boxed_str(),
                        from: path.to_path_buf(),
                    });
                };
                let imported = self.read_file_impl(&import_path)?;
                expanded.push_str(&imported);
                if !expanded.ends_with('\n') {
                    expanded.push('\n');
                }
            } else {
                expanded.push_str(line);
                expanded.push('\n');
            }
        }

        self.visiting.remove(path);

        let expanded = expanded.into_boxed_str();
        self.file_cache.insert(path.to_path_buf(), expanded.clone());
        Ok(expanded)
    }

    fn resolve_import_from(&self, name: &str, local_dir: Option<&Path>) -> Option<PathBuf> {
        let raw = Path::new(name);
        if raw.is_absolute() {
            return canonicalize_path(raw).ok();
        }

        let mut candidates = Vec::new();
        if let Some(base) = local_dir {
            candidates.push(base.join(name));
        }
        candidates.extend(self.search_paths.iter().map(|base| base.join(name)));

        for candidate in candidates {
            if candidate.exists() {
                return canonicalize_path(&candidate).ok();
            }
        }
        None
    }
}

fn canonicalize_path(path: &Path) -> Result<PathBuf, SourceReaderError> {
    path.canonicalize().map_err(|err| SourceReaderError::Io {
        path: path.to_path_buf(),
        message: err.to_string().into_boxed_str(),
    })
}

fn parse_import_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let suffix = trimmed.strip_prefix("import")?.trim_start();
    let suffix = suffix.strip_prefix('(')?.trim_start();
    let suffix = suffix.strip_prefix('"')?;
    let end_quote = suffix.find('"')?;
    let import_name = &suffix[..end_quote];
    let rest = suffix[end_quote + 1..].trim();
    if rest != ");" {
        return None;
    }
    Some(import_name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::parse_import_line;

    #[test]
    fn parses_import_line_variants() {
        assert_eq!(
            parse_import_line(r#"import("stdfaust.lib");"#).as_deref(),
            Some("stdfaust.lib")
        );
        assert_eq!(
            parse_import_line(r#"  import( "foo/bar.lib" ); "#).as_deref(),
            Some("foo/bar.lib")
        );
        assert!(parse_import_line(r#"process = _;"#).is_none());
    }
}
