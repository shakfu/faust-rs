//! `SchedulingStrategy::Special`: C++ `spschedule` / `recschedule` /
//! `recschedulenode` / `interleave`
//! (`compiler/DirectedGraph/Schedule.hh`, `DirectedGraphAlgorythm.hh`).
//!
//! # Growth guardrail
//! `rec_schedule_node` is the literal, unmemoized C++ recursion: a node with
//! `k` distinct consumers is re-expanded once per consumer, so a DAG whose
//! nodes are shared across many paths produces a duplicate list whose length
//! grows with the **path count**, not the node count (plan §4.1). See
//! `tests::growth` for a measured bound: a two-wide "ladder" of `L` fully
//! cross-connected layers (`2L` nodes) produces a duplicate list of length
//! `~2^(L+1)`. This is a faithful port, not a regression: C++
//! `schedulingcost` exists in the same header to *measure* a schedule, but
//! no compiler path invokes it to pick a cheaper one, so there is no
//! established C++ optimization being left behind. A future compact rewrite
//! must be proven order-equivalent to this literal form before it may
//! replace it (plan §4.1).

use ahash::AHashSet;

use super::common::roots_of;
use super::dag::ScheduleDag;

/// C++ `interleave` (`DirectedGraphAlgorythm.hh`): alternates elements of
/// `a` and `b`, then appends the remainder of whichever list is longer.
pub(crate) fn interleave<N: Copy>(a: &[N], b: &[N]) -> Vec<N> {
    let mut result = Vec::with_capacity(a.len() + b.len());
    for (&x, &y) in a.iter().zip(b.iter()) {
        result.push(x);
        result.push(y);
    }
    let n = a.len().min(b.len());
    result.extend_from_slice(&a[n..]);
    result.extend_from_slice(&b[n..]);
    result
}

/// C++ `recschedulenode`: `rec(v) = [v] ++ fold(interleave, [], [rec(d) | d
/// in deps(v)])`. Literal recursion, matching the C++ recursive function —
/// duplicates on purpose; `run` deduplicates afterward with a reverse scan.
fn rec_schedule_node<D: ScheduleDag>(dag: &D, n: D::Node) -> Vec<D::Node> {
    let mut acc: Vec<D::Node> = Vec::new();
    for d in dag.dependencies(n) {
        let sub = rec_schedule_node(dag, d);
        acc = interleave(&acc, &sub);
    }
    let mut out = Vec::with_capacity(acc.len() + 1);
    out.push(n);
    out.extend(acc);
    out
}

/// C++ `recschedule`: `raw(G) = fold(interleave, [], [rec(r) | r in
/// roots(G)])` — the duplicate-laden list before the reverse-scan dedup.
/// Exposed (crate-visible) so `tests::growth` can measure its length
/// directly, separately from the deduplicated schedule `run` returns.
pub(crate) fn raw<D: ScheduleDag>(dag: &D, nodes: &[D::Node]) -> Vec<D::Node> {
    let roots = roots_of(dag, nodes);
    let mut raw: Vec<D::Node> = Vec::new();
    for &root in &roots {
        let q = rec_schedule_node(dag, root);
        raw = interleave(&raw, &q);
    }
    raw
}

/// C++ `spschedule`: scan `reverse(raw(G))` retaining only the first
/// occurrence of each node — the order nodes are appended during that
/// reverse scan is the schedule.
pub(super) fn run<D: ScheduleDag>(dag: &D, nodes: &[D::Node]) -> Vec<D::Node> {
    let raw = raw(dag, nodes);
    let mut seen: AHashSet<D::Node> = AHashSet::new();
    let mut order = Vec::with_capacity(nodes.len());
    for &n in raw.iter().rev() {
        if seen.insert(n) {
            order.push(n);
        }
    }
    order
}
