//! Opaque FFI types and callback glue definitions (scaffold).
//!
//! This module intentionally mirrors the role of `interp-ffi/src/types.rs` but
//! does not expose runtime behavior yet.
//!
//! # API mapping status
//! - External compatibility surface: `1:1 target` (export set/lifecycle/cache
//!   strategy) — implementation deferred to later phases.

use std::ffi::c_char;
use std::ffi::c_void;

/// `FAUSTFLOAT` used by the exported C API (v1 planned default).
///
/// Note: exact type and compatibility details will be locked by the Phase 0
/// parity matrix/ABI contract.
pub type FaustFloat = f32;

/// Opaque Cranelift DSP factory wrapper (scaffold).
pub struct CraneliftDspFactory {
    _private: (),
}

/// Opaque Cranelift DSP instance wrapper (scaffold).
pub struct CraneliftDspInstance {
    _private: (),
}

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

#[cfg(test)]
mod tests {
    use super::{CraneliftDspFactory, CraneliftDspInstance, MetaGlue, UIGlue};

    #[test]
    fn scaffold_types_are_constructible_in_type_system() {
        let _ = std::mem::size_of::<CraneliftDspFactory>();
        let _ = std::mem::size_of::<CraneliftDspInstance>();
        let _ = std::mem::size_of::<UIGlue>();
        let _ = std::mem::size_of::<MetaGlue>();
    }
}
