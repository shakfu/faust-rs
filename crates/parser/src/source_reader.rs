//! Source reader for the production parser crate.
//!
//! # Source provenance (C++)
//! - `compiler/parser/sourcereader.hh`
//! - `compiler/parser/sourcereader.cpp`
//!
//! # Scope
//! - Search-path based import resolution.
//! - Recursive import expansion with cycle detection.
//! - Read cache and used-file tracking for deterministic parser runs.
//! - Local-file import policy in current parser scope:
//!   - URL/network fetch is intentionally out-of-scope in `parser` and tracked as deferred
//!     in Phase 3 porting docs (no temporary network stub in this crate).

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// One source-origin marker for a line in expanded source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLineOrigin {
    /// Canonical file path where this expanded line originates.
    pub file: PathBuf,
    /// 1-based line number in the original source file.
    pub line: u32,
}

/// Expanded source payload returned by [`SourceReader`], including per-line origin mapping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandedSource {
    /// Expanded source text after recursive import substitution.
    pub text: Box<str>,
    /// Origin for each line in `text` (same ordering, 1:1 mapping).
    pub line_origins: Vec<SourceLineOrigin>,
}

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
    file_cache: HashMap<PathBuf, ExpandedSource>,
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
        self.read_file_impl(&canonical)
            .map(|expanded| expanded.text.into())
    }

    /// Reads one source file and recursively expands imports, preserving line origins.
    pub fn read_file_with_origins(
        &mut self,
        path: &Path,
    ) -> Result<ExpandedSource, SourceReaderError> {
        let canonical = canonicalize_path(path)?;
        self.read_file_impl(&canonical)
    }

    fn read_file_impl(&mut self, path: &Path) -> Result<ExpandedSource, SourceReaderError> {
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
        let mut line_origins = Vec::new();
        for (line_index, line) in source.lines().enumerate() {
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
                expanded.push_str(&imported.text);
                line_origins.extend(imported.line_origins);
                if !expanded.ends_with('\n') {
                    expanded.push('\n');
                }
            } else {
                expanded.push_str(line);
                expanded.push('\n');
                line_origins.push(SourceLineOrigin {
                    file: path.to_path_buf(),
                    line: u32::try_from(line_index + 1).unwrap_or(u32::MAX),
                });
            }
        }

        self.visiting.remove(path);

        let expanded = ExpandedSource {
            text: expanded.into_boxed_str(),
            line_origins,
        };
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
