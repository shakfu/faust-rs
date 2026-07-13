//! P3 shadow-mode corpus evidence (observation-only).
//!
//! Compiles a spread of plain scalar corpus programs end-to-end through the
//! real front-end and asserts the activation-safety property the P3 plan
//! needs before making `Hsched` authoritative: the current demand-driven
//! lowering order already **respects every same-tick (immediate) dependency
//! edge** of the hierarchical graph. Where that holds, an `Hsched`-driven
//! order can only reorder independent nodes — activation introduces no
//! dependency-ordering change.
//!
//! Also tallies how many programs' demand-driven order is *already identical*
//! to the depth-first `-ss 0` schedule on the comparable intersection (the
//! "no golden churn" signal), printed for the record.
//!
//! Scope: deliberately plain, forward-time scalar programs (recursion,
//! delays, UI, fork/join). RAD/BRA/clocked programs have a different,
//! epoch-structured ordering model (forward vs reverse sweeps, guarded
//! blocks) that the flat same-tick `Hgraph` does not describe, so they are
//! out of scope for this same-tick invariant and are covered by P5/P6.

use compiler::Compiler;
use transform::signal_fir::shadow::ShadowReport;
use transform::signal_fir::{RealType, SignalFirOptions, compile_signals_to_fir_fastlane_with_ui};

fn shadow_for(name: &str, source: &str) -> ShadowReport {
    let out = Compiler::new()
        .compile_source_to_signals(name, source)
        .unwrap_or_else(|e| panic!("{name}: front-end compile failed: {e}"));
    let fir = compile_signals_to_fir_fastlane_with_ui(
        &out.parse.state.arena,
        &out.signals,
        out.process_arity.inputs,
        out.process_arity.outputs,
        &out.ui,
        &SignalFirOptions {
            module_name: "mydsp".to_owned(),
            real_type: RealType::Float32,
            ..SignalFirOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("{name}: fast-lane lowering failed: {e}"));
    fir.shadow_report
        .unwrap_or_else(|| panic!("{name}: a plain scalar program must build an Hgraph"))
}

/// Plain, forward-time scalar corpus shapes (mirrors `tests/corpus/rep_0*`).
const PROGRAMS: &[(&str, &str)] = &[
    ("passthrough", "process = _;"),
    ("gain_bias", "process = _ * 0.5 + 0.1;"),
    (
        "stereo_mix",
        "process = _,_ <: (_ + _) * 0.5, (_ - _) * 0.5;",
    ),
    (
        "delay_echo",
        "process = _ <: _,(mem : @(2205) : *(0.35)) : +;",
    ),
    ("one_pole", "process = +~(*(0.5)) : *(0.5);"),
    ("comb_feedforward", "process = _ + (_ : @(1103) : *(0.6));"),
    (
        "branch_and_sum",
        "process = _ <: *(0.7),*(0.2),*(-0.1) :> _+_+_;",
    ),
    ("shared_expr", "process = _ * 0.5 <: _ + 1.0, _ * 2.0 :> _;"),
    ("two_pole_ish", "process = _ : +~(*(0.4)) : +~(*(0.2));"),
];

#[test]
fn demand_driven_order_respects_immediate_edges_across_the_scalar_corpus() {
    let mut matched = 0usize;
    for (name, source) in PROGRAMS {
        let report = shadow_for(name, source);
        assert!(
            report.respects_all_immediate_edges(),
            "{name}: demand-driven lowering must already respect every \
             same-tick dependency edge before P3 activation is safe; \
             inversions: {:?}",
            report.inversions
        );
        if report.matches_schedule_everywhere() {
            matched += 1;
        }
        let covered: usize = report.graphs.iter().map(|g| g.covered_nodes).sum();
        let uncovered: usize = report.graphs.iter().map(|g| g.uncovered_nodes).sum();
        eprintln!(
            "{name}: graphs={} covered={covered} uncovered={uncovered} \
             matches_ss0={}",
            report.graphs.len(),
            report.matches_schedule_everywhere()
        );
    }
    // Recorded finding, not an assertion target: how many of the sampled
    // scalar programs would see zero `-ss 0` statement-order change on
    // activation (their demand-driven order already equals the DFS schedule
    // on the comparable intersection).
    eprintln!(
        "P3 shadow mode: {matched}/{} sampled scalar programs already match \
         the -ss 0 schedule on the comparable intersection",
        PROGRAMS.len()
    );
}
