//! Variability-driven statement placement — Phase 1 analysis.
//!
//! This module contains the *pre-lowering analysis* that drives Phase 1 of the
//! FIR emission pipeline: deciding **where** (in which lifecycle section) each
//! signal node's FIR statement will be placed.
//!
//! # Background
//!
//! Faust distinguishes three execution tiers based on how often a value can
//! change:
//!
//! | Tier | [`Variability`] | FIR section | C++ section |
//! |------|-----------------|-------------|-------------|
//! | Init-time constant | [`Variability::Konst`] | `constants_statements` | `instanceConstants` |
//! | Block-rate control | [`Variability::Block`] | `control_statements` | `compute` preamble |
//! | Per-sample | [`Variability::Samp`] | `sample_statements` | `compute` inner loop |
//!
//! Phase 1 ensures that every FIR statement is emitted **exactly once, in the
//! correct section**, rather than being re-evaluated on every sample tick.
//!
//! # What this module provides
//!
//! [`analyze_signal_sharing`] performs a single DFS pre-pass over the signal
//! DAG before any lowering takes place.  It produces two maps that the lowering
//! engine ([`super::module::SignalToFirLower`]) stores and consults during
//! [`lower_sig`](super::module) dispatch:
//!
//! - **`ref_counts`**: how many parent nodes reference each [`SigId`].  A node
//!   with `ref_count >= 2` is *shared* and benefits from being materialized into
//!   a named variable (`fConst*` / `fSlow*`) so the expression is computed only
//!   once.
//! - **`has_higher_parent`**: nodes that sit at a *variability boundary* — i.e.
//!   at least one parent has strictly higher variability.  Even a single-use
//!   node at a boundary must be materialized, otherwise it would be inlined into
//!   its parent's (faster) execution tier and re-evaluated too frequently.
//!
//! [`Bucket`] is the runtime tag that identifies which section a hoisted
//! variable belongs to.
//!
//! [`is_trivial_fir`] is a predicate consulted by the placement gate inside
//! `lower_sig`: literals, variable loads, and null values are free to duplicate,
//! so they are never materialized into temporary variables.
//!
//! # Placement gate (inside `lower_sig`)
//!
//! The actual hoisting decision lives in `module.rs` as part of the main
//! `lower_sig` dispatch loop (it needs mutable access to the lowering engine's
//! internal statement lists and counters).  The gate combines the three
//! pieces provided here:
//!
//! ```text
//! if !is_trivial_fir(lowered)
//!    && !is_recursive_projection(sig)       // impl on SignalToFirLower
//!    && !matches!(sig, WrTbl(..))
//!    && (sig_shared || at_boundary)          // ref_counts / has_higher_parent
//! {
//!     match variability_of(sig) {            // impl on SignalToFirLower
//!         Konst => materialize_in_bucket(Constants)
//!         Block => materialize_in_bucket(Control)
//!         _     => inline
//!     }
//! }
//! ```
//!
//! See also: `porting/fir-cse-runtime-optimizations-plan-2026-04-03-en.md`,
//! section "Phase 1 — variability-driven statement placement".

use std::collections::{HashMap, HashSet};

use fir::{FirId, FirMatch, FirStore, match_fir};
use signals::SigId;
use sigtype::{SigType, Variability};
use tlib::TreeArena;

// ─── Bucket ──────────────────────────────────────────────────────────────────

/// Execution-tier bucket for variability-driven statement placement.
///
/// Maps directly to the C++ Faust compiler's three execution tiers: init-time
/// constants (`instanceConstants`), block-rate control expressions (before
/// the sample loop in `compute`), and sample-rate expressions (inside the loop).
///
/// See [Phase 1 of the FIR runtime optimization plan](../../porting/fir-cse-runtime-optimizations-plan-2026-04-03-en.md#2-phase-1--variability-driven-statement-placement).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Bucket {
    /// Init-time constants — written once in `instanceConstants`.
    Constants,
    /// Block-rate controls — written once per `compute()` call, before the loop.
    Control,
}

// ─── Trivial-node predicate ───────────────────────────────────────────────────

