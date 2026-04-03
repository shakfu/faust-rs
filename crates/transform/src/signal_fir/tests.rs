//! Test suite for the `signal_fir` fast-lane lowering pass.
//!
//! # Structure
//!
//! Each `#[test]` function follows the same three-step pattern:
//!
//! 1. **Build a signal forest** — use [`SigBuilder`] and, where needed,
//!    [`de_bruijn_rec`] / [`de_bruijn_ref`] to construct the input signal tree
//!    directly in a [`TreeArena`].
//! 2. **Lower to FIR** — call [`compile_fastlane_without_ui`] (or the full
//!    entry point for UI tests) and unwrap the [`SignalFirOutput`].
//! 3. **Assert on the FIR tree** — navigate to the relevant node with
//!    [`find_compute_loop_body`] / [`find_decl_fun_body`], strip the mandatory
//!    output cast with [`unwrap_output_cast`], then pattern-match with
//!    [`match_fir`].
//!
//! The private helpers below form a minimal test DSL that keeps the
//! assertion-focused body of each test free from boilerplate traversal code.

use super::{
    RealType, SignalFirErrorCode, SignalFirOptions, compile_signals_to_fir_fastlane_with_ui,
    module::interpret_generator_for_test,
};
use fir::{AccessType, FirBinOp, FirMatch, FirType, match_fir};
use signals::{BinOp, SigBuilder};

use tlib::{TreeArena, de_bruijn_rec, de_bruijn_ref};
use ui::{ControlKind, ControlRange, ControlSpec, UiBuilder, UiProgram, UiRootOrigin};

use crate::signal_prepare::{SimpleSigType, prepare_signals_for_fir};

// ── FIR tree navigation ──────────────────────────────────────────────────────

/// Peels off a `Cast(FaustFloat, inner)` wrapper if present, returning the
/// inner node unchanged if no such wrapper exists.
///
/// The lowering pass always stores output samples through an explicit
/// `Cast(FaustFloat, …)` regardless of the internal real type, so that
/// the generated C always writes `float` (or `double`) to the output
/// buffer regardless of the internal computation type.
///
/// Tests that want to assert on the *computation* result — rather than the
/// cast that writes it to the buffer — should call this first to peel the
/// wrapper and reach the actual expression node.
fn unwrap_output_cast(store: &fir::FirStore, id: fir::FirId) -> fir::FirId {
    match match_fir(store, id) {
        FirMatch::Cast {
            typ: FirType::FaustFloat,
            value,
        } => value,
        _ => id,
    }
}

/// Locates a named `DeclareFun` in a FIR functions block and returns its body.
///
/// `functions` must be a `FirMatch::Block` of `DeclareFun` nodes (the
/// top-level functions block of a generated FIR module). Panics with a
/// descriptive message if the block or the named function cannot be found,
/// or if the matching declaration has no body.
///
/// Used by [`find_compute_loop_body`] and directly by tests that need to
/// inspect generated functions other than `compute` (e.g. `init`,
/// `instanceInit`, `getNumInputs`).
fn find_decl_fun_body(store: &fir::FirStore, functions: fir::FirId, target: &str) -> fir::FirId {
    let FirMatch::Block(decls) = match_fir(store, functions) else {
        panic!("functions block expected");
    };
    let fun = decls
        .iter()
        .copied()
        .find(|id| {
            matches!(
                match_fir(store, *id),
                FirMatch::DeclareFun { ref name, .. } if name == target
            )
        })
        .unwrap_or_else(|| panic!("function `{target}` expected"));
    let FirMatch::DeclareFun {
        body: Some(body), ..
    } = match_fir(store, fun)
    else {
        panic!("declare fun with body expected for `{target}`");
    };
    body
}

/// Returns the body block of the sample loop inside the generated `compute`
/// function.
///
/// Every compiled DSP produces a `compute(count, inputs, outputs)` function
/// whose body contains exactly one sample-processing for-loop
/// (`SimpleForLoop` or `ForLoop`). This helper navigates past the function
/// declaration and loop header to return the loop body directly, so that
/// tests can pattern-match on individual statements (assignments, stores,
/// calls) without repeating the traversal.
///
/// Panics if the `compute` function or its sample loop is absent.
fn find_compute_loop_body(store: &fir::FirStore, functions: fir::FirId) -> fir::FirId {
    let compute_body = find_decl_fun_body(store, functions, "compute");
    let FirMatch::Block(stmts) = match_fir(store, compute_body) else {
        panic!("compute block expected");
    };
    stmts
        .iter()
        .find_map(|id| match match_fir(store, *id) {
            FirMatch::SimpleForLoop { body, .. } | FirMatch::ForLoop { body, .. } => Some(body),
            _ => None,
        })
        .unwrap_or_else(|| panic!("compute should contain an explicit sample loop"))
}

// ── Compilation entry-point wrappers ─────────────────────────────────────────

/// Runs the full fast-lane lowering pipeline with an empty UI program.
///
/// Most signal-level tests are not concerned with UI widget lowering.
/// This wrapper passes an empty [`UiProgram`] so those tests do not need
/// to construct one explicitly, reducing per-test boilerplate.
fn compile_fastlane_without_ui(
    arena: &TreeArena,
    signals: &[signals::SigId],
    num_inputs: usize,
    num_outputs: usize,
    options: &SignalFirOptions,
) -> Result<super::SignalFirOutput, super::SignalFirError> {
    let empty_ui = UiProgram::empty();
    compile_signals_to_fir_fastlane_with_ui(
        arena,
        signals,
        num_inputs,
        num_outputs,
        &empty_ui,
        options,
    )
}

// ── UI fixture builders ───────────────────────────────────────────────────────

/// Builds a minimal [`UiProgram`] containing exactly one control.
///
/// The generated program has a single top-level `vgroup("")` containing one
/// leaf node whose slot index is `0`. The `ControlSpec` at that slot is
/// filled with `kind`, `label`, and `range` as provided.
///
/// The three boolean flags select the leaf node type:
/// - `soundfile = true` → `UiBuilder::soundfile(0)` (takes precedence)
/// - `output = true` → `UiBuilder::output_control(0)` (bargraph)
/// - otherwise → `UiBuilder::input_control(0)` (slider / button / etc.)
///
/// Used by tests that exercise the UI lowering path (bargraphs, sliders,
/// soundfiles) without needing a full hand-crafted `UiProgram`.
fn one_control_ui(
    kind: ControlKind,
    label: &str,
    range: Option<ControlRange>,
    output: bool,
    soundfile: bool,
) -> UiProgram {
    let mut arena = TreeArena::new();
    let leaf = {
        let mut b = UiBuilder::new(&mut arena);
        if soundfile {
            b.soundfile(0)
        } else if output {
            b.output_control(0)
        } else {
            b.input_control(0)
        }
    };
    let root = UiBuilder::new(&mut arena).vgroup("", &[leaf]);
    UiProgram {
        arena,
        root,
        controls: vec![ControlSpec {
            id: 0,
            kind,
            label: label.to_owned(),
            metadata: Vec::new(),
            range,
        }],
        root_origin: UiRootOrigin::Synthesized,
        emit_ui: true,
    }
}

