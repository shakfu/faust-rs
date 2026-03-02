//! Opaque FFI types and allocation helpers.
//!
//! `InterpreterDspFactory` and `InterpreterDspInstance` are heap-allocated
//! Rust objects exposed to C as opaque pointer types.  Ownership rules mirror
//! the original Faust C API:
//! - A factory is created by one of the `read/create*Factory` functions and
//!   deleted by `deleteCInterpreterDSPFactory`.
//! - An instance is created by `createCInterpreterDSPInstance` and deleted by
//!   `deleteCInterpreterDSPInstance`.
//! - The factory **must** outlive all of its instances.

use std::ffi::c_char;

use codegen::backends::interp::{FbcDspFactory, FbcExecutor};

/// `FAUSTFLOAT` type (always f32 in this export).
pub type FaustFloat = f32;

/// Shared UI callback table (`UIGlue`) for Faust C FFI.
///
/// Re-exported from `utils` so all Rust FFI backends use the same `#[repr(C)]`
/// definition and callback signatures.
pub use utils::UIGlue;

/// Shared metadata callback table (`MetaGlue`) for Faust C FFI.
pub use utils::MetaGlue;

// ── Opaque wrapper types ────────────────────────────────────────────────────

/// Opaque DSP factory, exported as `interpreter_dsp_factory*` in C.
///
/// Owns a `FbcDspFactory<f32>` on the Rust heap.
/// Allocated via `alloc_factory`, freed via `free_factory`.
pub struct InterpreterDspFactory {
    pub(crate) inner: FbcDspFactory<FaustFloat>,
}

/// Opaque DSP instance, exported as `interpreter_dsp*` in C.
///
/// Holds a non-owning raw pointer to its parent `InterpreterDspFactory`.
/// The factory MUST outlive this instance (same contract as the C++ API).
/// Allocated via `alloc_instance`, freed via `free_instance`.
pub struct InterpreterDspInstance {
    /// Non-owning pointer to the parent factory (factory outlives instance).
    pub(crate) factory: *const InterpreterDspFactory,
    /// Execution heaps (int + real).
    pub(crate) executor: FbcExecutor<FaustFloat>,
    /// Whether `init()` has been called.
    pub(crate) initialized: bool,
    /// Number of `compute()` cycles executed.
    pub(crate) cycle: usize,
}

// SAFETY: DSP instances are not shared between threads (Faust API contract).
unsafe impl Send for InterpreterDspInstance {}

// ── Allocation / deallocation helpers ───────────────────────────────────────

/// Boxes a `FbcDspFactory<f32>` and returns a raw owning pointer.
///
/// The caller is responsible for eventually calling [`free_factory`].
pub(crate) fn alloc_factory(inner: FbcDspFactory<FaustFloat>) -> *mut InterpreterDspFactory {
    utils::alloc_opaque(InterpreterDspFactory { inner })
}

/// Drops the boxed `InterpreterDspFactory`.
///
/// # Safety
/// `ptr` must be a valid non-null pointer previously returned by [`alloc_factory`],
/// and must not be used after this call.
pub(crate) unsafe fn free_factory(ptr: *mut InterpreterDspFactory) {
    unsafe { utils::free_opaque(ptr) }
}

/// Boxes a new `InterpreterDspInstance` and returns a raw owning pointer.
pub(crate) fn alloc_instance(
    factory: *const InterpreterDspFactory,
    executor: FbcExecutor<FaustFloat>,
) -> *mut InterpreterDspInstance {
    utils::alloc_opaque(InterpreterDspInstance {
        factory,
        executor,
        initialized: false,
        cycle: 0,
    })
}

/// Drops the boxed `InterpreterDspInstance`.
///
/// # Safety
/// `ptr` must be a valid non-null pointer previously returned by [`alloc_instance`],
/// and must not be used after this call.
pub(crate) unsafe fn free_instance(ptr: *mut InterpreterDspInstance) {
    unsafe { utils::free_opaque(ptr) }
}

/// Allocates a C string on the Rust heap and returns a raw owning pointer.
///
/// The returned pointer must be freed with [`free_c_string`].
pub(crate) fn alloc_c_string(s: &str) -> *mut c_char {
    utils::alloc_c_string(s)
}
