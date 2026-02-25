//! `cranelift-ffi` — planned C/C++ FFI export for the Cranelift backend.
//!
//! # Purpose (Phase 1 scaffold)
//! - Host the exported `cranelift_dsp` / `cranelift_dsp_factory` C and C++
//!   runtime API surface required by the Cranelift backend plan.
//! - Mirror the overall strategy used by `llvm_dsp` / `interpreter_dsp`
//!   families (factory cache + instance lifecycle + UI/meta callbacks).
//!
//! # Current status
//! - Phase 1 scaffold only.
//! - No runtime/JIT integration is implemented yet.
//! - Header files are placeholders documenting intended exported surfaces.
//!
//! # Planned modules
//! - [`types`] — opaque FFI wrapper types and callback glue structs.
//! - [`cache`] — global factory cache (placeholder; semantics to match parity matrix).
//! - [`factory`] — factory lifecycle and source compilation entry points.
//! - [`instance`] — instance lifecycle and `compute`/UI/meta dispatch entry points.
//! - [`ui`] — UI/meta callback dispatch helpers.

#![allow(unsafe_code)] // Future FFI implementation will require raw pointers.

pub mod cache;
pub mod factory;
pub mod instance;
pub mod types;
pub mod ui;
