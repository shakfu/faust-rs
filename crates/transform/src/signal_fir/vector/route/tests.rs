//! Tests for `vector::route` (relocated from the former inline
//! `mod tests` block; test names unchanged).

use std::collections::BTreeSet;

use crate::schedule::SchedulingStrategy;
use fir::{AccessType, FirBuilder, FirId, FirMatch, FirStore, FirType, match_fir};
use propagate::ClockDomainTable;
use signals::SigBuilder;
use tlib::TreeArena;

use super::*;
use crate::clk_env::annotate;
use crate::signal_fir::decoration_verify::certify_decorations;
use crate::signal_fir::vector::clock_ad::{
    ClockTransportMode, ClockTransportPolicy, ForwardAdPolicy, VECTOR_CLOCK_AD_PLAN_VERSION,
    VectorClockAdPlan, VerifiedVectorClockAdPlan, verified_vector_clock_ad_plan_for_test,
};
use crate::signal_fir::vector::plan::VerifiedVectorPlan;
use crate::signal_fir::vector::plan::{build_vector_plan, verified_vector_plan_for_test};
use crate::signal_fir::vector::verify::{
    EpochRecord, LoopEdge, LoopKind, LoopRecord, Placement, Rate, SignalRecord, TransportRecord,
    ValueType, VecSafeWitness, VectorPlan, Vectorability, WitnessKind,
};
use crate::signal_prepare::prepare_signals_for_fir_verified;

