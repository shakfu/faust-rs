#![doc = "Top-level compiler facade crate."]

use std::path::Path;

pub struct Compiler;

impl Compiler {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    #[must_use]
    pub fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

#[must_use]
pub fn golden_snapshot(source_name: &str, source: &str) -> String {
    let line_count = source.lines().count();
    let byte_count = source.len();
    let hash = fnv1a64(source.as_bytes());

    format!(
        "faust-rs-golden-v1\nsource={source_name}\nbytes={byte_count}\nlines={line_count}\nfnv1a64={hash:016x}\n"
    )
}

pub fn golden_snapshot_from_file(path: &Path) -> Result<String, std::io::Error> {
    let source = std::fs::read_to_string(path)?;
    Ok(golden_snapshot(&path.display().to_string(), &source))
}

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0001_0000_01b3;

fn fnv1a64(input: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in input {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}
