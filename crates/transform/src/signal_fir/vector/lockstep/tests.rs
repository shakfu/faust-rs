//! Tests for `vector::lockstep` (relocated from the former inline
//! `mod tests` block; test names unchanged).

use propagate::ClockDomainTable;
use signals::SigBuilder;
use tlib::TreeArena;

use crate::clk_env::annotate;
use crate::signal_fir::decoration_verify::certify_decorations;
use crate::signal_fir::vector::plan::build_vector_plan_with_lockstep;
use crate::signal_fir::vector::verify::LoopKind;
use crate::signal_prepare::prepare_signals_for_fir_verified;

use super::*;

fn one_poles(outer_ops: [signals::BinOp; 2]) -> (VerifiedPreparedSignals, VectorPlan) {
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
            builder.binop(outer_ops[channel as usize], input, scaled)
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
    let plan = build_vector_plan_with_lockstep(&prepared, &decorations, 8)
        .unwrap()
        .into_plan();
    (prepared, plan)
}

#[test]
fn prepared_parallel_traversal_accepts_isomorphic_recursive_lanes() {
    let (prepared, plan) = one_poles([signals::BinOp::Add; 2]);
    assert_eq!(plan.lockstep_bundles.len(), 1);
    assert_eq!(plan.lockstep_bundles[0].member_loop_ids.len(), 2);
    assert!(
        plan.lockstep_bundles[0]
            .member_loop_ids
            .iter()
            .all(|loop_id| {
                plan.loops[*loop_id as usize].kind == LoopKind::Lockstep { width: 2 }
            })
    );
    verify_lockstep_isomorphism(&plan, &prepared).unwrap();
}

#[test]
fn prepared_parallel_traversal_rejects_corrupted_shape_hash() {
    let (prepared, mut plan) = one_poles([signals::BinOp::Add; 2]);
    plan.lockstep_bundles[0].lanes[1].roots[0].shape_hash ^= 1;
    assert!(matches!(
        verify_lockstep_isomorphism(&plan, &prepared),
        Err(VectorPlanError::LockstepIsoWitnessMismatch {
            bundle_id: 0,
            loop_id: 1,
        })
    ));
}

#[test]
fn near_isomorphic_recursive_lanes_remain_unbundled() {
    let (_, plan) = one_poles([signals::BinOp::Add, signals::BinOp::Sub]);
    assert!(plan.lockstep_bundles.is_empty());
    assert!(
        plan.loops
            .iter()
            .filter(|loop_record| matches!(loop_record.kind, LoopKind::Recursive(_)))
            .count()
            >= 2
    );
}

#[test]
fn parallel_shape_memoizes_deeply_shared_lane_pairs() {
    const LAYERS: usize = 18;

    let mut arena = TreeArena::new();
    let representative = {
        let mut builder = SigBuilder::new(&mut arena);
        let mut node = builder.input(0);
        for _ in 0..LAYERS {
            let sine = builder.sin(node);
            let cosine = builder.cos(node);
            node = builder.binop(signals::BinOp::Add, sine, cosine);
        }
        node
    };
    let lane = {
        let mut builder = SigBuilder::new(&mut arena);
        let mut node = builder.input(1);
        for _ in 0..LAYERS {
            let sine = builder.sin(node);
            let cosine = builder.cos(node);
            node = builder.binop(signals::BinOp::Add, sine, cosine);
        }
        node
    };
    let prepared =
        prepare_signals_for_fir_verified(&arena, &[representative, lane], &ui::UiProgram::empty())
            .unwrap();
    let mut shape = ParallelShape::new(&prepared);

    shape
        .visit(prepared.outputs()[0], prepared.outputs()[1])
        .expect("the two shared DAGs are isomorphic");

    assert_eq!(shape.expanded_pairs, LAYERS * 3 + 1);
    assert_eq!(shape.verified.len(), LAYERS * 3 + 1);
}
