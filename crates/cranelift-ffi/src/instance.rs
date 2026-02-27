//! Instance-level `extern "C"` functions for `cranelift_dsp`.
//!
//! This module owns the runtime DSP instance contract:
//! - allocate one backend `dsp*` state buffer per instance,
//! - invoke finalized Cranelift `compute` entry points,
//! - dispatch UI/meta callbacks through interpreter sidecar instruction blocks.
//!
//! The design keeps one factory -> multiple instances semantics and isolates all
//! function pointer invocation in documented `unsafe` boundaries.

use std::ffi::c_void;
use std::os::raw::c_int;

use crate::types::{
    CraneliftDspFactory, CraneliftDspInstance, DspStateBuffer, FaustFloat, MetaGlue, UIGlue,
    alloc_instance, free_instance,
};
use crate::ui::{dispatch_meta, dispatch_ui};

type ComputeFn =
    unsafe extern "C" fn(*mut c_void, c_int, *mut *mut FaustFloat, *mut *mut FaustFloat);

/// Create a new Cranelift DSP instance from a factory.
///
/// # Safety
/// `factory` must be a valid non-null factory pointer that outlives the
/// returned instance.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createCCraneliftDSPInstance(
    factory: *mut CraneliftDspFactory,
) -> *mut CraneliftDspInstance {
    unsafe {
        if factory.is_null() {
            return std::ptr::null_mut();
        }
        let Some(jit) = (*factory).compiled_jit.as_ref() else {
            return std::ptr::null_mut();
        };
        let layout = jit.struct_layout();
        let state = match DspStateBuffer::new(
            layout.size_bytes() as usize,
            layout.align_bytes() as usize,
        ) {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let sidecar_executor = (*factory).interp_sidecar.as_ref().map(|sidecar| {
            codegen::backends::interp::FbcExecutor::new(
                sidecar.int_heap_size as usize,
                sidecar.real_heap_size as usize,
            )
        });

        alloc_instance(factory.cast_const(), 0, state, sidecar_executor)
    }
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

/// Clone a Cranelift DSP instance (state + sidecar heaps).
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
        let state = match (*dsp).dsp_state.deep_clone() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        let sidecar_executor = (*dsp).sidecar_executor.as_ref().map(|exec| {
            let mut cloned = codegen::backends::interp::FbcExecutor::new(
                exec.int_heap.len(),
                exec.real_heap.len(),
            );
            cloned.int_heap.copy_from_slice(&exec.int_heap);
            cloned.real_heap.copy_from_slice(&exec.real_heap);
            cloned
        });
        let clone = CraneliftDspInstance {
            factory: (*dsp).factory,
            sample_rate: (*dsp).sample_rate,
            initialized: (*dsp).initialized,
            cycle: (*dsp).cycle,
            dsp_state: state,
            sidecar_executor,
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

/// Return the current sample rate recorded in the instance.
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

/// Instance init entry point (runs class-init/constants/reset/clear sequence).
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
        class_init_instance(dsp);
        instanceConstantsCCraneliftDSPInstance(dsp, sample_rate);
        instanceResetUserInterfaceCCraneliftDSPInstance(dsp);
        instanceClearCCraneliftDSPInstance(dsp);
    }
}

/// Record the sample rate in the instance and run sidecar init block.
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
        let Some(factory) = (*dsp).factory.as_ref() else {
            return;
        };
        let Some(sidecar) = factory.interp_sidecar.as_ref() else {
            return;
        };
        let Some(exec) = (*dsp).sidecar_executor.as_mut() else {
            return;
        };
        if let Some(slot) = exec.int_heap.get_mut(sidecar.sr_offset as usize) {
            *slot = sample_rate;
        }
        exec.execute_block(&sidecar.arena, sidecar.init_block);
    }
}

/// Reset UI state by executing sidecar reset-ui instructions when available.
///
/// # Safety
/// `dsp` must be a valid instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instanceResetUserInterfaceCCraneliftDSPInstance(
    dsp: *mut CraneliftDspInstance,
) {
    unsafe {
        let Some(dsp) = dsp.as_mut() else {
            return;
        };
        let Some(factory) = dsp.factory.as_ref() else {
            return;
        };
        let Some(sidecar) = factory.interp_sidecar.as_ref() else {
            return;
        };
        let Some(exec) = dsp.sidecar_executor.as_mut() else {
            return;
        };
        exec.execute_block(&sidecar.arena, sidecar.reset_ui_block);
    }
}

/// Clear DSP state and reset cycle counter.
///
/// # Safety
/// `dsp` must be a valid instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instanceClearCCraneliftDSPInstance(dsp: *mut CraneliftDspInstance) {
    unsafe {
        if dsp.is_null() {
            return;
        }
        (*dsp).dsp_state.zero();
        if let Some(factory) = (*dsp).factory.as_ref()
            && let Some(sidecar) = factory.interp_sidecar.as_ref()
            && let Some(exec) = (*dsp).sidecar_executor.as_mut()
        {
            exec.execute_block(&sidecar.arena, sidecar.clear_block);
        }
        (*dsp).cycle = 0;
    }
}

