//! Command-line interface support for the `faust-rs` binary.
//!
//! The CLI is split by concern:
//! argument parsing and compatibility normalization live in [`args`],
//! diagnostic rendering lives in [`diagnostics`], global timing support lives
//! in [`timer`], and process-level orchestration lives in [`runner`].  The
//! crate root keeps `main.rs` intentionally small so the large-stack launcher
//! contract is isolated from the command implementation.

pub mod args;
pub mod diagnostics;
pub mod runner;
pub mod timer;

#[cfg(test)]
mod tests;
