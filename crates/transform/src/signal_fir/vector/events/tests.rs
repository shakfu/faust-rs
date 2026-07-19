//! Tests for `vector::events` (relocated from the former inline
//! `mod tests` block; test names unchanged).

use fir::{FirBuilder, FirStore, FirType};
use propagate::ClockDomainTable;
use tlib::TreeArena;

use super::*;
use crate::clk_env::annotate;
use crate::schedule::SchedulingStrategy;
use crate::signal_fir::decoration_verify::{VerifiedDecorationCertificate, certify_decorations};
use crate::signal_fir::pv_slice::build_pv_signals;
use crate::signal_fir::vector::analysis::{
    ForeignResource, ForeignTypeCode, StateCell, StateResource,
};
use crate::signal_fir::vector::plan::{build_vector_plan, verified_vector_plan_for_test};
use crate::signal_fir::vector::route::{RouteResolution, VectorRouteSession};
use crate::signal_fir::vector::state::{
    LoopStatePhases, RecursionProjectionTransition, RecursionTransition, VECTOR_STATE_PLAN_VERSION,
    VectorStateAction, VectorStatePlan, build_vector_state_plan,
    verified_vector_state_plan_for_test,
};
use crate::signal_fir::vector::verify::{
    EpochRecord, IsoLeafMapping, IsoRootWitness, LockstepBundleRecord, LockstepLaneRecord,
    LoopKind, LoopRecord, Placement, Rate, SignalRecord, TransportRecord, ValueType,
    VecSafeWitness, Vectorability, WitnessKind,
};

const ALL_STRATEGIES: [SchedulingStrategy; 4] = [
    SchedulingStrategy::DepthFirst,
    SchedulingStrategy::BreadthFirst,
    SchedulingStrategy::Special,
    SchedulingStrategy::ReverseBreadthFirst,
];

fn certify(arena: &TreeArena, roots: &[signals::SigId]) -> VerifiedDecorationCertificate {
    let prepared = crate::signal_prepare::prepare_signals_for_fir_verified(
        arena,
        roots,
        &ui::UiProgram::empty(),
    )
    .unwrap();
    let clocks = annotate(
        prepared.arena(),
        &ClockDomainTable::new(),
        prepared.outputs(),
    )
    .unwrap();
    certify_decorations(&prepared, &clocks).unwrap()
}

#[test]
fn grouped_effect_dependencies_exhaustively_match_literal_pairing() {
    fn literal_dependencies(
        effects: &[(u64, &EffectAtom)],
        scalar_positions: &BTreeMap<u64, usize>,
    ) -> BTreeSet<EventDependency> {
        let mut result = BTreeSet::new();
        for (index, &(left_id, left)) in effects.iter().enumerate() {
            for &(right_id, right) in &effects[index + 1..] {
                if effects_conflict(left, right) {
                    let (before, after) =
                        if scalar_positions[&left_id] < scalar_positions[&right_id] {
                            (left_id, right_id)
                        } else {
                            (right_id, left_id)
                        };
                    result.insert(EventDependency { before, after });
                }
            }
        }
        result
    }

    let state_a = StateResource::Signal {
        owner: 1,
        cell: StateCell::Delay,
    };
    let state_b = StateResource::Signal {
        owner: 2,
        cell: StateCell::Delay,
    };
    let foreign = |name: &str, purity| EffectAtom::Foreign {
        resource: ForeignResource::Variable {
            name: name.to_owned(),
            value_type: ForeignTypeCode(1),
        },
        purity,
    };
    let atoms = vec![
        EffectAtom::ReadState(state_a.clone()),
        EffectAtom::WriteState(state_a),
        EffectAtom::ReadState(state_b.clone()),
        EffectAtom::WriteState(state_b),
        EffectAtom::ReadTable(3),
        EffectAtom::WriteTable(3),
        EffectAtom::ReadTable(4),
        EffectAtom::WriteTable(4),
        EffectAtom::WriteUi(5),
        EffectAtom::WriteUi(5),
        EffectAtom::WriteUi(6),
        EffectAtom::WriteOutput(7),
        EffectAtom::WriteOutput(7),
        foreign("impure", ForeignPurity::Impure),
        foreign("unknown", ForeignPurity::Unknown),
        foreign("pure", ForeignPurity::Pure),
    ];

    for mask in 0_u64..(1_u64 << atoms.len()) {
        let selected = atoms
            .iter()
            .enumerate()
            .filter(|(index, _)| mask & (1_u64 << index) != 0)
            .map(|(index, effect)| (u64::try_from(index).unwrap(), effect))
            .collect::<Vec<_>>();
        for reverse in [false, true] {
            let mut scalar_effects = selected.clone();
            if reverse {
                scalar_effects.reverse();
            }
            let scalar_positions = scalar_effects
                .iter()
                .enumerate()
                .map(|(position, (event_id, _))| (*event_id, position))
                .collect::<BTreeMap<_, _>>();
            let expected = literal_dependencies(&scalar_effects, &scalar_positions);
            assert_eq!(
                producer_effect_dependencies(&scalar_effects),
                expected,
                "producer mismatch for mask {mask:#x}, reverse={reverse}"
            );

            let mut checker_input = scalar_effects.clone();
            checker_input.sort_unstable_by_key(|(event_id, _)| *event_id);
            assert_eq!(
                checker_required_effect_dependencies(&checker_input, &scalar_positions),
                expected,
                "checker mismatch for mask {mask:#x}, reverse={reverse}"
            );
        }
    }
}

