//! Tests for `vector::schedule` (relocated from the former inline
//! `mod tests` block; test names unchanged).

use std::collections::BTreeSet;

use super::*;
use crate::schedule::{SchedulingStrategy, verify_schedule};
use crate::signal_fir::pv_slice::{build_pv_plan, build_pv_signals};
use crate::signal_fir::vector::verify::{
    FusedSerialGroupRecord, IsoLeafMapping, IsoRootWitness, LockstepBundleRecord,
    LockstepLaneRecord, LoopEdge, LoopKind, LoopRecord, Placement, Rate, SignalRecord, ValueType,
    VecSafeWitness, Vectorability, WitnessKind,
};

const ALL_STRATEGIES: [SchedulingStrategy; 4] = [
    SchedulingStrategy::DepthFirst,
    SchedulingStrategy::BreadthFirst,
    SchedulingStrategy::Special,
    SchedulingStrategy::ReverseBreadthFirst,
];

fn plan_with_epochs(
    loop_count: u64,
    epochs: &[(u64, u64, &[u64])],
    data_edges: &[LoopEdge],
    effect_edges: &[LoopEdge],
) -> VectorPlan {
    let mut loop_epoch = vec![None; loop_count as usize];
    for &(epoch_id, _, loops) in epochs {
        for &loop_id in loops {
            loop_epoch[loop_id as usize] = Some(epoch_id);
        }
    }

    VectorPlan {
        schema_version: crate::signal_fir::vector::verify::VECTOR_PLAN_SCHEMA_VERSION,
        lockstep_bundles: Vec::new(),
        vec_size: 16,
        signals: (0..loop_count)
            .map(|loop_id| SignalRecord {
                signal_id: loop_id,
                value_type: ValueType::Real,
                structural: false,
                rate: Rate::Samp,
                vectorability: Vectorability::Vect,
                clock_id: 0,
                effects: Vec::new(),
                direct_effects: Vec::new(),
                placement: Placement::Owned(loop_id),
                duplicable: true,
            })
            .collect(),
        loops: (0..loop_count)
            .map(|loop_id| LoopRecord {
                loop_id,
                stable_name: format!("loop_{loop_id}"),
                kind: LoopKind::Vectorizable,
                roots: vec![loop_id],
                epoch_id: loop_epoch[loop_id as usize].expect("every loop has an epoch"),
            })
            .collect(),
        epochs: epochs
            .iter()
            .map(|&(epoch_id, rank, loops)| EpochRecord {
                epoch_id,
                rank,
                loops: loops.to_vec(),
            })
            .collect(),
        transports: Vec::new(),
        data_edges: data_edges.to_vec(),
        effect_edges: effect_edges.to_vec(),
        vec_safe_witnesses: (0..loop_count)
            .map(|loop_id| VecSafeWitness {
                loop_id,
                witness_kind: WitnessKind::Pointwise,
            })
            .collect(),
        fused_serial_groups: Vec::new(),
    }
}

fn asymmetric_plan() -> VectorPlan {
    plan_with_epochs(
        5,
        &[(0, 0, &[0, 1, 2, 3, 4])],
        &[
            LoopEdge {
                consumer: 1,
                dependency: 0,
            },
            LoopEdge {
                consumer: 2,
                dependency: 0,
            },
            LoopEdge {
                consumer: 4,
                dependency: 1,
            },
            LoopEdge {
                consumer: 4,
                dependency: 2,
            },
            LoopEdge {
                consumer: 4,
                dependency: 3,
            },
        ],
        &[],
    )
}

fn lockstep_plan() -> VectorPlan {
    let mut plan = plan_with_epochs(2, &[(0, 0, &[0, 1])], &[], &[]);
    for loop_id in 0..2 {
        plan.loops[loop_id].kind = LoopKind::Lockstep { width: 2 };
        plan.vec_safe_witnesses[loop_id].witness_kind = WitnessKind::SerialStateInternal;
    }
    plan.lockstep_bundles = vec![LockstepBundleRecord {
        bundle_id: 0,
        representative_loop_id: 0,
        member_loop_ids: vec![0, 1],
        lanes: (0..2)
            .map(|loop_id| LockstepLaneRecord {
                loop_id,
                recursion_group: 10 + loop_id,
                roots: vec![IsoRootWitness {
                    representative_root: 0,
                    lane_root: loop_id,
                    shape_hash: 1,
                    leaf_mapping: vec![IsoLeafMapping {
                        representative_signal_id: 0,
                        lane_signal_id: loop_id,
                    }],
                }],
            })
            .collect(),
    }];
    plan
}

#[test]
fn lockstep_bundle_is_one_scheduler_node_and_expands_canonically() {
    let plan = lockstep_plan();
    let dag = EpochDag::new(&plan.epochs[0].loops, &plan, SchedulingStrategy::DepthFirst).unwrap();
    assert_eq!(dag.nodes(), vec![0]);
    let schedule = schedule_vector_plan(&plan, SchedulingStrategy::DepthFirst).unwrap();
    assert_eq!(schedule.epochs[0].loops, vec![0, 1]);
}

#[test]
fn all_strategies_produce_valid_complete_epoch_orders() {
    let plan = asymmetric_plan();
    for strategy in ALL_STRATEGIES {
        let schedule = schedule_vector_plan(&plan, strategy).expect("valid plan schedules");
        let epoch = &schedule.epochs[0];
        let dag = EpochDag::new(&plan.epochs[0].loops, &plan, strategy).unwrap();
        verify_schedule(&dag, &epoch.loops).expect("order is complete and valid");
    }
}

