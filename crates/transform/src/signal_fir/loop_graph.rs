//! Loop graph for vector mode (`-vec`) — roadmap P6, vector doc V2.
//!
//! Scalar mode compiles the whole per-sample block into one `for i in 0..count`
//! loop. Vector mode restructures it into an **outer chunk loop** of `vec_size`
//! samples containing a **DAG of small inner loops** — one per recursive group,
//! per delayed-or-shared signal, etc. — so the C compiler can auto-vectorize the
//! non-recursive ones (SIMD), while recursive computations stay in serial loops.
//!
//! This module owns the loop-DAG **data model** and its **deterministic
//! levelization** (a port of the C++ `sortGraph`, whose `std::set<Loop*>` is
//! pointer-ordered and therefore non-deterministic across runs — here loops are
//! keyed by insertion-ordered [`LoopId`], so emission order is stable). Two
//! later slices consume it:
//!
//! - **V3–V4** populate it from the signal lowering (a current-loop stack
//!   mirroring the C++ `openLoop`/`closeLoop`, the `needSeparateLoop` criterion,
//!   cross-loop chunk buffers, and vector delay-line layouts);
//! - **V5** emits it (each [`LoopNode`] becomes a chunk `for` with its
//!   pre/exec/post phases; levels drive `// Section : n` grouping).
//!
//! Nothing here is wired into scalar codegen yet, so it cannot affect existing
//! output; the `dead_code` allowance is removed when V3 starts populating it.
#![allow(dead_code)]

use std::collections::BTreeSet;

use fir::FirId;

/// Index of a loop node in a [`LoopGraph`].
///
/// Allocation order == insertion order, and every set/queue below is
/// `LoopId`-ordered, so the levelization and emission are deterministic — the
/// fix for the C++ pointer-ordered `lset` non-determinism.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub(crate) struct LoopId(pub(crate) u32);

/// Whether a chunk loop may be auto-vectorized, and why not when it may not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum LoopKind {
    /// Non-recursive: the inner `for` is a candidate for auto-vectorization.
    Vectorizable,
    /// A recursive group (`maxDelay > 0` back-edge / recursive projection):
    /// must run serially, one sample after another.
    Recursive,
    /// A clocked-domain block (`ondemand`/`upsampling`/`downsampling`): a serial
    /// **scalar island** (vector doc §6, rule D1). Its externals are chunk
    /// buffers; its inner-domain state stays scalar.
    Island,
}

impl LoopKind {
    /// Whether the C backend may auto-vectorize this loop's inner body.
    #[must_use]
    pub(crate) fn is_vectorizable(self) -> bool {
        matches!(self, Self::Vectorizable)
    }
}

/// One chunk loop: three phase statement lists plus its backward dependencies.
///
/// The three phases mirror the C++ `fPreCode` / `fExecCode` / `fPostCode`
/// printed around the per-chunk `for`: `pre` is the head-copy / index setup,
/// `exec` is the chunk body (`for i in 0..count`), `post` is the tail-copy /
/// index save. Scalar-equivalent loops leave `pre`/`post` empty.
#[derive(Clone, Debug)]
pub(crate) struct LoopNode {
    /// Vectorizable / recursive / island classification.
    pub(crate) kind: LoopKind,
    /// Whether the chunk `for` runs in reverse sample time (RAD/BRA).
    pub(crate) is_reverse: bool,
    /// Statements emitted **before** the chunk `for` (per-chunk setup / head copy).
    pub(crate) pre: Vec<FirId>,
    /// Statements forming the chunk `for` body (`for i in 0..count`).
    pub(crate) exec: Vec<FirId>,
    /// Statements emitted **after** the chunk `for` (tail copy / index save).
    pub(crate) post: Vec<FirId>,
    /// Loops that must run before this one (this loop reads their chunk buffers).
    pub(crate) deps: BTreeSet<LoopId>,
}

impl LoopNode {
    fn new(kind: LoopKind, is_reverse: bool) -> Self {
        Self {
            kind,
            is_reverse,
            pre: Vec::new(),
            exec: Vec::new(),
            post: Vec::new(),
            deps: BTreeSet::new(),
        }
    }
}

/// A DAG of chunk loops. Nodes are stored in insertion order; edges are backward
/// dependencies (`a` depends on `b` ⇒ `b` is emitted before `a`).
#[derive(Clone, Debug, Default)]
pub(crate) struct LoopGraph {
    nodes: Vec<LoopNode>,
}

/// Error returned when the loop DAG has a cycle (which must never happen: a
/// backward dependency edge always points at an earlier-produced value).
#[derive(Clone, PartialEq, Eq, Debug)]
pub(crate) struct LoopCycle {
    /// The loops that remained unscheduled (participate in a cycle).
    pub(crate) unscheduled: Vec<LoopId>,
}

impl LoopGraph {
    /// Creates an empty graph.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Allocates a new loop node and returns its id.
    pub(crate) fn add_loop(&mut self, kind: LoopKind, is_reverse: bool) -> LoopId {
        let id = LoopId(u32::try_from(self.nodes.len()).expect("loop count fits u32"));
        self.nodes.push(LoopNode::new(kind, is_reverse));
        id
    }

    /// Number of loops.
    #[must_use]
    pub(crate) fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph has no loops.
    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    fn index(id: LoopId) -> usize {
        id.0 as usize
    }

    /// Immutable access to a loop node.
    #[must_use]
    pub(crate) fn node(&self, id: LoopId) -> &LoopNode {
        &self.nodes[Self::index(id)]
    }

    /// Mutable access to a loop node (to push phase statements).
    pub(crate) fn node_mut(&mut self, id: LoopId) -> &mut LoopNode {
        &mut self.nodes[Self::index(id)]
    }

