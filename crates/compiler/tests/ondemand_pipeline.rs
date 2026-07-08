//! Integration tests for the roadmap P0 exit criteria (clocked wrappers ×
//! signal_prepare × AD diagnostics).
//!
//! Scope (roadmap P0, `porting/ondemand-vec-fad-interleave-synthesis-2026-07-07-en.md` §6):
//! - P0.1: propagation of the three clocked wrappers produces a clocked
//!   signal graph and `prepare_signals_for_fir` accepts it (the clock-env
//!   child of `Clocked` is an opaque annotation, never a signal); `signal_fir`
//!   rejects the still-unlowered clocked nodes with the structured
//!   `FRS-SFIR-0007` "not lowered yet" error — never a panic or the generic
//!   `FRS-SFIR-0004`.
//! - P0.2: two structurally identical wrapper instances in different contexts
//!   get *distinct* clock domains (the C++ de Bruijn collision class).
//! - P0.4: `fad` across a boundary fails loudly (`FRS-PROP-0004`), never
//!   silently-zero tangents; `rad` names the clocked construct it rejects.
//!   The four rows of the cohabitation §4 table are pinned below.

use std::collections::BTreeSet;

use compiler::Compiler;
use signals::{SigMatch, match_sig};
use tlib::TreeArena;
use transform::signal_fir::{SignalFirOptions, compile_signals_to_fir_fastlane_with_ui};
use transform::signal_prepare::prepare_signals_for_fir;

fn compile_inline(name: &str, source: &str) -> compiler::SignalCompileOutput {
    Compiler::new()
        .compile_source_to_signals(name, source)
        .unwrap_or_else(|e| panic!("failed to compile {name} to signals: {e}"))
}

fn prepare_inline(name: &str, source: &str) -> transform::signal_prepare::PreparedSignals {
    let out = compile_inline(name, source);
    prepare_signals_for_fir(&out.parse.state.arena, &out.signals, &out.ui)
        .unwrap_or_else(|e| panic!("{name} should pass signal_prepare: {e}"))
}

#[test]
fn ondemand_fixture_passes_signal_prepare() {
    let prepared = prepare_inline(
        "od_gate_times_two",
        r#"process = (button("gate"), _) : ondemand(*(2));"#,
    );
    assert_eq!(prepared.outputs().len(), 1);
}

#[test]
fn upsampling_fixture_passes_signal_prepare() {
    let prepared = prepare_inline(
        "us_gate_times_two",
        r#"process = (button("gate"), _) : upsampling(*(2));"#,
    );
    assert_eq!(prepared.outputs().len(), 1);
}

#[test]
fn downsampling_fixture_passes_signal_prepare() {
    let prepared = prepare_inline(
        "ds_gate_times_two",
        r#"process = (button("gate"), _) : downsampling(*(2));"#,
    );
    assert_eq!(prepared.outputs().len(), 1);
}

#[test]
fn ondemand_with_internal_state_passes_signal_prepare() {
    // Recursive state inside the on-demand block: exercises SYMREC bodies
    // under `Clocked` wrappers plus `TempVar` inputs.
    let prepared = prepare_inline(
        "od_counter",
        r#"process = (button("gate"), _) : ondemand(+ ~ _);"#,
    );
    assert_eq!(prepared.outputs().len(), 1);
}

// ── P0.1 — signal_fir clean structured rejection ─────────────────────────────

#[test]
fn ondemand_signal_fir_rejects_with_clocked_not_lowered() {
    // Cohabitation §4 row 1: `ondemand` alone must reach `signal_fir` and be
    // rejected there with the dedicated `FRS-SFIR-0007`, not the generic
    // `FRS-SFIR-0004`, and never a panic.
    let out = compile_inline(
        "od_gate_times_two_fir",
        r#"process = (button("gate"), _) : ondemand(*(2));"#,
    );
    let err = compile_signals_to_fir_fastlane_with_ui(
        &out.parse.state.arena,
        &out.signals,
        out.process_arity.inputs,
        out.process_arity.outputs,
        &out.ui,
        &SignalFirOptions::default(),
    )
    .expect_err("clocked graphs must be rejected until the P3 lowering lands");
    assert_eq!(err.code().as_str(), "FRS-SFIR-0007", "got: {err}");
}

