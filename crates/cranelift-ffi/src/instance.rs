//! Instance-level `extern "C"` functions for `cranelift_dsp` (scaffold ABI).
//!
//! These symbols implement lifecycle/UI/meta/compute entry points with
//! placeholder behavior so the full C ABI path can be exercised before real
//! Cranelift lowering/JIT execution is connected.

use std::os::raw::c_int;

use crate::types::{
    CraneliftDspFactory, CraneliftDspInstance, FaustFloat, MetaGlue, UIGlue, alloc_instance,
    free_instance,
};

/// Create a new Cranelift DSP instance from a factory.
///
/// # Safety
/// `factory` must be a valid non-null factory pointer that outlives the
/// returned instance.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createCCraneliftDSPInstance(
    factory: *mut CraneliftDspFactory,
) -> *mut CraneliftDspInstance {
    if factory.is_null() {
        return std::ptr::null_mut();
    }
    alloc_instance(factory.cast_const(), 0)
}

/// Delete a Cranelift DSP instance.
///
/// # Safety
/// `dsp` must be a valid pointer returned by
/// [`createCCraneliftDSPInstance`] and must not be used after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn deleteCCraneliftDSPInstance(dsp: *mut CraneliftDspInstance) {
    unsafe {
        if !dsp.is_null() {
            free_instance(dsp);
        }
    }
}

/// Clone a Cranelift DSP instance (scaffold heap/state clone).
///
/// # Safety
/// `dsp` must be a valid non-null instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cloneCCraneliftDSPInstance(
    dsp: *mut CraneliftDspInstance,
) -> *mut CraneliftDspInstance {
    unsafe {
        if dsp.is_null() {
            return std::ptr::null_mut();
        }
        let clone = CraneliftDspInstance {
            factory: (*dsp).factory,
            sample_rate: (*dsp).sample_rate,
            initialized: (*dsp).initialized,
            cycle: (*dsp).cycle,
        };
        Box::into_raw(Box::new(clone))
    }
}

/// Return the number of audio inputs.
///
/// # Safety
/// `dsp` must be a valid instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getNumInputsCCraneliftDSPInstance(
    dsp: *mut CraneliftDspInstance,
) -> c_int {
    unsafe {
        if dsp.is_null() || (*dsp).factory.is_null() {
            return 0;
        }
        (*(*dsp).factory).num_inputs
    }
}

/// Return the number of audio outputs.
///
/// # Safety
/// `dsp` must be a valid instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getNumOutputsCCraneliftDSPInstance(
    dsp: *mut CraneliftDspInstance,
) -> c_int {
    unsafe {
        if dsp.is_null() || (*dsp).factory.is_null() {
            return 0;
        }
        (*(*dsp).factory).num_outputs
    }
}

/// Return the current sample rate recorded in the scaffold instance.
///
/// # Safety
/// `dsp` must be a valid instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getSampleRateCCraneliftDSPInstance(
    dsp: *mut CraneliftDspInstance,
) -> c_int {
    unsafe {
        if dsp.is_null() {
            return 0;
        }
        (*dsp).sample_rate
    }
}

/// Full initialization entry point (`init`): records sample rate and marks initialized.
///
/// # Safety
/// `dsp` must be a valid instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn initCCraneliftDSPInstance(
    dsp: *mut CraneliftDspInstance,
    sample_rate: c_int,
) {
    unsafe {
        if dsp.is_null() {
            return;
        }
        (*dsp).initialized = true;
        instanceInitCCraneliftDSPInstance(dsp, sample_rate);
    }
}

/// Instance init entry point (scaffold records sample rate and runs substeps).
///
/// # Safety
/// `dsp` must be a valid instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instanceInitCCraneliftDSPInstance(
    dsp: *mut CraneliftDspInstance,
    sample_rate: c_int,
) {
    unsafe {
        if dsp.is_null() {
            return;
        }
        instanceConstantsCCraneliftDSPInstance(dsp, sample_rate);
        instanceResetUserInterfaceCCraneliftDSPInstance(dsp);
        instanceClearCCraneliftDSPInstance(dsp);
    }
}

/// Record the sample rate in the scaffold instance.
///
/// # Safety
/// `dsp` must be a valid instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instanceConstantsCCraneliftDSPInstance(
    dsp: *mut CraneliftDspInstance,
    sample_rate: c_int,
) {
    unsafe {
        if dsp.is_null() {
            return;
        }
        (*dsp).sample_rate = sample_rate;
    }
}

/// Reset UI state (scaffold no-op).
///
/// # Safety
/// `dsp` must be a valid instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instanceResetUserInterfaceCCraneliftDSPInstance(
    _dsp: *mut CraneliftDspInstance,
) {
}

/// Clear DSP state (scaffold resets `cycle` counter).
///
/// # Safety
/// `dsp` must be a valid instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instanceClearCCraneliftDSPInstance(dsp: *mut CraneliftDspInstance) {
    unsafe {
        if dsp.is_null() {
            return;
        }
        (*dsp).cycle = 0;
    }
}

/// Trigger UI callbacks for the instance (scaffold currently emits no widgets).
///
/// # Safety
/// `dsp` and `ui` may be null; the scaffold performs no dereference when null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buildUserInterfaceCCraneliftDSPInstance(
    _dsp: *mut CraneliftDspInstance,
    _ui: *mut UIGlue,
) {
}