/// Returns `true` when a FIR value node is *trivial* — meaning it should
/// never be materialized into a named variable because it is already free
/// to duplicate (literals, variable loads, null values).
///
/// This prevents variability placement from hoisting bare constants or
/// variable references into unnecessary temporary variables.
pub(super) fn is_trivial_fir(store: &FirStore, node: FirId) -> bool {
    matches!(
        match_fir(store, node),
        FirMatch::Int32 { .. }
            | FirMatch::Int64 { .. }
            | FirMatch::Float32 { .. }
            | FirMatch::Float64 { .. }
            | FirMatch::Bool { .. }
            | FirMatch::LoadVar { .. }
            | FirMatch::LoadVarAddress { .. }
            | FirMatch::NullValue { .. }
    )
}

// ─── Signal-sharing analysis ──────────────────────────────────────────────────

/// Pre-analysis of the signal DAG for Phase 1 placement decisions.
///
/// Performs a single depth-first traversal of the signal DAG rooted at
/// `roots` and returns two maps:
///
/// - **`ref_counts`**: how many times each [`SigId`] appears as a child across
///   the entire DAG.  Nodes with `ref_count >= 2` are *shared*: materializing
///   them as a named variable (`fConst*` / `fSlow*`) avoids redundant
///   re-evaluation.
/// - **`has_higher_parent`**: the set of [`SigId`]s that have at least one
///   parent whose variability is strictly higher (faster).  These nodes sit at
///   a *variability boundary* and must be materialized even if they are
///   single-use, to guarantee they execute in their own (slower) bucket.
///
/// All roots are assumed to be consumed by the `compute` output store, which
/// runs at sample rate ([`Variability::Samp`]).
pub(super) fn analyze_signal_sharing(
    arena: &TreeArena,
    roots: &[SigId],
    sig_types: &HashMap<SigId, SigType>,
) -> (HashMap<SigId, usize>, HashSet<SigId>) {
    let mut ref_counts: HashMap<SigId, usize> = HashMap::new();
    let mut has_higher_parent: HashSet<SigId> = HashSet::new();
    let mut visited: HashSet<SigId> = HashSet::new();
    // Roots are consumed by the output store (Samp context).
    let root_var = Some(Variability::Samp);
    for &root in roots {
        analyze_sig_rec(
            arena,
            root,
            root_var,
            sig_types,
            &mut ref_counts,
            &mut has_higher_parent,
            &mut visited,
        );
    }
    (ref_counts, has_higher_parent)
}

/// Recursive DFS helper for [`analyze_signal_sharing`].
///
/// Increments `ref_counts[sig]` on every visit (including revisits), but
/// only descends into children on the *first* visit (`visited` gate).  This
/// correctly counts how many parent edges reach each node while avoiding
/// exponential blowup on dense DAGs.
///
/// `parent_var` is the variability of the calling node (`None` at the root).
/// If `parent_var > my_var` the node is added to `has_higher_parent`,
/// flagging it as sitting at a variability boundary.
fn analyze_sig_rec(
    arena: &TreeArena,
    sig: SigId,
    parent_var: Option<Variability>,
    sig_types: &HashMap<SigId, SigType>,
    ref_counts: &mut HashMap<SigId, usize>,
    has_higher_parent: &mut HashSet<SigId>,
    visited: &mut HashSet<SigId>,
) {
    *ref_counts.entry(sig).or_insert(0) += 1;

    // Check variability boundary: parent variability > this node's variability.
    let my_var = sig_types.get(&sig).map(|t| t.variability());
    if let (Some(pv), Some(mv)) = (parent_var, my_var)
        && pv > mv
    {
        has_higher_parent.insert(sig);
    }

    if !visited.insert(sig) {
        return; // already descended into children
    }
    if let Some(node) = arena.node(sig) {
        for &child_tid in node.children.as_slice() {
            analyze_sig_rec(
                arena,
                child_tid,
                my_var,
                sig_types,
                ref_counts,
                has_higher_parent,
                visited,
            );
        }
    }
}