fn pure_transport_plan() -> VerifiedVectorPlan {
    verified_vector_plan_for_test(VectorPlan {
        schema_version: crate::signal_fir::vector::verify::VECTOR_PLAN_SCHEMA_VERSION,
        lockstep_bundles: Vec::new(),
        vec_size: 3,
        signals: vec![
            signal(0, Placement::Owned(0), vec![]),
            signal(1, Placement::Owned(1), vec![]),
        ],
        loops: vec![vector_loop(0, vec![0]), vector_loop(1, vec![1])],
        epochs: vec![EpochRecord {
            epoch_id: 0,
            rank: 0,
            loops: vec![0, 1],
        }],
        transports: vec![TransportRecord {
            transport_id: 0,
            stable_name: "transport_s0_l0_l1".to_owned(),
            signal_id: 0,
            producer_loop: 0,
            consumer_loop: 1,
            element_type: ValueType::Real,
            length: 3,
            layout: crate::signal_fir::vector::verify::TransportLayout::Planar,
        }],
        data_edges: vec![LoopEdge {
            consumer: 1,
            dependency: 0,
        }],
        effect_edges: vec![],
        vec_safe_witnesses: vec![
            VecSafeWitness {
                loop_id: 0,
                witness_kind: WitnessKind::Pointwise,
            },
            VecSafeWitness {
                loop_id: 1,
                witness_kind: WitnessKind::Pointwise,
            },
        ],
        fused_serial_groups: vec![],
    })
}

fn compact_lockstep_plan() -> VerifiedVectorPlan {
    verified_vector_plan_for_test(VectorPlan {
        schema_version: crate::signal_fir::vector::verify::VECTOR_PLAN_SCHEMA_VERSION,
        vec_size: 32,
        signals: (0..2)
            .map(|lane| signal(10 + lane, Placement::Owned(lane), vec![]))
            .collect(),
        loops: (0..2)
            .map(|lane| LoopRecord {
                loop_id: lane,
                stable_name: format!("lockstep_lane_{lane}"),
                kind: LoopKind::Lockstep { width: 2 },
                roots: vec![10 + lane],
                epoch_id: 0,
            })
            .collect(),
        epochs: vec![EpochRecord {
            epoch_id: 0,
            rank: 0,
            loops: vec![0, 1],
        }],
        transports: vec![],
        data_edges: vec![],
        effect_edges: vec![],
        vec_safe_witnesses: (0..2)
            .map(|loop_id| VecSafeWitness {
                loop_id,
                witness_kind: WitnessKind::SerialStateInternal,
            })
            .collect(),
        fused_serial_groups: vec![],
        lockstep_bundles: vec![LockstepBundleRecord {
            bundle_id: 0,
            representative_loop_id: 0,
            member_loop_ids: vec![0, 1],
            lanes: (0..2)
                .map(|lane| LockstepLaneRecord {
                    loop_id: lane,
                    recursion_group: 20 + lane,
                    roots: vec![IsoRootWitness {
                        representative_root: 10,
                        lane_root: 10 + lane,
                        shape_hash: 0x55,
                        leaf_mapping: vec![IsoLeafMapping {
                            representative_signal_id: 10,
                            lane_signal_id: 10 + lane,
                        }],
                    }],
                })
                .collect(),
        }],
    })
}

