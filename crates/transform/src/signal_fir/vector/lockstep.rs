//! Lockstep instance-vectorization isomorphism producer and independent gate.
//!
//! Faust C++ has no corresponding instance-SIMD pass. The signal constructor
//! semantics and recursive-group decoding come from
//! `compiler/signals/signals.cpp` and `recursivness.cpp`; the adapted Rust
//! contract is section 8 of
//! `porting/vector-mode-signal-level-analysis-cpp-port-plan-2026-07-10-en.md`.
//! A shape hash is only a candidate key: [`verify_lockstep_isomorphism`]
//! confirms every match by a second parallel traversal and checks the explicit
//! leaf mapping before lowering may consume a bundle.

use std::collections::{BTreeMap, BTreeSet};

use signals::{SigId, SigMatch, match_sig};
use tlib::match_sym_ref;

use crate::signal_prepare::VerifiedPreparedSignals;

use super::recursion::decode_symbolic_group_bodies;
use super::vector_verify::{
    IsoLeafMapping, IsoRootWitness, VectorPlan, VectorPlanError, verify_vector_plan,
};

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

struct ShapeHasher(u64);

impl ShapeHasher {
    fn new() -> Self {
        Self(FNV_OFFSET)
    }

    fn token(&mut self, token: &str) {
        for byte in token.as_bytes().iter().copied().chain([0xff]) {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(FNV_PRIME);
        }
    }

    fn finish(self) -> u64 {
        if self.0 == 0 { 1 } else { self.0 }
    }
}

struct ParallelShape<'a> {
    prepared: &'a VerifiedPreparedSignals,
    hasher: ShapeHasher,
    leaves: BTreeMap<u64, u64>,
    active: BTreeSet<(u64, u64)>,
}

impl<'a> ParallelShape<'a> {
    fn new(prepared: &'a VerifiedPreparedSignals) -> Self {
        Self {
            prepared,
            hasher: ShapeHasher::new(),
            leaves: BTreeMap::new(),
            active: BTreeSet::new(),
        }
    }

    fn leaf(&mut self, tag: &str, representative: SigId, lane: SigId) -> Option<()> {
        self.hasher.token(tag);
        let representative = u64::from(representative.as_u32());
        let lane = u64::from(lane.as_u32());
        match self.leaves.insert(representative, lane) {
            Some(previous) if previous != lane => None,
            _ => Some(()),
        }
    }

    fn unary(&mut self, tag: &str, left: SigId, right: SigId) -> Option<()> {
        self.hasher.token(tag);
        self.visit(left, right)
    }

    fn binary(
        &mut self,
        tag: &str,
        left_a: SigId,
        left_b: SigId,
        right_a: SigId,
        right_b: SigId,
    ) -> Option<()> {
        self.hasher.token(tag);
        self.visit(left_a, right_a)?;
        self.visit(left_b, right_b)
    }

