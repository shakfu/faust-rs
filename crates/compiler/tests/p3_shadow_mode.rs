//! P3 scalar-scheduling corpus evidence.
//!
//! Compiles a spread of plain scalar corpus programs end-to-end through the
//! real front-end and checks that authoritative `Hsched` lowering respects
//! every same-tick dependency. A separate asymmetric fixture proves that the
//! four public strategies do not collapse to one actual emission order.
//!
//! Exact schedule equality is checked on the comparable materialized subset.
//! Recursive bodies remain context-bound units, so their internal first-cache
//! trace may differ while still respecting every dependency.
//!
//! Scope: deliberately plain, forward-time scalar programs (recursion,
//! delays, UI, fork/join). RAD/BRA/clocked programs have a different,
//! epoch-structured ordering model (forward vs reverse sweeps, guarded
//! blocks) that the flat same-tick `Hgraph` does not describe, so they are
//! out of scope for this same-tick invariant and are covered by P5/P6.

use std::collections::BTreeSet;

use compiler::{Compiler, SchedulingStrategy};
use transform::signal_fir::shadow::ShadowReport;
use transform::signal_fir::{
    RealType, SignalFirOptions, SignalFirOutput, compile_signals_to_fir_fastlane_with_ui,
};

fn scalar_fir_for(name: &str, source: &str, strategy: SchedulingStrategy) -> SignalFirOutput {
    let out = Compiler::new()
        .compile_source_to_signals(name, source)
        .unwrap_or_else(|e| panic!("{name}: front-end compile failed: {e}"));
    compile_signals_to_fir_fastlane_with_ui(
        &out.parse.state.arena,
        &out.signals,
        out.process_arity.inputs,
        out.process_arity.outputs,
        &out.ui,
        &SignalFirOptions {
            module_name: "mydsp".to_owned(),
            real_type: RealType::Float32,
            scheduling_strategy: strategy,
            ..SignalFirOptions::default()
        },
    )
    .unwrap_or_else(|e| panic!("{name}: fast-lane lowering failed: {e}"))
}

fn shadow_for(name: &str, source: &str) -> ShadowReport {
    scalar_fir_for(name, source, SchedulingStrategy::DepthFirst)
        .shadow_report
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
fn authoritative_order_respects_immediate_edges_across_the_scalar_corpus() {
    let mut matched = 0usize;
    for (name, source) in PROGRAMS {
        let report = shadow_for(name, source);
        assert!(
            report.respects_all_immediate_edges(),
            "{name}: demand-driven lowering must already respect every \
             same-tick dependency edge under authoritative P3 lowering; \
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
    // Diagnostic only: recursive context bundling can make the first-cache
    // trace differ from the abstract per-node schedule.
    eprintln!(
        "P3 conformance: {matched}/{} sampled scalar programs exactly match \
         the -ss 0 schedule on the comparable materialized intersection",
        PROGRAMS.len()
    );
}

#[test]
fn selected_strategy_is_authoritative_and_changes_an_asymmetric_scalar_dag() {
    let source = "process = _,_ <: (_ * 2.0 + 1.0), (_ * 3.0 + 4.0) :> _;";
    let strategies = [
        SchedulingStrategy::DepthFirst,
        SchedulingStrategy::BreadthFirst,
        SchedulingStrategy::Special,
        SchedulingStrategy::ReverseBreadthFirst,
    ];
    let mut distinct_orders = BTreeSet::new();
    for strategy in strategies {
        let fir = scalar_fir_for("p3_authoritative", source, strategy);
        let report = fir
            .shadow_report
            .as_ref()
            .expect("plain scalar lowering records the accepted schedule");
        assert!(
            report.respects_all_immediate_edges(),
            "{strategy:?} introduced an immediate-edge inversion: {:?}",
            report.inversions
        );
        assert!(
            report.matches_schedule_everywhere(),
            "{strategy:?} must drive first lowering on this non-recursive graph"
        );
        distinct_orders.insert(
            fir.emission_order
                .iter()
                .map(|sig| sig.as_u32())
                .collect::<Vec<_>>(),
        );
    }
    assert!(
        distinct_orders.len() >= 2,
        "the four strategies must not collapse to one emission order on an asymmetric DAG"
    );
}
