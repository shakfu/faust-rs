//! Opaque FFI types and allocation helpers for `cranelift_dsp`.
//!
//! This is the first executable FFI scaffold layer for the Cranelift backend:
//! it provides heap-owned opaque pointers and utility helpers used by the
//! exported C ABI functions while the real JIT runtime is still under
//! development.
//!
//! # API mapping status
//! - External compatibility surface: `adapted` during scaffolding.
//! - Naming and V1 family coverage are driven by
//!   `porting/cranelift-dsp-ffi-parity-matrix-en.md`.

use std::ffi::c_char;

use codegen::backends::cranelift::JitDspModule;

/// `FAUSTFLOAT` used by the exported C API (v1 planned default).
pub type FaustFloat = f32;

/// Shared UI callback table (`UIGlue`) for Faust C FFI backends.
pub use utils::UIGlue;

/// Shared metadata callback table (`MetaGlue`) for Faust C FFI backends.
pub use utils::MetaGlue;

/// Opaque Cranelift DSP factory wrapper exported as `cranelift_dsp_factory*`.
///
/// This scaffold stores lightweight metadata so that the C ABI can already be
/// exercised end-to-end (factory create -> instance create -> lifecycle calls)
/// before real Cranelift code generation is connected.
pub struct CraneliftDspFactory {
    /// Display name (`declare name`, file stem, or `name_app` fallback).
    pub(crate) name: String,
    /// Placeholder factory hash key (stable enough for tests, not final parity).
    pub(crate) sha_key: String,
    /// Expanded DSP source placeholder text.
    pub(crate) dsp_code: String,
    /// Compiled options summary string.
    pub(crate) compile_options: String,
    /// Placeholder JSON UI/metadata payload.
    pub(crate) json: String,
    /// Compiled Cranelift JIT module (present for real file/string compilation paths).
    pub(crate) compiled_jit: Option<JitDspModule>,
    /// Whether the backend lowered the FIR `compute` body (vs stub fallback).
    pub(crate) compute_body_lowered: bool,
    /// Audio layout (placeholder until real lowering/JIT metadata is wired).
    pub(crate) num_inputs: i32,
    /// Audio layout (placeholder until real lowering/JIT metadata is wired).
    pub(crate) num_outputs: i32,
}

/// Opaque Cranelift DSP instance wrapper exported as `cranelift_dsp*`.
pub struct CraneliftDspInstance {
    /// Non-owning pointer to the parent factory (same C API lifetime contract
    /// as `llvm_dsp`/`interpreter_dsp`).
    pub(crate) factory: *const CraneliftDspFactory,
    /// Current sample rate configured through `init`/`instance*`.
    pub(crate) sample_rate: i32,
    /// Whether `init()` has been called.
    pub(crate) initialized: bool,
    /// Number of `compute()` calls observed (scaffold diagnostic state).
    pub(crate) cycle: usize,
}

// SAFETY: Instances are opaque and not internally synchronized. The C API
// contract does not require shared concurrent access to the same instance.
unsafe impl Send for CraneliftDspInstance {}

/// Boxes a Cranelift factory scaffold and returns an owning raw pointer.
#[must_use]
pub(crate) fn alloc_factory(factory: CraneliftDspFactory) -> *mut CraneliftDspFactory {
    utils::alloc_opaque(factory)
}

/// Frees a factory pointer previously returned by [`alloc_factory`].
///
/// # Safety
/// `ptr` must be a valid pointer returned by [`alloc_factory`], and must not be
/// used after this call.
pub(crate) unsafe fn free_factory(ptr: *mut CraneliftDspFactory) {
    unsafe { utils::free_opaque(ptr) }
}

/// Boxes a Cranelift instance scaffold and returns an owning raw pointer.
#[must_use]
pub(crate) fn alloc_instance(
    factory: *const CraneliftDspFactory,
    sample_rate: i32,
) -> *mut CraneliftDspInstance {
    utils::alloc_opaque(CraneliftDspInstance {
        factory,
        sample_rate,
        initialized: false,
        cycle: 0,
    })
}

/// Frees an instance pointer previously returned by [`alloc_instance`].
///
/// # Safety
/// `ptr` must be a valid pointer returned by [`alloc_instance`], and must not
/// be used after this call.
pub(crate) unsafe fn free_instance(ptr: *mut CraneliftDspInstance) {
    unsafe { utils::free_opaque(ptr) }
}

/// Allocates a heap C string that can be returned through the C ABI.
///
/// Embedded NUL bytes are replaced by the textual sequence `\\0`.
#[must_use]
pub(crate) fn alloc_c_string(s: &str) -> *mut c_char {
    utils::alloc_c_string(s)
}

#[cfg(test)]
mod tests {
    use super::{
        CraneliftDspFactory, CraneliftDspInstance, MetaGlue, UIGlue, alloc_c_string, alloc_factory,
        alloc_instance, free_factory, free_instance,
    };

    #[test]
    fn scaffold_types_are_constructible_in_type_system() {
        let _ = std::mem::size_of::<CraneliftDspFactory>();
        let _ = std::mem::size_of::<CraneliftDspInstance>();
        let _ = std::mem::size_of::<UIGlue>();
        let _ = std::mem::size_of::<MetaGlue>();
    }

    #[test]
    fn alloc_and_free_helpers_roundtrip() {
        let factory = alloc_factory(CraneliftDspFactory {
            name: "n".into(),
            sha_key: "sha".into(),
            dsp_code: "process=_;".into(),
            compile_options: "-vec 0".into(),
            json: "{}".into(),
            compiled_jit: None,
            compute_body_lowered: false,
            num_inputs: 1,
            num_outputs: 1,
        });
        let instance = alloc_instance(factory, 48_000);
        let s = alloc_c_string("ok");
        unsafe {
            utils::free_c_string(s);
            free_instance(instance);
            free_factory(factory);
        }
    }
}