    fn visit(&mut self, representative: SigId, lane: SigId) -> Option<()> {
        let pair = (u64::from(representative.as_u32()), u64::from(lane.as_u32()));
        if !self.active.insert(pair) {
            return None;
        }

        let arena = self.prepared.arena();
        let result = if match_sym_ref(arena, representative).is_some()
            || match_sym_ref(arena, lane).is_some()
        {
            if match_sym_ref(arena, representative).is_some()
                && match_sym_ref(arena, lane).is_some()
            {
                self.leaf("leaf:state", representative, lane)
            } else {
                None
            }
        } else if let Some((_, representative_bodies)) =
            decode_symbolic_group_bodies(arena, representative)
        {
            let (_, lane_bodies) = decode_symbolic_group_bodies(arena, lane)?;
            if representative_bodies.len() != lane_bodies.len() {
                None
            } else {
                self.hasher
                    .token(&format!("rec:{}", representative_bodies.len()));
                representative_bodies
                    .into_iter()
                    .zip(lane_bodies)
                    .try_for_each(|(left, right)| self.visit(left, right))
            }
        } else if decode_symbolic_group_bodies(arena, lane).is_some() {
            None
        } else {
            match (match_sig(arena, representative), match_sig(arena, lane)) {
                (SigMatch::Int(_), SigMatch::Int(_)) => self.leaf("leaf:int", representative, lane),
                (SigMatch::Real(_), SigMatch::Real(_)) => {
                    self.leaf("leaf:real", representative, lane)
                }
                (SigMatch::Input(_), SigMatch::Input(_)) => {
                    self.leaf("leaf:input", representative, lane)
                }
                (SigMatch::FConst(_, _, _), SigMatch::FConst(_, _, _)) => {
                    self.leaf("leaf:fconst", representative, lane)
                }
                (SigMatch::FVar(_, _, _), SigMatch::FVar(_, _, _)) => {
                    self.leaf("leaf:fvar", representative, lane)
                }
                (SigMatch::Delay1(left), SigMatch::Delay1(right)) => {
                    self.unary("delay1", left, right)
                }
                (SigMatch::Delay(la, lb), SigMatch::Delay(ra, rb)) => {
                    self.binary("delay", la, lb, ra, rb)
                }
                (SigMatch::Prefix(la, lb), SigMatch::Prefix(ra, rb)) => {
                    self.binary("prefix", la, lb, ra, rb)
                }
                (SigMatch::IntCast(left), SigMatch::IntCast(right)) => {
                    self.unary("int_cast", left, right)
                }
                (SigMatch::BitCast(left), SigMatch::BitCast(right)) => {
                    self.unary("bit_cast", left, right)
                }
                (SigMatch::FloatCast(left), SigMatch::FloatCast(right)) => {
                    self.unary("float_cast", left, right)
                }
                (SigMatch::Proj(li, lg), SigMatch::Proj(ri, rg)) if li == ri => {
                    self.hasher.token(&format!("proj:{li}"));
                    self.visit(lg, rg)
                }
                (SigMatch::BinOp(lo, la, lb), SigMatch::BinOp(ro, ra, rb)) if lo == ro => {
                    self.binary(&format!("bin:{}", lo as i64), la, lb, ra, rb)
                }
                (SigMatch::Select2(la, lb, lc), SigMatch::Select2(ra, rb, rc)) => {
                    self.hasher.token("select2");
                    self.visit(la, ra)?;
                    self.visit(lb, rb)?;
                    self.visit(lc, rc)
                }
                (SigMatch::Output(li, left), SigMatch::Output(ri, right)) if li == ri => {
                    self.unary(&format!("output:{li}"), left, right)
                }
                (SigMatch::Pow(la, lb), SigMatch::Pow(ra, rb)) => {
                    self.binary("pow", la, lb, ra, rb)
                }
                (SigMatch::Min(la, lb), SigMatch::Min(ra, rb)) => {
                    self.binary("min", la, lb, ra, rb)
                }
                (SigMatch::Max(la, lb), SigMatch::Max(ra, rb)) => {
                    self.binary("max", la, lb, ra, rb)
                }
                (SigMatch::Atan2(la, lb), SigMatch::Atan2(ra, rb)) => {
                    self.binary("atan2", la, lb, ra, rb)
                }
                (SigMatch::Fmod(la, lb), SigMatch::Fmod(ra, rb)) => {
                    self.binary("fmod", la, lb, ra, rb)
                }
                (SigMatch::Remainder(la, lb), SigMatch::Remainder(ra, rb)) => {
                    self.binary("remainder", la, lb, ra, rb)
                }
                (SigMatch::Acos(left), SigMatch::Acos(right)) => self.unary("acos", left, right),
                (SigMatch::Asin(left), SigMatch::Asin(right)) => self.unary("asin", left, right),
                (SigMatch::Atan(left), SigMatch::Atan(right)) => self.unary("atan", left, right),
                (SigMatch::Cos(left), SigMatch::Cos(right)) => self.unary("cos", left, right),
                (SigMatch::Sin(left), SigMatch::Sin(right)) => self.unary("sin", left, right),
                (SigMatch::Tan(left), SigMatch::Tan(right)) => self.unary("tan", left, right),
                (SigMatch::Exp(left), SigMatch::Exp(right)) => self.unary("exp", left, right),
                (SigMatch::Exp10(left), SigMatch::Exp10(right)) => self.unary("exp10", left, right),
                (SigMatch::Log(left), SigMatch::Log(right)) => self.unary("log", left, right),
                (SigMatch::Log10(left), SigMatch::Log10(right)) => self.unary("log10", left, right),
                (SigMatch::Sqrt(left), SigMatch::Sqrt(right)) => self.unary("sqrt", left, right),
                (SigMatch::Abs(left), SigMatch::Abs(right)) => self.unary("abs", left, right),
                (SigMatch::Floor(left), SigMatch::Floor(right)) => self.unary("floor", left, right),
                (SigMatch::Ceil(left), SigMatch::Ceil(right)) => self.unary("ceil", left, right),
                (SigMatch::Rint(left), SigMatch::Rint(right)) => self.unary("rint", left, right),
                (SigMatch::Round(left), SigMatch::Round(right)) => self.unary("round", left, right),
                _ => None,
            }
        };
        self.active.remove(&pair);
        result
    }
}

