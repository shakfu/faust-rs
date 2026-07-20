//! Checked final-module integration for the signal-level vector pipeline.
//!
//! # C++ provenance and adaptation
//! The compute drivers mirror `VectorCodeContainer::processFIR` in the C++
//! compiler: `-lv 0` emits fixed-size chunks plus one remainder, while `-lv 1`
//! emits one min-bounded chunk loop. Lifecycle placement follows the common
//! Faust contract implemented by `CodeContainer`: persistent fields belong to
//! the DSP struct, constants to `instanceConstants`, resettable signal state to
//! `instanceClear`, and all chunk-local buffers to `compute`.
//!
//! Rust keeps this integration behind the complete producer/checker chain.
//! The final FIR module is independently checked for lifecycle shape, output
//! coverage, inclusion of the accepted assembly body, and generic FIR type
//! and scope correctness before production selection; rejections map to
//! stable fallback reasons.

pub mod build;
pub mod check;
pub mod lifecycle;
pub mod model;
pub mod outputs;

pub(crate) use build::*;

#[cfg(test)]
mod tests;
