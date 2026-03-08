//! Instance-level `extern "C"` functions.
//!
//! Implements the C API from `interpreter-dsp-c.h` for DSP instance lifecycle,
//! audio computation, UI, and metadata.
//!
//! # Lifetime contract
//! The factory passed to `createCInterpreterDSPInstance` must outlive all
//! instances created from it.  This mirrors the C++ API contract.
//!
//! # Float/Double dispatch
//! All operations delegate to `FbcDspFactoryAny` / `FbcExecutorAny` helpers
//! so that a single shared library handles both `float` and `double` factories
//! transparently.  Audio I/O buffers remain `f32*` at the C ABI boundary;
//! in double mode, samples are converted `f32 → f64` on entry and
//! `f64 → f32` on exit inside `computeCInterpreterDSPInstance`.

use std::ffi::c_char;
use std::os::raw::c_int;

use crate::types::{
    FaustFloat, FbcExecutorAny, InterpreterDspFactory, InterpreterDspInstance, MetaGlue, UIGlue,
    alloc_instance, free_instance,
};
use crate::ui::dispatch_meta;

// ── Instance creation / deletion ─────────────────────────────────────────────

/// Create a new DSP instance from a factory.
///
/// Triggers one-shot factory optimization.
///
/// # Safety
/// `factory` must be a valid non-null factory pointer that outlives the
/// returned instance.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createCInterpreterDSPInstance(
    factory: *mut InterpreterDspFactory,
) -> *mut InterpreterDspInstance {
    unsafe {
        if factory.is_null() {
            return std::ptr::null_mut();
        }
        // Trigger one-shot optimization (idempotent after first call).
        (*factory).inner.optimize();

        let executor = FbcExecutorAny::new_for_factory(&(*factory).inner);
        alloc_instance(factory as *const _, executor)
    }
}

/// Delete a DSP instance.
///
/// # Safety
/// `dsp` must be a valid non-null pointer previously returned by
/// `createCInterpreterDSPInstance`, and must not be used after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn deleteCInterpreterDSPInstance(dsp: *mut InterpreterDspInstance) {
    unsafe {
        if !dsp.is_null() {
            free_instance(dsp);
        }
    }
}

// ── Audio layout ──────────────────────────────────────────────────────────────

/// Return the number of audio inputs.
///
/// # Safety
/// `dsp` must be a valid non-null instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getNumInputsCInterpreterDSPInstance(
    dsp: *mut InterpreterDspInstance,
) -> c_int {
    unsafe {
        if dsp.is_null() {
            return 0;
        }
        (*(*dsp).factory).inner.num_inputs()
    }
}

/// Return the number of audio outputs.
///
/// # Safety
/// `dsp` must be a valid non-null instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getNumOutputsCInterpreterDSPInstance(
    dsp: *mut InterpreterDspInstance,
) -> c_int {
    unsafe {
        if dsp.is_null() {
            return 0;
        }
        (*(*dsp).factory).inner.num_outputs()
    }
}

/// Return the current sample rate.
///
/// # Safety
/// `dsp` must be a valid non-null instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getSampleRateCInterpreterDSPInstance(
    dsp: *mut InterpreterDspInstance,
) -> c_int {
    unsafe {
        if dsp.is_null() {
            return 0;
        }
        let sr_off = (*(*dsp).factory).inner.sr_offset() as usize;
        (*dsp).executor.int_heap().get(sr_off).copied().unwrap_or(0)
    }
}

// ── Initialization lifecycle ───────────────────────────────────────────────────

/// Full initialization: sets sample rate, runs all init sub-steps.
///
/// # Safety
/// `dsp` must be a valid non-null instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn initCInterpreterDSPInstance(
    dsp: *mut InterpreterDspInstance,
    sample_rate: c_int,
) {
    unsafe {
        if dsp.is_null() {
            return;
        }
        (*dsp).initialized = true;
        instanceInitCInterpreterDSPInstance(dsp, sample_rate);
    }
}

/// Run all instance init sub-steps (class_init + constants + reset UI + clear).
///
/// # Safety
/// `dsp` must be a valid non-null instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instanceInitCInterpreterDSPInstance(
    dsp: *mut InterpreterDspInstance,
    sample_rate: c_int,
) {
    unsafe {
        if dsp.is_null() {
            return;
        }
        class_init_instance(dsp, sample_rate);
        instanceConstantsCInterpreterDSPInstance(dsp, sample_rate);
        instanceResetUserInterfaceCInterpreterDSPInstance(dsp);
        instanceClearCInterpreterDSPInstance(dsp);
    }
}

/// Set the sample rate and execute the init (constants) block.
///
/// # Safety
/// `dsp` must be a valid non-null instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instanceConstantsCInterpreterDSPInstance(
    dsp: *mut InterpreterDspInstance,
    sample_rate: c_int,
) {
    unsafe {
        if dsp.is_null() {
            return;
        }
        let factory = &(*(*dsp).factory).inner;
        let sr_off = factory.sr_offset() as usize;
        if let Some(slot) = (*dsp).executor.int_heap_mut().get_mut(sr_off) {
            *slot = sample_rate;
        }
        let init_block = factory.init_block();
        factory.execute_block_on(&mut (*dsp).executor, init_block);
    }
}

/// Execute the reset-UI block (sets controls to default values).
///
/// # Safety
/// `dsp` must be a valid non-null instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instanceResetUserInterfaceCInterpreterDSPInstance(
    dsp: *mut InterpreterDspInstance,
) {
    unsafe {
        if dsp.is_null() {
            return;
        }
        let factory = &(*(*dsp).factory).inner;
        let block = factory.reset_ui_block();
        factory.execute_block_on(&mut (*dsp).executor, block);
    }
}

