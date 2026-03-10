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

use codegen::backends::cranelift::{StructFieldKind, StructFieldLayout, StructLayoutPlan};
use fir::FirType;

use crate::runtime::{RuntimeDescriptor, RuntimeFieldInit, RuntimeUiItem};
use crate::types::{
    CraneliftDspFactory, CraneliftDspInstance, DspStateBuffer, FaustFloat, MetaGlue, UIGlue,
    alloc_instance, free_instance,
};

type ComputeFn =
    unsafe extern "C" fn(*mut c_void, c_int, *mut *mut FaustFloat, *mut *mut FaustFloat);

fn arity_to_c_int(value: usize) -> c_int {
    i32::try_from(value).unwrap_or(i32::MAX)
}

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
        alloc_instance(factory.cast_const(), 0, state)
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

/// Clone a Cranelift DSP instance.
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
        let clone = CraneliftDspInstance {
            factory: (*dsp).factory,
            sample_rate: (*dsp).sample_rate,
            initialized: (*dsp).initialized,
            cycle: (*dsp).cycle,
            dsp_state: state,
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
        arity_to_c_int((*(*dsp).factory).num_inputs)
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
        arity_to_c_int((*(*dsp).factory).num_outputs)
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
        let Some(jit) = factory.compiled_jit.as_ref() else {
            return;
        };
        apply_constant_inits(&mut (*dsp).dsp_state, jit.struct_layout(), &factory.runtime);
        apply_sample_rate(&mut (*dsp).dsp_state, jit.struct_layout(), &factory.runtime, sample_rate);
    }
}

fn apply_constant_inits(
    dsp_state: &mut DspStateBuffer,
    layout: &StructLayoutPlan,
    runtime: &RuntimeDescriptor,
) {
    for (name, init) in &runtime.field_inits {
        let Some(field) = layout.field(name) else {
            continue;
        };
        write_field_init(dsp_state, field, init);
    }
}

