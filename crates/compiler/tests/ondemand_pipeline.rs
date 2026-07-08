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
fn integer_ondemand_repeats_body_clock_times() {
    // Counted-loop OD: the body runs `clock` times per tick, and OD inputs
    // are *not* zero-padded (unlike US) — the accumulator adds the snapshot
    // input on every inner iteration: with x ≡ 1 and clock 3, the held
    // value after tick n is 3 * (n + 1).
    let ones = vec![1.0; 5];
    let outputs = run_interp_with_inputs(
        "int_od_accumulator",
        r#"process = (3, _) : ondemand(+ ~ _);"#,
        &[ones],
    );
    for (n, &value) in outputs[0].iter().enumerate() {
        let expected = 3.0 * (n as f32 + 1.0);
        assert!(
            (value - expected).abs() < 1.0e-6,
            "frame {n}: expected {expected}, got {value}"
        );
    }
}

#[test]
fn domain_free_body_state_advances_at_outer_rate() {
    // Clock-calculus least-fixed-point semantics (plan §4.1): a recursion
    // group whose definitions touch no domain-internal signal stays at the
    // audio rate — it advances once per outer tick even under an
    // upsampling wrapper, and only its *value* is annotated into the
    // domain. Held output after tick n is therefore n + 1, not 2(n + 1).
    let x = vec![0.0; 4];
    let outputs = run_interp_with_inputs(
        "hoisted_counter",
        r#"process = (2, (_ : !)) : upsampling(1 : (+ ~ _));"#,
        &[x],
    );
    for (n, &value) in outputs[0].iter().enumerate() {
        let expected = n as f32 + 1.0;
        assert!(
            (value - expected).abs() < 1.0e-6,
            "frame {n}: expected {expected}, got {value}"
        );
    }
}

#[test]
fn upsampling_zero_pads_inputs_to_the_last_inner_iteration() {
    // US factor 2 around an accumulator: per tick the body runs twice, the
    // zero-padded input contributes only on the last inner iteration, so the
    // held output is the plain running sum of x (not 2x).
    let x = vec![1.0, 2.0, 3.0, 4.0];
    let outputs = run_interp_with_inputs(
        "us_running_sum",
        r#"process = (2, _) : upsampling(+ ~ _);"#,
        std::slice::from_ref(&x),
    );
    let mut acc = 0.0_f32;
    for (n, &value) in outputs[0].iter().enumerate() {
        acc += x[n];
        assert!(
            (value - acc).abs() < 1.0e-6,
            "frame {n}: expected {acc}, got {value}"
        );
    }
}

#[test]
fn downsampling_fires_once_every_clock_ticks() {
    // DS factor 2 around an accumulator fed with ones: fires at ticks
    // 0, 2, 4, … → held output 1, 1, 2, 2, 3, 3.
    let ones = vec![1.0; 6];
    let outputs = run_interp_with_inputs(
        "ds_accumulator",
        r#"process = (2, _) : downsampling(+ ~ _);"#,
        &[ones],
    );
    let expected = [1.0, 1.0, 2.0, 2.0, 3.0, 3.0];
    for (n, (&value, &want)) in outputs[0].iter().zip(expected.iter()).enumerate() {
        assert!(
            (value - want).abs() < 1.0e-6,
            "frame {n}: expected {want}, got {value}"
        );
    }
}

#[test]
fn inner_circular_delay_line_matches_unclocked_when_always_firing() {
    // @(17) > max_copy_delay forces the CircularPow2 strategy, which now
    // uses the per-domain fIOTA_d<i> cursor inside the block. With an
    // always-true clock the block fires every tick, so the clocked program
    // must match the plain unclocked one sample for sample.
    let frames = 48;
    let ramp: Vec<f32> = (0..frames).map(|n| n as f32).collect();
    let ones = vec![1.0_f32; frames];

    let clocked = run_interp_with_inputs(
        "od_delay_always",
        r#"process = ((_ != 0), _) : ondemand(_ <: _, @(17) :> +);"#,
        &[ones, ramp.clone()],
    );
    let unclocked = run_interp_with_inputs(
        "delay_reference",
        r#"process = _ <: _, @(17) :> +;"#,
        std::slice::from_ref(&ramp),
    );
    for n in 0..frames {
        assert!(
            (clocked[0][n] - unclocked[0][n]).abs() < 1.0e-6,
            "frame {n}: clocked {} vs unclocked {}",
            clocked[0][n],
            unclocked[0][n]
        );
    }
}

