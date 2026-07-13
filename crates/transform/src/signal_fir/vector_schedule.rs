//! Strategy-dependent vector execution schedules over [`VectorPlan`].
//!
//! This is the additive P5 scheduling slice from
//! `vector-mode-signal-level-analysis-cpp-port-plan-2026-07-10-en.md`: the
//! strategy-independent plan is verified first, then each epoch's induced DAG
//! is serialized in ascending rank. The R3 plan certificate remains unchanged
//! when the scheduling strategy changes.
//!
//! The implementation delegates every ordering decision to the generic
//! [`crate::schedule::schedule`] adapter, which is the Rust port of the C++
//! `dfschedule`, `bfschedule`, `spschedule`, and `rbschedule` functions in
//! `compiler/DirectedGraph/Schedule.hh`. Same-epoch data and effect edges use
//! the C++ dependency convention `consumer -> dependency`; cross-epoch edges
//! are barriers validated by [`verify_vector_plan`] and are intentionally
//! absent from each local DAG.

use std::fmt;

use crate::schedule::{
    ScheduleDag, ScheduleError, SchedulingStrategy, VerifyError, schedule, verify_schedule,
};

use super::vector_verify::{
    EpochRecord, LoopEdge, VectorPlan, VectorPlanError, verify_vector_plan,
};

/// The selected order for one verified vector-plan epoch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScheduledEpoch {
    /// The strategy-independent epoch identity.
    pub epoch_id: u64,
    /// The fixed execution rank of this epoch.
    pub rank: u64,
    /// Complete topological order of this epoch's loop IDs.
    pub loops: Vec<u64>,
}

/// A strategy-dependent execution schedule over a strategy-independent
/// [`VectorPlan`]. Epoch membership, IDs, transports, and edges remain owned
/// by the input plan; only the per-epoch loop order depends on `strategy`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorExecutionSchedule {
    /// The normalized scheduling policy used to produce `epochs`.
    pub strategy: SchedulingStrategy,
    /// Epoch schedules in ascending rank, independently of epoch IDs.
    pub epochs: Vec<ScheduledEpoch>,
}

/// Why [`schedule_vector_plan`] could not produce a complete execution
/// schedule.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VectorScheduleError {
    /// The strategy-independent plan failed its R3/P5 verification gate.
    PlanVerification(VectorPlanError),
    /// The generic scheduler rejected an induced epoch DAG.
    EpochScheduling {
        /// The epoch whose induced DAG failed.
        epoch_id: u64,
        /// The typed generic scheduler failure.
        source: ScheduleError<u64>,
    },
    /// The independent schedule postcondition checker rejected an order
    /// returned by the generic scheduler.
    Postcondition {
        /// The epoch whose candidate order was rejected.
        epoch_id: u64,
        /// The independently detected soundness/completeness failure.
        source: VerifyError<u64>,
    },
}

impl fmt::Display for VectorScheduleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlanVerification(error) => write!(f, "vector plan verification failed: {error}"),
            Self::EpochScheduling { epoch_id, source } => {
                write!(f, "scheduling epoch {epoch_id} failed: {source}")
            }
            Self::Postcondition { epoch_id, source } => {
                write!(
                    f,
                    "schedule postcondition failed for epoch {epoch_id}: {source}"
                )
            }
        }
    }
}

impl std::error::Error for VectorScheduleError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::PlanVerification(error) => Some(error),
            Self::EpochScheduling { source, .. } => Some(source),
            Self::Postcondition { source, .. } => Some(source),
        }
    }
}

