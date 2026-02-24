//! Shared FIR fixtures for backend generation tests/examples.
//!
//! These builders define canonical FIR modules reused across multiple backend
//! tests (C, C++, and future backends) to avoid copy/paste drift.

use fir::{
    AccessType, FirBinOp, FirBuilder, FirId, FirStore, FirType, NamedType, SliderType, UiBoxType,
};

/// Builds a canonical FIR module for a phasor-driven sine oscillator.
///
/// Module shape:
/// - UI controls: `freq` and `gain` sliders
/// - Stateful phase accumulator `fPhase`
/// - one output signal: `gain * sin(2*pi*phase)`
///
/// This fixture is intentionally backend-agnostic so all emitters can be
/// validated from the exact same FIR input.
#[must_use]
pub fn build_sine_phasor_test_module() -> (FirStore, FirId) {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);

    let f440 = b.float64(440.0);
    let f02 = b.float64(0.2);
    let f0 = b.float64(0.0);
    let dec_freq = b.declare_var("fFreq", FirType::FaustFloat, AccessType::Struct, Some(f440));
    let dec_gain = b.declare_var("fGain", FirType::FaustFloat, AccessType::Struct, Some(f02));
    let dec_phase = b.declare_var("fPhase", FirType::Float64, AccessType::Struct, Some(f0));
    let globals = b.block(&[dec_freq, dec_gain, dec_phase]);

    let open = b.open_box(UiBoxType::Vertical, "Oscillator");
    let freq_slider = b.add_slider(
        SliderType::Horizontal,
        "freq",
        "fFreq",
        fir::SliderRange {
            init: 440.0,
            lo: 20.0,
            hi: 3000.0,
            step: 1.0,
        },
    );
    let gain_slider = b.add_slider(
        SliderType::Horizontal,
        "gain",
        "fGain",
        fir::SliderRange {
            init: 0.2,
            lo: 0.0,
            hi: 1.0,
            step: 0.001,
        },
    );
    let close = b.close_box();
    let build_ui_body = b.block(&[open, freq_slider, gain_slider, close]);
    let build_ui_args = [
        NamedType {
            name: "dsp".to_string(),
            typ: FirType::Ptr(Box::new(FirType::Obj)),
        },
        NamedType {
            name: "ui_interface".to_string(),
            typ: FirType::UI,
        },
    ];
    let build_ui = b.declare_fun(
        "buildUserInterface",
        FirType::Fun {
            args: vec![FirType::Ptr(Box::new(FirType::Obj)), FirType::UI],
            ret: Box::new(FirType::Void),
        },
        &build_ui_args,
        Some(build_ui_body),
        false,
    );

    let freq = b.load_var("fFreq", AccessType::Struct, FirType::FaustFloat);
    let gain = b.load_var("fGain", AccessType::Struct, FirType::FaustFloat);
    let phase = b.load_var("fPhase", AccessType::Struct, FirType::Float64);
    let sample_rate = b.float64(48_000.0);
    let one = b.float64(1.0);
    let two_pi = b.float64(std::f64::consts::TAU);

    let phase_inc = b.binop(FirBinOp::Div, freq, sample_rate, FirType::Float64);
    let next_phase = b.binop(FirBinOp::Add, phase, phase_inc, FirType::Float64);
    let wrapped_phase_minus = b.binop(FirBinOp::Sub, next_phase, one, FirType::Float64);
    let wrap_cond = b.binop(FirBinOp::Ge, next_phase, one, FirType::Bool);
    let wrapped_phase = b.select2(wrap_cond, wrapped_phase_minus, next_phase, FirType::Float64);
    let store_phase = b.store_var("fPhase", AccessType::Struct, wrapped_phase);

    let phase_angle = b.binop(FirBinOp::Mul, two_pi, wrapped_phase, FirType::Float64);
    let sine = b.fun_call("std::sin", &[phase_angle], FirType::Float64);
    let out = b.binop(FirBinOp::Mul, gain, sine, FirType::Float64);
    let out_chan = b.int32(0);
    let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
    let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
    let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
    let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
    let store_out = b.store_table("output0", AccessType::Stack, i0, out);

    let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
    let sample_body = b.block(&[store_phase, store_out]);
    let sample_loop = b.simple_for_loop("i0", count, sample_body, false);
    let compute_body = b.block(&[out_alias, sample_loop]);
    let compute_args = [
        NamedType {
            name: "dsp".to_string(),
            typ: FirType::Ptr(Box::new(FirType::Obj)),
        },
        NamedType {
            name: "count".to_string(),
            typ: FirType::Int32,
        },
        NamedType {
            name: "inputs".to_string(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
        NamedType {
            name: "outputs".to_string(),
            typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
        },
    ];
    let compute = b.declare_fun(
        "compute",
        FirType::Fun {
            args: vec![
                FirType::Ptr(Box::new(FirType::Obj)),
                FirType::Int32,
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            ],
            ret: Box::new(FirType::Void),
        },
        &compute_args,
        Some(compute_body),
        false,
    );

    let declarations = b.block(&[build_ui, compute]);
    let dsp_struct = b.block(&[]);
    let module = b.module("mydsp", dsp_struct, globals, declarations);
    (store, module)
}