#[test]
fn non_empty_signal_list_returns_fir_module_root() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let i0 = b.input(0);
        let c0 = b.real(0.5);
        b.binop(BinOp::Mul, i0, c0)
    };
    let out = compile_fastlane_without_ui(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
        .expect("Step 1A should emit a module for valid top-level inputs");

    assert!(matches!(
        match_fir(&out.store, out.module),
        FirMatch::Module { .. }
    ));
    let FirMatch::Module { functions, .. } = match_fir(&out.store, out.module) else {
        panic!("module root expected");
    };
    let FirMatch::Block(decls) = match_fir(&out.store, functions) else {
        panic!("module functions block expected");
    };
    for required_name in [
        "metadata",
        "instanceConstants",
        "instanceResetUserInterface",
        "instanceClear",
        "buildUserInterface",
        "compute",
    ] {
        assert!(
            decls.iter().any(|id| {
                matches!(
                    match_fir(&out.store, *id),
                    FirMatch::DeclareFun { ref name, .. } if name == required_name
                )
            }),
            "section function `{required_name}` must exist in fast-lane module"
        );
    }
    let loop_body = find_compute_loop_body(&out.store, functions);
    let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
        panic!("compute loop body block expected");
    };
    let stored_value = stmts
        .iter()
        .find_map(|id| match match_fir(&out.store, *id) {
            FirMatch::StoreTable { name, value, .. } if name == "output0" => Some(value),
            _ => None,
        })
        .expect("compute should include one output store");
    // The output store emits an explicit FaustFloat cast around the internal
    // computation node; unwrap it to reach the actual BinOp.
    let inner = unwrap_output_cast(&out.store, stored_value);
    assert!(matches!(
        match_fir(&out.store, inner),
        FirMatch::BinOp {
            op: FirBinOp::Mul,
            ..
        }
    ));
}

#[test]
fn comparison_binops_lower_to_int32_boolean_values() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let i0 = b.input(0);
        let zero = b.real(0.0);
        b.binop(BinOp::Eq, i0, zero)
    };
    let out = compile_fastlane_without_ui(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
        .expect("comparison should lower through fast-lane");

    let FirMatch::Module { functions, .. } = match_fir(&out.store, out.module) else {
        panic!("module root expected");
    };
    let loop_body = find_compute_loop_body(&out.store, functions);
    let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
        panic!("compute loop body block expected");
    };
    let stored_value = stmts
        .iter()
        .find_map(|id| match match_fir(&out.store, *id) {
            FirMatch::StoreTable { name, value, .. } if name == "output0" => Some(value),
            _ => None,
        })
        .expect("compute should include one output store");
    let inner = unwrap_output_cast(&out.store, stored_value);
    assert!(matches!(
        match_fir(&out.store, inner),
        FirMatch::BinOp {
            op: FirBinOp::Eq,
            typ: FirType::Int32,
            ..
        }
    ));
}

#[test]
fn enable_with_checkbox_lowers_select2_condition_to_int32() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let input = b.input(0);
        let gate = b.checkbox(0);
        b.enable(input, gate)
    };
    let ui = one_control_ui(ControlKind::Checkbox, "gate", None, false, false);
    let out = compile_signals_to_fir_fastlane_with_ui(
        &arena,
        &[sig0],
        1,
        1,
        &ui,
        &SignalFirOptions::default(),
    )
    .expect("checkbox-driven enable should lower through fast-lane");

    let FirMatch::Module { functions, .. } = match_fir(&out.store, out.module) else {
        panic!("module root expected");
    };
    let loop_body = find_compute_loop_body(&out.store, functions);
    let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
        panic!("compute loop body block expected");
    };
    let stored_value = stmts
        .iter()
        .find_map(|id| match match_fir(&out.store, *id) {
            FirMatch::StoreTable { name, value, .. } if name == "output0" => Some(value),
            _ => None,
        })
        .expect("compute should include one output store");
    let inner = unwrap_output_cast(&out.store, stored_value);
    let FirMatch::Select2 { cond, .. } = match_fir(&out.store, inner) else {
        panic!("enable should lower to FIR Select2");
    };
    assert_eq!(out.store.value_type(cond), Some(FirType::Int32));
}

#[test]
fn invalid_options_return_typed_error_code() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        b.input(0)
    };
    let err = compile_fastlane_without_ui(
        &arena,
        &[sig0],
        1,
        1,
        &SignalFirOptions {
            module_name: "".to_owned(),
            strict_mode: true,
            real_type: RealType::Float32,
        },
    )
    .expect_err("empty module name should fail option validation");

    assert_eq!(err.code(), SignalFirErrorCode::InvalidOptions);
    assert_eq!(err.code().as_str(), "FRS-SFIR-0001");
}

#[test]
fn section_routing_places_ui_and_state_resets_in_distinct_functions() {
    let mut arena = TreeArena::new();
    let ui = one_control_ui(
        ControlKind::HSlider,
        "gain",
        Some(ControlRange {
            init: 0.2,
            min: 0.0,
            max: 1.0,
            step: 0.01,
        }),
        false,
        false,
    );
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let slider = b.hslider(0);
        let delayed = b.delay1(slider);
        let in0 = b.input(0);
        b.binop(BinOp::Add, delayed, in0)
    };
    let out = compile_signals_to_fir_fastlane_with_ui(
        &arena,
        &[sig0],
        1,
        1,
        &ui,
        &SignalFirOptions::default(),
    )
    .expect("sectioned module should compile");

    let FirMatch::Module { functions, .. } = match_fir(&out.store, out.module) else {
        panic!("module root expected");
    };
    let reset_body = find_decl_fun_body(&out.store, functions, "instanceResetUserInterface");
    let clear_body = find_decl_fun_body(&out.store, functions, "instanceClear");

    let FirMatch::Block(reset_stmts) = match_fir(&out.store, reset_body) else {
        panic!("reset body block expected");
    };
    let FirMatch::Block(clear_stmts) = match_fir(&out.store, clear_body) else {
        panic!("clear body block expected");
    };

    assert!(
        reset_stmts.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::StoreVar { ref name, .. }
                if name.starts_with("fHslider")
                    || name.starts_with("fVslider")
                    || name.starts_with("fEntry")
                    || name.starts_with("fButton")
                    || name.starts_with("fCheckbox")
        )),
        "UI zone init should be emitted in instanceResetUserInterface"
    );
    assert!(
        clear_stmts.iter().any(|id| {
            let m = match_fir(&out.store, *id);
            match m {
                FirMatch::StoreVar { ref name, .. }
                    if name.starts_with("fRec")
                        || name.starts_with("iRec")
                        || name.starts_with("fVec")
                        || name.starts_with("iVec") =>
                {
                    true
                }
                FirMatch::SimpleForLoop { body, .. } => {
                    // Circular-buffer state uses a loop to clear the 2-element array
                    if let FirMatch::Block(inner) = match_fir(&out.store, body) {
                        inner.iter().any(|sid| {
                            matches!(
                                match_fir(&out.store, *sid),
                                FirMatch::StoreTable { ref name, .. }
                                    if name.starts_with("fRec")
                                        || name.starts_with("iRec")
                                        || name.starts_with("fVec")
                                        || name.starts_with("iVec")
                            )
                        })
                    } else {
                        false
                    }
                }
                _ => false,
            }
        }),
        "signal state init should be emitted in instanceClear"
    );
}

