//! Unified C/C++ FFI distribution crate.
//!
//! This crate owns the canonical `libfaust` artifacts (`staticlib` + `cdylib`).
//! Backend-specific FFI crates (`interp-ffi`, `cranelift-ffi`, `box-ffi`) are
//! linked as Rust libraries and their exported `extern "C"` symbols are
//! distributed through this single top-level library.

#![allow(unsafe_code)]

/// Box manipulation C API surface.
pub use faust_box as box_api;
/// Cranelift backend C API surface.
pub use faust_cranelift as cranelift;
/// Interpreter backend C API surface.
pub use interp_ffi as interp;
