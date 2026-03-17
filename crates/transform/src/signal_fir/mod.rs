//! Experimental signal->FIR fast-lane (Step 2A/2B/2C/2D/2E/2F/2G/2H slices).
//!
//! # Status
//! This module currently provides an **executable base slice**:
//! - contract validation (`Step 1A`),
//! - lowering for `SIGINPUT`, numeric constants, `SIGBINOP`, and `SIGOUTPUT`
//!   passthrough (`Step 2A`),
//! - core math and control/state bootstrap nodes (`Step 2B`),
//! - explicit state lowering for `delay`-family nodes, including fixed-size
//!   circular FIR delay lines for constant `SIGDELAY` amounts (`Step 2C` slice),
//! - first breadth coverage for extended primitives, waveform/table/UI families
//!   (`Step 2D`),
//! - first shim-reduction pass replacing several `frs_*` calls with native FIR
//!   lowering (`Step 2E`),
//! - critical shim elimination (`Step 2F`): no `frs_*` calls remain in fast-lane
//!   generated C++,
//! - first FIR-native table lowering (`Step 2G`) for
//!   `SIGWAVEFORM` / `SIGRDTBL` / `SIGWRTBL`,
//! - non-trivial table slice (`Step 2H`) for `SIGWRTBL(size, gen(..), ..)` with
//!   constant size and deterministic generator expansion.
//! - pre-lowering staging (`Preparation Step 1`): clone the output forest into a
//!   private arena and run forest-wide `de_bruijn_to_sym` before FIR emission.
//! - prepared typing/promotion (`Preparation Step 2/3/4`): consume the reduced
//!   `signal_prepare` type map so FIR lowering keeps integer delay/recursion/table
//!   carriers instead of defaulting every internal value to `real_ty`.
//!
//! Current `Step 2H` scope still excludes complex generator forms depending on
//! runtime context/loop variables; those are reported as typed
//! `UnsupportedSignalNode` errors.
//!
//! General `SIGDELAY` parity remains intentionally partial: the fast-lane now
//! supports constant integer delay amounts through fixed-size circular buffers,
//! and variable delays where the amount comes from a UI control with a bounded
//! interval (slider/numentry). Delays with unbounded intervals are
//! available.
//!
//! Other signal families still return typed `FRS-SFIR-*` errors until the
//! remaining lowering slices are implemented.
//!
//! # Crate boundary contract
//! - `transform` owns signal->FIR lowering entrypoints.
//! - `fir` owns FIR node model, builder, and matcher.
//! - `codegen` consumes resulting FIR modules.
//! - `compiler` chooses whether to route requests to this fast-lane.

mod error;
mod module;
mod planner;

pub use error::{SignalFirError, SignalFirErrorCode};

use fir::{FirId, FirStore, FirType};
use signals::SigId;
use tlib::TreeArena;
use ui::UiProgram;

use crate::signal_prepare::prepare_signals_for_fir;

/// Internal DSP computation precision.
///
/// Controls the type of internal state variables, arithmetic results, math
/// function signatures, waveform table element types, and real-constant nodes
/// in the generated FIR module.
///
/// **External interface types are not affected**: audio buffer samples
/// (`FAUSTFLOAT**` in `compute`) and UI zone variables (sliders, bargraphs,
/// buttons) always use `FirType::FaustFloat` regardless of this setting.
///
/// Corresponds to Faust's `-double` compilation flag and `gFLoatSize`:
/// - `Float32` → C++ `float` (default),
/// - `Float64` → C++ `double`.
///
/// Internal DSP real type used when lowering signals to FIR.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RealType {
    /// 32-bit single-precision float (`float` in C++). Default.
    #[default]
    Float32,
    /// 64-bit double-precision float (`double` in C++).
    Float64,
}

impl RealType {
    /// Returns the [`FirType`] that represents this precision in FIR lowering.
    #[must_use]
    pub fn as_fir_type(self) -> FirType {
        match self {
            Self::Float32 => FirType::Float32,
            Self::Float64 => FirType::Float64,
        }
    }
}

/// Options for [`compile_signals_to_fir_fastlane_with_ui`].
///
/// These options currently describe only the externally visible module contract.
/// Resource planning and lowering policies stay internal to the fast-lane until
/// more slices are promoted to stable configuration.
/// Configuration options for signal->FIR lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignalFirOptions {
    /// FIR module name to emit.
    pub module_name: String,
    /// Reserved for Step 2+ strictness/profile toggles.
    ///
    /// The current implementation keeps this field for forward-compatible CLI
    /// plumbing even though the active slices still use one conservative policy.
    pub strict_mode: bool,
    /// Internal DSP computation precision (default: [`RealType::Float32`]).
    ///
    /// Controls the FIR type used for internal arithmetic, state variables,
    /// math calls, waveform table elements, and real constants.
    ///
    /// External interface types (`FaustFloat`) are **not** affected: audio
    /// buffers (`FAUSTFLOAT** inputs/outputs` in `compute`) and UI zone
    /// variables (sliders, bargraphs, buttons) always use `FaustFloat`.
    ///
    /// Implicit casts between the internal real type and `FaustFloat` are
    /// emitted automatically at the DSP boundary (input sample load and output
    /// sample store) and at UI zone reads/writes.
    pub real_type: RealType,
}