fn pure_shared_plan() -> VerifiedVectorPlan {
    verified_vector_plan_for_test(VectorPlan {
        schema_version: crate::signal_fir::vector::verify::VECTOR_PLAN_SCHEMA_VERSION,
        lockstep_bundles: Vec::new(),
        vec_size: 16,
        signals: vec![
            SignalRecord {
                signal_id: 8,
                value_type: ValueType::Real,
                structural: false,
                rate: Rate::Block,
                vectorability: Vectorability::Vect,
                clock_id: 0,
                effects: vec![],
                direct_effects: vec![],
                placement: Placement::Control,
                duplicable: true,
            },
            SignalRecord {
                signal_id: 9,
                value_type: ValueType::Real,
                structural: false,
                rate: Rate::Samp,
                vectorability: Vectorability::Vect,
                clock_id: 0,
                effects: vec![],
                direct_effects: vec![],
                placement: Placement::Inline,
                duplicable: true,
            },
            SignalRecord {
                signal_id: 10,
                value_type: ValueType::Real,
                structural: false,
                rate: Rate::Samp,
                vectorability: Vectorability::Vect,
                clock_id: 0,
                effects: vec![],
                direct_effects: vec![],
                placement: Placement::Owned(0),
                duplicable: true,
            },
            SignalRecord {
                signal_id: 11,
                value_type: ValueType::Real,
                structural: false,
                rate: Rate::Samp,
                vectorability: Vectorability::Vect,
                clock_id: 0,
                effects: vec![],
                direct_effects: vec![],
                placement: Placement::Owned(1),
                duplicable: true,
            },
        ],
        loops: vec![
            LoopRecord {
                loop_id: 0,
                stable_name: "loop_owns_shared".to_owned(),
                kind: LoopKind::Vectorizable,
                roots: vec![10],
                epoch_id: 0,
            },
            LoopRecord {
                loop_id: 1,
                stable_name: "loop_consumes_shared".to_owned(),
                kind: LoopKind::Vectorizable,
                roots: vec![11],
                epoch_id: 0,
            },
        ],
        epochs: vec![EpochRecord {
            epoch_id: 0,
            rank: 0,
            loops: vec![0, 1],
        }],
        transports: vec![TransportRecord {
            transport_id: 0,
            stable_name: "transport_s10_l0_l1".to_owned(),
            signal_id: 10,
            producer_loop: 0,
            consumer_loop: 1,
            element_type: ValueType::Real,
            length: 16,
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

fn tuple_transport_plan() -> VerifiedVectorPlan {
    let mut plan = pure_shared_plan().into_plan();
    let tuple = nested_tuple_type();
    plan.signals
        .iter_mut()
        .find(|signal| signal.signal_id == 10)
        .unwrap()
        .value_type = tuple.clone();
    plan.transports[0].element_type = tuple;
    verified_vector_plan_for_test(plan)
}

fn tuple_definition_plan() -> VerifiedVectorPlan {
    let mut plan = pure_shared_plan().into_plan();
    plan.signals
        .iter_mut()
        .find(|signal| signal.signal_id == 8)
        .unwrap()
        .value_type = nested_tuple_type();
    verified_vector_plan_for_test(plan)
}

fn nested_tuple_type() -> ValueType {
    ValueType::Tuple(vec![
        ValueType::Real,
        ValueType::Tuple(vec![ValueType::Int, ValueType::Real]),
    ])
}

fn recursive_multi_projection_plan() -> VerifiedVectorPlan {
    let mut arena = TreeArena::new();
    let self_ref = tlib::de_bruijn_ref(&mut arena, 1);
    let (body0, body1) = {
        let mut builder = SigBuilder::new(&mut arena);
        let feedback0 = builder.proj(0, self_ref);
        let feedback1 = builder.proj(1, self_ref);
        (builder.delay1(feedback0), builder.delay1(feedback1))
    };
    let nil = arena.nil();
    let tail = arena.cons(body1, nil);
    let bodies = arena.cons(body0, tail);
    let group = tlib::de_bruijn_rec(&mut arena, bodies);
    let (out0, out1) = {
        let mut builder = SigBuilder::new(&mut arena);
        (builder.proj(0, group), builder.proj(1, group))
    };
    let prepared =
        prepare_signals_for_fir_verified(&arena, &[out0, out1], &ui::UiProgram::empty()).unwrap();
    let clocks = annotate(
        prepared.arena(),
        &ClockDomainTable::new(),
        prepared.outputs(),
    )
    .unwrap();
    let decorations = certify_decorations(&prepared, &clocks).unwrap();
    build_vector_plan(&decorations, 8).unwrap()
}

fn clock_plan_for_mode(
    vector_plan: &VerifiedVectorPlan,
    mode: ClockTransportMode,
) -> VerifiedVectorClockAdPlan {
    verified_vector_clock_ad_plan_for_test(
        VectorClockAdPlan {
            schema_version: VECTOR_CLOCK_AD_PLAN_VERSION,
            vec_size: vector_plan.plan().vec_size,
            clock_islands: vec![],
            transports: vec![ClockTransportPolicy {
                transport_id: 0,
                mode,
            }],
            forward_ad: ForwardAdPolicy::ExpandedSignalGraph,
            reverse_ad_fallbacks: vec![],
        },
        vector_plan,
    )
}

fn value_for_type(value_type: &ValueType, store: &mut FirStore) -> FirId {
    match value_type {
        ValueType::Int => FirBuilder::new(store).int32(0),
        ValueType::Real => FirBuilder::new(store).float32(0.0),
        ValueType::Sound => panic!("route fixture does not synthesize soundfile handles"),
        ValueType::Tuple(components) => {
            let values = components
                .iter()
                .map(|component| value_for_type(component, store))
                .collect::<Vec<_>>();
            let typ = value_fir_type(value_type, FirType::Float32);
            FirBuilder::new(store).value_array(&values, typ)
        }
    }
}

fn value_for(signal: &super::super::verify::SignalRecord, store: &mut FirStore) -> FirId {
    value_for_type(&signal.value_type, store)
}

fn define_complete<'a>(session: &mut VectorRouteSession<'a>, store: &mut FirStore) -> Vec<FirId> {
    let signals = session.plan.signals.clone();
    for signal in &signals {
        if signal.placement == Placement::Control {
            let value = value_for(signal, store);
            session
                .define_control(signal.signal_id, value, store)
                .unwrap();
        }
    }
    let loop_order = session
        .layout()
        .loops()
        .iter()
        .map(|region| region.loop_id)
        .collect::<Vec<_>>();
    let mut stores = Vec::new();
    let mut inline_defined = BTreeSet::new();
    for loop_id in loop_order {
        for signal in &signals {
            let should_define = match signal.placement {
                Placement::Owned(owner) => owner == loop_id,
                Placement::Inline => inline_defined.insert(signal.signal_id),
                Placement::Control => false,
            };
            if should_define {
                let value = value_for(signal, store);
                stores.extend(
                    session
                        .define_in_loop(loop_id, signal.signal_id, value, store)
                        .unwrap(),
                );
            }
        }
    }
    stores
}

#[test]
fn pure_plan_routes_all_preplanned_transports_and_verifies() {
    let verified_plan = pure_shared_plan();
    let routes = verified_plan.plan().transports.clone();
    let mut store = FirStore::new();
    let (mut session, declarations) = VectorRouteSession::new(
        &verified_plan,
        SchedulingStrategy::DepthFirst,
        FirType::Float32,
        &mut store,
    )
    .unwrap();
    assert_eq!(declarations.len(), routes.len());
    let stores = define_complete(&mut session, &mut store);
    assert_eq!(stores.len(), routes.len());
    for transport in &routes {
        assert!(matches!(
            session
                .resolve_in_loop(transport.consumer_loop, transport.signal_id, &mut store)
                .unwrap(),
            RouteResolution::Value(_)
        ));
    }
    let routed = session.finish(&store).unwrap();
    assert_eq!(routed.trace().transports().len(), routes.len());
    assert!(
        routed
            .trace()
            .transports()
            .iter()
            .all(|transport| transport.store.is_some() && transport.load.is_some())
    );
}

#[test]
fn tuple_transport_is_rejected_in_favor_of_scalar_projections() {
    let verified_plan = tuple_transport_plan();
    let mut store = FirStore::new();
    assert!(matches!(
        VectorRouteSession::new(
            &verified_plan,
            SchedulingStrategy::DepthFirst,
            FirType::Float32,
            &mut store,
        ),
        Err(VectorRouteError::UnsupportedTupleTransport { signal_id: 10 })
    ));
}

#[test]
fn p6_clock_policy_rematerializes_scalar_and_held_transports() {
    for (mode, expected_access) in [
        (
            ClockTransportMode::IslandScalar { domain_id: 3 },
            AccessType::Stack,
        ),
        (
            ClockTransportMode::HeldOutput { domain_id: 3 },
            AccessType::Struct,
        ),
    ] {
        let verified_plan = pure_shared_plan();
        let clock_plan = clock_plan_for_mode(&verified_plan, mode);
        let mut store = FirStore::new();
        let (mut session, declarations) = VectorRouteSession::new_with_clock_plan(
            &verified_plan,
            &clock_plan,
            SchedulingStrategy::DepthFirst,
            FirType::Float32,
            &mut store,
        )
        .unwrap();
        assert!(matches!(
            match_fir(&store, declarations[0]),
            FirMatch::DeclareVar {
                typ: FirType::Float32,
                access,
                init: None,
                ..
            } if access == expected_access
        ));
        define_complete(&mut session, &mut store);
        session.resolve_in_loop(1, 10, &mut store).unwrap();
        let routed = session.finish(&store).unwrap();
        let transport = &routed.trace().transports()[0];
        assert_eq!(transport.mode, mode);
        assert!(matches!(
            transport.store.map(|id| match_fir(&store, id)),
            Some(FirMatch::StoreVar { access, .. }) if access == expected_access
        ));
        assert!(matches!(
            transport.load.map(|id| match_fir(&store, id)),
            Some(FirMatch::LoadVar { access, .. }) if access == expected_access
        ));
        verify_routed_fir_with_clock_plan(
            verified_plan.plan(),
            &clock_plan,
            routed.trace(),
            &FirType::Float32,
            &store,
        )
        .unwrap();
    }
}

#[test]
fn tuple_definition_rejects_forged_outer_type_with_wrong_components() {
    let verified_plan = tuple_definition_plan();
    let tuple = verified_plan.plan().signals[0].value_type.clone();
    let tuple_fir = value_fir_type(&tuple, FirType::Float32);
    let mut store = FirStore::new();
    let (mut session, _) = VectorRouteSession::new(
        &verified_plan,
        SchedulingStrategy::DepthFirst,
        FirType::Float32,
        &mut store,
    )
    .unwrap();
    let only_component = FirBuilder::new(&mut store).float32(0.0);
    let forged = FirBuilder::new(&mut store).value_array(&[only_component], tuple_fir);
    assert_eq!(
        session.define_control(8, forged, &store),
        Err(VectorRouteError::TupleValueShape { signal_id: 8 })
    );
}

#[test]
fn production_recursive_tuple_plan_closes_routed_fir() {
    let verified_plan = recursive_multi_projection_plan();
    assert!(
        verified_plan
            .plan()
            .signals
            .iter()
            .any(|signal| matches!(signal.value_type, ValueType::Tuple(_)))
    );

    for strategy in [
        SchedulingStrategy::DepthFirst,
        SchedulingStrategy::BreadthFirst,
        SchedulingStrategy::Special,
        SchedulingStrategy::ReverseBreadthFirst,
    ] {
        let mut store = FirStore::new();
        let (mut session, _) =
            VectorRouteSession::new(&verified_plan, strategy, FirType::Float32, &mut store)
                .unwrap();
        define_complete(&mut session, &mut store);
        for transport in &verified_plan.plan().transports {
            session
                .resolve_in_loop(transport.consumer_loop, transport.signal_id, &mut store)
                .unwrap();
        }
        session.finish(&store).unwrap();
    }
}

#[test]
fn inline_cache_is_loop_local_and_control_cache_is_ancestor_visible() {
    let verified_plan = pure_shared_plan();
    let mut store = FirStore::new();
    let (mut session, _) = VectorRouteSession::new(
        &verified_plan,
        SchedulingStrategy::DepthFirst,
        FirType::Float32,
        &mut store,
    )
    .unwrap();
    let loops = session
        .layout()
        .loops()
        .iter()
        .map(|region| region.loop_id)
        .collect::<Vec<_>>();
    let inline = session
        .plan
        .signals
        .iter()
        .find(|signal| signal.placement == Placement::Inline)
        .unwrap()
        .clone();
    let value = value_for(&inline, &mut store);
    session
        .define_in_loop(loops[0], inline.signal_id, value, &mut store)
        .unwrap();
    assert_eq!(
        session
            .resolve_in_loop(loops[0], inline.signal_id, &mut store)
            .unwrap(),
        RouteResolution::Value(value)
    );
    assert_eq!(
        session
            .resolve_in_loop(loops[1], inline.signal_id, &mut store)
            .unwrap(),
        RouteResolution::NeedsInlineLowering
    );

    let control = session
        .plan
        .signals
        .iter()
        .find(|signal| signal.placement == Placement::Control)
        .unwrap()
        .clone();
    let control_value = value_for(&control, &mut store);
    session
        .define_control(control.signal_id, control_value, &store)
        .unwrap();
    for loop_id in loops {
        assert_eq!(
            session
                .resolve_in_loop(loop_id, control.signal_id, &mut store)
                .unwrap(),
            RouteResolution::Value(control_value)
        );
    }
}

#[test]
fn owned_definition_in_a_sibling_region_is_rejected() {
    let verified_plan = pure_shared_plan();
    let mut store = FirStore::new();
    let (mut session, _) = VectorRouteSession::new(
        &verified_plan,
        SchedulingStrategy::DepthFirst,
        FirType::Float32,
        &mut store,
    )
    .unwrap();
    let owned = session
        .plan
        .signals
        .iter()
        .find_map(|signal| match signal.placement {
            Placement::Owned(owner) => Some((signal.clone(), owner)),
            _ => None,
        })
        .unwrap();
    let sibling = session
        .layout()
        .loops()
        .iter()
        .map(|region| region.loop_id)
        .find(|loop_id| *loop_id != owned.1)
        .unwrap();
    let value = value_for(&owned.0, &mut store);
    assert!(matches!(
        session.define_in_loop(sibling, owned.0.signal_id, value, &mut store),
        Err(VectorRouteError::WrongRegion { .. })
    ));
}

#[test]
fn finish_fails_closed_when_a_planned_transport_has_no_load() {
    let verified_plan = pure_shared_plan();
    let expected = verified_plan.plan().transports[0].transport_id;
    let mut store = FirStore::new();
    let (mut session, _) = VectorRouteSession::new(
        &verified_plan,
        SchedulingStrategy::DepthFirst,
        FirType::Float32,
        &mut store,
    )
    .unwrap();
    define_complete(&mut session, &mut store);
    assert_eq!(
        session.finish(&store),
        Err(VectorRouteError::TransportLoad {
            transport_id: expected
        })
    );
}

#[test]
fn independent_verifier_rejects_a_mutated_transport_load() {
    let verified_plan = pure_shared_plan();
    let expected = verified_plan.plan().transports[0].transport_id;
    let mut store = FirStore::new();
    let (mut session, _) = VectorRouteSession::new(
        &verified_plan,
        SchedulingStrategy::DepthFirst,
        FirType::Float32,
        &mut store,
    )
    .unwrap();
    define_complete(&mut session, &mut store);
    for transport in &verified_plan.plan().transports {
        session
            .resolve_in_loop(transport.consumer_loop, transport.signal_id, &mut store)
            .unwrap();
    }
    let mut trace = session.finish(&store).unwrap().into_trace();
    trace.transports[0].load = Some(FirBuilder::new(&mut store).int32(0));
    assert_eq!(
        verify_routed_fir(verified_plan.plan(), &trace, &FirType::Float32, &store),
        Err(VectorRouteError::TransportLoad {
            transport_id: expected
        })
    );
}

#[test]
fn independent_verifier_rejects_a_mutated_direct_use() {
    let verified_plan = pure_shared_plan();
    let mut store = FirStore::new();
    let (mut session, _) = VectorRouteSession::new(
        &verified_plan,
        SchedulingStrategy::DepthFirst,
        FirType::Float32,
        &mut store,
    )
    .unwrap();
    define_complete(&mut session, &mut store);
    assert!(matches!(
        session.resolve_in_loop(0, 10, &mut store).unwrap(),
        RouteResolution::Value(_)
    ));
    session.resolve_in_loop(1, 10, &mut store).unwrap();
    let mut trace = session.finish(&store).unwrap().into_trace();
    trace.uses[0].value = FirBuilder::new(&mut store).float32(1.0);
    assert_eq!(
        verify_routed_fir(verified_plan.plan(), &trace, &FirType::Float32, &store),
        Err(VectorRouteError::InvalidUse {
            signal_id: 10,
            consumer_loop: 0
        })
    );
}

#[test]
fn all_strategies_change_only_region_order_not_transport_identity() {
    let verified_plan = pure_shared_plan();
    let expected = verified_plan
        .plan()
        .transports
        .iter()
        .map(|transport| {
            (
                transport.transport_id,
                transport.stable_name.clone(),
                transport.signal_id,
            )
        })
        .collect::<Vec<_>>();
    for strategy in [
        SchedulingStrategy::DepthFirst,
        SchedulingStrategy::BreadthFirst,
        SchedulingStrategy::Special,
        SchedulingStrategy::ReverseBreadthFirst,
    ] {
        let mut store = FirStore::new();
        let (session, declarations) =
            VectorRouteSession::new(&verified_plan, strategy, FirType::Float32, &mut store)
                .unwrap();
        assert_eq!(session.layout().strategy(), strategy);
        assert_eq!(declarations.len(), expected.len());
        assert_eq!(
            session
                .plan
                .transports
                .iter()
                .map(|transport| (
                    transport.transport_id,
                    transport.stable_name.clone(),
                    transport.signal_id
                ))
                .collect::<Vec<_>>(),
            expected
        );
    }
}