/// Trigger metadata callbacks for the instance (scaffold emits one placeholder pair).
///
/// # Safety
/// `meta` may be null. If non-null and `declare` is set, callback contract is
/// the caller's responsibility.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn metadataCCraneliftDSPInstance(
    _dsp: *mut CraneliftDspInstance,
    meta: *mut MetaGlue,
) {
    unsafe {
        if meta.is_null() {
            return;
        }
        let Some(declare) = (*meta).declare else {
            return;
        };
        let key = c"backend";
        let value = c"cranelift-scaffold";
        declare((*meta).meta_interface, key.as_ptr(), value.as_ptr());
    }
}

/// Compute audio for one block (scaffold no-op; increments internal cycle count).
///
/// # Safety
/// `dsp` must be a valid instance pointer. Input/output buffers are ignored in
/// the scaffold implementation.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn computeCCraneliftDSPInstance(
    dsp: *mut CraneliftDspInstance,
    _count: c_int,
    _input: *mut *mut FaustFloat,
    _output: *mut *mut FaustFloat,
) {
    unsafe {
        if dsp.is_null() {
            return;
        }
        (*dsp).cycle = (*dsp).cycle.saturating_add(1);
    }
}

/// Instance scaffold status string kept for module-presence tests.
#[must_use]
pub fn instance_status() -> &'static str {
    "cranelift-ffi instance scaffold"
}

#[cfg(test)]
mod tests {
    use std::ffi::{CStr, CString, c_char, c_void};

    use super::{
        buildUserInterfaceCCraneliftDSPInstance, cloneCCraneliftDSPInstance,
        computeCCraneliftDSPInstance, createCCraneliftDSPInstance, deleteCCraneliftDSPInstance,
        getNumInputsCCraneliftDSPInstance, getNumOutputsCCraneliftDSPInstance,
        getSampleRateCCraneliftDSPInstance, initCCraneliftDSPInstance, instance_status,
        metadataCCraneliftDSPInstance,
    };
    use crate::factory::{createCCraneliftDSPFactoryFromString, deleteCCraneliftDSPFactory};
    use crate::types::{FaustFloat, MetaGlue, UIGlue};

    #[test]
    fn instance_scaffold_status_is_stable() {
        assert_eq!(instance_status(), "cranelift-ffi instance scaffold");
    }

    unsafe extern "C" fn capture_meta(ctx: *mut c_void, key: *const c_char, value: *const c_char) {
        unsafe {
            let out = &mut *(ctx.cast::<Vec<(String, String)>>());
            out.push((
                CStr::from_ptr(key).to_str().unwrap().to_owned(),
                CStr::from_ptr(value).to_str().unwrap().to_owned(),
            ));
        }
    }

    #[test]
    fn instance_lifecycle_scaffold_roundtrip() {
        let name = CString::new("demo").unwrap();
        let src = CString::new("process = _;").unwrap();
        let mut err = [0_i8; 4096];

        let factory = unsafe {
            createCCraneliftDSPFactoryFromString(
                name.as_ptr(),
                src.as_ptr(),
                0,
                std::ptr::null(),
                err.as_mut_ptr(),
                1,
            )
        };
        assert!(!factory.is_null());

        let dsp = unsafe { createCCraneliftDSPInstance(factory) };
        assert!(!dsp.is_null());
        assert_eq!(unsafe { getNumInputsCCraneliftDSPInstance(dsp) }, 1);
        assert_eq!(unsafe { getNumOutputsCCraneliftDSPInstance(dsp) }, 1);
        assert_eq!(unsafe { getSampleRateCCraneliftDSPInstance(dsp) }, 0);

        unsafe { initCCraneliftDSPInstance(dsp, 48_000) };
        assert_eq!(unsafe { getSampleRateCCraneliftDSPInstance(dsp) }, 48_000);

        let clone = unsafe { cloneCCraneliftDSPInstance(dsp) };
        assert!(!clone.is_null());
        assert_eq!(unsafe { getSampleRateCCraneliftDSPInstance(clone) }, 48_000);

        let mut ui = UIGlue {
            ui_interface: std::ptr::null_mut(),
            open_tab_box: None,
            open_horizontal_box: None,
            open_vertical_box: None,
            close_box: None,
            add_button: None,
            add_check_button: None,
            add_vertical_slider: None,
            add_horizontal_slider: None,
            add_num_entry: None,
            add_horizontal_bargraph: None,
            add_vertical_bargraph: None,
            add_soundfile: None,
            declare: None,
        };
        unsafe { buildUserInterfaceCCraneliftDSPInstance(dsp, &mut ui) };

        let mut entries: Vec<(String, String)> = Vec::new();
        let mut meta = MetaGlue {
            meta_interface: (&mut entries as *mut Vec<(String, String)>).cast::<c_void>(),
            declare: Some(capture_meta),
        };
        unsafe { metadataCCraneliftDSPInstance(dsp, &mut meta) };
        assert!(
            entries
                .iter()
                .any(|(k, v)| k == "backend" && v == "cranelift-scaffold")
        );

        let mut in_buf = [0.0_f32; 8];
        let mut out_buf = [0.0_f32; 8];
        let mut inputs: [*mut FaustFloat; 1] = [in_buf.as_mut_ptr()];
        let mut outputs: [*mut FaustFloat; 1] = [out_buf.as_mut_ptr()];
        unsafe { computeCCraneliftDSPInstance(dsp, 8, inputs.as_mut_ptr(), outputs.as_mut_ptr()) };

        unsafe {
            deleteCCraneliftDSPInstance(clone);
            deleteCCraneliftDSPInstance(dsp);
            assert!(deleteCCraneliftDSPFactory(factory));
        }
    }
}
