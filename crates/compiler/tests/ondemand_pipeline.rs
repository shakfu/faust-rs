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
