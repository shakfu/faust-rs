//! Production P4.4 construction of a strategy-independent vector plan.
//!
//! # C++ provenance and adaptation
//! Placement uses `DAGInstructionsCompiler::needSeparateLoop` from
//! `compiler/generator/compile_vect.cpp` and
//! `compiler/generator/dag_instructions_compiler.cpp`. Unlike the C++ pass,
//! this builder never rediscovers occurrence, delay, clock, recursion, type,
//! or effect facts while lowering. It accepts only an independently checked
//! [`VerifiedDecorationCertificate`], allocates stable loop/transport ids, and
//! then calls the independent [`verify_vector_plan`] trust boundary.
//!
//! The result deliberately contains no scheduling order and this API has no
//! `SchedulingStrategy` parameter. `-ss` is applied later, independently in
//! each fixed epoch. Delayed inter-loop uses contribute ordering edges but no
//! immediate-value transports: this is the Rust counterpart of the C++ delay
//! line loop preceding its readers within a vector chunk. P5 still owns
//! region-aware FIR routing; P6 owns complete clock-domain epochs and
//! delay/recursion storage geometry.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use sigtype::{Nature, Variability, Vectorability as SigVectorability};

use super::decoration_verify::{
    CanonicalSigType, DecorationRecord, DependencyFact, VerifiedDecorationCertificate,
};
use super::loop_graph::{LoopSeparation, SignalLoopProps, needs_separate_loop};
use super::vector_analysis::{DepKind, effect_sets_conflict};
use super::vector_verify::{
    EpochRecord, LoopEdge, LoopKind, LoopRecord, Placement, Rate, SignalRecord, TransportRecord,
    ValueType, VecSafeWitness, VectorPlan, VectorPlanError, Vectorability, WitnessKind,
    effects_duplicable, effects_sample_reorderable, verify_vector_plan,
};

const EFFECT_ISLAND_TAG: u64 = 1 << 63;

/// Opaque evidence that P4.4 constructed and independently verified a plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedVectorPlan {
    plan: VectorPlan,
}

impl VerifiedVectorPlan {
    /// Returns the accepted strategy-independent plan.
    #[must_use]
    pub fn plan(&self) -> &VectorPlan {
        &self.plan
    }

    /// Consumes the evidence wrapper and returns the accepted plan.
    #[must_use]
    pub fn into_plan(self) -> VectorPlan {
        self.plan
    }
}

#[cfg(test)]
pub(crate) fn verified_vector_plan_for_test(plan: VectorPlan) -> VerifiedVectorPlan {
    verify_vector_plan(&plan).expect("test vector plan must satisfy the production verifier");
    VerifiedVectorPlan { plan }
}

/// Why production P4.4 plan construction failed closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VectorPlanBuildError {
    /// Chunk size must be positive.
    VecSizeZero,
    /// A certified dependency unexpectedly names no certified record.
    MissingRecord { signal_id: u32 },
    /// A compute-visible sample signal was not reached by occurrence facts.
    SampleSignalUnplaced { signal_id: u32 },
    /// A table carrier cannot be copied through a numeric chunk transport.
    TableTransport { signal_id: u32 },
    /// The independent plan verifier rejected the constructed DTO.
    Verification(VectorPlanError),
}

impl fmt::Display for VectorPlanBuildError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::VecSizeZero => write!(f, "vector-plan chunk size must be positive"),
            Self::MissingRecord { signal_id } => {
                write!(f, "vector-plan dependency names missing signal {signal_id}")
            }
            Self::SampleSignalUnplaced { signal_id } => {
                write!(f, "sample signal {signal_id} has no vector placement")
            }
            Self::TableTransport { signal_id } => {
                write!(f, "table signal {signal_id} cannot use a chunk transport")
            }
            Self::Verification(error) => write!(f, "constructed vector plan is invalid: {error}"),
        }
    }
}

impl std::error::Error for VectorPlanBuildError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Verification(error) => Some(error),
            _ => None,
        }
    }
}

