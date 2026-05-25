//! Golden regression snapshot generation and comparison helpers.
//!
//! Golden tests in this crate record a compact fingerprint of generated code
//! rather than storing the full source text.  The fingerprint encodes byte
//! count, line count, and an FNV-1a 64-bit hash of the newline-normalised
//! output, making it platform-independent and diff-friendly.
//!
//! - `golden_snapshot` — builds the fingerprint string from in-memory source;
//! - `golden_snapshot_from_file` — builds the fingerprint by reading a file;
//! - `fnv1a64` / `normalize_newlines` — hash and normalisation primitives.

use super::*;

// ─── Golden snapshot helpers ──────────────────────────────────────────────────

/// Generates a stable source snapshot string for regression testing.
///
/// The snapshot encodes the source name, byte count, line count, and an
/// FNV-1a 64-bit hash of the newline-normalized source text.  Comparing
/// snapshots across compiler versions or platforms detects unintended changes
/// to code generation output without storing full generated files.
///
/// The format is plain text, one key/value pair per line:
/// ```text
/// faust-rs-golden-v1
/// source=<name>
/// bytes=<n>
/// lines=<n>
/// fnv1a64=<hex>
/// ```
#[must_use]
pub fn golden_snapshot(source_name: &str, source: &str) -> String {
    let normalized_source = normalize_newlines(source);
    let line_count = normalized_source.lines().count();
    let byte_count = normalized_source.len();
    let hash = fnv1a64(normalized_source.as_bytes());

    format!(
        "faust-rs-golden-v1\nsource={source_name}\nbytes={byte_count}\nlines={line_count}\nfnv1a64={hash:016x}\n"
    )
}

/// File-backed variant of [`golden_snapshot`]: reads `path`, then delegates.
///
/// Useful for comparing generated output files in CI by snapshotting their
/// contents rather than storing full copies.
pub fn golden_snapshot_from_file(path: &Path) -> Result<String, std::io::Error> {
    let source = std::fs::read_to_string(path)?;
    Ok(golden_snapshot(&path.display().to_string(), &source))
}

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0001_0000_01b3;

/// Computes a FNV-1a 64-bit hash of `input`.
///
/// Used exclusively by [`golden_snapshot`] to produce a stable, portable
/// fingerprint.  FNV-1a is chosen for simplicity and determinism across
/// platforms (no endianness or SIMD dependency), not for cryptographic strength.
pub(crate) fn fnv1a64(input: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in input {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Normalizes Windows (`\r\n`) and old Mac (`\r`) line endings to Unix `\n`.
///
/// Applied before hashing in [`golden_snapshot`] so that snapshots are
/// identical regardless of whether the source or generated file uses LF or CRLF.
pub(crate) fn normalize_newlines(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}
