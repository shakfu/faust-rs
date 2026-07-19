//! Checker-side reachability, derived only from the plan's own edges
//! (trust boundary: independent of the producer's reachability).

use super::model::*;
use ahash::AHashMap;

/// Checker-local transitive closure. This deliberately does not reuse the
/// producer's implementation: the certificate boundary remains independent,
/// while avoiding a BFS and signal-map rebuild for every loop pair.
pub(super) struct CheckedReachability {
    index: AHashMap<u64, usize>,
    rows: Vec<Vec<u64>>,
}
impl CheckedReachability {
    pub(super) fn new(plan: &VectorPlan) -> Self {
        Self::from_edges(plan, plan.data_edges.iter().chain(&plan.effect_edges))
    }

    fn from_edges<'a>(plan: &VectorPlan, edges: impl Iterator<Item = &'a LoopEdge>) -> Self {
        let index = plan
            .loops
            .iter()
            .enumerate()
            .map(|(index, loop_record)| (loop_record.loop_id, index))
            .collect::<AHashMap<_, _>>();
        let words = plan.loops.len().div_ceil(u64::BITS as usize);
        let mut rows = vec![vec![0_u64; words]; plan.loops.len()];
        for edge in edges {
            let from = index[&edge.dependency];
            let to = index[&edge.consumer];
            rows[from][to / u64::BITS as usize] |= 1_u64 << (to % u64::BITS as usize);
        }
        for intermediate in 0..rows.len() {
            let additions = rows[intermediate].clone();
            for row in &mut rows {
                if row[intermediate / u64::BITS as usize]
                    & (1_u64 << (intermediate % u64::BITS as usize))
                    != 0
                {
                    for (target, addition) in row.iter_mut().zip(&additions) {
                        *target |= addition;
                    }
                }
            }
        }
        Self { index, rows }
    }

    pub(super) fn reaches(&self, from: u64, to: u64) -> bool {
        let from = self.index[&from];
        let to = self.index[&to];
        self.rows[from][to / u64::BITS as usize] & (1_u64 << (to % u64::BITS as usize)) != 0
    }
}