fn split_state_plan() -> VerifiedVectorPlan {
    let resource = StateResource::Signal {
        owner: 7,
        cell: StateCell::Delay,
    };
    split_effect_plan(
        EffectAtom::WriteState(resource.clone()),
        EffectAtom::ReadState(resource),
    )
}

fn split_effect_plan(left: EffectAtom, right: EffectAtom) -> VerifiedVectorPlan {
    split_plan_with_signals(vec![
        signal(0, Placement::Owned(0), vec![left]),
        signal(1, Placement::Owned(1), vec![right]),
    ])
}

fn split_plan_with_signals(signals: Vec<SignalRecord>) -> VerifiedVectorPlan {
    verified_vector_plan_for_test(VectorPlan {
        schema_version: crate::signal_fir::vector::verify::VECTOR_PLAN_SCHEMA_VERSION,
        lockstep_bundles: Vec::new(),
        vec_size: 3,
        signals,
        loops: vec![serial_loop(0, vec![0]), serial_loop(1, vec![1])],
        epochs: vec![EpochRecord {
            epoch_id: 0,
            rank: 0,
            loops: vec![0, 1],
        }],
        transports: vec![],
        data_edges: vec![],
        effect_edges: vec![LoopEdge {
            consumer: 1,
            dependency: 0,
        }],
        vec_safe_witnesses: vec![
            VecSafeWitness {
                loop_id: 0,
                witness_kind: WitnessKind::SerialStateInternal,
            },
            VecSafeWitness {
                loop_id: 1,
                witness_kind: WitnessKind::SerialStateInternal,
            },
        ],
        fused_serial_groups: vec![],
    })
}

fn colocated_state_plan() -> VerifiedVectorPlan {
    let resource = StateResource::Signal {
        owner: 7,
        cell: StateCell::Delay,
    };
    verified_vector_plan_for_test(VectorPlan {
        schema_version: crate::signal_fir::vector::verify::VECTOR_PLAN_SCHEMA_VERSION,
        lockstep_bundles: Vec::new(),
        vec_size: 3,
        signals: vec![
            signal(
                0,
                Placement::Owned(0),
                vec![EffectAtom::WriteState(resource.clone())],
            ),
            signal(
                1,
                Placement::Owned(0),
                vec![EffectAtom::ReadState(resource)],
            ),
        ],
        loops: vec![serial_loop(0, vec![0, 1])],
        epochs: vec![EpochRecord {
            epoch_id: 0,
            rank: 0,
            loops: vec![0],
        }],
        transports: vec![],
        data_edges: vec![],
        effect_edges: vec![],
        vec_safe_witnesses: vec![VecSafeWitness {
            loop_id: 0,
            witness_kind: WitnessKind::SerialStateInternal,
        }],
        fused_serial_groups: vec![],
    })
}

fn recursive_event_plan() -> (VerifiedVectorPlan, VerifiedVectorStatePlan) {
    let projection0 = StateResource::Recursion {
        group: 7,
        projection: 0,
    };
    let projection1 = StateResource::Recursion {
        group: 7,
        projection: 1,
    };
    let plan = verified_vector_plan_for_test(VectorPlan {
        schema_version: crate::signal_fir::vector::verify::VECTOR_PLAN_SCHEMA_VERSION,
        lockstep_bundles: Vec::new(),
        vec_size: 3,
        signals: vec![
            signal(
                0,
                Placement::Owned(0),
                vec![
                    EffectAtom::ReadState(projection0.clone()),
                    EffectAtom::WriteState(projection0),
                ],
            ),
            signal(
                1,
                Placement::Owned(0),
                vec![
                    EffectAtom::ReadState(projection1.clone()),
                    EffectAtom::WriteState(projection1),
                ],
            ),
        ],
        loops: vec![LoopRecord {
            loop_id: 0,
            stable_name: "loop_rec_7".to_owned(),
            kind: LoopKind::Recursive(7),
            roots: vec![0, 1],
            epoch_id: 0,
        }],
        epochs: vec![EpochRecord {
            epoch_id: 0,
            rank: 0,
            loops: vec![0],
        }],
        transports: vec![],
        data_edges: vec![],
        effect_edges: vec![],
        vec_safe_witnesses: vec![VecSafeWitness {
            loop_id: 0,
            witness_kind: WitnessKind::SerialStateInternal,
        }],
        fused_serial_groups: vec![],
    });
    let state = verified_vector_state_plan_for_test(
        VectorStatePlan {
            schema_version: VECTOR_STATE_PLAN_VERSION,
            vec_size: 3,
            max_copy_delay: 16,
            loops: vec![LoopStatePhases {
                loop_id: 0,
                pre: vec![],
                exec: vec![VectorStateAction::RecursionStep { group: 7 }],
                post: vec![],
            }],
            delays: vec![],
            recursions: vec![RecursionTransition {
                group: 7,
                loop_id: 0,
                projections: vec![
                    RecursionProjectionTransition {
                        index: 0,
                        signal_ids: vec![0],
                        value_signal_id: 0,
                    },
                    RecursionProjectionTransition {
                        index: 1,
                        signal_ids: vec![1],
                        value_signal_id: 1,
                    },
                ],
            }],
            lockstep_register_bundles: vec![],
            prefixes: vec![],
            waveforms: vec![],
            no_op_resources: vec![],
        },
        &plan,
    );
    (plan, state)
}

