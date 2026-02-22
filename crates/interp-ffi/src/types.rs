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
use std::ffi::c_void;

use codegen::backends::interp::{FbcDspFactory, FbcExecutor};

/// `FAUSTFLOAT` type (always f32 in this export).
pub type FaustFloat = f32;

// ── Opaque wrapper types ────────────────────────────────────────────────────

/// Opaque DSP factory, exported as `interpreter_dsp_factory*` in C.
///
/// Owns a `FbcDspFactory<f32>` on the Rust heap.
/// Allocated via [`alloc_factory`], freed via [`free_factory`].
pub struct InterpreterDspFactory {
    pub(crate) inner: FbcDspFactory<FaustFloat>,
}

/// Opaque DSP instance, exported as `interpreter_dsp*` in C.
///
/// Holds a non-owning raw pointer to its parent `InterpreterDspFactory`.
/// The factory MUST outlive this instance (same contract as the C++ API).
/// Allocated via [`alloc_instance`], freed via [`free_instance`].
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

// ── UIGlue (mirrors faust/gui/CInterface.h) ─────────────────────────────────

/// C callback table for building a UI (mirrors `UIGlue` in `CInterface.h`).
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
    /// addSoundfile: `sf_zone` is `Soundfile**` — passed as `*mut *mut c_void` here.
    pub add_soundfile:
        Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, *mut *mut c_void)>,
    pub declare:
        Option<unsafe extern "C" fn(*mut c_void, *mut FaustFloat, *const c_char, *const c_char)>,
}

/// C callback table for metadata (mirrors `MetaGlue` in `CInterface.h`).
#[repr(C)]
pub struct MetaGlue {
    pub meta_interface: *mut c_void,
    pub declare: Option<unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char)>,
}

// ── Allocation / deallocation helpers ───────────────────────────────────────

/// Boxes a `FbcDspFactory<f32>` and returns a raw owning pointer.
///
/// The caller is responsible for eventually calling [`free_factory`].
pub(crate) fn alloc_factory(inner: FbcDspFactory<FaustFloat>) -> *mut InterpreterDspFactory {
    Box::into_raw(Box::new(InterpreterDspFactory { inner }))
}

/// Drops the boxed `InterpreterDspFactory`.
///
/// # Safety
/// `ptr` must be a valid non-null pointer previously returned by [`alloc_factory`],
/// and must not be used after this call.
pub(crate) unsafe fn free_factory(ptr: *mut InterpreterDspFactory) {
    unsafe {
        drop(Box::from_raw(ptr));
    }
}

/// Boxes a new `InterpreterDspInstance` and returns a raw owning pointer.
pub(crate) fn alloc_instance(
    factory: *const InterpreterDspFactory,
    executor: FbcExecutor<FaustFloat>,
) -> *mut InterpreterDspInstance {
    Box::into_raw(Box::new(InterpreterDspInstance {
        factory,
        executor,
        initialized: false,
        cycle: 0,
    }))
}

/// Drops the boxed `InterpreterDspInstance`.
///
/// # Safety
/// `ptr` must be a valid non-null pointer previously returned by [`alloc_instance`],
/// and must not be used after this call.
pub(crate) unsafe fn free_instance(ptr: *mut InterpreterDspInstance) {
    unsafe {
        drop(Box::from_raw(ptr));
    }
}

/// Allocates a C string on the Rust heap and returns a raw owning pointer.
///
/// The returned pointer must be freed with [`free_c_string`].
pub(crate) fn alloc_c_string(s: &str) -> *mut c_char {
    use std::ffi::CString;
    // Replace any embedded NUL bytes to avoid CString panics.
    let safe = s.replace('\0', "\\0");
    match CString::new(safe) {
        Ok(cs) => cs.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Frees a C string allocated by [`alloc_c_string`].
///
/// # Safety
/// `ptr` must be a valid non-null pointer returned by `alloc_c_string`.
pub(crate) unsafe fn free_c_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe {
            drop(std::ffi::CString::from_raw(ptr));
        }
    }
}
