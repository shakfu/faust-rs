//! Import search path construction and merge helpers.
//!
//! Mirrors the C++ `global::initDocumentNames()` / `initDirectories()` model:
//! - `default_import_search_paths` — builds the ordered path list for a
//!   file-backed session (current dir, `FAUST_LIB_PATH`, exe-relative, system);
//! - `merge_import_search_paths` / `build_import_search_paths` — utilities for
//!   combining caller-supplied paths with the defaults;
//! - `ensure_parse_success` — converts a parse result into a `CompilerError`
//!   with consistent source attribution.

use super::*;

// ─── Helpers: path resolution ─────────────────────────────────────────────────

/// Resolves the default built-in import search paths for one file-backed
/// compilation session.
///
/// # Source provenance (C++)
/// - `global::initDocumentNames()` / `global::initDirectories()` in
///   `compiler/global.cpp`
///
/// # Effective order
/// 1. current file parent directory (or `"."` for a bare filename)
/// 2. `FAUST_LIB_PATH` when present
/// 3. executable-relative `../share/faust`
/// 4. `/usr/local/share/faust`
/// 5. `/usr/share/faust`
///
/// This mirrors the C++ hardcoded library-search model as closely as possible
/// in a standalone Rust binary.
#[must_use]
/// Returns the default Faust import search paths for `path`.
pub fn default_import_search_paths(path: &Path) -> Vec<PathBuf> {
    build_import_search_paths(
        path,
        &[],
        std::env::var_os("FAUST_LIB_PATH"),
        std::env::current_exe().ok(),
    )
}

/// Builds the import search path list for a given source file, merging user-supplied
/// extra paths with the built-in defaults discovered from the environment.
///
/// This is a convenience wrapper over [`build_import_search_paths`] that reads
/// `FAUST_LIB_PATH` and the current executable location automatically.
pub(crate) fn merge_import_search_paths(path: &Path, extra_paths: &[PathBuf]) -> Vec<PathBuf> {
    build_import_search_paths(
        path,
        extra_paths,
        std::env::var_os("FAUST_LIB_PATH"),
        std::env::current_exe().ok(),
    )
}

/// Core implementation of the import search path algorithm.
///
/// Produces an ordered, deduplicated list following the same priority rules as
/// the C++ Faust compiler:
///
/// 1. User-supplied `extra_paths` (highest priority).
/// 2. Directory containing the source file.
/// 3. Paths from the `FAUST_LIB_PATH` environment variable (colon/semicolon-separated).
/// 4. Standard library locations relative to the running executable.
///
/// Parameters are explicit so the function is pure and fully testable without
/// touching the environment.
pub(crate) fn build_import_search_paths(
    path: &Path,
    extra_paths: &[PathBuf],
    faust_lib_path: Option<OsString>,
    current_exe: Option<PathBuf>,
) -> Vec<PathBuf> {
    /// Appends `candidate` only if it is not already present in `paths`.
    fn push_unique(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
        if !paths.iter().any(|existing| existing == &candidate) {
            paths.push(candidate);
        }
    }

    let mut ordered = Vec::with_capacity(extra_paths.len() + 5);
    for path in extra_paths {
        push_unique(&mut ordered, path.clone());
    }

    push_unique(
        &mut ordered,
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
    );

    if let Some(env_path) = faust_lib_path {
        push_unique(&mut ordered, PathBuf::from(env_path));
    }

    if let Some(share_root) = current_exe
        .as_deref()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|root| root.join("share").join("faust"))
    {
        push_unique(&mut ordered, share_root);
    }

    push_unique(&mut ordered, PathBuf::from("/usr/local/share/faust"));
    push_unique(&mut ordered, PathBuf::from("/usr/share/faust"));
    ordered
}

// ─── Helpers: parse validation ────────────────────────────────────────────────

/// Converts raw parser output into the facade-level success/error contract.
///
/// The parser may return a root node even when recoveries or hard errors were
/// recorded. The compiler facade treats any non-zero parse error or recovery
/// count as a stage failure, matching the stricter "ready for later phases"
/// contract expected by `eval` and `propagate`.
pub(crate) fn ensure_parse_success(
    source: &str,
    output: ParseOutput,
) -> Result<ParseOutput, CompilerError> {
    let parse_errors = usize::try_from(output.state.ctx.parse_error_count()).unwrap_or(usize::MAX);
    let recoveries = output.state.ctx.recovery_count();
    let has_root = output.root.is_some();
    if has_root && parse_errors == 0 && recoveries == 0 {
        Ok(output)
    } else {
        Err(CompilerError::Parse {
            source: source.into(),
            parse_errors,
            recoveries,
            diagnostics: output.diagnostics,
        })
    }
}