fn empty_state_plan(plan: &VerifiedVectorPlan) -> VerifiedVectorStatePlan {
    verified_vector_state_plan_for_test(
        VectorStatePlan {
            schema_version: VECTOR_STATE_PLAN_VERSION,
            vec_size: plan.plan().vec_size,
            max_copy_delay: 16,
            loops: vec![],
            delays: vec![],
            recursions: vec![],
            lockstep_register_bundles: vec![],
            prefixes: vec![],
            waveforms: vec![],
            no_op_resources: vec![],
        },
        plan,
    )
}

fn compact_lockstep_state_plan(plan: &VerifiedVectorPlan) -> VerifiedVectorStatePlan {
    verified_vector_state_plan_for_test(
        VectorStatePlan {
            schema_version: VECTOR_STATE_PLAN_VERSION,
            vec_size: plan.plan().vec_size,
            max_copy_delay: 16,
            loops: (0..2)
                .map(|lane| LoopStatePhases {
                    loop_id: lane,
                    pre: vec![],
                    exec: vec![VectorStateAction::RecursionStep { group: 20 + lane }],
                    post: vec![],
                })
                .collect(),
            delays: vec![],
            recursions: (0..2)
                .map(|lane| RecursionTransition {
                    group: 20 + lane,
                    loop_id: lane,
                    projections: vec![RecursionProjectionTransition {
                        index: 0,
                        signal_ids: vec![10 + lane],
                        value_signal_id: 10 + lane,
                    }],
                })
                .collect(),
            lockstep_register_bundles: vec![],
            prefixes: vec![],
            waveforms: vec![],
            no_op_resources: vec![],
        },
        plan,
    )
}

fn signal(signal_id: u64, placement: Placement, effects: Vec<EffectAtom>) -> SignalRecord {
    SignalRecord {
        signal_id,
        value_type: ValueType::Real,
        structural: false,
        rate: Rate::Samp,
        vectorability: if effects.is_empty() {
            Vectorability::Vect
        } else {
            Vectorability::Scal
        },
        clock_id: 0,
        duplicable: effects.is_empty(),
        direct_effects: effects.clone(),
        effects,
        placement,
    }
}

fn vector_loop(loop_id: u64, roots: Vec<u64>) -> LoopRecord {
    LoopRecord {
        loop_id,
        stable_name: format!("loop_{loop_id}"),
        kind: LoopKind::Vectorizable,
        roots,
        epoch_id: 0,
    }
}

#[test]
fn event_bound_precheck_accepts_a_small_plan_without_certifying_it() {
    let plan = pure_transport_plan();
    let state = empty_state_plan(&plan);
    assert_eq!(
        precheck_state_event_bound(&plan, &state, EventLimits::uniform(14)),
        Ok(())
    );
}

#[test]
fn event_bound_precheck_rejects_when_even_the_compact_basis_is_too_large() {
    let plan = pure_transport_plan();
    let state = empty_state_plan(&plan);
    assert_eq!(
        precheck_state_event_bound(&plan, &state, EventLimits::uniform(9)),
        Err(VectorEventError::EventLowerBoundExceeded {
            minimum: 10,
            limit: 9,
        })
    );
}

fn serial_loop(loop_id: u64, roots: Vec<u64>) -> LoopRecord {
    LoopRecord {
        loop_id,
        stable_name: format!("serial_{loop_id}"),
        kind: LoopKind::Island(loop_id),
        roots,
        epoch_id: 0,
    }
}

