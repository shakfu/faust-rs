#![doc = "Top-level compiler facade crate."]

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