#[test]
fn ui_only_slider_still_emits_reset_init() {
    let mut arena = TreeArena::new();
    let ui = one_control_ui(
        ControlKind::HSlider,
        "gain",
        Some(ControlRange {
            init: 0.2,
            min: 0.0,
            max: 1.0,
            step: 0.01,
        }),
        false,
        false,
    );
    let sig0 = SigBuilder::new(&mut arena).real(0.0);

    let out = compile_signals_to_fir_fastlane_with_ui(
        &arena,
        &[sig0],
        0,
        1,
        &ui,
        &SignalFirOptions::default(),
    )
    .expect("UI-only slider should still compile");

    let FirMatch::Module { functions, .. } = match_fir(&out.store, out.module) else {
        panic!("module root expected");
    };
    let reset_body = find_decl_fun_body(&out.store, functions, "instanceResetUserInterface");
    let build_ui_body = find_decl_fun_body(&out.store, functions, "buildUserInterface");

    let FirMatch::Block(reset_stmts) = match_fir(&out.store, reset_body) else {
        panic!("reset body block expected");
    };
    let FirMatch::Block(ui_stmts) = match_fir(&out.store, build_ui_body) else {
        panic!("buildUserInterface body block expected");
    };

    assert!(
        reset_stmts.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::StoreVar { ref name, .. } if name == "fHslider0"
        )),
        "UI-only controls must still be initialized in instanceResetUserInterface"
    );
    assert!(
        ui_stmts.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::AddSlider { ref var, .. } if var == "fHslider0"
        )),
        "buildUserInterface should still expose the UI-only slider"
    );
}

#[test]
fn section_routing_places_table_initialization_in_instance_constants() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let size = b.int(4);
        let init = b.real(0.5);
        let ridx = b.input(0);
        b.read_only_table(size, init, ridx)
    };
    let out = compile_fastlane_without_ui(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
        .expect("table section routing should compile");

    let FirMatch::Module {
        functions,
        static_decls,
        ..
    } = match_fir(&out.store, out.module)
    else {
        panic!("module root expected");
    };
    let clear_body = find_decl_fun_body(&out.store, functions, "instanceClear");

    let FirMatch::Block(clear_stmts) = match_fir(&out.store, clear_body) else {
        panic!("clear body block expected");
    };

    // With static table declarations, table data is embedded inline at file scope
    // rather than initialized via StoreTable in instanceConstants.
    let FirMatch::Block(static_items) = match_fir(&out.store, static_decls) else {
        panic!("static_decls block expected");
    };
    assert!(
        static_items
            .iter()
            .any(|id| matches!(match_fir(&out.store, *id), FirMatch::DeclareTable { .. })),
        "table declaration should be emitted in static_decls (compile-time constant)"
    );
    assert!(
        !clear_stmts
            .iter()
            .any(|id| matches!(match_fir(&out.store, *id), FirMatch::StoreTable { .. })),
        "instanceClear should not contain table initialization stores"
    );
}

#[test]
fn bargraph_emits_runtime_zone_store_in_compute() {
    let mut arena = TreeArena::new();
    let ui = one_control_ui(
        ControlKind::HBargraph,
        "level",
        Some(ControlRange {
            init: 0.0,
            min: -60.0,
            max: 6.0,
            step: 0.0,
        }),
        true,
        false,
    );
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let in0 = b.input(0);
        b.hbargraph(0, in0)
    };
    let out = compile_signals_to_fir_fastlane_with_ui(
        &arena,
        &[sig0],
        1,
        1,
        &ui,
        &SignalFirOptions::default(),
    )
    .expect("bargraph signal should compile");

    let FirMatch::Module { functions, .. } = match_fir(&out.store, out.module) else {
        panic!("module root expected");
    };
    let compute_loop_body = find_compute_loop_body(&out.store, functions);
    let ui_body = find_decl_fun_body(&out.store, functions, "buildUserInterface");

    let FirMatch::Block(compute_stmts) = match_fir(&out.store, compute_loop_body) else {
        panic!("compute loop body block expected");
    };
    assert!(
        compute_stmts.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::StoreVar { ref name, .. } if name.starts_with("fHbargraph")
        )),
        "bargraph should emit runtime zone store in compute body"
    );

    let FirMatch::Block(ui_stmts) = match_fir(&out.store, ui_body) else {
        panic!("buildUserInterface body block expected");
    };
    assert!(
        ui_stmts
            .iter()
            .any(|id| matches!(match_fir(&out.store, *id), FirMatch::AddBargraph { .. })),
        "bargraph should be declared in buildUserInterface"
    );
}

#[test]
fn unsupported_signal_family_returns_typed_error_code() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let i0 = b.input(0);
        b.upsampling(&[i0])
    };
    let err = compile_fastlane_without_ui(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
        .expect_err("upsampling is outside current lowering slice");

    assert_eq!(err.code(), SignalFirErrorCode::UnsupportedSignalNode);
    assert_eq!(err.code().as_str(), "FRS-SFIR-0004");
}

#[test]
fn input_index_out_of_range_returns_typed_error_code() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        b.input(1)
    };
    let err = compile_fastlane_without_ui(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
        .expect_err("input(1) is invalid when num_inputs=1");

    assert_eq!(err.code(), SignalFirErrorCode::InputIndexOutOfRange);
    assert_eq!(err.code().as_str(), "FRS-SFIR-0006");
}

#[test]
fn pow_min_max_and_unary_math_lower_to_fir_fun_calls() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let i0 = b.input(0);
        let s0 = b.sin(i0);
        let c0 = b.real(0.25);
        let c1 = b.real(0.5);
        let mx = b.max(c0, c1);
        b.pow(s0, mx)
    };
    let out = compile_fastlane_without_ui(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
        .expect("pow/min/max/unary should be supported in Step 2B.1");

    let FirMatch::Module {
        globals, functions, ..
    } = match_fir(&out.store, out.module)
    else {
        panic!("module root expected");
    };
    let FirMatch::Block(globals_items) = match_fir(&out.store, globals) else {
        panic!("module globals block expected");
    };
    for expected in ["sin", "pow"] {
        assert!(
            globals_items.iter().any(|id| {
                matches!(
                    match_fir(&out.store, *id),
                    FirMatch::DeclareFun { ref name, body: None, .. } if name == expected
                )
            }),
            "globals should declare extern math prototype '{expected}'"
        );
    }
    let loop_body = find_compute_loop_body(&out.store, functions);
    let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
        panic!("compute loop body block expected");
    };
    let store_value = stmts
        .iter()
        .find_map(|id| match match_fir(&out.store, *id) {
            FirMatch::StoreTable { name, value, .. } if name == "output0" => Some(value),
            _ => None,
        })
        .expect("compute should include one output store");
    // Unwrap the FaustFloat cast wrapping the output to reach the computation.
    let store_value = unwrap_output_cast(&out.store, store_value);
    let FirMatch::FunCall { name, args, .. } = match_fir(&out.store, store_value) else {
        panic!("top-level pow should lower to FIR fun call");
    };
    assert_eq!(name, "pow");
    assert_eq!(args.len(), 2);

    let FirMatch::FunCall { name: lhs_name, .. } = match_fir(&out.store, args[0]) else {
        panic!("lhs should lower to unary fun call");
    };
    assert_eq!(lhs_name, "sin");
    match match_fir(&out.store, args[1]) {
        FirMatch::FunCall { name: rhs_name, .. } => assert_eq!(rhs_name, "fmax"),
        FirMatch::Float32 { value, .. } => assert_eq!(value, 0.5),
        FirMatch::Float64 { value, .. } => assert_eq!(value, 0.5),
        _ => panic!("rhs should lower to fmax or the simplified constant 0.5"),
    }
}