fn route(
    plan: &VerifiedVectorPlan,
    strategy: SchedulingStrategy,
    with_transport_use: bool,
) -> VerifiedRoutedFir {
    let mut store = FirStore::new();
    let (mut session, _) =
        VectorRouteSession::new(plan, strategy, FirType::Float32, &mut store).unwrap();
    let loop_order = session
        .layout()
        .loops()
        .iter()
        .map(|region| region.loop_id)
        .collect::<Vec<_>>();
    for loop_id in loop_order {
        let roots = session
            .plan()
            .loops
            .iter()
            .find(|record| record.loop_id == loop_id)
            .unwrap()
            .roots
            .clone();
        if with_transport_use && loop_id == 1 {
            assert!(matches!(
                session.resolve_in_loop(1, 0, &mut store).unwrap(),
                RouteResolution::Value(_)
            ));
        }
        for signal_id in roots {
            let value = FirBuilder::new(&mut store).float32(signal_id as f32);
            session
                .define_in_loop(loop_id, signal_id, value, &mut store)
                .unwrap();
        }
    }
    session.finish(&store).unwrap()
}

fn route_all_transports(
    plan: &VerifiedVectorPlan,
    strategy: SchedulingStrategy,
) -> VerifiedRoutedFir {
    let mut store = FirStore::new();
    let (mut session, _) =
        VectorRouteSession::new(plan, strategy, FirType::Float32, &mut store).unwrap();
    let signals = session.plan().signals.clone();
    for signal in signals
        .iter()
        .filter(|signal| signal.placement == Placement::Control)
    {
        let value = routed_test_value(signal, &mut store);
        session
            .define_control(signal.signal_id, value, &store)
            .unwrap();
    }
    let loop_order = session
        .layout()
        .loops()
        .iter()
        .map(|region| region.loop_id)
        .collect::<Vec<_>>();
    let mut inline_defined = BTreeSet::new();
    for loop_id in loop_order {
        let incoming = session
            .plan()
            .transports
            .iter()
            .filter(|transport| transport.consumer_loop == loop_id)
            .map(|transport| transport.signal_id)
            .collect::<Vec<_>>();
        for signal_id in incoming {
            session
                .resolve_in_loop(loop_id, signal_id, &mut store)
                .unwrap();
        }
        for signal in &signals {
            let should_define = match signal.placement {
                Placement::Owned(owner) => owner == loop_id,
                Placement::Inline => inline_defined.insert(signal.signal_id),
                Placement::Control => false,
            };
            if !should_define {
                continue;
            }
            let value = routed_test_value(signal, &mut store);
            session
                .define_in_loop(loop_id, signal.signal_id, value, &mut store)
                .unwrap();
        }
    }
    session.finish(&store).unwrap()
}

fn routed_test_value(signal: &SignalRecord, store: &mut FirStore) -> fir::FirId {
    let mut builder = FirBuilder::new(store);
    match signal.value_type {
        ValueType::Int => builder.int32(0),
        ValueType::Real => builder.float32(0.0),
        ValueType::Sound | ValueType::Tuple(_) => {
            panic!("event route fixture does not synthesize handle or tuple FIR")
        }
    }
}

#[test]
fn pure_transport_is_fission_safe_for_all_scheduling_strategies() {
    let plan = pure_transport_plan();
    for strategy in ALL_STRATEGIES {
        let routed = route(&plan, strategy, true);
        let verified = build_event_order_certificate(&plan, &routed, DEFAULT_EVENT_LIMITS).unwrap();
        let certificate = verified.certificate();
        assert_eq!(certificate.sample_count(), 3);
        assert_eq!(certificate.events().len(), 17);
        assert_ne!(certificate.scalar_order(), certificate.vector_order());
        assert!(certificate.events().iter().any(|event| matches!(
            event.kind,
            VectorEventKind::TransportStore { transport_id: 0 }
        )));
        assert!(certificate.events().iter().any(|event| matches!(
            event.kind,
            VectorEventKind::TransportLoad { transport_id: 0 }
        )));
    }
}