/// Produces one exact root witness after confirming structural equality by
/// parallel traversal. Hash equality alone is never sufficient.
pub(crate) fn build_iso_root_witness(
    prepared: &VerifiedPreparedSignals,
    representative_root: SigId,
    lane_root: SigId,
) -> Option<IsoRootWitness> {
    let mut shape = ParallelShape::new(prepared);
    shape.visit(representative_root, lane_root)?;
    Some(IsoRootWitness {
        representative_root: u64::from(representative_root.as_u32()),
        lane_root: u64::from(lane_root.as_u32()),
        shape_hash: shape.hasher.finish(),
        leaf_mapping: shape
            .leaves
            .into_iter()
            .map(
                |(representative_signal_id, lane_signal_id)| IsoLeafMapping {
                    representative_signal_id,
                    lane_signal_id,
                },
            )
            .collect(),
    })
}

fn prepared_ids(prepared: &VerifiedPreparedSignals) -> BTreeMap<u64, SigId> {
    let mut result = BTreeMap::new();
    let mut stack = prepared.outputs().to_vec();
    while let Some(signal) = stack.pop() {
        if result.insert(u64::from(signal.as_u32()), signal).is_some() {
            continue;
        }
        if let Some(children) = prepared.arena().children(signal) {
            stack.extend(children.iter().copied());
        }
    }
    result
}