#[test]
fn inner_circular_delay_line_advances_in_fire_time() {
    // Sparse clock (fires on even ticks): the inner delay line must count
    // *fires*, not samples. Reference model: on fire, append the snapshot
    // input; the delayed tap reads the value written 17 fires ago (0 before
    // the line fills); the output holds between fires.
    let frames = 60;
    let ramp: Vec<f32> = (0..frames).map(|n| n as f32).collect();
    let clk: Vec<f32> = (0..frames)
        .map(|n| f32::from(u8::from(n % 2 == 0)))
        .collect();

    let outputs = run_interp_with_inputs(
        "od_delay_sparse",
        r#"process = ((_ != 0), _) : ondemand(_ <: _, @(17) :> +);"#,
        &[clk.clone(), ramp.clone()],
    );

    let mut fired: Vec<f32> = Vec::new();
    let mut held = 0.0_f32;
    for n in 0..frames {
        if clk[n] != 0.0 {
            fired.push(ramp[n]);
            let delayed = if fired.len() > 17 {
                fired[fired.len() - 1 - 17]
            } else {
                0.0
            };
            held = ramp[n] + delayed;
        }
        assert!(
            (outputs[0][n] - held).abs() < 1.0e-6,
            "frame {n}: expected {held}, got {}",
            outputs[0][n]
        );
    }
}

// ── P4 — FAD Phase A: fad strictly inside a clock domain ────────────────────

/// Runs one single-param program at `param` (baked as the hslider init) and
/// returns its outputs.
fn run_od_fad_source(stem: &str, source: String, inputs: &[Vec<f32>]) -> Vec<Vec<f32>> {
    run_interp_with_inputs(stem, &source, inputs)
}

#[test]
fn fad_inside_ondemand_tangent_matches_central_difference() {
    // Phase A (cohabitation §5): differentiation strictly inside one domain
    // needs zero new AD code. The differentiated expression reads only the
    // control seed, so the FAD sweep never touches boundary glue; the
    // primal/tangent pair is held by the block like any other output.
    let frames = 8;
    let ones = vec![1.0_f32; frames];
    let base = 0.7_f32;
    let eps = 1.0e-3_f32;

    let fad_src = |p: f32| {
        format!(
            r#"g = hslider("g", {p}, -10, 10, 0.001);
               process = ((_ != 0), (_ : !)) : ondemand(fad(sin(g) * g, g));"#
        )
    };
    let primal_src = |p: f32| {
        format!(
            r#"g = hslider("g", {p}, -10, 10, 0.001);
               process = ((_ != 0), (_ : !)) : ondemand(sin(g) * g);"#
        )
    };

    let fad_out = run_od_fad_source("od_fad", fad_src(base), &[ones.clone(), ones.clone()]);
    assert_eq!(fad_out.len(), 2, "fad bundle = [primal, tangent]");
    let plus = run_od_fad_source(
        "od_fad_p",
        primal_src(base + eps),
        &[ones.clone(), ones.clone()],
    );
    let minus = run_od_fad_source("od_fad_m", primal_src(base - eps), &[ones.clone(), ones]);

    let expected_primal = base.sin() * base;
    let expected_tangent = (plus[0][frames - 1] - minus[0][frames - 1]) / (2.0 * eps);
    for (n, (&primal, &tangent)) in fad_out[0].iter().zip(fad_out[1].iter()).enumerate() {
        assert!(
            (primal - expected_primal).abs() < 1.0e-5,
            "frame {n}: primal {primal} vs {expected_primal}"
        );
        assert!(
            (tangent - expected_tangent).abs() < 2.0e-3,
            "frame {n}: tangent {tangent} vs central difference {expected_tangent}"
        );
    }
}