#[test]
fn independent_checker_rejects_order_and_dependency_mutations() {
    let plan = pure_transport_plan();
    let routed = route(&plan, SchedulingStrategy::DepthFirst, true);
    let verified = build_event_order_certificate(&plan, &routed, DEFAULT_EVENT_LIMITS).unwrap();

    let mut order_mutation = verified.certificate().clone();
    order_mutation.vector_order.swap(2, 3);
    assert_eq!(
        verify_event_order_certificate(plan.plan(), &routed, &order_mutation, DEFAULT_EVENT_LIMITS),
        Err(VectorEventError::VectorOrderMismatch)
    );

    let mut dependency_mutation = verified.into_certificate();
    dependency_mutation.dependencies.pop();
    assert_eq!(
        verify_event_order_certificate(
            plan.plan(),
            &routed,
            &dependency_mutation,
            DEFAULT_EVENT_LIMITS
        ),
        Err(VectorEventError::DependencyMismatch)
    );
}

#[test]
fn general_routed_plan_uses_two_sample_basis_only_when_complete_chunk_exceeds_bound() {
    let plan = pure_transport_plan();
    let routed = route(&plan, SchedulingStrategy::DepthFirst, true);
    let complete = build_event_order_certificate(&plan, &routed, EventLimits::uniform(17))
        .expect("complete routed certificate");
    assert_eq!(complete.certificate().checked_sample_count(), 3);
    assert!(!complete.certificate().is_compact());

    let compact = build_event_order_certificate(&plan, &routed, EventLimits::uniform(16))
        .expect("compact routed certificate");
    assert_eq!(compact.certificate().sample_count(), 3);
    assert_eq!(compact.certificate().checked_sample_count(), 2);
    assert!(compact.certificate().is_compact());

    let split_budget = build_event_order_certificate(&plan, &routed, EventLimits::new(11, 12))
        .expect("separate compact budget");
    assert!(split_budget.certificate().is_compact());

    assert_eq!(
        build_event_order_certificate(&plan, &routed, EventLimits::uniform(11)),
        Err(VectorEventError::EventBoundExceeded {
            needed: 12,
            limit: 11,
        })
    );
}

#[test]
fn expanded_and_compact_general_routed_models_have_the_same_two_sample_projection() {
    fn retained(event: &VectorEvent) -> bool {
        event.sample.is_none_or(|sample| sample < 2)
    }

    fn event_key(event: &VectorEvent) -> EventKey {
        (event.region, event.sample, event.kind.clone())
    }

    fn projected_order(certificate: &EventOrderCertificate, order: &[u64]) -> Vec<EventKey> {
        let by_id = certificate
            .events
            .iter()
            .map(|event| (event.event_id, event))
            .collect::<BTreeMap<_, _>>();
        order
            .iter()
            .map(|event_id| by_id[event_id])
            .filter(|event| retained(event))
            .map(event_key)
            .collect()
    }

    fn projected_dependencies(
        certificate: &EventOrderCertificate,
    ) -> BTreeSet<(EventKey, EventKey)> {
        let by_id = certificate
            .events
            .iter()
            .map(|event| (event.event_id, event))
            .collect::<BTreeMap<_, _>>();
        certificate
            .dependencies
            .iter()
            .filter_map(|dependency| {
                let before = by_id[&dependency.before];
                let after = by_id[&dependency.after];
                (retained(before) && retained(after)).then(|| (event_key(before), event_key(after)))
            })
            .collect()
    }

    let plan = pure_transport_plan();
    let routed = route(&plan, SchedulingStrategy::DepthFirst, true);
    let expanded = build_event_order_certificate(&plan, &routed, EventLimits::uniform(17))
        .expect("expanded certificate")
        .into_certificate();
    let compact = build_event_order_certificate(&plan, &routed, EventLimits::uniform(16))
        .expect("compact certificate")
        .into_certificate();

    assert_eq!(
        expanded
            .events
            .iter()
            .filter(|event| retained(event))
            .map(event_key)
            .collect::<Vec<_>>(),
        compact.events.iter().map(event_key).collect::<Vec<_>>()
    );
    assert_eq!(
        projected_order(&expanded, &expanded.scalar_order),
        projected_order(&compact, &compact.scalar_order)
    );
    assert_eq!(
        projected_order(&expanded, &expanded.vector_order),
        projected_order(&compact, &compact.vector_order)
    );
    assert_eq!(
        projected_dependencies(&expanded),
        projected_dependencies(&compact)
    );
}

#[test]
fn lockstep_uses_two_sample_compact_basis_when_full_chunk_exceeds_bound() {
    let plan = compact_lockstep_plan();
    let routed = route(&plan, SchedulingStrategy::DepthFirst, false);
    let verified = build_event_order_certificate(&plan, &routed, EventLimits::uniform(10))
        .expect("compact lockstep certificate");
    let certificate = verified.certificate();
    assert_eq!(certificate.sample_count(), 32);
    assert_eq!(certificate.checked_sample_count(), 2);
    assert!(certificate.is_compact());
    assert_eq!(certificate.events().len(), 6);
}