/// Re-traverses every accepted lockstep lane against its representative.
///
/// This checker is intentionally separate from the producer and from
/// [`verify_vector_plan`]: the plan-local gate validates finite graph facts,
/// while this gate consumes the authoritative prepared forest and proves that
/// each stored leaf map is the exact result of constructor-by-constructor
/// comparison.
pub fn verify_lockstep_isomorphism(
    plan: &VectorPlan,
    prepared: &VerifiedPreparedSignals,
) -> Result<(), VectorPlanError> {
    verify_vector_plan(plan)?;
    let ids = prepared_ids(prepared);
    for bundle in &plan.lockstep_bundles {
        for lane in &bundle.lanes {
            for root in &lane.roots {
                let Some(&representative_root) = ids.get(&root.representative_root) else {
                    return Err(VectorPlanError::LockstepIsoWitnessMismatch {
                        bundle_id: bundle.bundle_id,
                        loop_id: lane.loop_id,
                    });
                };
                let Some(&lane_root) = ids.get(&root.lane_root) else {
                    return Err(VectorPlanError::LockstepIsoWitnessMismatch {
                        bundle_id: bundle.bundle_id,
                        loop_id: lane.loop_id,
                    });
                };
                if build_iso_root_witness(prepared, representative_root, lane_root).as_ref()
                    != Some(root)
                {
                    return Err(VectorPlanError::LockstepIsoWitnessMismatch {
                        bundle_id: bundle.bundle_id,
                        loop_id: lane.loop_id,
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use propagate::ClockDomainTable;
    use signals::SigBuilder;
    use tlib::TreeArena;

    use crate::clk_env::annotate;
    use crate::signal_fir::decoration_verify::certify_decorations;
    use crate::signal_fir::vector_plan::build_vector_plan;
    use crate::signal_fir::vector_verify::{LockstepBundleRecord, LockstepLaneRecord, LoopKind};
    use crate::signal_prepare::prepare_signals_for_fir_verified;

    use super::*;

    fn independent_one_poles() -> (VerifiedPreparedSignals, VectorPlan) {
        let mut arena = TreeArena::new();
        let mut roots = Vec::new();
        for channel in 0..2 {
            let self_ref = tlib::de_bruijn_ref(&mut arena, 1);
            let body = {
                let mut builder = SigBuilder::new(&mut arena);
                let feedback = builder.proj(0, self_ref);
                let previous = builder.delay1(feedback);
                let input = builder.input(channel);
                let half = builder.real(0.5);
                let scaled = builder.binop(signals::BinOp::Mul, previous, half);
                builder.binop(signals::BinOp::Add, input, scaled)
            };
            let nil = arena.nil();
            let bodies = arena.cons(body, nil);
            let recursion = tlib::de_bruijn_rec(&mut arena, bodies);
            roots.push(SigBuilder::new(&mut arena).proj(0, recursion));
        }
        let prepared =
            prepare_signals_for_fir_verified(&arena, &roots, &ui::UiProgram::empty()).unwrap();
        let clocks = annotate(
            prepared.arena(),
            &ClockDomainTable::new(),
            prepared.outputs(),
        )
        .unwrap();
        let decorations = certify_decorations(&prepared, &clocks).unwrap();
        let mut plan = build_vector_plan(&decorations, 8).unwrap().into_plan();
        let recursive = plan
            .loops
            .iter()
            .filter_map(|loop_record| match loop_record.kind {
                LoopKind::Recursive(group) => Some((loop_record.loop_id, group)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(recursive.len(), 2);
        let width = recursive.len() as u64;
        let representative = plan.loops[recursive[0].0 as usize].roots.clone();
        let lanes = recursive
            .iter()
            .map(|&(loop_id, recursion_group)| {
                let roots = plan.loops[loop_id as usize]
                    .roots
                    .iter()
                    .copied()
                    .zip(representative.iter().copied())
                    .map(|(lane_root, representative_root)| {
                        let ids = prepared_ids(&prepared);
                        build_iso_root_witness(
                            &prepared,
                            ids[&representative_root],
                            ids[&lane_root],
                        )
                        .expect("one-pole roots are isomorphic")
                    })
                    .collect();
                LockstepLaneRecord {
                    loop_id,
                    recursion_group,
                    roots,
                }
            })
            .collect::<Vec<_>>();
        for &(loop_id, _) in &recursive {
            plan.loops[loop_id as usize].kind = LoopKind::Lockstep { width };
        }
        plan.lockstep_bundles = vec![LockstepBundleRecord {
            bundle_id: 0,
            representative_loop_id: recursive[0].0,
            member_loop_ids: recursive.iter().map(|&(loop_id, _)| loop_id).collect(),
            lanes,
        }];
        (prepared, plan)
    }

    #[test]
    fn prepared_parallel_traversal_accepts_isomorphic_recursive_lanes() {
        let (prepared, plan) = independent_one_poles();
        verify_lockstep_isomorphism(&plan, &prepared).unwrap();
    }

    #[test]
    fn prepared_parallel_traversal_rejects_corrupted_shape_hash() {
        let (prepared, mut plan) = independent_one_poles();
        plan.lockstep_bundles[0].lanes[1].roots[0].shape_hash ^= 1;
        assert!(matches!(
            verify_lockstep_isomorphism(&plan, &prepared),
            Err(VectorPlanError::LockstepIsoWitnessMismatch {
                bundle_id: 0,
                loop_id: 1,
            })
        ));
    }
}