#[test]
fn foreign_function_calls_lower_to_fir_fun_calls_and_prototypes() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let ty_int = arena.int(0);
        let ty_real = arena.int(1);
        let incfile = arena.symbol("<math.h>");
        let libfile = arena.symbol("\"\"");
        let name_f32 = arena.symbol("isnanf");
        let name_f64 = arena.symbol("isnan");
        let name_f80 = arena.symbol("isnanl");
        let name_fx = arena.symbol("isnanfx");
        let nil = arena.nil();
        let names = {
            let tail0 = arena.cons(name_fx, nil);
            let tail1 = arena.cons(name_f80, tail0);
            let tail2 = arena.cons(name_f64, tail1);
            arena.cons(name_f32, tail2)
        };
        let arg_types = arena.cons(ty_real, nil);
        let payload = arena.cons(names, arg_types);
        let signature = arena.cons(ty_int, payload);
        let ff_tag = arena.intern_tag("FFUN");
        let ff = arena.intern(tlib::NodeKind::Tag(ff_tag), &[signature, incfile, libfile]);
        let input0 = {
            let mut b = SigBuilder::new(&mut arena);
            b.input(0)
        };
        let args = arena.cons(input0, arena.nil());
        let mut b = SigBuilder::new(&mut arena);
        b.ffun(ff, args)
    };

    let out = compile_fastlane_without_ui(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
        .expect("SIGFFUN should lower to FIR fun calls");

    let FirMatch::Module {
        globals, functions, ..
    } = match_fir(&out.store, out.module)
    else {
        panic!("module root expected");
    };
    let FirMatch::Block(globals_items) = match_fir(&out.store, globals) else {
        panic!("module globals block expected");
    };
    assert!(
        globals_items.iter().any(|id| {
            matches!(
                match_fir(&out.store, *id),
                FirMatch::DeclareFun { ref name, body: None, .. } if name == "isnanf"
            )
        }),
        "globals should declare extern foreign prototype 'isnanf'"
    );

    let loop_body = find_compute_loop_body(&out.store, functions);
    let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
        panic!("compute loop body block expected");
    };
    let store_value = stmts
        .iter()
        .find_map(|id| match match_fir(&out.store, *id) {
            FirMatch::StoreTable { name, value, .. } if name == "output0" => Some(value),
            _ => None,
        })
        .expect("compute should include one output store");
    let store_value = unwrap_output_cast(&out.store, store_value);
    let FirMatch::FunCall { name, args, typ } = match_fir(&out.store, store_value) else {
        panic!("SIGFFUN should lower to a FIR fun call");
    };
    assert_eq!(name, "isnanf");
    assert_eq!(args.len(), 1);
    assert_eq!(typ, FirType::Int32);
}

#[test]
fn delay_prefix_select_and_cast_nodes_are_supported() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let in0 = b.input(0);
        let z0 = b.real(0.0);
        let pre = b.prefix(z0, in0);
        let d1 = b.delay1(pre);
        let n1 = b.int(1);
        let delayed = b.delay(d1, n1);
        let as_int = b.int_cast(delayed);
        let as_float = b.float_cast(as_int);
        let c1 = b.real(1.0);
        let c0 = b.real(0.0);
        b.select2(c1, as_float, c0)
    };

    compile_fastlane_without_ui(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
        .expect("Step 2B.2 should support delay/prefix/select/casts slice");
}

#[test]
fn waveform_and_rdtbl_lower_to_fir_table_nodes() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let v0 = b.real(1.0);
        let v1 = b.real(-2.0);
        let v2 = b.real(3.5);
        let table = b.waveform(&[v0, v1, v2]);
        let idx = b.input(0);
        b.rdtbl(table, idx)
    };
    let out = compile_fastlane_without_ui(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
        .expect("Step 2G should support waveform+rdtbl table lowering");

    let FirMatch::Module {
        static_decls,
        functions,
        ..
    } = match_fir(&out.store, out.module)
    else {
        panic!("module expected");
    };
    let FirMatch::Block(static_items) = match_fir(&out.store, static_decls) else {
        panic!("static_decls block expected");
    };
    assert!(
        static_items
            .iter()
            .any(|id| matches!(match_fir(&out.store, *id), FirMatch::DeclareTable { .. })),
        "Step 2G should allocate waveform table in static_decls (file-scope constant)"
    );
    let loop_body = find_compute_loop_body(&out.store, functions);
    let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
        panic!("compute loop body block expected");
    };
    let stored_value = stmts
        .iter()
        .find_map(|id| match match_fir(&out.store, *id) {
            FirMatch::StoreTable { name, value, .. } if name == "output0" => Some(value),
            _ => None,
        })
        .expect("compute should include one output store");
    // Unwrap the FaustFloat cast wrapping the output to reach the LoadTable.
    let inner = unwrap_output_cast(&out.store, stored_value);
    assert!(
        matches!(match_fir(&out.store, inner), FirMatch::LoadTable { .. }),
        "rdtbl output should lower to FIR table read"
    );
}

#[test]
fn wrtbl_readonly_generator_constant_lowers_to_declared_table() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let size = b.int(8);
        let init = b.real(0.25);
        let ridx = b.input(0);
        b.read_only_table(size, init, ridx)
    };
    let out = compile_fastlane_without_ui(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
        .expect("Step 2H should support readonly wrtbl with constant generator");

    let FirMatch::Module { static_decls, .. } = match_fir(&out.store, out.module) else {
        panic!("module expected");
    };
    let FirMatch::Block(static_items) = match_fir(&out.store, static_decls) else {
        panic!("static_decls block expected");
    };
    let table = static_items
        .iter()
        .copied()
        .find(|id| matches!(match_fir(&out.store, *id), FirMatch::DeclareTable { .. }))
        .expect("readonly wrtbl should declare one table");
    let FirMatch::DeclareTable { values, .. } = match_fir(&out.store, table) else {
        panic!("declare table expected");
    };
    assert_eq!(values.len(), 8, "table must use requested constant size");
}

#[test]
fn wrtbl_runtime_write_emits_store_table_update() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let size = b.int(4);
        let init = b.real(0.0);
        let widx = b.input(0);
        let wsig = b.input(1);
        let ridx = b.input(0);
        b.write_read_table(size, init, widx, wsig, ridx)
    };
    let out = compile_fastlane_without_ui(&arena, &[sig0], 2, 1, &SignalFirOptions::default())
        .expect("Step 2H should support wrtbl runtime write/read shape");

    let FirMatch::Module {
        dsp_struct,
        functions,
        static_decls,
        ..
    } = match_fir(&out.store, out.module)
    else {
        panic!("module expected");
    };
    let FirMatch::Block(struct_items) = match_fir(&out.store, dsp_struct) else {
        panic!("dsp_struct block expected");
    };
    let struct_table = struct_items
        .iter()
        .copied()
        .find(|id| {
            matches!(
                match_fir(&out.store, *id),
                FirMatch::DeclareTable {
                    access: AccessType::Struct,
                    ..
                }
            )
        })
        .expect("runtime wrtbl should allocate one mutable struct table");
    let FirMatch::DeclareTable {
        values, elem_type, ..
    } = match_fir(&out.store, struct_table)
    else {
        panic!("struct table declaration expected");
    };
    assert_eq!(
        values.len(),
        4,
        "mutable wrtbl table should keep requested size"
    );
    assert_eq!(
        elem_type,
        FirType::Float32,
        "runtime wrtbl table should keep real element type"
    );

    let FirMatch::Block(static_items) = match_fir(&out.store, static_decls) else {
        panic!("static_decls block expected");
    };
    assert!(
        !static_items
            .iter()
            .any(|id| matches!(match_fir(&out.store, *id), FirMatch::DeclareTable { .. })),
        "runtime wrtbl should not be lowered as a static table"
    );

    let loop_body = find_compute_loop_body(&out.store, functions);
    let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
        panic!("compute loop body block expected");
    };
    assert!(
        stmts.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::StoreTable {
                access: AccessType::Struct,
                ..
            }
        )),
        "runtime wrtbl should emit FIR store_table update in compute body"
    );
}

