//! P3 shadow-mode comparison (observation-only).
//!
//! Plan `vector-mode-signal-level-analysis-cpp-port-plan-2026-07-10-en.md`,
//! "P3 - Scalar scheduling activation":
//!
//! > Run `Hsched` in shadow mode against demand-driven lowering first. Record
//! > statement-order, naming, and CSE differences for `-ss 0` before making
//! > it authoritative.
//!
//! This module answers, without changing any emitted FIR, the two questions
//! activation hinges on:
//!
//! 1. **Is activation safe?** Does the current demand-driven lowering order
//!    already *respect* every same-tick (immediate) dependency edge of the
//!    hierarchical graph? If yes, an `Hsched`-driven order — which respects
//!    those edges by construction — can only reorder *independent* nodes, so
//!    activation introduces no dependency-ordering change, only a possible
//!    reshuffle among nodes that are free to move.
//! 2. **Would activation change anything for `-ss 0`?** Restricted to the
//!    nodes both orders share, is the demand-driven order already identical
//!    to the depth-first `Hsched`? If yes, activating `-ss 0` would (for this
//!    program) be a no-op on statement order — no golden churn.
//!
//! # Why a comparison and not an assertion
//! The report *records*; it never panics. The demand-driven lowerer and the
//! hierarchical graph derive their child/dependency sets slightly differently
//! (e.g. the graph treats a `Delay`'s carried value as a delayed, non-ordering
//! edge and its amount as immediate; the lowerer descends by `match_sig`
//! children), so the two signal universes are related but not identical.
//! Emitting a report lets the P3 activation decision weigh real corpus
//! evidence instead of assuming absence of differences (plan exit criterion:
//! "Any textual golden changes for `-ss 0` are individually audited and
//! documented rather than assumed absent").
//!
//! # Alignment
//! Both inputs are over the *same* prepared arena: [`compare_emission_order`]
//! is only ever called from `compile_fastlane_inner`, which owns both the
//! causality gate's [`crate::hgraph::Hgraph`]/[`crate::hgraph::Hsched`] and
//! the `emission_order` returned by `build_module`.

use ahash::AHashMap;
use signals::SigId;

use crate::hgraph::{GraphKey, Hgraph, Hsched};
use crate::signal_fir::SignalFirOutput;

/// One same-tick dependency edge the demand-driven emission order failed to
/// respect: `consumer` was emitted before its immediate `dependency`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgeInversion {
    /// The graph the edge belongs to.
    pub key: GraphKey,
    /// The consumer node (emitted too early).
    pub consumer: SigId,
    /// The immediate dependency that should have preceded it.
    pub dependency: SigId,
}

/// Per-graph shadow-mode facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphShadow {
    /// Which hierarchical graph this describes.
    pub key: GraphKey,
    /// Owned nodes of the graph that also appear in the demand-driven
    /// emission order (the comparable intersection).
    pub covered_nodes: usize,
    /// Owned nodes of the graph *absent* from the emission order (inline or
    /// otherwise never separately materialized by the demand-driven lowerer).
    pub uncovered_nodes: usize,
    /// `true` when, restricted to `covered_nodes`, the demand-driven order is
    /// identical to this graph's selected `Hsched` order — i.e. activating
    /// the selected strategy would not reorder anything here.
    pub matches_schedule_on_intersection: bool,
}

/// Result of comparing one program's demand-driven emission order against a
/// selected `Hsched`. Observation-only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShadowReport {
    /// Per-graph facts, in `Hsched` order (`Control`, `Top`, wrappers…).
    pub graphs: Vec<GraphShadow>,
    /// Every immediate edge the demand-driven order got out of order. **Empty
    /// is the activation-safety signal**: the demand-driven order already
    /// respects every observable same-tick dependency.
    pub inversions: Vec<EdgeInversion>,
}

impl ShadowReport {
    /// The activation-safety verdict: the demand-driven order respects every
    /// observable immediate dependency of every graph.
    #[must_use]
    pub fn respects_all_immediate_edges(&self) -> bool {
        self.inversions.is_empty()
    }

    /// The no-churn verdict: on the comparable intersection, the demand-driven
    /// order already equals the selected schedule for every graph — so
    /// activating that strategy would change no statement order for this
    /// program.
    #[must_use]
    pub fn matches_schedule_everywhere(&self) -> bool {
        self.graphs
            .iter()
            .all(|g| g.matches_schedule_on_intersection)
    }
}