// ── P0.2 — clock-domain instance uniqueness ──────────────────────────────────

/// Collects every `SIGCLOCKENV` token id reachable from `sig` (env child of
/// `Clocked` wrappers), without treating the token as a signal.
fn collect_clock_env_ids(
    arena: &TreeArena,
    sig: signals::SigId,
    visited: &mut BTreeSet<u32>,
    out: &mut BTreeSet<u32>,
) {
    if !visited.insert(sig.as_u32()) {
        return;
    }
    if let SigMatch::Clocked(env, y) = match_sig(arena, sig) {
        if let SigMatch::ClockEnvToken(id) = match_sig(arena, env) {
            out.insert(id);
        }
        collect_clock_env_ids(arena, y, visited, out);
        return;
    }
    if arena.is_nil(sig) {
        return;
    }
    let Some(node) = arena.node(sig) else {
        return;
    };
    for &child in node.children.as_slice() {
        collect_clock_env_ids(arena, child, visited, out);
    }
}

#[test]
fn structurally_identical_ondemand_instances_get_distinct_domains() {
    // The C++ de Bruijn collision class (plan §3.4): two structurally
    // identical `ondemand` applications in different contexts must yield
    // *distinct* clock-domain instances.
    let out = compile_inline(
        "od_twice",
        r#"od(x) = (button("gate"), x) : ondemand(*(2));
           process = _ <: od, od :> _;"#,
    );
    assert_eq!(
        out.clock_domains.len(),
        2,
        "each propagated wrapper instance must allocate its own domain"
    );
    let mut ids = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for &sig in &out.signals {
        collect_clock_env_ids(&out.parse.state.arena, sig, &mut visited, &mut ids);
    }
    assert_eq!(
        ids.len(),
        2,
        "both domain tokens must be reachable and distinct, got ids {ids:?}"
    );
}

// ── P1 — clock inference + hierarchical graph on the real pipeline ──────────

#[test]
fn clock_inference_and_hgraph_run_on_prepared_ondemand_program() {
    use transform::clk_env::annotate;
    use transform::hgraph::{GraphKey, audit_hgraph, build_hgraph, schedule};

    let out = compile_inline(
        "od_p1_end_to_end",
        r#"process = (button("gate"), _) : ondemand(*(2));"#,
    );
    let prepared = prepare_signals_for_fir(&out.parse.state.arena, &out.signals, &out.ui)
        .expect("P0 guarantees signal_prepare accepts the clocked graph");

    // The SIGCLOCKENV token ids survive the staging-arena clone, so the
    // propagation-owned domain table stays valid on the prepared forest.
    let envs = annotate(prepared.arena(), &out.clock_domains, prepared.outputs())
        .expect("prepared ondemand program must be well-clocked");

    // The output (Seq) lives at the audio rate; exactly one domain exists.
    assert_eq!(out.clock_domains.len(), 1);
    assert_eq!(envs.env(prepared.outputs()[0]), Some(None));

    let hgraph = build_hgraph(
        prepared.arena(),
        &out.clock_domains,
        &envs,
        prepared.outputs(),
    )
    .expect("hgraph builds on the prepared forest");
    audit_hgraph(&hgraph).expect("partition property");

    // One top graph + one wrapper subgraph, both scheduled deterministically.
    assert_eq!(hgraph.graphs().len(), 2);
    let sched = schedule(&hgraph).expect("per-domain graphs are acyclic");
    let top = sched.schedule(GraphKey::Top).expect("top schedule");
    assert!(!top.is_empty());
    let (wrapper_key, _) = hgraph.graphs()[1];
    let sub = sched.schedule(wrapper_key).expect("subgraph schedule");
    assert!(!sub.is_empty(), "the block body must be scheduled");
}

