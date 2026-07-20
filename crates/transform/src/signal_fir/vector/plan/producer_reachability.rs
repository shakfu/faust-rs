//! Producer-side reachability over the plan under construction.
//!
//! Trust boundary: this is deliberately separate from the checker's
//! `verify::checker_reachability` — do not merge them.

use crate::signal_fir::vector::verify::LoopEdge;
use std::collections::{BTreeMap, BTreeSet};

/// Compact transitive closure used while orienting effect conflicts.
///
/// The previous implementation ran one graph BFS for every conflicting loop
/// pair. Large UI DSPs have hundreds of loops and many effects propagated to
/// their output roots, making that quadratic pair scan cubic in graph size.
/// Bit rows make each query constant-time and update only predecessors of a
/// newly inserted acyclic edge.
pub(super) struct PlanReachability {
    pub(super) index: BTreeMap<u64, usize>,
    pub(super) rows: Vec<Vec<u64>>,
}
impl PlanReachability {
    pub(super) fn new(loops: &[u64], edges: &BTreeSet<LoopEdge>) -> Self {
        let index = loops
            .iter()
            .enumerate()
            .map(|(index, &loop_id)| (loop_id, index))
            .collect::<BTreeMap<_, _>>();
        let words = loops.len().div_ceil(u64::BITS as usize);
        let mut closure = Self {
            index,
            rows: vec![vec![0; words]; loops.len()],
        };
        for edge in edges {
            closure.set(edge.dependency, edge.consumer);
        }
        for intermediate in 0..loops.len() {
            let additions = closure.rows[intermediate].clone();
            for source in 0..loops.len() {
                if closure.bit(source, intermediate) {
                    or_bits(&mut closure.rows[source], &additions);
                }
            }
        }
        closure
    }

    pub(super) fn reaches(&self, from: u64, to: u64) -> bool {
        self.bit(self.index[&from], self.index[&to])
    }

    pub(super) fn add_edge(&mut self, from: u64, to: u64) {
        let from = self.index[&from];
        let to = self.index[&to];
        let mut additions = self.rows[to].clone();
        set_bit(&mut additions, to);
        for source in 0..self.rows.len() {
            if source == from || self.bit(source, from) {
                or_bits(&mut self.rows[source], &additions);
            }
        }
    }

    fn set(&mut self, from: u64, to: u64) {
        let from = self.index[&from];
        let to = self.index[&to];
        set_bit(&mut self.rows[from], to);
    }

    pub(super) fn bit(&self, from: usize, to: usize) -> bool {
        self.rows[from][to / u64::BITS as usize] & (1_u64 << (to % u64::BITS as usize)) != 0
    }

    pub(super) fn words(&self) -> usize {
        self.rows.first().map_or(0, Vec::len)
    }
}
pub(super) fn bit_at(bits: &[u64], index: usize) -> bool {
    bits[index / u64::BITS as usize] & (1_u64 << (index % u64::BITS as usize)) != 0
}
pub(super) fn bits_intersect(left: &[u64], right: &[u64]) -> bool {
    left.iter()
        .zip(right)
        .any(|(left, right)| left & right != 0)
}
pub(super) fn set_bit(bits: &mut [u64], index: usize) {
    bits[index / u64::BITS as usize] |= 1_u64 << (index % u64::BITS as usize);
}
pub(super) fn or_bits(target: &mut [u64], additions: &[u64]) {
    for (target, additions) in target.iter_mut().zip(additions) {
        *target |= additions;
    }
}
pub(super) fn and_bits(target: &mut [u64], mask: &[u64]) {
    for (target, mask) in target.iter_mut().zip(mask) {
        *target &= mask;
    }
}
pub(super) fn set_bit_indices(bits: &[u64]) -> impl Iterator<Item = usize> + '_ {
    bits.iter().enumerate().flat_map(|(word, value)| {
        let base = word * u64::BITS as usize;
        (0..u64::BITS as usize)
            .filter(move |bit| value & (1_u64 << bit) != 0)
            .map(move |bit| base + bit)
    })
}