#[test]
fn wrtbl_generator_expansion_preserves_integer_table_types() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let size = {
            let mut b = SigBuilder::new(&mut arena);
            b.int(5)
        };
        let init = {
            let mut b = SigBuilder::new(&mut arena);
            let real = b.real(1.75);
            b.int_cast(real)
        };
        let ridx = {
            let mut b = SigBuilder::new(&mut arena);
            b.input(0)
        };
        let mut b = SigBuilder::new(&mut arena);
        b.read_only_table(size, init, ridx)
    };
    let out = compile_fastlane_without_ui(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
        .expect("readonly wrtbl with computed int generator should lower");

    let FirMatch::Module { static_decls, .. } = match_fir(&out.store, out.module) else {
        panic!("module expected");
    };
    let FirMatch::Block(static_items) = match_fir(&out.store, static_decls) else {
        panic!("static_decls block expected");
    };
    let table = static_items
        .iter()
        .copied()
        .find(|id| matches!(match_fir(&out.store, *id), FirMatch::DeclareTable { .. }))
        .expect("readonly wrtbl should declare one static table");
    let FirMatch::DeclareTable {
        elem_type, values, ..
    } = match_fir(&out.store, table)
    else {
        panic!("declare table expected");
    };
    assert_eq!(
        elem_type,
        FirType::Int32,
        "computed integer generator should keep Int32 table type"
    );
    assert!(
        values
            .iter()
            .all(|id| matches!(match_fir(&out.store, *id), FirMatch::Int32 { .. })),
        "computed integer generator should lower to Int32 FIR literals"
    );
}

#[test]
fn siggen_interpreter_delay1_uses_previous_step_for_non_recursive_sources() {
    let mut arena = TreeArena::new();
    let generator = {
        let mut b = SigBuilder::new(&mut arena);
        let v0 = b.real(1.0);
        let v1 = b.real(2.0);
        let v2 = b.real(3.0);
        let v3 = b.real(4.0);
        let waveform = b.waveform(&[v0, v1, v2, v3]);
        b.delay1(waveform)
    };

    let values =
        interpret_generator_for_test(&arena, generator, 4).expect("generator should interpret");
    assert_eq!(values, vec![0.0, 1.0, 2.0, 3.0]);
}

#[test]
fn rec_proj_lowers_without_placeholder_nodes() {
    let mut arena = TreeArena::new();
    let self_ref = de_bruijn_ref(&mut arena, 1);
    let body = {
        let mut b = SigBuilder::new(&mut arena);
        let in0 = b.input(0);
        let c0 = b.real(0.1);
        let feedback = b.proj(0, self_ref);
        // body = input(0) + 0.1 + feedback  (uses recursion so proj is emitted)
        let sum = b.binop(BinOp::Add, in0, c0);
        b.add(sum, feedback)
    };
    let body_list = arena.cons(body, arena.nil());
    let group = de_bruijn_rec(&mut arena, body_list);
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        b.proj(0, group)
    };

    let prepared = prepare_signals_for_fir(&arena, &[sig0], &UiProgram::empty())
        .expect("rec/proj signal should prepare");
    let out = compile_fastlane_without_ui(
        &prepared.arena,
        &prepared.outputs,
        1,
        1,
        &SignalFirOptions::default(),
    )
    .expect("Step 2C.2 should support rec/proj real lowering");

    let FirMatch::Module {
        dsp_struct,
        functions,
        ..
    } = match_fir(&out.store, out.module)
    else {
        panic!("module expected");
    };
    let FirMatch::Block(struct_items) = match_fir(&out.store, dsp_struct) else {
        panic!("dsp_struct block expected");
    };
    assert!(
        struct_items.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::DeclareVar {
                typ: FirType::Array(_, 2),
                ..
            }
        )),
        "rec/proj should allocate a 2-slot recursion array"
    );
    let loop_body = find_compute_loop_body(&out.store, functions);
    let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
        panic!("compute loop body block expected");
    };
    assert!(
        stmts
            .iter()
            .filter(|id| matches!(match_fir(&out.store, **id), FirMatch::StoreTable { .. }))
            .count()
            >= 2,
        "rec/proj should write the current recursion slot and shift it to the previous slot"
    );
    assert!(
        stmts
            .iter()
            .all(|id| !matches!(match_fir(&out.store, *id), FirMatch::Cast { .. })),
        "rec/proj lowering should not need cast wrappers around recursive array accesses"
    );
}

#[test]
fn recursive_feedback_delay1_reuses_two_slot_recursion_array() {
    let mut arena = TreeArena::new();
    let self_ref = de_bruijn_ref(&mut arena, 1);
    let body = {
        let mut b = SigBuilder::new(&mut arena);
        let feedback = b.proj(0, self_ref);
        let prev = b.delay1(feedback);
        let inc = b.real(0.25);
        b.add(prev, inc)
    };
    let body_list = arena.cons(body, arena.nil());
    let group = de_bruijn_rec(&mut arena, body_list);
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        b.proj(0, group)
    };

    let prepared = prepare_signals_for_fir(&arena, &[sig0], &UiProgram::empty())
        .expect("feedback group should prepare");
    let out = compile_fastlane_without_ui(
        &prepared.arena,
        &prepared.outputs,
        0,
        1,
        &SignalFirOptions::default(),
    )
    .expect("feedback delay1 should lower through recursion array slots");

    let FirMatch::Module {
        dsp_struct,
        functions,
        ..
    } = match_fir(&out.store, out.module)
    else {
        panic!("module expected");
    };
    let FirMatch::Block(struct_items) = match_fir(&out.store, dsp_struct) else {
        panic!("dsp_struct block expected");
    };
    let mut array_rec_fields = 0usize;
    for item in struct_items {
        if let FirMatch::DeclareVar { name, typ, .. } = match_fir(&out.store, item)
            && name.starts_with("fRec")
            && matches!(typ, FirType::Array(_, 2))
        {
            array_rec_fields += 1;
        }
    }
    assert_eq!(
        array_rec_fields, 1,
        "feedback recurrence should use one 2-slot recursion array without shadow scalar state"
    );

    let loop_body = find_compute_loop_body(&out.store, functions);
    let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
        panic!("compute loop body block expected");
    };
    let mut rec_store_count = 0usize;
    for stmt in &stmts {
        if let FirMatch::StoreTable { name, .. } = match_fir(&out.store, *stmt)
            && name.starts_with("fRec")
        {
            rec_store_count += 1;
        }
    }
    assert_eq!(
        rec_store_count, 1,
        "circular-buffer recurrence should write one store per sample (at fIOTA & 1)"
    );
    assert!(
        stmts
            .iter()
            .all(|id| !matches!(match_fir(&out.store, *id), FirMatch::Cast { .. })),
        "feedback delay1 recursion reuse should not insert cast wrappers"
    );
}