// ── P3 (slice 1) — boolean ondemand guarded-block lowering ──────────────────

/// Compiles a temp DSP source through the interpreter fast lane and runs it
/// with explicit per-channel inputs.
fn run_interp_with_inputs(stem: &str, source: &str, inputs: &[Vec<f32>]) -> Vec<Vec<f32>> {
    use std::io::Cursor;

    use codegen::backends::interp::{FbcDspInstance, InterpOptions, read_fbc};
    use compiler::SignalFirLane;

    let path =
        std::env::temp_dir().join(format!("faust-rs-odp3-{stem}-{}.dsp", std::process::id()));
    std::fs::write(&path, source)
        .unwrap_or_else(|e| panic!("failed to write temporary DSP {}: {e}", path.display()));
    let fbc = Compiler::new()
        .compile_file_default_to_interp_with_lane(
            &path,
            &InterpOptions::default(),
            SignalFirLane::TransformFastLane,
        )
        .unwrap_or_else(|e| panic!("{stem}: interp compilation failed: {e}"));
    let _ = std::fs::remove_file(&path);

    let frame_count = inputs.first().map_or(0, Vec::len);
    let mut reader = Cursor::new(fbc);
    let mut factory =
        read_fbc::<f32>(&mut reader).unwrap_or_else(|e| panic!("{stem}: fbc parse failed: {e}"));
    let mut instance = FbcDspInstance::new(&mut factory);
    instance.init(48_000);

    let num_outputs = usize::try_from(instance.get_num_outputs()).expect("non-negative outputs");
    let input_slices: Vec<&[f32]> = inputs.iter().map(Vec::as_slice).collect();
    let mut outputs = vec![vec![0.0_f32; frame_count]; num_outputs];
    let mut output_slices: Vec<&mut [f32]> = outputs.iter_mut().map(Vec::as_mut_slice).collect();
    instance
        .try_compute(frame_count as i32, &input_slices, &mut output_slices)
        .unwrap_or_else(|e| panic!("{stem}: interp execution failed: {e}"));
    outputs
}

#[test]
fn boolean_ondemand_compiles_to_fir_through_clocked_entry() {
    use transform::signal_fir::compile_signals_to_fir_fastlane_clocked;

    let out = compile_inline(
        "od_bool_fir",
        r#"process = ((_ != 0), _) : ondemand(*(2));"#,
    );
    compile_signals_to_fir_fastlane_clocked(
        &out.parse.state.arena,
        &out.signals,
        out.process_arity.inputs,
        out.process_arity.outputs,
        &out.ui,
        &out.clock_domains,
        &SignalFirOptions::default(),
    )
    .expect("boolean ondemand must lower through the clocked entry (P3 slice 1)");
}

#[test]
fn boolean_ondemand_holds_and_fires_at_runtime() {
    // y[n] = 2*x[n] when clk[n] != 0, else hold y[n-1]; holds start at 0.
    let clk = vec![0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0];
    let x = vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0, 80.0];
    let outputs = run_interp_with_inputs(
        "hold_fire",
        r#"process = ((_ != 0), _) : ondemand(*(2));"#,
        &[clk.clone(), x.clone()],
    );
    assert_eq!(outputs.len(), 1);
    let mut held = 0.0_f32;
    for n in 0..clk.len() {
        if clk[n] != 0.0 {
            held = 2.0 * x[n];
        }
        assert!(
            (outputs[0][n] - held).abs() < 1.0e-6,
            "frame {n}: expected {held}, got {}",
            outputs[0][n]
        );
    }
}

