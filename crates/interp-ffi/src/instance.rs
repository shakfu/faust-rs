//! Instance-level `extern "C"` functions.
//!
//! Implements the C API from `interpreter-dsp-c.h` for DSP instance lifecycle,
//! audio computation, UI, and metadata.
//!
//! # Lifetime contract
//! The factory passed to `createCInterpreterDSPInstance` must outlive all
//! instances created from it.  This mirrors the C++ API contract.
//!
//! # Implementation notes
//! `InterpreterDspInstance` holds a raw `*const InterpreterDspFactory` (non-owning).
//! The instance execution logic replicates the semantics of `FbcDspInstance`
//! without using its borrow-based lifetime (which is incompatible with FFI).

use std::ffi::c_char;
use std::os::raw::c_int;

use codegen::backends::interp::FbcExecutor;

use crate::types::{
    alloc_instance, free_instance, FaustFloat, InterpreterDspFactory,
    InterpreterDspInstance, MetaGlue, UIGlue,
};
use crate::ui::{dispatch_meta, dispatch_ui};

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

        let executor = FbcExecutor::new(
            (*factory).inner.int_heap_size as usize,
            (*factory).inner.real_heap_size as usize,
        );

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

// ── Audio layout ─────────────────────────────────────────────────────────────

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
        (*(*dsp).factory).inner.num_inputs
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
        (*(*dsp).factory).inner.num_outputs
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
        let sr_off = (*(*dsp).factory).inner.sr_offset as usize;
        // explicit &-ref required to silence `dangerous_implicit_autorefs` (edition 2024)
        #[allow(clippy::needless_borrow)]
        let result = (&(*dsp).executor.int_heap).get(sr_off).copied().unwrap_or(0);
        result
    }
}

// ── Initialization lifecycle ──────────────────────────────────────────────────

/// Full initialization: sets sample rate, runs all init sub-steps.
///
/// Equivalent to `init()` in the C++ API.
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
        // classInit has to be called per instance (tables are not shared).
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
        let sr_off = factory.sr_offset as usize;
        // explicit &mut ref required to silence `dangerous_implicit_autorefs` (edition 2024)
        #[allow(clippy::needless_borrow)]
        if let Some(slot) = (&mut (*dsp).executor.int_heap).get_mut(sr_off) {
            *slot = sample_rate;
        }
        (*dsp)
            .executor
            .execute_block(&factory.arena, factory.init_block);
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
        (*dsp)
            .executor
            .execute_block(&factory.arena, factory.reset_ui_block);
    }
}

/// Execute the clear block (zeros delay lines and state variables).
///
/// # Safety
/// `dsp` must be a valid non-null instance pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn instanceClearCInterpreterDSPInstance(
    dsp: *mut InterpreterDspInstance,
) {
    unsafe {
        if dsp.is_null() {
            return;
        }
        let factory = &(*(*dsp).factory).inner;
        (*dsp)
            .executor
            .execute_block(&factory.arena, factory.clear_block);
    }
}

// ── Clone ────────────────────────────────────────────────────────────────────

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

        let mut new_executor = FbcExecutor::new(
            factory.int_heap_size as usize,
            factory.real_heap_size as usize,
        );
        // Copy heap state from the original.
        new_executor.int_heap.copy_from_slice(&(*dsp).executor.int_heap);
        new_executor.real_heap.copy_from_slice(&(*dsp).executor.real_heap);

        let clone = alloc_instance(factory_ptr, new_executor);
        (*clone).initialized = (*dsp).initialized;
        (*clone).cycle = 0; // new instance starts fresh cycle count
        clone
    }
}

// ── User interface ────────────────────────────────────────────────────────────

/// Traverse the UI instruction list and invoke `UIGlue` callbacks.
///
/// `zone` pointers in the callbacks reference the instance's `real_heap`.
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
        let ui_block = &(*(*dsp).factory).inner.ui_block;
        #[allow(clippy::needless_borrow)]
        dispatch_ui(ui_block, &mut (&mut (*dsp).executor.real_heap)[..], glue);
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
        let meta_block = &(*(*dsp).factory).inner.meta_block;
        dispatch_meta(meta_block, meta);
    }
}

// ── Audio computation ─────────────────────────────────────────────────────────

/// Process one buffer of audio samples.
///
/// - `count` — number of frames.
/// - `inputs` — array of `num_inputs` non-interleaved `float*` buffers.
/// - `outputs` — array of `num_outputs` non-interleaved `float*` buffers.
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
        let num_in = factory.num_inputs as usize;
        let num_out = factory.num_outputs as usize;

        // Build input/output slice views.
        let input_slices: Vec<&[FaustFloat]> = (0..num_in)
            .map(|i| {
                let ptr = *inputs.add(i);
                std::slice::from_raw_parts(ptr, n)
            })
            .collect();

        let mut output_slices: Vec<&mut [FaustFloat]> = (0..num_out)
            .map(|i| {
                let ptr = *outputs.add(i);
                std::slice::from_raw_parts_mut(ptr, n)
            })
            .collect();

        // Store frame count in the 'count' heap slot.
        // explicit &mut ref required to silence `dangerous_implicit_autorefs` (edition 2024)
        #[allow(clippy::needless_borrow)]
        if let Some(slot) = (&mut (*dsp).executor.int_heap)
            .get_mut(factory.count_offset as usize)
        {
            *slot = count;
        }

        // Execute the control block.
        (*dsp)
            .executor
            .execute_block(&factory.arena, factory.compute_block);

        // Execute the DSP block with audio I/O.
        (*dsp).executor.execute_block_io(
            &factory.arena,
            factory.compute_dsp_block,
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
        (*dsp)
            .executor
            .execute_block(&factory.arena, factory.static_init_block);
    }
}

// ── C-string helper re-exported for the C++ wrapper ──────────────────────────

/// Expose a non-null version string for the C++ wrapper header.
#[unsafe(no_mangle)]
pub extern "C" fn getInterpreterDSPInstanceVersion() -> *const c_char {
    // Returns the same version as getCLibFaustVersion.
    crate::factory::getCLibFaustVersion()
}
