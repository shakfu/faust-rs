//! Lightweight structural checks for the `transform` crate (cleanup plan R9.3).
//!
//! Deterministic, filesystem-only checks (findings sorted, repo-relative
//! paths only — never absolute paths):
//!
//! 1. no stale legacy internal `vector_*` import paths (R3 migrated the
//!    workspace to `signal_fir::vector::{...}`; the `pub use` facade
//!    re-exports in `signal_fir/mod.rs` are the only allowed mention);
//! 2. no production file above the review threshold
//!    ([`MAX_PRODUCTION_LINES`] lines, `tests.rs` and `tests/` excluded);
//! 3. no checker file importing a producer entry point (the checker
//!    re-derivation architecture may share derivation helpers, but the
//!    producer entry points listed in [`PRODUCER_ENTRY_POINTS`] must never
//!    be callable from a `check`/`verify` module);
//! 4. Rustdoc `missing_docs` cleanliness is enforced separately by
//!    `cargo rustdoc -p transform --lib -- -D missing-docs` (see plan R9.2);
//!    this check does not shell out to cargo so it stays fast and
//!    deterministic across platforms.

use std::fs;
use std::path::{Path, PathBuf};

/// Review threshold for production files (plan R9.3). The largest file kept
/// intact by design is `vector/lower/signal.rs` (single lowerer `impl`).
const MAX_PRODUCTION_LINES: usize = 2_000;

/// Legacy internal alias paths that R3 retired for workspace-internal use.
const LEGACY_VECTOR_SEGMENTS: [&str; 4] = [
    "signal_fir::vector_analysis",
    "signal_fir::vector_plan",
    "signal_fir::vector_verify",
    "signal_fir::vector_state",
];

/// Producer entry points a checker module must never import or call.
const PRODUCER_ENTRY_POINTS: [&str; 8] = [
    "build_vector_plan(",
    "build_vector_clock_ad_plan(",
    "build_vector_state_plan(",
    "assemble_vector_fir(",
    "lower_vector_program(",
    "lower_pure_vector_program(",
    "build_event_order_certificate(",
    "build_state_event_order_certificate(",
];

/// Runs every structural check and fails with a sorted finding list.
pub fn structure_check() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new("crates/transform/src");
    if !root.is_dir() {
        return Err("structure-check must run from the repository root".into());
    }
    let mut files = Vec::new();
    collect_rust_files(root, &mut files)?;
    files.sort();

    let mut findings: Vec<String> = Vec::new();
    for path in &files {
        let rel = path.to_string_lossy().replace('\\', "/");
        let text = fs::read_to_string(path)?;
        let is_test_file = rel.ends_with("/tests.rs") || rel.contains("/tests/");

        if !is_test_file {
            let lines = text.lines().count();
            if lines > MAX_PRODUCTION_LINES {
                findings.push(format!(
                    "{rel}: {lines} lines exceeds the {MAX_PRODUCTION_LINES}-line review threshold"
                ));
            }
        }

        if !rel.ends_with("signal_fir/mod.rs") {
            for segment in LEGACY_VECTOR_SEGMENTS {
                if text.contains(segment) {
                    findings.push(format!(
                        "{rel}: stale legacy internal import path `{segment}`"
                    ));
                }
            }
        }

        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let is_checker_file = file_name == "check.rs"
            || file_name == "checker_reachability.rs"
            || (rel.contains("/verify/") && !is_test_file);
        if is_checker_file {
            for entry in PRODUCER_ENTRY_POINTS {
                if text.contains(entry) {
                    findings.push(format!(
                        "{rel}: checker file references producer entry point `{}`",
                        entry.trim_end_matches('(')
                    ));
                }
            }
        }
    }

    findings.sort();
    if findings.is_empty() {
        println!(
            "structure-check: OK ({} files, threshold {} lines)",
            files.len(),
            MAX_PRODUCTION_LINES
        );
        Ok(())
    } else {
        for finding in &findings {
            eprintln!("structure-check: {finding}");
        }
        Err(format!("structure-check: {} finding(s)", findings.len()).into())
    }
}

/// Collects every `.rs` file under `dir`, depth-first.
fn collect_rust_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
    Ok(())
}