#[test]
fn foreign_var_count_lowers_to_compute_funarg() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let ty = arena.int(0);
        let name = arena.symbol("count");
        let file = arena.symbol("<math.h>");
        SigBuilder::new(&mut arena).fvar(ty, name, file)
    };

    let out = compile_fastlane_without_ui(&arena, &[sig0], 0, 1, &SignalFirOptions::default())
        .expect("foreign `count` variable should lower via compute fun args");

    let FirMatch::Module { functions, .. } = match_fir(&out.store, out.module) else {
        panic!("module expected");
    };
    let loop_body = find_compute_loop_body(&out.store, functions);
    let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
        panic!("compute loop body block expected");
    };
    assert!(
        stmts.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::StoreTable { value, .. }
                if matches!(
                    unwrap_output_cast(&out.store, value),
                    inner if matches!(
                        match_fir(&out.store, inner),
                        FirMatch::LoadVar {
                            ref name,
                            access: AccessType::FunArgs,
                            typ: FirType::Int32,
                        } if name == "count"
                    )
                )
        )),
        "output should store a load of compute(count, ...) fun arg"
    );
}

#[test]
fn delay1_lowers_to_struct_state_declaration_and_update() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let i0 = b.input(0);
        b.delay1(i0)
    };
    let out = compile_fastlane_without_ui(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
        .expect("delay1 should lower with explicit state");

    let FirMatch::Module {
        dsp_struct,
        functions,
        ..
    } = match_fir(&out.store, out.module)
    else {
        panic!("module expected");
    };
    let FirMatch::Block(struct_items) = match_fir(&out.store, dsp_struct) else {
        panic!("dsp_struct block expected");
    };
    assert!(
        struct_items
            .iter()
            .any(|id| matches!(match_fir(&out.store, *id), FirMatch::DeclareVar { .. })),
        "delay state should create struct declaration"
    );

    let loop_body = find_compute_loop_body(&out.store, functions);
    let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
        panic!("compute loop body block expected");
    };
    assert!(
        stmts
            .iter()
            .any(|id| matches!(match_fir(&out.store, *id), FirMatch::StoreVar { .. })),
        "delay state should create compute update store"
    );
}

#[test]
fn int_delay1_uses_int32_state_slot() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let one = b.int(1);
        b.delay1(one)
    };
    let out = compile_fastlane_without_ui(&arena, &[sig0], 0, 1, &SignalFirOptions::default())
        .expect("integer delay1 should lower");

    let FirMatch::Module { dsp_struct, .. } = match_fir(&out.store, out.module) else {
        panic!("module expected");
    };
    let FirMatch::Block(struct_items) = match_fir(&out.store, dsp_struct) else {
        panic!("dsp_struct block expected");
    };
    assert!(
        struct_items.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::DeclareVar {
                typ: FirType::Int32,
                ..
            }
        )),
        "integer delay state should allocate an Int32 slot"
    );
}

#[test]
fn integer_recursive_min_lowers_to_int_recursion_and_min_i_call() {
    let mut arena = TreeArena::new();
    let self_ref = de_bruijn_ref(&mut arena, 1);
    let body = {
        let mut b = SigBuilder::new(&mut arena);
        let feedback = b.proj(0, self_ref);
        let prev = b.delay1(feedback);
        let one = b.int(1);
        let sum = b.add(prev, one);
        let three = b.int(3);
        b.min(sum, three)
    };
    let body_list = arena.cons(body, arena.nil());
    let group = de_bruijn_rec(&mut arena, body_list);
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        b.proj(0, group)
    };

    let prepared = prepare_signals_for_fir(&arena, &[sig0], &UiProgram::empty())
        .expect("integer recursion should prepare");
    assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Int));

    let out = compile_fastlane_without_ui(
        &prepared.arena,
        &prepared.outputs,
        0,
        1,
        &SignalFirOptions::default(),
    )
    .expect("integer min recursion should lower");

    let FirMatch::Module { dsp_struct, .. } = match_fir(&out.store, out.module) else {
        panic!("module expected");
    };
    let FirMatch::Block(struct_items) = match_fir(&out.store, dsp_struct) else {
        panic!("dsp_struct block expected");
    };
    assert!(
        struct_items.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::DeclareVar {
                typ: FirType::Array(inner, 2),
                ..
            } if *inner == FirType::Int32
        )),
        "integer recursive min should allocate a 2-slot Int32 recursion array"
    );

    let dump = fir::dump_fir(&out.store, out.module);
    assert!(
        !dump.contains("name=fmin") && !dump.contains("name=fminf"),
        "integer min recursion should not call floating-point fmin helpers"
    );
    assert!(
        dump.contains("min_i"),
        "integer min recursion should stay an explicit integer min_i function call"
    );
}

#[test]
fn integer_recursive_abs_lowers_to_int_recursion_and_abs_call() {
    let mut arena = TreeArena::new();
    let self_ref = de_bruijn_ref(&mut arena, 1);
    let body = {
        let mut b = SigBuilder::new(&mut arena);
        let feedback = b.proj(0, self_ref);
        let prev = b.delay1(feedback);
        let one = b.int(1);
        let sum = b.add(prev, one);
        b.abs(sum)
    };
    let body_list = arena.cons(body, arena.nil());
    let group = de_bruijn_rec(&mut arena, body_list);
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        b.proj(0, group)
    };

    let prepared = prepare_signals_for_fir(&arena, &[sig0], &UiProgram::empty())
        .expect("integer abs recursion should prepare");
    assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Int));

    let out = compile_fastlane_without_ui(
        &prepared.arena,
        &prepared.outputs,
        0,
        1,
        &SignalFirOptions::default(),
    )
    .expect("integer abs recursion should lower");

    let dump = fir::dump_fir(&out.store, out.module);
    assert!(
        !dump.contains("name=fabs") && !dump.contains("name=fabsf"),
        "integer abs recursion should not call floating-point fabs helpers"
    );
    assert!(
        dump.contains("abs"),
        "integer abs recursion should stay an explicit integer abs function call"
    );
}