#[test]
fn boolean_ondemand_with_recursive_state_accumulates_on_fire_only() {
    // Inside the block: acc = acc' + x, advanced only when the clock fires.
    let clk = vec![1.0, 0.0, 1.0, 0.0, 0.0, 1.0];
    let x = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
    let outputs = run_interp_with_inputs(
        "acc_on_fire",
        r#"process = ((_ != 0), _) : ondemand(+ ~ _);"#,
        &[clk.clone(), x.clone()],
    );
    let mut acc = 0.0_f32;
    let mut held = 0.0_f32;
    for n in 0..clk.len() {
        if clk[n] != 0.0 {
            acc += x[n];
            held = acc;
        }
        assert!(
            (outputs[0][n] - held).abs() < 1.0e-6,
            "frame {n}: expected {held}, got {}",
            outputs[0][n]
        );
    }
}

#[test]
fn upsampling_keeps_named_rejection_through_clocked_entry() {
    use transform::signal_fir::compile_signals_to_fir_fastlane_clocked;

    let out = compile_inline(
        "us_still_rejected",
        r#"process = ((_ != 0), _) : upsampling(*(2));"#,
    );
    let err = compile_signals_to_fir_fastlane_clocked(
        &out.parse.state.arena,
        &out.signals,
        out.process_arity.inputs,
        out.process_arity.outputs,
        &out.ui,
        &out.clock_domains,
        &SignalFirOptions::default(),
    )
    .expect_err("upsampling is outside the boolean-ondemand slice");
    assert_eq!(err.code().as_str(), "FRS-SFIR-0007", "got: {err}");
    assert!(err.message().contains("upsampling"), "got: {err}");
}

// ── P0.4 — loud AD diagnostics at domain boundaries ──────────────────────────

fn expect_compile_error(name: &str, source: &str) -> compiler::CompilerError {
    Compiler::new()
        .compile_source_to_signals(name, source)
        .expect_err("program must be rejected during propagation")
}

#[test]
fn fad_around_ondemand_is_rejected_loudly() {
    // Cohabitation §4 row 2: `fad(… : ondemand(*(g)), g)` used to propagate
    // *silently-zero* tangents. It must now fail with the structured
    // FRS-PROP-0004 boundary error.
    let err = expect_compile_error(
        "fad_around_od",
        r#"g = hslider("g", 1, 0, 10, 0.1);
           process = fad((button("gate"), _) : ondemand(*(g)), g);"#,
    );
    let diagnostics = err
        .diagnostics()
        .expect("boundary rejection should expose diagnostics");
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|d| d.code.0 == "FRS-PROP-0004"),
        "expected FRS-PROP-0004, got: {err}"
    );
}

#[test]
fn fad_inside_ondemand_with_crossing_seed_is_rejected_loudly() {
    // Cohabitation §4 row 3: `ondemand(fad(*(g), g))` — the differentiated
    // body reads the clocked wrapper inputs, so the seed path crosses the
    // boundary. Must fail with FRS-PROP-0004 instead of zero tangents.
    let err = expect_compile_error(
        "fad_inside_od",
        r#"g = hslider("g", 1, 0, 10, 0.1);
           process = (button("gate"), _) : ondemand(fad(*(g), g));"#,
    );
    let diagnostics = err
        .diagnostics()
        .expect("boundary rejection should expose diagnostics");
    assert!(
        diagnostics
            .as_slice()
            .iter()
            .any(|d| d.code.0 == "FRS-PROP-0004"),
        "expected FRS-PROP-0004, got: {err}"
    );
}

#[test]
fn rad_around_ondemand_names_the_construct() {
    // Cohabitation §4 row 4: the RAD rejection must name the clocked
    // construct instead of the generic kind "other".
    let err = expect_compile_error(
        "rad_around_od",
        r#"g = hslider("g", 1, 0, 10, 0.1);
           process = rad((button("gate"), _) : ondemand(*(g)), g);"#,
    );
    let message = err.to_string();
    assert!(
        message.contains("ondemand")
            || message.contains("clocked")
            || message.contains("clock-domain"),
        "RAD rejection must name the clocked construct, got: {message}"
    );
}