impl From<VectorPlanError> for VectorPlanBuildError {
    fn from(value: VectorPlanError) -> Self {
        Self::Verification(value)
    }
}

struct PlacementState<'a> {
    records: BTreeMap<u32, &'a DecorationRecord>,
    children: BTreeMap<u32, Vec<u32>>,
    placement: BTreeMap<u32, Placement>,
    contexts: BTreeMap<u32, BTreeSet<u64>>,
    roots_by_loop: BTreeMap<u64, BTreeSet<u64>>,
    visited: BTreeSet<(u32, u64)>,
}

impl<'a> PlacementState<'a> {
    fn visit(&mut self, signal_id: u32, current_loop: u64) -> Result<(), VectorPlanBuildError> {
        let record = self
            .records
            .get(&signal_id)
            .copied()
            .ok_or(VectorPlanBuildError::MissingRecord { signal_id })?;
        if record.variability != Variability::Samp {
            self.placement.insert(signal_id, Placement::Control);
            return Ok(());
        }

        let execution_loop = match self.placement.get(&signal_id).copied() {
            Some(Placement::Owned(owner)) => owner,
            Some(Placement::Inline) => current_loop,
            Some(Placement::Control) => return Ok(()),
            None if effects_duplicable(&record.effects) => {
                self.placement.insert(signal_id, Placement::Inline);
                current_loop
            }
            None => {
                self.placement
                    .insert(signal_id, Placement::Owned(current_loop));
                self.roots_by_loop
                    .entry(current_loop)
                    .or_default()
                    .insert(u64::from(signal_id));
                current_loop
            }
        };
        self.contexts
            .entry(signal_id)
            .or_default()
            .insert(execution_loop);
        if !self.visited.insert((signal_id, execution_loop)) {
            return Ok(());
        }
        let children = self.children.get(&signal_id).cloned().unwrap_or_default();
        for child in children {
            self.visit(child, execution_loop)?;
        }
        Ok(())
    }
}