#[test]
fn fixed_delay_lowers_to_struct_array_and_iota_updates() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let in0 = b.input(0);
        let n3 = b.int(3);
        b.delay(in0, n3)
    };
    let out = compile_fastlane_without_ui(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
        .expect("constant fixed delay should lower");

    let FirMatch::Module {
        dsp_struct,
        functions,
        ..
    } = match_fir(&out.store, out.module)
    else {
        panic!("module expected");
    };
    let FirMatch::Block(struct_items) = match_fir(&out.store, dsp_struct) else {
        panic!("dsp_struct block expected");
    };

    let delay_decl = struct_items
        .iter()
        .find(|id| {
            matches!(
                match_fir(&out.store, **id),
                FirMatch::DeclareVar {
                    ref name,
                    typ: FirType::Array(_, 4),
                    ..
                } if name.starts_with("fVec") || name.starts_with("iVec")
            )
        })
        .copied()
        .expect("constant delay should allocate a size-4 delay line");
    let FirMatch::DeclareVar {
        name: delay_name,
        typ,
        ..
    } = match_fir(&out.store, delay_decl)
    else {
        panic!("delay declaration expected");
    };
    match typ {
        FirType::Array(inner, 4) => assert_eq!(*inner, FirType::Float32),
        other => panic!("unexpected delay declaration type: {other:?}"),
    }
    assert!(
        struct_items.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::DeclareVar {
                ref name,
                typ: FirType::Int32,
                ..
            } if name == "fIOTA"
        )),
        "fixed delay should declare persistent fIOTA state"
    );

    let clear_body = find_decl_fun_body(&out.store, functions, "instanceClear");
    let FirMatch::Block(clear_stmts) = match_fir(&out.store, clear_body) else {
        panic!("instanceClear block expected");
    };
    assert!(
        clear_stmts.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::StoreVar { ref name, .. } if name == "fIOTA"
        )),
        "instanceClear should reset fIOTA"
    );
    assert!(
        clear_stmts
            .iter()
            .any(|id| matches!(match_fir(&out.store, *id), FirMatch::SimpleForLoop { .. })),
        "instanceClear should zero the delay-line array"
    );

    let loop_body = find_compute_loop_body(&out.store, functions);
    let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
        panic!("compute loop body block expected");
    };
    assert!(
        stmts.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::StoreTable { ref name, .. } if name == &delay_name
        )),
        "compute loop should write the current sample into the delay line"
    );
    let write_index = stmts
        .iter()
        .find_map(|id| match match_fir(&out.store, *id) {
            FirMatch::StoreTable { name, index, .. } if name == delay_name => Some(index),
            _ => None,
        })
        .expect("compute loop should include one delay-line write");
    let FirMatch::BinOp {
        op: FirBinOp::And,
        lhs,
        rhs,
        ..
    } = match_fir(&out.store, write_index)
    else {
        panic!("delay write index should be masked");
    };
    assert!(matches!(
        match_fir(&out.store, lhs),
        FirMatch::LoadVar { ref name, .. } if name == "fIOTA"
    ));
    assert!(matches!(
        match_fir(&out.store, rhs),
        FirMatch::Int32 { value: 3, .. }
    ));
    assert!(
        stmts.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::StoreVar { ref name, .. } if name == "fIOTA"
        )),
        "compute loop should increment fIOTA once per sample"
    );

    let stored_value = stmts
        .iter()
        .find_map(|id| match match_fir(&out.store, *id) {
            FirMatch::StoreTable { name, value, .. } if name == "output0" => Some(value),
            _ => None,
        })
        .expect("compute should include one output store");
    let inner = unwrap_output_cast(&out.store, stored_value);
    let FirMatch::LoadTable { name, index, .. } = match_fir(&out.store, inner) else {
        panic!("fixed delay output should lower to a delay-line read");
    };
    assert_eq!(name, delay_name);
    let FirMatch::BinOp {
        op: FirBinOp::And,
        lhs,
        rhs,
        ..
    } = match_fir(&out.store, index)
    else {
        panic!("delay index should be masked");
    };
    assert!(matches!(
        match_fir(&out.store, rhs),
        FirMatch::Int32 { value: 3, .. }
    ));
    assert!(matches!(
        match_fir(&out.store, lhs),
        FirMatch::BinOp {
            op: FirBinOp::Sub,
            ..
        }
    ));
}

#[test]
fn zero_delay_uses_fast_path_without_delay_resources() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let in0 = b.input(0);
        let n0 = b.int(0);
        b.delay(in0, n0)
    };
    let out = compile_fastlane_without_ui(&arena, &[sig0], 1, 1, &SignalFirOptions::default())
        .expect("zero delay should lower through fast path");

    let FirMatch::Module {
        dsp_struct,
        functions,
        ..
    } = match_fir(&out.store, out.module)
    else {
        panic!("module expected");
    };
    let FirMatch::Block(struct_items) = match_fir(&out.store, dsp_struct) else {
        panic!("dsp_struct block expected");
    };
    assert!(
        !struct_items.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::DeclareVar { ref name, .. } if name == "fIOTA"
        )),
        "zero delay should not allocate fIOTA"
    );
    assert!(
        !struct_items.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::DeclareVar { ref name, .. }
                if name.starts_with("fVec") || name.starts_with("iVec")
        )),
        "zero delay should not allocate a delay line"
    );

    let loop_body = find_compute_loop_body(&out.store, functions);
    let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
        panic!("compute loop body block expected");
    };
    let stored_value = stmts
        .iter()
        .find_map(|id| match match_fir(&out.store, *id) {
            FirMatch::StoreTable { name, value, .. } if name == "output0" => Some(value),
            _ => None,
        })
        .expect("compute should include one output store");
    let inner = unwrap_output_cast(&out.store, stored_value);
    assert!(
        matches!(
            match_fir(&out.store, inner),
            FirMatch::Cast { .. } | FirMatch::LoadTable { .. }
        ),
        "zero delay should lower to the carried value without delay-line readback"
    );
}

#[test]
fn variable_delay_with_audio_input_amount_uses_tinput_interval() {
    // `process = _ : @(_)` — audio input as delay amount.
    // TINPUT has interval(-1, 1), so hi=1 → delay line next_pow2(2) = 2.
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let in0 = b.input(0);
        let amount = b.input(1);
        b.delay(in0, amount)
    };
    let out = compile_fastlane_without_ui(&arena, &[sig0], 2, 1, &SignalFirOptions::default())
        .expect("audio-input delay amount must be accepted");
    // Verify a 2-sample delay line was allocated.
    let FirMatch::Module { dsp_struct, .. } = match_fir(&out.store, out.module) else {
        panic!("expected Module");
    };
    let FirMatch::Block(struct_items) = match_fir(&out.store, dsp_struct) else {
        panic!("dsp_struct block expected");
    };
    let delay_sizes: Vec<usize> = struct_items
        .iter()
        .filter_map(|id| match match_fir(&out.store, *id) {
            FirMatch::DeclareVar {
                ref name,
                typ: FirType::Array(_, size),
                ..
            } if name.starts_with("fVec") || name.starts_with("iVec") => Some(size),
            _ => None,
        })
        .collect();
    assert_eq!(delay_sizes, [2], "expected a single 2-sample delay line");
}

#[test]
fn variable_delay_with_strictly_negative_hi_is_rejected() {
    // A slider shifted so that its interval is entirely negative (hi < 0)
    // must be rejected — C++ `checkDelayInterval` rejects `hi() < 0`.
    // e.g. hslider("d",10,0,100,1) - 200  →  interval [-200, -100], hi=-100.
    let ui = one_control_ui(
        ControlKind::HSlider,
        "d",
        Some(ControlRange {
            init: 10.0,
            min: 0.0,
            max: 100.0,
            step: 1.0,
        }),
        false,
        false,
    );
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let in0 = b.input(0);
        let slider = b.hslider(0);
        let offset = b.real(200.0);
        let shifted = b.binop(BinOp::Sub, slider, offset);
        b.delay(in0, shifted)
    };
    let err = compile_signals_to_fir_fastlane_with_ui(
        &arena,
        &[sig0],
        1,
        1,
        &ui,
        &SignalFirOptions::default(),
    )
    .expect_err("slider with hi<0 interval must be rejected as delay amount");
    assert_eq!(err.code(), SignalFirErrorCode::UnsupportedSignalNode);
}