#[test]
fn asymmetric_same_epoch_dag_exposes_strategy_difference() {
    let plan = asymmetric_plan();
    let orders: BTreeSet<Vec<u64>> = ALL_STRATEGIES
        .into_iter()
        .map(|strategy| {
            schedule_vector_plan(&plan, strategy)
                .expect("valid plan schedules")
                .epochs[0]
                .loops
                .clone()
        })
        .collect();
    assert!(
        orders.len() >= 2,
        "strategies should expose different orders"
    );
}

#[test]
fn effect_edges_constrain_each_epoch_order() {
    let plan = plan_with_epochs(
        4,
        &[(0, 0, &[0, 1, 2, 3])],
        &[],
        &[LoopEdge {
            consumer: 3,
            dependency: 1,
        }],
    );
    for strategy in ALL_STRATEGIES {
        let order = &schedule_vector_plan(&plan, strategy)
            .expect("valid effect-constrained plan")
            .epochs[0]
            .loops;
        assert!(
            order.iter().position(|&id| id == 1) < order.iter().position(|&id| id == 3),
            "effect edge must constrain {order:?}"
        );
    }
}

#[test]
fn fused_group_schedule_envelopes_all_external_dependencies() {
    let mut plan = plan_with_epochs(
        5,
        &[(0, 0, &[0, 1, 2, 3, 4])],
        &[
            LoopEdge {
                consumer: 3,
                dependency: 0,
            },
            LoopEdge {
                consumer: 3,
                dependency: 1,
            },
            LoopEdge {
                consumer: 4,
                dependency: 1,
            },
        ],
        &[],
    );
    plan.loops[3].kind = LoopKind::Recursive(9);
    plan.vec_safe_witnesses[3].witness_kind = WitnessKind::SerialStateInternal;
    plan.fused_serial_groups = vec![FusedSerialGroupRecord {
        group_id: 0,
        owner_loop_id: 3,
        member_loop_ids: vec![1, 3],
        state_carrier_signal_ids: vec![3],
        delayed_read_signal_ids: vec![1],
        state_write_signal_ids: vec![3],
        internal_transport_ids: vec![],
        output_or_transport_roots: vec![3],
    }];
    for strategy in ALL_STRATEGIES {
        let order = &schedule_vector_plan(&plan, strategy)
            .expect("fused envelope schedules")
            .epochs[0]
            .loops;
        let position = |loop_id| {
            order
                .iter()
                .position(|candidate| *candidate == loop_id)
                .expect("complete schedule")
        };
        assert!(position(0) < position(1));
        assert!(position(0) < position(3));
        assert!(position(1) < position(3));
        assert!(position(3) < position(4));
        assert_eq!(position(3), position(1) + 1);
    }
}

#[test]
fn epochs_are_emitted_in_rank_order() {
    let plan = plan_with_epochs(2, &[(42, 0, &[0]), (7, 1, &[1])], &[], &[]);
    let schedule = schedule_vector_plan(&plan, SchedulingStrategy::DepthFirst)
        .expect("valid ranked plan schedules");
    assert_eq!(
        schedule
            .epochs
            .iter()
            .map(|epoch| (epoch.rank, epoch.epoch_id))
            .collect::<Vec<_>>(),
        vec![(0, 42), (1, 7)]
    );
}

#[test]
fn cross_epoch_edges_are_local_barriers_only() {
    let plan = plan_with_epochs(
        4,
        &[(9, 0, &[0, 1]), (1, 1, &[2, 3])],
        &[LoopEdge {
            consumer: 2,
            dependency: 0,
        }],
        &[],
    );
    let schedule = schedule_vector_plan(&plan, SchedulingStrategy::BreadthFirst)
        .expect("monotone barrier plan schedules");
    let first = &schedule.epochs[0];
    let second = &schedule.epochs[1];
    let second_dag = EpochDag::new(
        &plan.epochs[1].loops,
        &plan,
        SchedulingStrategy::BreadthFirst,
    )
    .unwrap();
    assert!(second_dag.dependencies(2).is_empty());
    verify_schedule(&second_dag, &second.loops)
        .expect("cross-epoch dependency is not a local DAG edge");
    assert_eq!(first.epoch_id, 9);
    assert_eq!(second.epoch_id, 1);
}

#[test]
fn scheduling_all_strategies_preserves_plan_and_pv_projection_is_schedulable() {
    let (arena, y, z) = build_pv_signals(20);
    let pv_plan = build_pv_plan(&arena, y, z, 16);
    let plan = pv_plan.to_vector_plan();
    let original = plan.clone();
    for strategy in ALL_STRATEGIES {
        schedule_vector_plan(&plan, strategy).expect("PV projection schedules");
        assert_eq!(plan, original);
    }
    assert_eq!(plan.epochs.len(), 1);
    assert_eq!(plan.loops.len(), 2);
}

#[test]
fn invalid_plan_fails_before_scheduling() {
    let mut plan = asymmetric_plan();
    plan.vec_size = 0;
    assert_eq!(
        schedule_vector_plan(&plan, SchedulingStrategy::Special),
        Err(VectorScheduleError::PlanVerification(
            VectorPlanError::VecSizeZero
        ))
    );
}