/// Execute the clear block (zeros delay lines and state variables).
///
/// # Safety
/// `dsp` must be a valid non-null instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instanceClearCInterpreterDSPInstance(dsp: *mut InterpreterDspInstance) {
    unsafe {
        if dsp.is_null() {
            return;
        }
        let factory = &(*(*dsp).factory).inner;
        let block = factory.clear_block();
        factory.execute_block_on(&mut (*dsp).executor, block);
    }
}

// ── Clone ─────────────────────────────────────────────────────────────────────

/// Clone a DSP instance.
///
/// The clone shares the same factory but has independent heaps.
///
/// # Safety
/// `dsp` must be a valid non-null instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn cloneCInterpreterDSPInstance(
    dsp: *mut InterpreterDspInstance,
) -> *mut InterpreterDspInstance {
    unsafe {
        if dsp.is_null() {
            return std::ptr::null_mut();
        }
        let factory_ptr = (*dsp).factory;
        let factory = &(*factory_ptr).inner;

        let mut new_executor = FbcExecutorAny::new_for_factory(factory);
        new_executor.copy_from(&(*dsp).executor);

        let clone = alloc_instance(factory_ptr, new_executor);
        (*clone).initialized = (*dsp).initialized;
        (*clone).cycle = 0;
        clone
    }
}

// ── User interface ─────────────────────────────────────────────────────────────

/// Traverse the UI instruction list and invoke `UIGlue` callbacks.
///
/// In double mode, scalar params are narrowed `f64 → f32`; zone pointers
/// are `*mut f64` reinterpreted as `*mut f32` (app must use `FAUSTFLOAT=double`).
///
/// # Safety
/// - `dsp` must be a valid non-null instance pointer.
/// - `glue` must be a valid non-null `UIGlue` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn buildUserInterfaceCInterpreterDSPInstance(
    dsp: *mut InterpreterDspInstance,
    glue: *mut UIGlue,
) {
    unsafe {
        if dsp.is_null() || glue.is_null() {
            return;
        }
        let factory = &(*(*dsp).factory).inner;
        factory.dispatch_ui_glue(&mut (*dsp).executor, glue);
    }
}

/// Traverse the metadata instruction list and invoke `MetaGlue.declare`.
///
/// # Safety
/// - `dsp` must be a valid non-null instance pointer.
/// - `meta` must be a valid non-null `MetaGlue` pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn metadataCInterpreterDSPInstance(
    dsp: *mut InterpreterDspInstance,
    meta: *mut MetaGlue,
) {
    unsafe {
        if dsp.is_null() || meta.is_null() {
            return;
        }
        let meta_block = (*(*dsp).factory).inner.meta_block();
        dispatch_meta(meta_block, meta);
    }
}

// ── Audio computation ──────────────────────────────────────────────────────────

/// Process one buffer of audio samples.
///
/// - `count`   — number of frames.
/// - `inputs`  — array of `num_inputs` non-interleaved `float*` buffers.
/// - `outputs` — array of `num_outputs` non-interleaved `float*` buffers.
///
/// In double mode, samples are converted `f32 → f64` on input and
/// `f64 → f32` on output transparently.
///
/// # Safety
/// - `dsp` must be a valid non-null instance pointer.
/// - Each `inputs[i]` and `outputs[i]` must point to at least `count` floats.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn computeCInterpreterDSPInstance(
    dsp: *mut InterpreterDspInstance,
    count: c_int,
    inputs: *mut *mut FaustFloat,
    outputs: *mut *mut FaustFloat,
) {
    unsafe {
        if dsp.is_null() || count <= 0 {
            return;
        }

        let factory = &(*(*dsp).factory).inner;
        let n = count as usize;
        let num_in = factory.num_inputs() as usize;
        let num_out = factory.num_outputs() as usize;

        // Build input/output slice views.
        let input_slices: Vec<&[FaustFloat]> = (0..num_in)
            .map(|i| std::slice::from_raw_parts(*inputs.add(i), n))
            .collect();
        let mut output_slices: Vec<&mut [FaustFloat]> = (0..num_out)
            .map(|i| std::slice::from_raw_parts_mut(*outputs.add(i), n))
            .collect();

        // Store frame count in the 'count' heap slot.
        let count_off = factory.count_offset() as usize;
        if let Some(slot) = (*dsp).executor.int_heap_mut().get_mut(count_off) {
            *slot = count;
        }

        // Execute control block then DSP block with audio I/O.
        let compute_block = factory.compute_block();
        let compute_dsp_block = factory.compute_dsp_block();
        factory.execute_block_on(&mut (*dsp).executor, compute_block);
        factory.execute_block_io_f32(
            &mut (*dsp).executor,
            compute_dsp_block,
            &input_slices,
            &mut output_slices,
        );

        (*dsp).cycle += 1;
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Execute the static init block (classInit equivalent).
unsafe fn class_init_instance(dsp: *mut InterpreterDspInstance, _sample_rate: c_int) {
    unsafe {
        let factory = &(*(*dsp).factory).inner;
        let block = factory.static_init_block();
        factory.execute_block_on(&mut (*dsp).executor, block);
    }
}

// ── C-string helper re-exported for the C++ wrapper ──────────────────────────

/// Expose a non-null version string for the C++ wrapper header.
///
/// # Safety
/// The returned pointer is process-static and must not be freed or mutated.
#[unsafe(no_mangle)]
pub extern "C" fn getInterpreterDSPInstanceVersion() -> *const c_char {
    crate::factory::getCLibFaustVersion()
}