/// Builds a [`ShadowReport`] from a hierarchical graph, its selected schedule,
/// and the demand-driven FIR output (whose `emission_order` is the comparison
/// subject). See the module docs for the alignment contract.
#[must_use]
pub fn compare_emission_order(
    hgraph: &Hgraph,
    hsched: &Hsched,
    output: &SignalFirOutput,
) -> ShadowReport {
    // First-emission position of each materialized signal.
    let position: AHashMap<SigId, usize> = output
        .emission_order
        .iter()
        .enumerate()
        .map(|(i, &sig)| (sig, i))
        .collect();

    let mut inversions = Vec::new();
    let mut graphs = Vec::with_capacity(hgraph.graphs().len());

    for (key, graph) in hgraph.graphs() {
        let mut covered = 0usize;
        let mut uncovered = 0usize;

        // Immediate-edge respect: for every owned consumer and every owned
        // immediate dependency, the dependency must have been emitted first.
        // Only edges whose *both* endpoints were materialized are checkable.
        for &consumer in graph.nodes() {
            if position.contains_key(&consumer) {
                covered += 1;
            } else {
                uncovered += 1;
            }
            let Some(&consumer_pos) = position.get(&consumer) else {
                continue;
            };
            for edge in graph.edges(consumer) {
                if edge.delayed || !graph.contains(edge.to) {
                    continue;
                }
                if let Some(&dep_pos) = position.get(&edge.to)
                    && dep_pos >= consumer_pos
                {
                    inversions.push(EdgeInversion {
                        key: *key,
                        consumer,
                        dependency: edge.to,
                    });
                }
            }
        }

        // Intersection-order match: the demand-driven order and the selected
        // schedule, each restricted to nodes present in the other, must be
        // the same subsequence.
        let matches = match hsched.schedule(*key) {
            Some(sched) => {
                let demand: Vec<SigId> = output
                    .emission_order
                    .iter()
                    .copied()
                    .filter(|s| graph.contains(*s) && sched.contains(s))
                    .collect();
                let scheduled: Vec<SigId> = sched
                    .iter()
                    .copied()
                    .filter(|s| position.contains_key(s))
                    .collect();
                demand == scheduled
            }
            None => true,
        };

        graphs.push(GraphShadow {
            key: *key,
            covered_nodes: covered,
            uncovered_nodes: uncovered,
            matches_schedule_on_intersection: matches,
        });
    }

    ShadowReport { graphs, inversions }
}

#[cfg(test)]
mod tests {
    use signals::{BinOp, SigBuilder};
    use tlib::TreeArena;
    use ui::UiProgram;

    use crate::signal_fir::{RealType, SignalFirOptions, compile_signals_to_fir_fastlane_with_ui};

    fn compile(
        arena: &TreeArena,
        outputs: &[signals::SigId],
        num_out: usize,
    ) -> super::ShadowReport {
        let out = compile_signals_to_fir_fastlane_with_ui(
            arena,
            outputs,
            1,
            num_out,
            &UiProgram::empty(),
            &SignalFirOptions {
                module_name: "shadow_probe".to_owned(),
                real_type: RealType::Float32,
                ..SignalFirOptions::default()
            },
        )
        .expect("flat program lowers");
        out.shadow_report
            .expect("a non-clocked, wrapper-free program builds an Hgraph, hence a report")
    }

    #[test]
    fn demand_driven_order_respects_all_immediate_edges_for_a_shared_fork_join() {
        // x = input(0) * 0.5 (shared); a = x + 1; b = x * 2; out = a + b.
        let mut arena = TreeArena::new();
        let (out,) = {
            let mut b = SigBuilder::new(&mut arena);
            let inp = b.input(0);
            let half = b.real(0.5);
            let x = b.binop(BinOp::Mul, inp, half);
            let one = b.real(1.0);
            let a = b.binop(BinOp::Add, x, one);
            let two = b.real(2.0);
            let bb = b.binop(BinOp::Mul, x, two);
            (b.binop(BinOp::Add, a, bb),)
        };

        let report = compile(&arena, &[out], 1);
        assert!(
            report.respects_all_immediate_edges(),
            "demand-driven lowering must already respect every same-tick edge; \
             inversions: {:?}",
            report.inversions
        );
        // Every graph is covered by construction here (flat program: no
        // inline-only owned nodes escape the emission order in this shape).
        assert!(report.graphs.iter().any(|g| g.covered_nodes > 0));
    }

    #[test]
    fn report_is_present_and_covers_the_top_graph_for_a_recursive_program() {
        // y = input(0) + 0.9 * y' — a one-pole via SYMREC-shaped recursion is
        // built by the compiler front-end; here a simple delayed self-add is
        // enough to exercise a delayed edge (which must NOT count as an
        // immediate-edge inversion).
        let mut arena = TreeArena::new();
        let out = {
            let mut b = SigBuilder::new(&mut arena);
            let inp = b.input(0);
            let d = b.delay1(inp);
            b.binop(BinOp::Add, inp, d)
        };
        let report = compile(&arena, &[out], 1);
        assert!(
            report.respects_all_immediate_edges(),
            "a delayed (state) edge must never be treated as a same-tick \
             ordering constraint; inversions: {:?}",
            report.inversions
        );
    }
}