fn apply_sample_rate(
    dsp_state: &mut DspStateBuffer,
    layout: &StructLayoutPlan,
    runtime: &RuntimeDescriptor,
    sample_rate: c_int,
) {
    for name in &runtime.sample_rate_fields {
        let Some(field) = layout.field(name) else {
            continue;
        };
        match &field.kind {
            StructFieldKind::Scalar(FirType::Int32) => {
                write_i32(dsp_state, field.offset_bytes as usize, sample_rate);
            }
            StructFieldKind::Scalar(FirType::Float32 | FirType::FaustFloat) => {
                write_f32(dsp_state, field.offset_bytes as usize, sample_rate as f32);
            }
            StructFieldKind::Scalar(FirType::Float64) => {
                write_f64(dsp_state, field.offset_bytes as usize, sample_rate as f64);
            }
            _ => {}
        }
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
        let Some(jit) = factory.compiled_jit.as_ref() else {
            return;
        };
        apply_control_defaults(&mut dsp.dsp_state, jit.struct_layout(), &factory.runtime);
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
        let Some(factory) = (*dsp).factory.as_ref() else {
            return;
        };
        let Some(jit) = factory.compiled_jit.as_ref() else {
            return;
        };
        clear_runtime_state(&mut (*dsp).dsp_state, jit.struct_layout(), &factory.runtime);
        for name in &factory.runtime.sample_rate_fields {
            if let Some(field) = jit.struct_layout().field(name) {
                match &field.kind {
                    StructFieldKind::Scalar(FirType::Int32) => {
                        write_i32(&mut (*dsp).dsp_state, field.offset_bytes as usize, (*dsp).sample_rate);
                    }
                    StructFieldKind::Scalar(FirType::Float32 | FirType::FaustFloat) => {
                        write_f32(
                            &mut (*dsp).dsp_state,
                            field.offset_bytes as usize,
                            (*dsp).sample_rate as f32,
                        );
                    }
                    StructFieldKind::Scalar(FirType::Float64) => {
                        write_f64(
                            &mut (*dsp).dsp_state,
                            field.offset_bytes as usize,
                            (*dsp).sample_rate as f64,
                        );
                    }
                    _ => {}
                }
            }
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
        let Some(jit) = factory.compiled_jit.as_ref() else {
            return;
        };
        dispatch_ui_runtime(
            &factory.runtime,
            jit.struct_layout(),
            &mut (*dsp).dsp_state,
            ui,
        );
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
        for (key, value) in &factory.runtime.meta_entries {
            let key = std::ffi::CString::new(key.as_str()).ok();
            let value = std::ffi::CString::new(value.as_str()).ok();
            if let (Some(key), Some(value)) = (key, value) {
                declare((*meta).meta_interface, key.as_ptr(), value.as_ptr());
            }
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

unsafe fn class_init_instance(_dsp: *mut CraneliftDspInstance) {}

fn dispatch_ui_runtime(
    runtime: &RuntimeDescriptor,
    layout: &StructLayoutPlan,
    dsp_state: &mut DspStateBuffer,
    ui: *mut UIGlue,
) {
    unsafe {
        let ui = &*ui;
        for item in &runtime.ui_items {
            match item {
                RuntimeUiItem::OpenTabBox { label } => {
                    if let Some(f) = ui.open_tab_box
                        && let Ok(label) = std::ffi::CString::new(label.as_str())
                    {
                        f(ui.ui_interface, label.as_ptr());
                    }
                }
                RuntimeUiItem::OpenHorizontalBox { label } => {
                    if let Some(f) = ui.open_horizontal_box
                        && let Ok(label) = std::ffi::CString::new(label.as_str())
                    {
                        f(ui.ui_interface, label.as_ptr());
                    }
                }
                RuntimeUiItem::OpenVerticalBox { label } => {
                    if let Some(f) = ui.open_vertical_box
                        && let Ok(label) = std::ffi::CString::new(label.as_str())
                    {
                        f(ui.ui_interface, label.as_ptr());
                    }
                }
                RuntimeUiItem::CloseBox => {
                    if let Some(f) = ui.close_box {
                        f(ui.ui_interface);
                    }
                }
                RuntimeUiItem::Button { label, zone } => {
                    if let Some(f) = ui.add_button
                        && let (Ok(label), Some(zone)) = (
                            std::ffi::CString::new(label.as_str()),
                            zone_ptr(dsp_state, layout, zone),
                        )
                    {
                        f(ui.ui_interface, label.as_ptr(), zone);
                    }
                }
                RuntimeUiItem::CheckButton { label, zone } => {
                    if let Some(f) = ui.add_check_button
                        && let (Ok(label), Some(zone)) = (
                            std::ffi::CString::new(label.as_str()),
                            zone_ptr(dsp_state, layout, zone),
                        )
                    {
                        f(ui.ui_interface, label.as_ptr(), zone);
                    }
                }
                RuntimeUiItem::VerticalSlider {
                    label,
                    zone,
                    init,
                    lo,
                    hi,
                    step,
                } => {
                    if let Some(f) = ui.add_vertical_slider
                        && let (Ok(label), Some(zone)) = (
                            std::ffi::CString::new(label.as_str()),
                            zone_ptr(dsp_state, layout, zone),
                        )
                    {
                        f(ui.ui_interface, label.as_ptr(), zone, *init, *lo, *hi, *step);
                    }
                }
                RuntimeUiItem::HorizontalSlider {
                    label,
                    zone,
                    init,
                    lo,
                    hi,
                    step,
                } => {
                    if let Some(f) = ui.add_horizontal_slider
                        && let (Ok(label), Some(zone)) = (
                            std::ffi::CString::new(label.as_str()),
                            zone_ptr(dsp_state, layout, zone),
                        )
                    {
                        f(ui.ui_interface, label.as_ptr(), zone, *init, *lo, *hi, *step);
                    }
                }
                RuntimeUiItem::NumEntry {
                    label,
                    zone,
                    init,
                    lo,
                    hi,
                    step,
                } => {
                    if let Some(f) = ui.add_num_entry
                        && let (Ok(label), Some(zone)) = (
                            std::ffi::CString::new(label.as_str()),
                            zone_ptr(dsp_state, layout, zone),
                        )
                    {
                        f(ui.ui_interface, label.as_ptr(), zone, *init, *lo, *hi, *step);
                    }
                }
                RuntimeUiItem::HorizontalBargraph { label, zone, lo, hi } => {
                    if let Some(f) = ui.add_horizontal_bargraph
                        && let (Ok(label), Some(zone)) = (
                            std::ffi::CString::new(label.as_str()),
                            zone_ptr(dsp_state, layout, zone),
                        )
                    {
                        f(ui.ui_interface, label.as_ptr(), zone, *lo, *hi);
                    }
                }
                RuntimeUiItem::VerticalBargraph { label, zone, lo, hi } => {
                    if let Some(f) = ui.add_vertical_bargraph
                        && let (Ok(label), Some(zone)) = (
                            std::ffi::CString::new(label.as_str()),
                            zone_ptr(dsp_state, layout, zone),
                        )
                    {
                        f(ui.ui_interface, label.as_ptr(), zone, *lo, *hi);
                    }
                }
                RuntimeUiItem::Soundfile { label, url, zone } => {
                    if let Some(f) = ui.add_soundfile
                        && let (Ok(label), Ok(url)) = (
                            std::ffi::CString::new(label.as_str()),
                            std::ffi::CString::new(url.as_str()),
                        )
                    {
                        let zone = zone_ptr(dsp_state, layout, zone).map_or(std::ptr::null_mut(), |p| p.cast());
                        f(ui.ui_interface, label.as_ptr(), url.as_ptr(), zone);
                    }
                }
            }
        }
    }
}

fn zone_ptr(
    dsp_state: &mut DspStateBuffer,
    layout: &StructLayoutPlan,
    name: &str,
) -> Option<*mut FaustFloat> {
    let field = layout.field(name)?;
    match &field.kind {
        StructFieldKind::Scalar(FirType::Float32 | FirType::FaustFloat)
        | StructFieldKind::Scalar(FirType::Int32)
        | StructFieldKind::Scalar(FirType::Bool) => {
            Some(dsp_state.ptr_at(field.offset_bytes as usize).cast::<FaustFloat>())
        }
        _ => None,
    }
}

fn clear_runtime_state(
    dsp_state: &mut DspStateBuffer,
    layout: &StructLayoutPlan,
    runtime: &RuntimeDescriptor,
) {
    for name in &runtime.clear_fields {
        let Some(field) = layout.field(name) else {
            continue;
        };
        unsafe {
            std::ptr::write_bytes(
                dsp_state.ptr_at(field.offset_bytes as usize),
                0_u8,
                field.size_bytes as usize,
            );
        }
    }
}

fn apply_control_defaults(
    dsp_state: &mut DspStateBuffer,
    layout: &StructLayoutPlan,
    runtime: &RuntimeDescriptor,
) {
    for (name, value) in &runtime.control_defaults {
        let Some(field) = layout.field(name) else {
            continue;
        };
        match &field.kind {
            StructFieldKind::Scalar(FirType::Float32 | FirType::FaustFloat) => {
                write_f32(dsp_state, field.offset_bytes as usize, *value);
            }
            StructFieldKind::Scalar(FirType::Float64) => {
                write_f64(dsp_state, field.offset_bytes as usize, *value as f64);
            }
            StructFieldKind::Scalar(FirType::Int32) => {
                write_i32(dsp_state, field.offset_bytes as usize, *value as i32);
            }
            _ => {}
        }
    }
}

fn write_field_init(
    dsp_state: &mut DspStateBuffer,
    field: &StructFieldLayout,
    init: &RuntimeFieldInit,
) {
    match init {
        RuntimeFieldInit::I32(v) => write_i32(dsp_state, field.offset_bytes as usize, *v),
        RuntimeFieldInit::I64(v) => write_i64(dsp_state, field.offset_bytes as usize, *v),
        RuntimeFieldInit::F32(v) => write_f32(dsp_state, field.offset_bytes as usize, *v),
        RuntimeFieldInit::F64(v) => write_f64(dsp_state, field.offset_bytes as usize, *v),
        RuntimeFieldInit::Bool(v) => write_bool(dsp_state, field.offset_bytes as usize, *v),
        RuntimeFieldInit::I32Array(values) => {
            for (i, v) in values.iter().enumerate() {
                write_i32(dsp_state, field.offset_bytes as usize + i * 4, *v);
            }
        }
        RuntimeFieldInit::F32Array(values) => {
            for (i, v) in values.iter().enumerate() {
                write_f32(dsp_state, field.offset_bytes as usize + i * 4, *v);
            }
        }
        RuntimeFieldInit::F64Array(values) => {
            for (i, v) in values.iter().enumerate() {
                write_f64(dsp_state, field.offset_bytes as usize + i * 8, *v);
            }
        }
    }
}

fn write_i32(dsp_state: &mut DspStateBuffer, offset: usize, value: i32) {
    unsafe { std::ptr::write_unaligned(dsp_state.ptr_at(offset).cast::<i32>(), value) };
}

fn write_i64(dsp_state: &mut DspStateBuffer, offset: usize, value: i64) {
    unsafe { std::ptr::write_unaligned(dsp_state.ptr_at(offset).cast::<i64>(), value) };
}

fn write_f32(dsp_state: &mut DspStateBuffer, offset: usize, value: f32) {
    unsafe { std::ptr::write_unaligned(dsp_state.ptr_at(offset).cast::<f32>(), value) };
}

fn write_f64(dsp_state: &mut DspStateBuffer, offset: usize, value: f64) {
    unsafe { std::ptr::write_unaligned(dsp_state.ptr_at(offset).cast::<f64>(), value) };
}

fn write_bool(dsp_state: &mut DspStateBuffer, offset: usize, value: bool) {
    unsafe { std::ptr::write_unaligned(dsp_state.ptr_at(offset).cast::<u8>(), u8::from(value)) };
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
    use crate::factory::{
        createCCraneliftDSPFactoryFromFile, createCCraneliftDSPFactoryFromString,
        deleteCCraneliftDSPFactory,
    };
    use crate::types::{FaustFloat, MetaGlue, UIGlue};

    fn workspace_root() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root")
    }

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

    #[test]
    fn runtime_rep38_produces_non_silent_output() {
        let _guard = crate::test_serial_guard();
        let case = workspace_root().join("tests/corpus/rep_38_sine_phasor.dsp");
        assert_runtime_case_non_silent(&case);
    }

    #[test]
    fn runtime_rep55_produces_non_silent_output() {
        let _guard = crate::test_serial_guard();
        let case = workspace_root().join("tests/corpus/rep_55_sine_phasor_echo_feedback.dsp");
        assert_runtime_case_non_silent(&case);
    }

    fn assert_runtime_case_non_silent(case: &std::path::Path) {
        let case_c = CString::new(case.to_string_lossy().as_bytes()).expect("path CString");
        let mut err = [0_i8; 4096];
        let factory = unsafe {
            createCCraneliftDSPFactoryFromFile(
                case_c.as_ptr(),
                0,
                std::ptr::null(),
                err.as_mut_ptr(),
                1,
            )
        };
        assert!(!factory.is_null(), "factory failed: {:?}", unsafe {
            CStr::from_ptr(err.as_ptr()).to_str().ok()
        });

        let dsp = unsafe { createCCraneliftDSPInstance(factory) };
        assert!(!dsp.is_null());
        unsafe { initCCraneliftDSPInstance(dsp, 48_000) };

        let num_inputs = unsafe { getNumInputsCCraneliftDSPInstance(dsp) }.max(0) as usize;
        let num_outputs = unsafe { getNumOutputsCCraneliftDSPInstance(dsp) }.max(0) as usize;
        let frames = 256usize;
        let mut input_buffers = vec![vec![0.0_f32; frames]; num_inputs];
        let mut output_buffers = vec![vec![0.0_f32; frames]; num_outputs.max(1)];
        let mut input_ptrs: Vec<*mut FaustFloat> =
            input_buffers.iter_mut().map(|buf| buf.as_mut_ptr()).collect();
        let mut output_ptrs: Vec<*mut FaustFloat> =
            output_buffers.iter_mut().map(|buf| buf.as_mut_ptr()).collect();

        unsafe {
            computeCCraneliftDSPInstance(
                dsp,
                frames as i32,
                if input_ptrs.is_empty() {
                    std::ptr::null_mut()
                } else {
                    input_ptrs.as_mut_ptr()
                },
                output_ptrs.as_mut_ptr(),
            );
        }

        let non_silent = output_buffers
            .iter()
            .flat_map(|buf| buf.iter())
            .any(|sample| sample.abs() > 1.0e-6);
        assert!(non_silent, "{} output stayed silent", case.display());

        unsafe {
            deleteCCraneliftDSPInstance(dsp);
            assert!(deleteCCraneliftDSPFactory(factory));
        }
    }
}
