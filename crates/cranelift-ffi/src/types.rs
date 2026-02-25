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

use std::ffi::{CString, c_char, c_void};

/// `FAUSTFLOAT` used by the exported C API (v1 planned default).
pub type FaustFloat = f32;

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

/// C callback table for UI building (parity-target scaffold; mirrors `UIGlue`).
#[repr(C)]
pub struct UIGlue {
    pub ui_interface: *mut c_void,
    pub open_tab_box: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    pub open_horizontal_box: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    pub open_vertical_box: Option<unsafe extern "C" fn(*mut c_void, *const c_char)>,
    pub close_box: Option<unsafe extern "C" fn(*mut c_void)>,
    pub add_button: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *mut FaustFloat)>,
    pub add_check_button: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *mut FaustFloat)>,
    pub add_vertical_slider: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            *mut FaustFloat,
            FaustFloat,
            FaustFloat,
            FaustFloat,
            FaustFloat,
        ),
    >,
    pub add_horizontal_slider: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            *mut FaustFloat,
            FaustFloat,
            FaustFloat,
            FaustFloat,
            FaustFloat,
        ),
    >,
    pub add_num_entry: Option<
        unsafe extern "C" fn(
            *mut c_void,
            *const c_char,
            *mut FaustFloat,
            FaustFloat,
            FaustFloat,
            FaustFloat,
            FaustFloat,
        ),
    >,
    pub add_horizontal_bargraph: Option<
        unsafe extern "C" fn(*mut c_void, *const c_char, *mut FaustFloat, FaustFloat, FaustFloat),
    >,
    pub add_vertical_bargraph: Option<
        unsafe extern "C" fn(*mut c_void, *const c_char, *mut FaustFloat, FaustFloat, FaustFloat),
    >,
    pub add_soundfile:
        Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *mut *mut c_void)>,
    pub declare:
        Option<unsafe extern "C" fn(*mut c_void, *mut FaustFloat, *const c_char, *const c_char)>,
}

/// C callback table for metadata collection (parity-target scaffold).
#[repr(C)]
pub struct MetaGlue {
    pub meta_interface: *mut c_void,
    pub declare: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
}

/// Boxes a Cranelift factory scaffold and returns an owning raw pointer.
#[must_use]
pub(crate) fn alloc_factory(factory: CraneliftDspFactory) -> *mut CraneliftDspFactory {
    Box::into_raw(Box::new(factory))
}

/// Frees a factory pointer previously returned by [`alloc_factory`].
///
/// # Safety
/// `ptr` must be a valid pointer returned by [`alloc_factory`], and must not be
/// used after this call.
pub(crate) unsafe fn free_factory(ptr: *mut CraneliftDspFactory) {
    unsafe {
        drop(Box::from_raw(ptr));
    }
}

/// Boxes a Cranelift instance scaffold and returns an owning raw pointer.
#[must_use]
pub(crate) fn alloc_instance(
    factory: *const CraneliftDspFactory,
    sample_rate: i32,
) -> *mut CraneliftDspInstance {
    Box::into_raw(Box::new(CraneliftDspInstance {
        factory,
        sample_rate,
        initialized: false,
        cycle: 0,
    }))
}

/// Frees an instance pointer previously returned by [`alloc_instance`].
///
/// # Safety
/// `ptr` must be a valid pointer returned by [`alloc_instance`], and must not
/// be used after this call.
pub(crate) unsafe fn free_instance(ptr: *mut CraneliftDspInstance) {
    unsafe {
        drop(Box::from_raw(ptr));
    }
}

/// Allocates a heap C string that can be returned through the C ABI.
///
/// Embedded NUL bytes are replaced by the textual sequence `\\0`.
#[must_use]
pub(crate) fn alloc_c_string(s: &str) -> *mut c_char {
    let safe = s.replace('\0', "\\0");
    match CString::new(safe) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Frees a pointer returned by [`alloc_c_string`].
///
/// # Safety
/// `ptr` must be a valid non-null pointer returned by [`alloc_c_string`].
pub(crate) unsafe fn free_c_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            drop(CString::from_raw(ptr));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CraneliftDspFactory, CraneliftDspInstance, MetaGlue, UIGlue, alloc_c_string, alloc_factory,
        alloc_instance, free_c_string, free_factory, free_instance,
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
            num_inputs: 1,
            num_outputs: 1,
        });
        let instance = alloc_instance(factory, 48_000);
        let s = alloc_c_string("ok");
        unsafe {
            free_c_string(s);
            free_instance(instance);
            free_factory(factory);
        }
    }
}