#[test]
fn compact_checker_rejects_sample_basis_and_template_mutations() {
    let plan = compact_lockstep_plan();
    let routed = route(&plan, SchedulingStrategy::DepthFirst, false);
    let verified = build_event_order_certificate(&plan, &routed, EventLimits::uniform(10))
        .expect("compact lockstep certificate");

    let mut basis_mutation = verified.certificate().clone();
    basis_mutation.checked_sample_count = 1;
    assert_eq!(
        verify_event_order_certificate(
            plan.plan(),
            &routed,
            &basis_mutation,
            EventLimits::uniform(10)
        ),
        Err(VectorEventError::CompactRepetitionMismatch)
    );

    let mut template_mutation = verified.into_certificate();
    let sample_one = template_mutation
        .events
        .iter_mut()
        .find(|event| event.sample == Some(1))
        .expect("sample-one template");
    sample_one.kind = VectorEventKind::Definition { signal_id: 999 };
    assert_eq!(
        verify_event_order_certificate(
            plan.plan(),
            &routed,
            &template_mutation,
            EventLimits::uniform(10)
        ),
        Err(VectorEventError::EventTableMismatch)
    );
}

#[test]
fn compact_checker_rejects_a_missing_carried_recursion_edge() {
    let plan = compact_lockstep_plan();
    let state = compact_lockstep_state_plan(&plan);
    let routed = route(&plan, SchedulingStrategy::DepthFirst, false);
    let verified =
        build_state_event_order_certificate(&plan, &routed, &state, EventLimits::uniform(12))
            .expect("compact state certificate");
    assert!(verified.certificate().is_compact());

    let mut mutation = verified.into_certificate();
    let recursion_steps = recursion_step_events(&mutation.events);
    let before = recursion_steps[&(0, 0, 20)];
    let after = recursion_steps[&(0, 1, 20)];
    mutation
        .dependencies
        .retain(|edge| *edge != EventDependency { before, after });
    assert_eq!(
        verify_state_event_order_certificate(
            plan.plan(),
            &routed,
            &state,
            &mutation,
            EventLimits::uniform(12)
        ),
        Err(VectorEventError::DependencyMismatch)
    );
}

#[test]
fn general_recursive_plan_compacts_and_keeps_the_carried_state_boundary() {
    let (plan, state) = recursive_event_plan();
    let routed = route(&plan, SchedulingStrategy::DepthFirst, false);
    let verified =
        build_state_event_order_certificate(&plan, &routed, &state, EventLimits::new(0, 64))
            .expect("compact general recursion certificate");
    assert!(verified.certificate().is_compact());

    let mut mutation = verified.into_certificate();
    let recursion_steps = recursion_step_events(&mutation.events);
    let before = recursion_steps[&(0, 0, 7)];
    let after = recursion_steps[&(0, 1, 7)];
    mutation
        .dependencies
        .retain(|edge| *edge != EventDependency { before, after });
    assert_eq!(
        verify_state_event_order_certificate(
            plan.plan(),
            &routed,
            &state,
            &mutation,
            EventLimits::new(0, 64)
        ),
        Err(VectorEventError::DependencyMismatch)
    );
}

#[test]
fn cross_loop_carried_state_is_rejected_despite_an_effect_edge() {
    let plan = split_state_plan();
    let routed = route(&plan, SchedulingStrategy::DepthFirst, false);
    assert!(matches!(
        build_event_order_certificate(&plan, &routed, DEFAULT_EVENT_LIMITS),
        Err(VectorEventError::FissionSafeViolation { .. })
    ));
}

#[test]
fn conflicting_observable_effects_in_separate_loops_are_rejected() {
    let plan = split_effect_plan(EffectAtom::WriteOutput(0), EffectAtom::WriteOutput(0));
    let routed = route(&plan, SchedulingStrategy::DepthFirst, false);
    assert!(matches!(
        build_event_order_certificate(&plan, &routed, DEFAULT_EVENT_LIMITS),
        Err(VectorEventError::FissionSafeViolation { .. })
    ));
}