/// Schedules every epoch of `plan` with the selected generic scheduling
/// strategy.
///
/// Verification is deliberately the first operation. Once it succeeds,
/// cross-epoch edges are treated only as already-checked rank barriers and do
/// not enter any epoch DAG. The input is borrowed read-only, so changing
/// `strategy` cannot change the [`VectorPlan`] structurally (R3/P5 invariant).
///
/// # Errors
/// Returns [`VectorScheduleError::PlanVerification`] for the first failed R3
/// plan obligation, or [`VectorScheduleError::EpochScheduling`] with the
/// original generic scheduler error and epoch ID. A scheduler result that
/// fails the independent checker returns [`VectorScheduleError::Postcondition`].
pub fn schedule_vector_plan(
    plan: &VectorPlan,
    strategy: SchedulingStrategy,
) -> Result<VectorExecutionSchedule, VectorScheduleError> {
    verify_vector_plan(plan).map_err(VectorScheduleError::PlanVerification)?;

    let mut epochs: Vec<&EpochRecord> = plan.epochs.iter().collect();
    epochs.sort_unstable_by_key(|epoch| (epoch.rank, epoch.epoch_id));

    let scheduled_epochs = epochs
        .into_iter()
        .map(|epoch| {
            let dag = EpochDag {
                nodes: &epoch.loops,
                data_edges: &plan.data_edges,
                effect_edges: &plan.effect_edges,
            };
            let loops = schedule(strategy, &dag).map_err(|source| {
                VectorScheduleError::EpochScheduling {
                    epoch_id: epoch.epoch_id,
                    source,
                }
            })?;
            verify_schedule(&dag, &loops).map_err(|source| VectorScheduleError::Postcondition {
                epoch_id: epoch.epoch_id,
                source,
            })?;
            Ok(ScheduledEpoch {
                epoch_id: epoch.epoch_id,
                rank: epoch.rank,
                loops,
            })
        })
        .collect::<Result<Vec<_>, VectorScheduleError>>()?;

    Ok(VectorExecutionSchedule {
        strategy,
        epochs: scheduled_epochs,
    })
}

/// `ScheduleDag` view of one epoch. Filtering both endpoints is what keeps
/// cross-epoch barriers out of local scheduling while preserving all
/// same-epoch data and effect constraints.
struct EpochDag<'a> {
    nodes: &'a [u64],
    data_edges: &'a [LoopEdge],
    effect_edges: &'a [LoopEdge],
}

impl ScheduleDag for EpochDag<'_> {
    type Node = u64;

    fn nodes(&self) -> Vec<Self::Node> {
        self.nodes.to_vec()
    }

    fn dependencies(&self, node: Self::Node) -> Vec<Self::Node> {
        let is_member = |id| self.nodes.binary_search(&id).is_ok();
        let mut dependencies: Vec<u64> = self
            .data_edges
            .iter()
            .chain(self.effect_edges)
            .filter(|edge| {
                edge.consumer == node && is_member(edge.consumer) && is_member(edge.dependency)
            })
            .map(|edge| edge.dependency)
            .collect();
        dependencies.sort_unstable();
        dependencies.dedup();
        dependencies
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::schedule::{SchedulingStrategy, verify_schedule};
    use crate::signal_fir::pv_slice::{build_pv_plan, build_pv_signals};
    use crate::signal_fir::vector_verify::{
        LoopKind, LoopRecord, Placement, Rate, SignalRecord, ValueType, VecSafeWitness,
        Vectorability, WitnessKind,
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
            vec_size: 16,
            signals: (0..loop_count)
                .map(|loop_id| SignalRecord {
                    signal_id: loop_id,
                    value_type: ValueType::Real,
                    rate: Rate::Samp,
                    vectorability: Vectorability::Vect,
                    clock_id: 0,
                    effects: Vec::new(),
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

    #[test]
    fn all_strategies_produce_valid_complete_epoch_orders() {
        let plan = asymmetric_plan();
        for strategy in ALL_STRATEGIES {
            let schedule = schedule_vector_plan(&plan, strategy).expect("valid plan schedules");
            let epoch = &schedule.epochs[0];
            let dag = EpochDag {
                nodes: &plan.epochs[0].loops,
                data_edges: &plan.data_edges,
                effect_edges: &plan.effect_edges,
            };
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
        let second_dag = EpochDag {
            nodes: &plan.epochs[1].loops,
            data_edges: &plan.data_edges,
            effect_edges: &plan.effect_edges,
        };
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
}