    /// Records that `from` must run after `to` (`from` reads `to`'s output).
    /// A self-edge is ignored; edges within one loop are not dependencies.
    pub(crate) fn add_dep(&mut self, from: LoopId, to: LoopId) {
        if from != to {
            self.nodes[Self::index(from)].deps.insert(to);
        }
    }

    /// Iterates loop ids in insertion order.
    pub(crate) fn ids(&self) -> impl Iterator<Item = LoopId> {
        (0..self.nodes.len()).map(|i| LoopId(i as u32))
    }

    /// Deterministic topological order (dependencies before dependents).
    ///
    /// Kahn's algorithm with a `LoopId`-ordered ready set: among loops whose
    /// dependencies are all satisfied, the lowest [`LoopId`] is emitted first,
    /// so independent loops keep their insertion order. This is the stable
    /// replacement for the C++ pointer-ordered `sortGraph`.
    pub(crate) fn topological_order(&self) -> Result<Vec<LoopId>, LoopCycle> {
        let n = self.nodes.len();
        // Outgoing "dependents" adjacency + in-degree = number of unmet deps.
        let mut indegree = vec![0usize; n];
        let mut dependents: Vec<BTreeSet<LoopId>> = vec![BTreeSet::new(); n];
        for (i, node) in self.nodes.iter().enumerate() {
            indegree[i] = node.deps.len();
            for &dep in &node.deps {
                dependents[Self::index(dep)].insert(LoopId(i as u32));
            }
        }
        // BTreeSet keeps the ready frontier LoopId-ordered.
        let mut ready: BTreeSet<LoopId> = (0..n)
            .filter(|&i| indegree[i] == 0)
            .map(|i| LoopId(i as u32))
            .collect();
        let mut order = Vec::with_capacity(n);
        while let Some(&next) = ready.iter().next() {
            ready.remove(&next);
            order.push(next);
            for &d in &dependents[Self::index(next)] {
                let di = Self::index(d);
                indegree[di] -= 1;
                if indegree[di] == 0 {
                    ready.insert(d);
                }
            }
        }
        if order.len() == n {
            Ok(order)
        } else {
            let scheduled: BTreeSet<LoopId> = order.iter().copied().collect();
            Err(LoopCycle {
                unscheduled: self
                    .ids()
                    .filter(|id| !scheduled.contains(id))
                    .collect(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_graph_orders_to_nothing() {
        let g = LoopGraph::new();
        assert!(g.is_empty());
        assert_eq!(g.topological_order().unwrap(), vec![]);
    }

    #[test]
    fn independent_loops_keep_insertion_order() {
        let mut g = LoopGraph::new();
        let a = g.add_loop(LoopKind::Vectorizable, false);
        let b = g.add_loop(LoopKind::Recursive, false);
        let c = g.add_loop(LoopKind::Island, true);
        assert_eq!(g.len(), 3);
        // No edges → insertion order, deterministically.
        assert_eq!(g.topological_order().unwrap(), vec![a, b, c]);
        assert!(g.node(a).kind.is_vectorizable());
        assert!(!g.node(b).kind.is_vectorizable());
        assert!(g.node(c).is_reverse);
    }

    #[test]
    fn dependencies_are_emitted_before_dependents() {
        // c depends on b, b depends on a → a, b, c regardless of alloc order.
        let mut g = LoopGraph::new();
        let a = g.add_loop(LoopKind::Vectorizable, false);
        let b = g.add_loop(LoopKind::Vectorizable, false);
        let c = g.add_loop(LoopKind::Vectorizable, false);
        g.add_dep(c, b);
        g.add_dep(b, a);
        assert_eq!(g.topological_order().unwrap(), vec![a, b, c]);
    }

    #[test]
    fn ready_frontier_is_loop_id_ordered() {
        // a is a shared root feeding b and c; b and c are independent, so they
        // come out in LoopId order (b before c), deterministically.
        let mut g = LoopGraph::new();
        let a = g.add_loop(LoopKind::Vectorizable, false);
        let b = g.add_loop(LoopKind::Vectorizable, false);
        let c = g.add_loop(LoopKind::Vectorizable, false);
        g.add_dep(b, a);
        g.add_dep(c, a);
        assert_eq!(g.topological_order().unwrap(), vec![a, b, c]);
    }

    #[test]
    fn self_edges_are_ignored() {
        let mut g = LoopGraph::new();
        let a = g.add_loop(LoopKind::Recursive, false);
        g.add_dep(a, a);
        assert!(g.node(a).deps.is_empty());
        assert_eq!(g.topological_order().unwrap(), vec![a]);
    }

    #[test]
    fn a_cycle_is_reported() {
        let mut g = LoopGraph::new();
        let a = g.add_loop(LoopKind::Vectorizable, false);
        let b = g.add_loop(LoopKind::Vectorizable, false);
        g.add_dep(a, b);
        g.add_dep(b, a);
        let err = g.topological_order().unwrap_err();
        assert_eq!(err.unscheduled, vec![a, b]);
    }

    #[test]
    fn phase_statements_and_deps_round_trip() {
        let mut store = fir::FirStore::new();
        let (s0, s1) = {
            let mut b = fir::FirBuilder::new(&mut store);
            (b.int32(0), b.int32(1))
        };
        let mut g = LoopGraph::new();
        let l = g.add_loop(LoopKind::Vectorizable, false);
        g.node_mut(l).pre.push(s0);
        g.node_mut(l).exec.push(s1);
        assert_eq!(g.node(l).pre, vec![s0]);
        assert_eq!(g.node(l).exec, vec![s1]);
        assert!(g.node(l).post.is_empty());
    }
}