#[test]
fn a_transitive_carrier_of_an_effect_manufactures_no_conflict() {
    // Same two-loop shape as the rejected case above, but the second
    // signal only contains the performer in its subtree: its transitive
    // set holds the atom while its direct set is empty. Attributing the
    // operation to the carrier as well is what made `mixer` report a
    // write-after-write on a UI zone the compiler stores once.
    let atom = EffectAtom::WriteOutput(0);
    let carrier = SignalRecord {
        signal_id: 1,
        value_type: ValueType::Real,
        structural: false,
        rate: Rate::Samp,
        vectorability: Vectorability::Scal,
        clock_id: 0,
        duplicable: false,
        direct_effects: Vec::new(),
        effects: vec![atom.clone()],
        placement: Placement::Owned(1),
    };
    let plan = split_plan_with_signals(vec![
        signal(0, Placement::Owned(0), vec![atom.clone()]),
        carrier,
    ]);
    let routed = route(&plan, SchedulingStrategy::DepthFirst, false);
    let verified = build_event_order_certificate(&plan, &routed, DEFAULT_EVENT_LIMITS)
        .expect("a carrier must not conflict with the performer it inherits from");
    let performers = verified
        .certificate()
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            VectorEventKind::Effect {
                signal_id, effect, ..
            } if *effect == atom => Some(*signal_id),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        performers,
        BTreeSet::from([0]),
        "only the performing signal may emit the effect event"
    );
}

#[test]
fn conflicting_state_colocated_in_one_serial_loop_is_accepted() {
    let plan = colocated_state_plan();
    let routed = route(&plan, SchedulingStrategy::DepthFirst, false);
    build_event_order_certificate(&plan, &routed, DEFAULT_EVENT_LIMITS).unwrap();
}

#[test]
fn p6_delay_phases_refine_managed_effects_for_all_strategies() {
    let (arena, y, z) = build_pv_signals(20);
    let decorations = certify(&arena, &[y, z]);
    let plan = build_vector_plan(&decorations, 3).unwrap();
    let state = build_vector_state_plan(&decorations, &plan, 16).unwrap();
    let delayed_signal = state
        .plan()
        .delays
        .iter()
        .find(|delay| delay.max_delay == 20)
        .unwrap()
        .signal_id;

    for strategy in ALL_STRATEGIES {
        let routed = route_all_transports(&plan, strategy);
        let verified =
            build_state_event_order_certificate(&plan, &routed, &state, DEFAULT_EVENT_LIMITS)
                .unwrap();
        let certificate = verified.certificate();
        assert!(certificate.events().iter().any(|event| matches!(
            event.kind,
            VectorEventKind::StateTransition {
                action: VectorStateAction::DelayRingAdvance { signal_id }
            } if signal_id == delayed_signal
        )));
        assert!(certificate.events().iter().any(|event| matches!(
            event.kind,
            VectorEventKind::StateTransition {
                action: VectorStateAction::DelayRingSaveAdvance { signal_id }
            } if signal_id == delayed_signal
        )));
        assert!(!certificate.events().iter().any(|event| matches!(
            &event.kind,
            VectorEventKind::Effect {
                effect: EffectAtom::ReadState(StateResource::Signal {
                    owner,
                    cell: StateCell::Delay,
                }) | EffectAtom::WriteState(StateResource::Signal {
                    owner,
                    cell: StateCell::Delay,
                }),
                ..
            } if u64::from(*owner) == delayed_signal
        )));
    }
}

#[test]
fn p6_recursion_steps_are_sample_ordered_for_all_strategies() {
    let (plan, state) = recursive_event_plan();
    let group = state.plan().recursions[0].group;

    for strategy in ALL_STRATEGIES {
        let routed = route_all_transports(&plan, strategy);
        let verified =
            build_state_event_order_certificate(&plan, &routed, &state, DEFAULT_EVENT_LIMITS)
                .unwrap();
        let certificate = verified.certificate();
        let mut steps = certificate
            .events()
            .iter()
            .filter(|event| {
                matches!(
                    event.kind,
                    VectorEventKind::StateTransition {
                        action: VectorStateAction::RecursionStep { group: event_group }
                    } if event_group == group
                )
            })
            .map(|event| (event.sample.unwrap(), event.event_id))
            .collect::<Vec<_>>();
        steps.sort_unstable();
        assert_eq!(steps.len(), 3);
        for pair in steps.windows(2) {
            assert!(certificate.dependencies().contains(&EventDependency {
                before: pair[0].1,
                after: pair[1].1,
            }));
        }
    }
}