#[test]
fn variable_delay_with_slider_bound_lowers_to_interval_sized_delay_line() {
    // process = @(100) : @(hslider("Delay",10,0,100,1));
    // The outer @(100) is constant; the inner @(hslider(...)) is variable
    // with interval [0,100].  Both should lower: the slider delay uses a
    // delay line sized to next_power_of_two(101) = 128.
    let ui = one_control_ui(
        ControlKind::HSlider,
        "Delay",
        Some(ControlRange {
            init: 10.0,
            min: 0.0,
            max: 100.0,
            step: 1.0,
        }),
        false,
        false,
    );
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let in0 = b.input(0);
        let n100 = b.int(100);
        let delayed_fixed = b.delay(in0, n100);
        let raw_slider = b.hslider(0);
        let slider_amount = b.int_cast(raw_slider);
        b.delay(delayed_fixed, slider_amount)
    };
    let out = compile_signals_to_fir_fastlane_with_ui(
        &arena,
        &[sig0],
        1,
        1,
        &ui,
        &SignalFirOptions::default(),
    )
    .expect("variable delay with bounded slider should lower successfully");

    let FirMatch::Module { dsp_struct, .. } = match_fir(&out.store, out.module) else {
        panic!("module expected");
    };
    let FirMatch::Block(struct_items) = match_fir(&out.store, dsp_struct) else {
        panic!("dsp_struct block expected");
    };
    // Expect two delay-line DeclareVar(Array) entries: one for @(100)
    // (size=128) and one for the slider-bounded @(hslider) (size=128 too,
    // since next_power_of_two(101)=128).
    let delay_line_sizes: Vec<usize> = struct_items
        .iter()
        .filter_map(|id| match match_fir(&out.store, *id) {
            FirMatch::DeclareVar {
                ref name,
                typ: FirType::Array(_, size),
                ..
            } if name.starts_with("fVec") || name.starts_with("iVec") => Some(size),
            _ => None,
        })
        .collect();
    assert_eq!(
        delay_line_sizes.len(),
        2,
        "expected two delay-line arrays (one fixed @100, one slider-bounded), got {:?}",
        delay_line_sizes
    );
    let max_size = delay_line_sizes.iter().copied().max().unwrap_or(0);
    assert!(
        max_size >= 128,
        "slider delay line must be >= 128 samples (next_power_of_two(101)), got {max_size}"
    );
}

#[test]
fn variable_delay_with_zero_hi_interval_uses_zero_delay_passthrough() {
    // @(hslider("Delay1",10,0,100,1) - 100) → interval [-100, 0], hi=0.
    // C++ checkDelayInterval rejects only hi < 0, so hi==0 is accepted
    // and produces a zero-delay (passthrough).  Regression for the
    // `<= 0.0` vs `< 0.0` boundary condition in variable_delay_max_bound.
    let ui = one_control_ui(
        ControlKind::HSlider,
        "Delay1",
        Some(ControlRange {
            init: 10.0,
            min: 0.0,
            max: 100.0,
            step: 1.0,
        }),
        false,
        false,
    );
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let in0 = b.input(0);
        let n100_fixed = b.int(100);
        let stage1 = b.delay(in0, n100_fixed);
        // hslider - 100  → interval [0,100] - 100 = [-100, 0], hi == 0
        let slider = b.hslider(0);
        let offset = b.real(100.0);
        let shifted = b.binop(BinOp::Sub, slider, offset);
        b.delay(stage1, shifted)
    };
    compile_signals_to_fir_fastlane_with_ui(
        &arena,
        &[sig0],
        1,
        1,
        &ui,
        &SignalFirOptions::default(),
    )
    .expect("delay with hi=0 interval should lower as zero-delay (passthrough)");
}

#[test]
fn int_waveform_declares_int32_table() {
    let mut arena = TreeArena::new();
    let sig0 = {
        let mut b = SigBuilder::new(&mut arena);
        let v0 = b.int(1);
        let v1 = b.int(2);
        let v2 = b.int(3);
        let table = b.waveform(&[v0, v1, v2]);
        let idx = b.int(0);
        b.rdtbl(table, idx)
    };
    let out = compile_fastlane_without_ui(&arena, &[sig0], 0, 1, &SignalFirOptions::default())
        .expect("integer waveform should lower");

    let FirMatch::Module { static_decls, .. } = match_fir(&out.store, out.module) else {
        panic!("module expected");
    };
    let FirMatch::Block(static_items) = match_fir(&out.store, static_decls) else {
        panic!("static_decls block expected");
    };
    assert!(
        static_items.iter().any(|id| matches!(
            match_fir(&out.store, *id),
            FirMatch::DeclareTable {
                name,
                elem_type: FirType::Int32,
                ..
            } if name.starts_with("iTbl")
        )),
        "integer waveform tables should declare Int32 element type and use the iTbl prefix"
    );
}

#[test]
fn left_shift_binop_lowers_to_int32_fir_shift() {
    let mut arena = TreeArena::default();
    let shifted = {
        let mut b = SigBuilder::new(&mut arena);
        let lhs = b.int(1);
        let rhs = b.int(3);
        b.binop(BinOp::Lsh, lhs, rhs)
    };

    let out = compile_signals_to_fir_fastlane_with_ui(
        &arena,
        &[shifted],
        0,
        1,
        &UiProgram::empty(),
        &SignalFirOptions::default(),
    )
    .expect("lsh should lower through the fast-lane");

    let FirMatch::Module { functions, .. } = match_fir(&out.store, out.module) else {
        panic!("module root expected");
    };
    let loop_body = find_compute_loop_body(&out.store, functions);
    let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
        panic!("compute loop body block expected");
    };
    let store_value = stmts
        .iter()
        .find_map(|id| match match_fir(&out.store, *id) {
            FirMatch::StoreTable { name, value, .. } if name == "output0" => Some(value),
            _ => None,
        })
        .expect("compute should include one output store");
    let store_value = unwrap_output_cast(&out.store, store_value);
    assert!(
        matches!(
            match_fir(&out.store, store_value),
            FirMatch::BinOp {
                op: FirBinOp::Lsh,
                typ: FirType::Int32,
                ..
            }
        ) || matches!(
            match_fir(&out.store, store_value),
            FirMatch::Int32 {
                value: 8,
                typ: FirType::Int32
            }
        ),
        "left shift should lower to an Int32 FIR Lsh binop or simplify to Int32(8)"
    );
}

#[test]
fn right_shift_binops_lower_to_int32_fir_shifts() {
    for (source_op, expected_op) in [(BinOp::ARsh, FirBinOp::ARsh), (BinOp::LRsh, FirBinOp::LRsh)] {
        let mut arena = TreeArena::default();
        let shifted = {
            let mut b = SigBuilder::new(&mut arena);
            let lhs = b.int(16);
            let rhs = b.int(2);
            b.binop(source_op, lhs, rhs)
        };

        let out = compile_signals_to_fir_fastlane_with_ui(
            &arena,
            &[shifted],
            0,
            1,
            &UiProgram::empty(),
            &SignalFirOptions::default(),
        )
        .expect("right shift should lower through the fast-lane");

        let FirMatch::Module { functions, .. } = match_fir(&out.store, out.module) else {
            panic!("module root expected");
        };
        let loop_body = find_compute_loop_body(&out.store, functions);
        let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
            panic!("compute loop body block expected");
        };
        let store_value = stmts
            .iter()
            .find_map(|id| match match_fir(&out.store, *id) {
                FirMatch::StoreTable { name, value, .. } if name == "output0" => Some(value),
                _ => None,
            })
            .expect("compute should include one output store");
        let store_value = unwrap_output_cast(&out.store, store_value);
        let expected_value = match source_op {
            BinOp::ARsh => 4,
            BinOp::LRsh => 4,
            _ => unreachable!(),
        };
        assert!(
            matches!(
                match_fir(&out.store, store_value),
                FirMatch::BinOp {
                    op,
                    typ: FirType::Int32,
                    ..
                } if op == expected_op
            ) || matches!(
                match_fir(&out.store, store_value),
                FirMatch::Int32 {
                    value,
                    typ: FirType::Int32
                } if value == expected_value
            ),
            "right shift should lower to an Int32 FIR {expected_op:?} binop or simplify to Int32({expected_value})"
        );
    }
}