#[test]
fn fad_inside_ondemand_tangent_holds_between_fires() {
    // Sparse clock: both held outputs (primal and tangent) are 0 before the
    // first fire and hold their value between fires — the tangent is
    // co-clocked with the primal by construction.
    let frames = 6;
    let clk = vec![0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
    let ones = vec![1.0_f32; frames];
    let g = 0.5_f32;

    let out = run_od_fad_source(
        "od_fad_hold",
        format!(
            r#"g = hslider("g", {g}, -10, 10, 0.001);
               process = ((_ != 0), (_ : !)) : ondemand(fad(g * g, g));"#
        ),
        &[clk.clone(), ones],
    );
    let expected_primal = g * g;
    let expected_tangent = 2.0 * g;
    for (n, (&primal, &tangent)) in out[0].iter().zip(out[1].iter()).enumerate() {
        let (want_p, want_t) = if n < 2 {
            (0.0, 0.0)
        } else {
            (expected_primal, expected_tangent)
        };
        assert!(
            (primal - want_p).abs() < 1.0e-6,
            "frame {n}: primal {primal} vs {want_p}"
        );
        assert!(
            (tangent - want_t).abs() < 1.0e-6,
            "frame {n}: tangent {tangent} vs {want_t}"
        );
    }
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

// ── S1–S2 — the `interleave` spectral primitive (frame-rate serialization) ──
//
// `interleave(N, FX) = serialize_in(N) : periodic_od(FX) : serialize_out(N)`
// with a boolean periodic clock of period N and phase N-1 (fires at
// t ≡ N-1 mod N). Built entirely on top of the P3 boolean-ondemand block —
// no new compiler primitive (option A: `up0(H, y) = y * (H != 0)`).
// See `porting/interleave-spectral-primitive-2026-07-07-en.md`.
//
// The prelude below is the hermetic (library-free) copy of `interleave.lib`;
// the two must stay in sync.

/// The generic `interleave` prelude, parameterized by `N` and the frame
/// operator `FX` through Faust's `par`. Self-contained (no `stdfaust.lib`).
const INTERLEAVE_PRELUDE: &str = r#"
frame_clock(N) = ((+(1) : %(N)) ~ _) == 0;
bus(N) = par(i, N, _);
serialize_in(N) = _ <: par(i, N, @(N-1-i));
interleave(N, FX) = serialize_in(N) : od : serialize_out
with {
    od = (frame_clock(N), bus(N)) : ondemand(FX);
    up0(y) = y * (frame_clock(N) != 0);
    serialize_out = par(j, N, up0 : @(j)) :> _;
};
"#;

fn interleave_source(process: &str) -> String {
    format!("{INTERLEAVE_PRELUDE}\nprocess = {process};")
}

/// Runs a ramp `x[n] = n` through `process` and returns the single output.
fn run_ramp(stem: &str, process: &str, frames: usize) -> Vec<f32> {
    let ramp: Vec<f32> = (0..frames).map(|n| n as f32).collect();
    let out = run_interp_with_inputs(
        stem,
        &interleave_source(process),
        std::slice::from_ref(&ramp),
    );
    assert_eq!(out.len(), 1, "{stem}: expected a single output channel");
    out.into_iter().next().expect("one output")
}

#[test]
fn interleave_identity_is_a_constant_delay_n_minus_1() {
    // S1 locking milestone: interleave(N, id) == @(N-1) — constant latency
    // N-1, identity up to that delay. For N=2 this is exactly `mem`.
    for n in [2usize, 4, 8] {
        let frames = 6 * n;
        let got = run_ramp(
            &format!("interleave_id_{n}"),
            &format!("interleave({n}, bus({n}))"),
            frames,
        );
        for (t, &value) in got.iter().enumerate() {
            let expected = t.checked_sub(n - 1).map_or(0.0, |k| k as f32);
            assert!(
                (value - expected).abs() < 1.0e-6,
                "N={n} frame {t}: interleave(id) = {value}, expected @(N-1) = {expected}"
            );
        }
    }
}

#[test]
fn interleave_matches_a_plain_mem_for_n_2() {
    // The N=2 unrolled-table anchor of the design note: interleave(2, id)
    // is sample-for-sample identical to `mem`.
    let frames = 16;
    let ramp: Vec<f32> = (0..frames).map(|n| n as f32).collect();
    let both = run_interp_with_inputs(
        "interleave2_vs_mem",
        &format!("{INTERLEAVE_PRELUDE}\nprocess = _ <: interleave(2, bus(2)), mem;"),
        std::slice::from_ref(&ramp),
    );
    for (t, (&il, &m)) in both[0].iter().zip(both[1].iter()).enumerate() {
        assert!(
            (il - m).abs() < 1.0e-6,
            "frame {t}: interleave(2,id) = {il} vs mem = {m}"
        );
    }
}

#[test]
fn interleave_with_a_framewise_gain_scales_the_delayed_stream() {
    // A stateless frame operator (scale every lane by 2) commutes with the
    // serialization: interleave(N, 2*id) == 2 * @(N-1).
    let n = 4usize;
    let frames = 6 * n;
    let got = run_ramp(
        "interleave_gain",
        &format!("interleave({n}, par(i, {n}, *(2.0)))"),
        frames,
    );
    for (t, &value) in got.iter().enumerate() {
        let expected = t.checked_sub(n - 1).map_or(0.0, |k| 2.0 * k as f32);
        assert!(
            (value - expected).abs() < 1.0e-6,
            "frame {t}: interleave(2*id) = {value}, expected 2*@(N-1) = {expected}"
        );
    }
}

#[test]
fn interleave_with_an_internal_frame_delay_adds_one_frame_of_latency() {
    // S2 fixture: a delay inside the frame operator is measured in *frames*.
    // The boolean clock fires once every N samples, so `@(1)` inside the
    // block advances on the per-domain time (P3 slice 3): delaying every
    // lane by one frame shifts the whole reconstructed stream by N samples.
    // interleave(N, frame_delay_1) == @(N-1) then @(N) == @(2N-1).
    for n in [2usize, 4] {
        let frames = 6 * n;
        let got = run_ramp(
            &format!("interleave_framedelay_{n}"),
            &format!("interleave({n}, par(i, {n}, @(1)))"),
            frames,
        );
        for (t, &value) in got.iter().enumerate() {
            let expected = t.checked_sub(2 * n - 1).map_or(0.0, |k| k as f32);
            assert!(
                (value - expected).abs() < 1.0e-6,
                "N={n} frame {t}: interleave(frame_delay) = {value}, expected @(2N-1) = {expected}"
            );
        }
    }
}