/// Trigger UI callbacks for the instance from sidecar UI instruction lists.
///
/// # Safety
/// `dsp` and `ui` may be null; null values are ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buildUserInterfaceCCraneliftDSPInstance(
    dsp: *mut CraneliftDspInstance,
    ui: *mut UIGlue,
) {
    unsafe {
        if dsp.is_null() || ui.is_null() {
            return;
        }
        let Some(factory) = (*dsp).factory.as_ref() else {
            return;
        };
        let Some(sidecar) = factory.interp_sidecar.as_ref() else {
            return;
        };
        let Some(exec) = (*dsp).sidecar_executor.as_mut() else {
            return;
        };
        dispatch_ui(&sidecar.ui_block, &mut exec.real_heap, ui);
    }
}

/// Trigger metadata callbacks for the instance.
///
/// # Safety
/// `meta` may be null. If non-null and `declare` is set, callback contract is
/// the caller's responsibility.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn metadataCCraneliftDSPInstance(
    dsp: *mut CraneliftDspInstance,
    meta: *mut MetaGlue,
) {
    unsafe {
        if meta.is_null() || dsp.is_null() {
            return;
        }
        let Some(factory) = (*dsp).factory.as_ref() else {
            return;
        };
        let Some(declare) = (*meta).declare else {
            return;
        };
        if let Some(sidecar) = factory.interp_sidecar.as_ref() {
            dispatch_meta(&sidecar.meta_block, meta);
        }
        let key = c"backend";
        let value = c"cranelift";
        declare((*meta).meta_interface, key.as_ptr(), value.as_ptr());
        let key = c"cranelift-jit-compiled";
        let value = if factory.compiled_jit.is_some() {
            c"true"
        } else {
            c"false"
        };
        declare((*meta).meta_interface, key.as_ptr(), value.as_ptr());
        let key = c"cranelift-compute-body-lowered";
        let value = if factory.compute_body_lowered {
            c"true"
        } else {
            c"false"
        };
        declare((*meta).meta_interface, key.as_ptr(), value.as_ptr());
    }
}

/// Compute audio for one block by invoking the finalized Cranelift JIT entry.
///
/// # Safety
/// `dsp` must be a valid instance pointer and `inputs`/`outputs` must follow
/// the standard Faust `FAUSTFLOAT**` contract for `count` frames.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn computeCCraneliftDSPInstance(
    dsp: *mut CraneliftDspInstance,
    count: c_int,
    input: *mut *mut FaustFloat,
    output: *mut *mut FaustFloat,
) {
    unsafe {
        if dsp.is_null() || count <= 0 {
            return;
        }
        let Some(factory) = (*dsp).factory.as_ref() else {
            return;
        };
        let Some(jit) = factory.compiled_jit.as_ref() else {
            return;
        };
        let compute = match compute_fn_from_addr(jit.compute_entry_addr()) {
            Some(f) => f,
            None => return,
        };
        let dsp_ptr = (*dsp).dsp_state.as_mut_ptr().cast::<c_void>();
        if dsp_ptr.is_null() {
            return;
        }
        compute(dsp_ptr, count, input, output);
        (*dsp).cycle = (*dsp).cycle.saturating_add(1);
    }
}

/// Instance scaffold status string kept for module-presence tests.
#[must_use]
pub fn instance_status() -> &'static str {
    "cranelift-ffi instance runtime"
}

unsafe fn class_init_instance(dsp: *mut CraneliftDspInstance) {
    unsafe {
        let Some(factory) = (*dsp).factory.as_ref() else {
            return;
        };
        let Some(sidecar) = factory.interp_sidecar.as_ref() else {
            return;
        };
        let Some(exec) = (*dsp).sidecar_executor.as_mut() else {
            return;
        };
        exec.execute_block(&sidecar.arena, sidecar.static_init_block);
    }
}

fn compute_fn_from_addr(addr: usize) -> Option<ComputeFn> {
    if addr == 0 {
        None
    } else {
        // SAFETY: address comes from finalized Cranelift symbol for `compute` with
        // known ABI/signature in this backend module.
        Some(unsafe { std::mem::transmute::<usize, ComputeFn>(addr) })
    }
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
    fn instance_status_is_stable() {
        let _guard = crate::test_serial_guard();
        assert_eq!(instance_status(), "cranelift-ffi instance runtime");
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
        let _guard = crate::test_serial_guard();
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
        assert!(!entries.is_empty());

        let mut in_buf = [0.0_f32; 8];
        let mut out_buf = [0.0_f32; 8];
        let mut inputs: [*mut FaustFloat; 1] = [in_buf.as_mut_ptr()];
        let mut outputs: [*mut FaustFloat; 1] = [out_buf.as_mut_ptr()];
        unsafe { computeCCraneliftDSPInstance(dsp, 8, inputs.as_mut_ptr(), outputs.as_mut_ptr()) };
        assert!(out_buf.iter().any(|x| x.is_finite()));

        unsafe {
            deleteCCraneliftDSPInstance(clone);
            deleteCCraneliftDSPInstance(dsp);
            assert!(deleteCCraneliftDSPFactory(factory));
        }
    }
}
