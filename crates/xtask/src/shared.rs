//! Shared workspace helpers for `xtask` workflows.
//!
//! This module centralizes path handling that is reused across snapshot,
//! report, runtime-trace, and build-artifact commands. Keeping these helpers in
//! one place avoids each workflow re-deriving the workspace root differently.

use super::*;

// ---------------------------------------------------------------------------
// Shared workspace/path helpers
// ---------------------------------------------------------------------------

/// Returns the canonical workspace root path.
pub(crate) fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .to_path_buf()
        })
}

/// Formats a path relative to the workspace root when possible.
pub(crate) fn workspace_relative_path(path: &Path) -> String {
    let root = workspace_root();
    if let Ok(relative) = path.strip_prefix(&root) {
        return relative.display().to_string();
    }
    if let Ok(canonical) = path.canonicalize()
        && let Ok(relative) = canonical.strip_prefix(&root)
    {
        return relative.display().to_string();
    }
    path.display().to_string()
}