impl Default for SignalFirOptions {
    fn default() -> Self {
        Self {
            module_name: "mydsp".to_owned(),
            strict_mode: true,
            real_type: RealType::Float32,
        }
    }
}

/// Output package of the fast-lane compiler.
///
/// The fast-lane returns ownership of the FIR store together with the module
/// root so downstream backends can keep using normal `fir` builder/matcher APIs
/// without relying on hidden global state.
#[derive(Debug)]
/// Output bundle produced by the signal->FIR lowering entry point.
pub struct SignalFirOutput {
    /// FIR storage arena.
    pub store: FirStore,
    /// Root node id of the generated FIR module.
    pub module: FirId,
}

/// Compiles propagated signals plus canonical grouped UI into a FIR module.
///
/// This is the grouped-UI-aware fast-lane entry point used by the compiler
/// facade once `propagate` owns explicit `UiProgram` output.
///
/// Callers that intentionally compile UI-free signal forests must pass an
/// explicit placeholder [`UiProgram`] such as [`UiProgram::empty()`] or a
/// root-only synthetic program constructed by the owning integration layer.
///
/// # Current behavior (Step 2A/2B/2C/2D/2E/2F/2G/2H)
/// - validates options and top-level signal/arity contract,
/// - builds a deterministic planning snapshot,
/// - prepares the whole output forest in a private staging arena,
/// - lowers one executable bootstrap signal slice to FIR using the prepared
///   reduced type annotations for state/table/result type selection.
///
/// # Errors
/// Returns [`SignalFirError`] when options are invalid or the top-level
/// signal/arity contract is inconsistent.
///
/// # Ownership contract
/// - `signals` must already be the propagated DSP outputs for the same source
///   program that produced `ui`,
/// - `ui` is the sole source of truth for grouped layout, labels, and UI
///   metadata,
/// - signal leaf widgets are expected to carry only stable `ControlId`
///   references back into `ui`.
pub fn compile_signals_to_fir_fastlane_with_ui(
    _arena: &TreeArena,
    signals: &[SigId],
    num_inputs: usize,
    num_outputs: usize,
    ui: &UiProgram,
    options: &SignalFirOptions,
) -> Result<SignalFirOutput, SignalFirError> {
    let plan = planner::plan_signals(signals, num_inputs, num_outputs, options)?;
    let prepared = prepare_signals_for_fir(_arena, signals, ui).map_err(|err| {
        SignalFirError::new(
            SignalFirErrorCode::UnsupportedSignalNode,
            format!("signal preparation failed: {err}"),
        )
    })?;
    module::build_module(
        &plan,
        options.module_name.as_str(),
        &prepared.arena,
        &prepared.outputs,
        ui,
        &prepared.types,
        &prepared.sig_types,
        options.real_type.as_fir_type(),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        RealType, SignalFirErrorCode, SignalFirOptions, compile_signals_to_fir_fastlane_with_ui,
    };
    use fir::{FirBinOp, FirMatch, FirType, match_fir};
    use signals::{BinOp, SigBuilder};

    use tlib::{TreeArena, de_bruijn_rec, de_bruijn_ref};
    use ui::{ControlKind, ControlRange, ControlSpec, UiBuilder, UiProgram, UiRootOrigin};

    use crate::signal_prepare::{SimpleSigType, prepare_signals_for_fir};

    /// Peels off a `Cast(FaustFloat, inner)` wrapper if present.
    ///
    /// Since the output sample store now always emits an explicit cast from the
    /// internal real type to `FaustFloat`, tests that inspect the *computation*
    /// node (not the cast itself) should call this helper before matching.
    fn unwrap_output_cast(store: &fir::FirStore, id: fir::FirId) -> fir::FirId {
        match match_fir(store, id) {
            FirMatch::Cast {
                typ: FirType::FaustFloat,
                value,
            } => value,
            _ => id,
        }
    }

    fn find_decl_fun_body(
        store: &fir::FirStore,
        functions: fir::FirId,
        target: &str,
    ) -> fir::FirId {
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
                        if name.starts_with("fRec") || name.starts_with("iRec") =>
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
                                        if name.starts_with("fRec") || name.starts_with("iRec")
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
        for expected in ["sin", "fmax", "pow"] {
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
        let FirMatch::FunCall { name: rhs_name, .. } = match_fir(&out.store, args[1]) else {
            panic!("rhs should lower to min/max fun call");
        };
        assert_eq!(rhs_name, "fmax");
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

        let FirMatch::Module { functions, .. } = match_fir(&out.store, out.module) else {
            panic!("module expected");
        };
        let loop_body = find_compute_loop_body(&out.store, functions);
        let FirMatch::Block(stmts) = match_fir(&out.store, loop_body) else {
            panic!("compute loop body block expected");
        };
        assert!(
            stmts
                .iter()
                .any(|id| matches!(match_fir(&out.store, *id), FirMatch::StoreTable { .. })),
            "runtime wrtbl should emit FIR store_table update in compute body"
        );
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
}