/// Builds the production P4.4 plan exclusively from accepted P4.3b facts.
///
/// Loop, epoch, transport, and stable-name identities depend only on the
/// certificate and `vec_size`; no scheduling strategy or FIR traversal is
/// consulted. Every non-duplicable sample value is materialized exactly once.
/// Recursive projections of one symbolic group share one serial loop. Other
/// non-`VecSafe` loops use a conservative serial `Island` until P6 supplies a
/// more precise clock/state execution model.
pub fn build_vector_plan(
    verified: &VerifiedDecorationCertificate,
    vec_size: u64,
) -> Result<VerifiedVectorPlan, VectorPlanBuildError> {
    if vec_size == 0 {
        return Err(VectorPlanBuildError::VecSizeZero);
    }
    let certificate = verified.certificate();
    let records = certificate
        .records
        .iter()
        .map(|record| (record.signal_id, record))
        .collect::<BTreeMap<_, _>>();
    let separations = certificate
        .records
        .iter()
        .map(|record| {
            let props = SignalLoopProps {
                variability: record.variability,
                max_delay: record.max_delay as usize,
                is_recursive_proj: record.recursive_projection.is_some(),
                is_shared: record.occurrences.multi,
                is_delay_read: record.is_delay_read,
                is_very_simple: record.very_simple,
            };
            (record.signal_id, needs_separate_loop(&props))
        })
        .collect::<BTreeMap<_, _>>();

    let inline_sample_root = certificate.roots.iter().any(|root| {
        records.get(root).is_some_and(|record| {
            record.variability == Variability::Samp && separations[root] == LoopSeparation::Inline
        })
    });
    let mut next_loop = 0_u64;
    let root_loop = inline_sample_root.then(|| {
        let id = next_loop;
        next_loop += 1;
        id
    });

    let recursion_groups = certificate
        .records
        .iter()
        .filter_map(|record| {
            record
                .recursive_projection
                .map(|projection| projection.group)
        })
        .collect::<BTreeSet<_>>();
    let mut recursion_loop = BTreeMap::new();
    for group in recursion_groups {
        recursion_loop.insert(group, next_loop);
        next_loop += 1;
    }

    let mut owner = BTreeMap::<u32, u64>::new();
    for record in &certificate.records {
        if separations[&record.signal_id] == LoopSeparation::SeparateSerial {
            let group = record
                .recursive_projection
                .expect("serial separation is a recursive projection")
                .group;
            owner.insert(record.signal_id, recursion_loop[&group]);
        }
    }
    for record in &certificate.records {
        if separations[&record.signal_id] == LoopSeparation::SeparateVectorizable {
            owner.insert(record.signal_id, next_loop);
            next_loop += 1;
        }
    }

    let mut children = BTreeMap::<u32, Vec<(u64, u32)>>::new();
    for occurrence in &certificate.occurrence_dependencies {
        children
            .entry(occurrence.from)
            .or_default()
            .push((occurrence.edge_key, occurrence.to));
    }
    let children = children
        .into_iter()
        .map(|(signal, mut edges)| {
            edges.sort_unstable();
            (signal, edges.into_iter().map(|(_, child)| child).collect())
        })
        .collect();
    let mut state = PlacementState {
        records,
        children,
        placement: BTreeMap::new(),
        contexts: BTreeMap::new(),
        roots_by_loop: BTreeMap::new(),
        visited: BTreeSet::new(),
    };
    for record in &certificate.records {
        if record.variability != Variability::Samp {
            state.placement.insert(record.signal_id, Placement::Control);
        }
    }
    for (&signal, &loop_id) in &owner {
        state.placement.insert(signal, Placement::Owned(loop_id));
        state
            .roots_by_loop
            .entry(loop_id)
            .or_default()
            .insert(u64::from(signal));
    }
    for &root in &certificate.roots {
        let record = state
            .records
            .get(&root)
            .copied()
            .ok_or(VectorPlanBuildError::MissingRecord { signal_id: root })?;
        if record.variability != Variability::Samp {
            continue;
        }
        let loop_id = match state.placement.get(&root).copied() {
            Some(Placement::Owned(loop_id)) => loop_id,
            Some(Placement::Inline) | None => {
                // A preceding output traversal may already have visited this
                // pure root inline. Its top-level use still needs a concrete
                // producer in the shared root loop; revisiting it records both
                // execution contexts and therefore the required transport.
                let loop_id = root_loop.expect("an inline sample root allocated the root loop");
                state.placement.insert(root, Placement::Owned(loop_id));
                state
                    .roots_by_loop
                    .entry(loop_id)
                    .or_default()
                    .insert(u64::from(root));
                loop_id
            }
            Some(Placement::Control) => {
                return Err(VectorPlanBuildError::SampleSignalUnplaced { signal_id: root });
            }
        };
        state.visit(root, loop_id)?;
    }
    for record in &certificate.records {
        if record.variability == Variability::Samp
            && !state.placement.contains_key(&record.signal_id)
        {
            return Err(VectorPlanBuildError::SampleSignalUnplaced {
                signal_id: record.signal_id,
            });
        }
    }

    let mut cross_uses = BTreeSet::<(u32, u64, u64)>::new();
    let mut delayed_edges = BTreeSet::<LoopEdge>::new();
    let mut effect_edges = BTreeSet::<LoopEdge>::new();
    for dependency in &certificate.dependencies {
        add_dependency_edges(
            dependency,
            &state,
            &mut cross_uses,
            &mut delayed_edges,
            &mut effect_edges,
        )?;
    }
    let mut data_edges = cross_uses
        .iter()
        .map(|&(_, producer, consumer)| LoopEdge {
            consumer,
            dependency: producer,
        })
        .collect::<BTreeSet<_>>();
    let mut ordering_edges = data_edges.clone();
    ordering_edges.extend(effect_edges.iter().copied());
    for edge in delayed_edges {
        if !reachable(edge.consumer, edge.dependency, &ordering_edges) {
            data_edges.insert(edge);
            ordering_edges.insert(edge);
        }
    }

    let loop_ids = (0..next_loop).collect::<Vec<_>>();
    orient_effect_conflicts(&loop_ids, &state, &data_edges, &mut effect_edges);
    data_edges.retain(|edge| edge.consumer != edge.dependency);
    effect_edges.retain(|edge| edge.consumer != edge.dependency);

    let signals = certificate
        .records
        .iter()
        .map(|record| SignalRecord {
            signal_id: u64::from(record.signal_id),
            value_type: value_type(&record.sig_type),
            rate: rate(record.variability),
            vectorability: vectorability(record.vectorability),
            clock_id: record
                .clock_domain
                .map_or(0, |domain| u64::from(domain) + 1),
            effects: record.effects.clone(),
            placement: state.placement[&record.signal_id],
            duplicable: effects_duplicable(&record.effects),
        })
        .collect::<Vec<_>>();

    let mut loops = Vec::new();
    let mut witnesses = Vec::new();
    for loop_id in &loop_ids {
        let roots = state
            .roots_by_loop
            .get(loop_id)
            .map(|roots| roots.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        let recursive_group = recursion_loop
            .iter()
            .find_map(|(&group, &id)| (id == *loop_id).then_some(group));
        let kind = if let Some(group) = recursive_group {
            LoopKind::Recursive(u64::from(group))
        } else if roots.iter().all(|root| {
            let record = state.records[&u32::try_from(*root).expect("signal id fits u32")];
            record.vectorability == SigVectorability::Vect
                && effects_sample_reorderable(&record.effects)
        }) {
            LoopKind::Vectorizable
        } else {
            let clock = roots.iter().find_map(|root| {
                state.records[&u32::try_from(*root).expect("signal id fits u32")].clock_domain
            });
            LoopKind::Island(clock.map_or(EFFECT_ISLAND_TAG | loop_id, |id| u64::from(id) + 1))
        };
        let stable_name = match kind {
            LoopKind::Vectorizable => format!("loop_vec_{loop_id}"),
            LoopKind::Recursive(group) => format!("loop_rec_{group}"),
            LoopKind::Island(island) => format!("loop_island_{island}"),
        };
        loops.push(LoopRecord {
            loop_id: *loop_id,
            stable_name,
            kind,
            roots,
            epoch_id: 0,
        });
        witnesses.push(VecSafeWitness {
            loop_id: *loop_id,
            witness_kind: match kind {
                LoopKind::Vectorizable => WitnessKind::Pointwise,
                LoopKind::Recursive(_) | LoopKind::Island(_) => WitnessKind::SerialStateInternal,
            },
        });
    }

    let mut transports = Vec::new();
    for (transport_id, &(signal_id, producer_loop, consumer_loop)) in cross_uses.iter().enumerate()
    {
        let record = state.records[&signal_id];
        if matches!(record.sig_type, CanonicalSigType::Table { .. }) {
            return Err(VectorPlanBuildError::TableTransport { signal_id });
        }
        transports.push(TransportRecord {
            transport_id: u64::try_from(transport_id).expect("transport count fits u64"),
            stable_name: format!("transport_s{signal_id}_l{producer_loop}_l{consumer_loop}"),
            signal_id: u64::from(signal_id),
            producer_loop,
            consumer_loop,
            element_type: value_type(&record.sig_type),
            length: vec_size,
        });
    }

    let plan = VectorPlan {
        vec_size,
        signals,
        loops,
        epochs: (!loop_ids.is_empty())
            .then_some(EpochRecord {
                epoch_id: 0,
                rank: 0,
                loops: loop_ids,
            })
            .into_iter()
            .collect(),
        transports,
        data_edges: data_edges.into_iter().collect(),
        effect_edges: effect_edges.into_iter().collect(),
        vec_safe_witnesses: witnesses,
    };
    verify_vector_plan(&plan)?;
    Ok(VerifiedVectorPlan { plan })
}

fn add_dependency_edges(
    dependency: &DependencyFact,
    state: &PlacementState<'_>,
    cross_uses: &mut BTreeSet<(u32, u64, u64)>,
    delayed_edges: &mut BTreeSet<LoopEdge>,
    effect_edges: &mut BTreeSet<LoopEdge>,
) -> Result<(), VectorPlanBuildError> {
    if state.placement.get(&dependency.from) == Some(&Placement::Control) {
        return Ok(());
    }
    let source_contexts =
        state
            .contexts
            .get(&dependency.from)
            .ok_or(VectorPlanBuildError::MissingRecord {
                signal_id: dependency.from,
            })?;
    let target = state.placement.get(&dependency.to).copied().ok_or(
        VectorPlanBuildError::MissingRecord {
            signal_id: dependency.to,
        },
    )?;
    let Placement::Owned(producer) = target else {
        return Ok(());
    };
    for &consumer in source_contexts {
        if consumer == producer {
            continue;
        }
        match dependency.kind {
            DepKind::Immediate | DepKind::Control => {
                cross_uses.insert((dependency.to, producer, consumer));
            }
            DepKind::Effect => {
                effect_edges.insert(LoopEdge {
                    consumer,
                    dependency: producer,
                });
            }
            DepKind::Delayed { .. } => {
                delayed_edges.insert(LoopEdge {
                    consumer,
                    dependency: producer,
                });
            }
            DepKind::ClockBoundary => {}
        }
    }
    Ok(())
}

fn orient_effect_conflicts(
    loops: &[u64],
    state: &PlacementState<'_>,
    data_edges: &BTreeSet<LoopEdge>,
    effect_edges: &mut BTreeSet<LoopEdge>,
) {
    let mut base_edges = data_edges.clone();
    base_edges.extend(effect_edges.iter().copied());
    let order = stable_topological_order(loops, &base_edges);
    let position = order
        .iter()
        .enumerate()
        .map(|(position, &loop_id)| (loop_id, position))
        .collect::<BTreeMap<_, _>>();
    for (index, &left) in loops.iter().enumerate() {
        for &right in &loops[index + 1..] {
            let left_effects = loop_effects(left, state);
            let right_effects = loop_effects(right, state);
            if !effect_sets_conflict(&left_effects, &right_effects)
                || reachable(left, right, &base_edges)
                || reachable(right, left, &base_edges)
            {
                continue;
            }
            let (consumer, dependency) = if position[&left] < position[&right] {
                (right, left)
            } else {
                (left, right)
            };
            let edge = LoopEdge {
                consumer,
                dependency,
            };
            effect_edges.insert(edge);
            base_edges.insert(edge);
        }
    }
}

fn loop_effects(
    loop_id: u64,
    state: &PlacementState<'_>,
) -> Vec<super::vector_analysis::EffectAtom> {
    let mut effects = BTreeSet::new();
    if let Some(roots) = state.roots_by_loop.get(&loop_id) {
        for root in roots {
            let signal = u32::try_from(*root).expect("signal id fits u32");
            effects.extend(state.records[&signal].effects.iter().cloned());
        }
    }
    effects.into_iter().collect()
}

fn stable_topological_order(loops: &[u64], edges: &BTreeSet<LoopEdge>) -> Vec<u64> {
    let mut dependencies = loops
        .iter()
        .map(|&loop_id| (loop_id, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for edge in edges {
        dependencies
            .entry(edge.consumer)
            .or_default()
            .insert(edge.dependency);
    }
    let mut order = Vec::new();
    let mut remaining = loops.iter().copied().collect::<BTreeSet<_>>();
    while let Some(next) = remaining
        .iter()
        .find(|loop_id| dependencies[loop_id].iter().all(|dep| order.contains(dep)))
        .copied()
    {
        remaining.remove(&next);
        order.push(next);
    }
    order.extend(remaining);
    order
}

fn reachable(from: u64, to: u64, edges: &BTreeSet<LoopEdge>) -> bool {
    let mut queue = VecDeque::from([from]);
    let mut seen = BTreeSet::new();
    while let Some(node) = queue.pop_front() {
        if !seen.insert(node) {
            continue;
        }
        for edge in edges.iter().filter(|edge| edge.dependency == node) {
            if edge.consumer == to {
                return true;
            }
            queue.push_back(edge.consumer);
        }
    }
    false
}

fn rate(variability: Variability) -> Rate {
    match variability {
        Variability::Konst => Rate::Konst,
        Variability::Block => Rate::Block,
        Variability::Samp => Rate::Samp,
    }
}

fn vectorability(vectorability: SigVectorability) -> Vectorability {
    match vectorability {
        SigVectorability::Vect => Vectorability::Vect,
        SigVectorability::Scal => Vectorability::Scal,
        SigVectorability::TrueScal => Vectorability::TrueScal,
    }
}

fn value_type(sig_type: &CanonicalSigType) -> ValueType {
    match sig_type {
        CanonicalSigType::Simple { nature, .. } => scalar_value_type(*nature),
        CanonicalSigType::Table { content, .. } => value_type(content),
        CanonicalSigType::Tuplet { components, .. } => {
            ValueType::Tuple(components.iter().map(value_type).collect())
        }
    }
}

fn scalar_value_type(nature: Nature) -> ValueType {
    match nature {
        Nature::Int => ValueType::Int,
        Nature::Real | Nature::Any => ValueType::Real,
    }
}

#[cfg(test)]
mod tests {
    use propagate::ClockDomainTable;
    use signals::SigBuilder;
    use tlib::TreeArena;

    use super::*;
    use crate::clk_env::annotate;
    use crate::schedule::SchedulingStrategy;
    use crate::signal_fir::decoration_verify::certify_decorations;
    use crate::signal_fir::pv_slice::build_pv_signals;
    use crate::signal_fir::vector_schedule::schedule_vector_plan;
    use crate::signal_prepare::prepare_signals_for_fir_verified;

    fn certify(arena: &TreeArena, roots: &[signals::SigId]) -> VerifiedDecorationCertificate {
        let prepared =
            prepare_signals_for_fir_verified(arena, roots, &ui::UiProgram::empty()).unwrap();
        let clocks = annotate(
            prepared.arena(),
            &ClockDomainTable::new(),
            prepared.outputs(),
        )
        .unwrap();
        certify_decorations(&prepared, &clocks).unwrap()
    }

    #[test]
    fn production_pv_plan_uses_certified_delay_and_occurrence_facts() {
        let (arena, y, z) = build_pv_signals(20);
        let decorations = certify(&arena, &[y, z]);
        let verified = build_vector_plan(&decorations, 16).unwrap();
        let plan = verified.plan();

        assert_eq!(plan.signals.len(), decorations.certificate().records.len());
        assert!(plan.loops.len() >= 2);
        let delayed_carrier = decorations
            .certificate()
            .records
            .iter()
            .find(|record| record.max_delay == 20)
            .unwrap();
        assert!(matches!(
            plan.signals
                .iter()
                .find(|signal| signal.signal_id == u64::from(delayed_carrier.signal_id))
                .unwrap()
                .placement,
            Placement::Owned(_)
        ));
        assert!(plan.transports.iter().all(|transport| {
            transport.length == 16
                && plan.data_edges.contains(&LoopEdge {
                    consumer: transport.consumer_loop,
                    dependency: transport.producer_loop,
                })
        }));
    }

    #[test]
    fn delayed_cross_loop_use_orders_chunks_without_an_immediate_transport() {
        let mut arena = TreeArena::new();
        let (input, delayed) = {
            let mut builder = SigBuilder::new(&mut arena);
            let input = builder.input(0);
            let ten = builder.int(10);
            (input, builder.delay(input, ten))
        };
        let decorations = certify(&arena, &[delayed]);
        let plan = build_vector_plan(&decorations, 8).unwrap();
        let signal = |id: signals::SigId| {
            plan.plan()
                .signals
                .iter()
                .find(|signal| signal.signal_id == u64::from(id.as_u32()))
                .unwrap()
        };
        let Placement::Owned(producer) = signal(input).placement else {
            panic!("delayed carrier must own its state loop");
        };
        let Placement::Owned(consumer) = signal(delayed).placement else {
            panic!("delay read must own its reader loop");
        };
        assert!(plan.plan().data_edges.contains(&LoopEdge {
            consumer,
            dependency: producer,
        }));
        assert!(!plan.plan().transports.iter().any(|transport| {
            transport.producer_loop == producer && transport.consumer_loop == consumer
        }));
    }

    #[test]
    fn plan_is_deterministic_and_independent_of_all_scheduling_strategies() {
        let (arena, y, z) = build_pv_signals(20);
        let decorations = certify(&arena, &[y, z]);
        let reference = build_vector_plan(&decorations, 16).unwrap().into_plan();
        assert_eq!(
            build_vector_plan(&decorations, 16).unwrap().into_plan(),
            reference
        );
        for strategy in [
            SchedulingStrategy::DepthFirst,
            SchedulingStrategy::BreadthFirst,
            SchedulingStrategy::Special,
            SchedulingStrategy::ReverseBreadthFirst,
        ] {
            schedule_vector_plan(&reference, strategy).unwrap();
            assert_eq!(
                build_vector_plan(&decorations, 16).unwrap().into_plan(),
                reference
            );
        }
    }

    #[test]
    fn recursive_projections_of_one_group_share_one_serial_loop() {
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
        let decorations = certify(&arena, &[out0, out1]);
        let plan = build_vector_plan(&decorations, 8).unwrap().into_plan();
        let recursive = plan
            .loops
            .iter()
            .filter(|loop_record| matches!(loop_record.kind, LoopKind::Recursive(_)))
            .collect::<Vec<_>>();
        assert_eq!(recursive.len(), 1);
        let group_id = match recursive[0].kind {
            LoopKind::Recursive(group_id) => group_id,
            _ => unreachable!(),
        };
        assert!(
            decorations
                .certificate()
                .records
                .iter()
                .filter_map(|record| record.recursive_projection)
                .all(|projection| u64::from(projection.group) == group_id)
        );
    }

    #[test]
    fn effectful_inline_candidate_is_materialized_exactly_once() {
        let mut arena = TreeArena::new();
        let root = {
            let mut builder = SigBuilder::new(&mut arena);
            let input = builder.input(0);
            builder.output(0, input)
        };
        let decorations = certify(&arena, &[root]);
        let plan = build_vector_plan(&decorations, 8).unwrap().into_plan();
        let output = plan
            .signals
            .iter()
            .find(|signal| signal.signal_id == u64::from(decorations.certificate().roots[0]))
            .unwrap();
        assert!(!output.duplicable);
        let Placement::Owned(owner) = output.placement else {
            panic!("effectful output must be materialized");
        };
        assert!(plan.loops[owner as usize].roots.contains(&output.signal_id));
    }

    #[test]
    fn previously_visited_inline_sample_root_is_promoted_without_panicking() {
        let mut arena = TreeArena::new();
        let (left, right) = {
            let mut builder = SigBuilder::new(&mut arena);
            let input = builder.input(0);
            let half = builder.real(0.5);
            let shared = builder.binop(signals::BinOp::Mul, input, half);
            let one = builder.real(1.0);
            let two = builder.real(2.0);
            (
                builder.binop(signals::BinOp::Add, shared, one),
                builder.binop(signals::BinOp::Mul, shared, two),
            )
        };
        let decorations = certify(&arena, &[left, right]);
        let plan = build_vector_plan(&decorations, 8).unwrap();
        let right_id = u64::from(decorations.certificate().roots[1]);
        assert!(matches!(
            plan.plan()
                .signals
                .iter()
                .find(|signal| signal.signal_id == right_id)
                .unwrap()
                .placement,
            Placement::Owned(_)
        ));
    }

    #[test]
    fn zero_chunk_size_is_rejected_before_plan_construction() {
        let (arena, y, z) = build_pv_signals(20);
        let decorations = certify(&arena, &[y, z]);
        assert_eq!(
            build_vector_plan(&decorations, 0),
            Err(VectorPlanBuildError::VecSizeZero)
        );
    }
}
