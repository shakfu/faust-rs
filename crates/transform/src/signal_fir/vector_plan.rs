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
use super::vector_analysis::{DepKind, EffectAtom, ForeignPurity, StateResource};
use super::vector_verify::{
    EpochRecord, FusedSerialGroupRecord, LoopEdge, LoopKind, LoopRecord, Placement, Rate,
    SignalRecord, TransportRecord, ValueType, VecSafeWitness, VectorPlan, VectorPlanError,
    Vectorability, WitnessKind, effects_duplicable, effects_sample_reorderable,
    verify_fused_serial_groups_after_plan, verify_vector_plan,
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
    /// A possible zero-delay state read crosses loops without one serial
    /// execution envelope.
    UnfusedImmediateDelayCrossing { producer: u64, consumer: u64 },
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
            Self::UnfusedImmediateDelayCrossing { producer, consumer } => write!(
                f,
                "state-mediated immediate delay crosses loop {producer} -> {consumer} without a fused serial group"
            ),
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
    delayed_values: BTreeSet<u32>,
    delayed_pairs: BTreeSet<(u32, u32)>,
    structural_carriers: BTreeSet<u32>,
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
        if self.structural_carriers.contains(&signal_id) {
            self.placement.insert(signal_id, Placement::Inline);
            return Ok(());
        }
        if !requires_sample_execution(
            record,
            self.delayed_values.contains(&signal_id),
            self.structural_carriers.contains(&signal_id),
        ) {
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
    let timing_enabled = std::env::var_os("FAUST_RS_VECTOR_TIMING").is_some();
    let mut stage_started = std::time::Instant::now();
    let mut trace_stage = |stage: &str| {
        if timing_enabled {
            eprintln!(
                "[vector-plan-stage] {stage}: {:.3}s",
                stage_started.elapsed().as_secs_f64()
            );
        }
        stage_started = std::time::Instant::now();
    };
    if vec_size == 0 {
        return Err(VectorPlanBuildError::VecSizeZero);
    }
    let certificate = verified.certificate();
    let delayed_values = certificate
        .occurrence_dependencies
        .iter()
        .filter_map(|dependency| (dependency.delay > 0).then_some(dependency.to))
        .collect::<BTreeSet<_>>();
    let delayed_pairs = certificate
        .occurrence_dependencies
        .iter()
        .filter_map(|dependency| (dependency.delay > 0).then_some((dependency.from, dependency.to)))
        .collect::<BTreeSet<_>>();
    let records = certificate
        .records
        .iter()
        .map(|record| (record.signal_id, record))
        .collect::<BTreeMap<_, _>>();
    let recursion_groups = certificate
        .records
        .iter()
        .filter_map(|record| {
            record
                .recursive_projection
                .map(|projection| projection.group)
        })
        .collect::<BTreeSet<_>>();
    let structural_carriers = certificate
        .records
        .iter()
        .filter(|record| record.is_symbolic_recursion_carrier)
        .map(|record| record.signal_id)
        .collect::<BTreeSet<_>>();
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
    trace_stage("records-and-separations");

    let inline_sample_root = certificate.roots.iter().any(|root| {
        records.get(root).is_some_and(|record| {
            requires_sample_execution(
                record,
                delayed_values.contains(root),
                structural_carriers.contains(root),
            ) && separations[root] == LoopSeparation::Inline
        })
    });
    let mut next_loop = 0_u64;
    let root_loop = inline_sample_root.then(|| {
        let id = next_loop;
        next_loop += 1;
        id
    });

    let mut recursion_loop = BTreeMap::new();
    for group in recursion_groups {
        recursion_loop.insert(group, next_loop);
        next_loop += 1;
    }

    let mut owner = BTreeMap::<u32, u64>::new();
    for record in &certificate.records {
        if requires_sample_execution(
            record,
            delayed_values.contains(&record.signal_id),
            structural_carriers.contains(&record.signal_id),
        ) && !certificate.lifecycle_boundaries.contains(&record.signal_id)
            && separations[&record.signal_id] == LoopSeparation::SeparateSerial
        {
            let group = record
                .recursive_projection
                .expect("serial separation is a recursive projection")
                .group;
            owner.insert(record.signal_id, recursion_loop[&group]);
        }
    }
    for record in &certificate.records {
        if requires_sample_execution(
            record,
            delayed_values.contains(&record.signal_id),
            structural_carriers.contains(&record.signal_id),
        ) && !certificate.lifecycle_boundaries.contains(&record.signal_id)
            && separations[&record.signal_id] == LoopSeparation::SeparateVectorizable
        {
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
    for dependency in &certificate.dependencies {
        children
            .entry(dependency.from)
            .or_default()
            .push((dependency.edge_key, dependency.to));
    }
    let children = children
        .into_iter()
        .map(|(signal, mut edges)| {
            edges.sort_unstable();
            edges.dedup();
            (signal, edges.into_iter().map(|(_, child)| child).collect())
        })
        .collect();
    let mut state = PlacementState {
        records,
        children,
        delayed_values: delayed_values.clone(),
        delayed_pairs,
        structural_carriers: structural_carriers.clone(),
        placement: BTreeMap::new(),
        contexts: BTreeMap::new(),
        roots_by_loop: BTreeMap::new(),
        visited: BTreeSet::new(),
    };
    for record in &certificate.records {
        if structural_carriers.contains(&record.signal_id) {
            state.placement.insert(record.signal_id, Placement::Inline);
        } else if !requires_sample_execution(
            record,
            delayed_values.contains(&record.signal_id),
            structural_carriers.contains(&record.signal_id),
        ) {
            state.placement.insert(record.signal_id, Placement::Control);
        }
    }
    for &boundary in &certificate.lifecycle_boundaries {
        state.placement.insert(boundary, Placement::Control);
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
        if !requires_sample_execution(
            record,
            delayed_values.contains(&root),
            structural_carriers.contains(&root),
        ) {
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
    // C++ OccMarkup expands a shared node's children only on its first visit.
    // Its canonical occurrence projection can therefore contain a
    // compute-visible effect component that is disconnected from the output
    // roots even though the full certified dependency facts retain it (for
    // example an `attach`-only bargraph branch). Materialize only the maximal
    // roots of each such component; visiting them then assigns their complete
    // closure without duplicating descendant effects.
    loop {
        let unplaced = certificate
            .records
            .iter()
            .filter(|record| {
                requires_sample_execution(
                    record,
                    delayed_values.contains(&record.signal_id),
                    structural_carriers.contains(&record.signal_id),
                ) && !certificate.lifecycle_boundaries.contains(&record.signal_id)
                    && !state.placement.contains_key(&record.signal_id)
            })
            .map(|record| record.signal_id)
            .collect::<BTreeSet<_>>();
        if unplaced.is_empty() {
            break;
        }
        let descendants = unplaced
            .iter()
            .flat_map(|signal| state.children.get(signal).into_iter().flatten())
            .filter(|child| unplaced.contains(child))
            .copied()
            .collect::<BTreeSet<_>>();
        let component_roots = unplaced
            .difference(&descendants)
            .copied()
            .collect::<Vec<_>>();
        if component_roots.is_empty() {
            break;
        }
        let mut made_progress = false;
        for root in component_roots {
            let record = state.records[&root];
            if effects_duplicable(&record.effects) {
                state.placement.insert(root, Placement::Inline);
                made_progress = true;
                continue;
            }
            let loop_id = next_loop;
            next_loop += 1;
            state.placement.insert(root, Placement::Owned(loop_id));
            state
                .roots_by_loop
                .entry(loop_id)
                .or_default()
                .insert(u64::from(root));
            state.visit(root, loop_id)?;
            made_progress = true;
        }
        if !made_progress {
            break;
        }
    }
    trace_stage("placement");
    for record in &certificate.records {
        if requires_sample_execution(
            record,
            delayed_values.contains(&record.signal_id),
            structural_carriers.contains(&record.signal_id),
        ) && !certificate.lifecycle_boundaries.contains(&record.signal_id)
            && !state.placement.contains_key(&record.signal_id)
        {
            if timing_enabled {
                let incoming = certificate
                    .occurrence_dependencies
                    .iter()
                    .filter(|dependency| dependency.to == record.signal_id)
                    .count();
                let outgoing = certificate
                    .occurrence_dependencies
                    .iter()
                    .filter(|dependency| dependency.from == record.signal_id)
                    .count();
                let parents = certificate
                    .occurrence_dependencies
                    .iter()
                    .filter(|dependency| dependency.to == record.signal_id)
                    .map(|dependency| {
                        (
                            dependency.from,
                            state.placement.get(&dependency.from).copied(),
                        )
                    })
                    .collect::<Vec<_>>();
                eprintln!(
                    "[vector-plan-unplaced] signal={} type={:?} effects={:?} incoming={} outgoing={} parents={:?} lifecycle_boundary={}",
                    record.signal_id,
                    record.sig_type,
                    record.effects,
                    incoming,
                    outgoing,
                    parents,
                    certificate.lifecycle_boundaries.contains(&record.signal_id)
                );
            }
            return Err(VectorPlanBuildError::SampleSignalUnplaced {
                signal_id: record.signal_id,
            });
        }
    }

    let mut cross_uses = BTreeSet::<(u32, u64, u64)>::new();
    let mut delayed_edges = BTreeSet::<LoopEdge>::new();
    let mut immediate_delay_edges = BTreeSet::<LoopEdge>::new();
    let mut effect_edges = BTreeSet::<LoopEdge>::new();
    for dependency in &certificate.dependencies {
        add_dependency_edges(
            dependency,
            &state,
            &mut cross_uses,
            &mut delayed_edges,
            &mut immediate_delay_edges,
            &mut effect_edges,
        )?;
    }
    trace_stage("dependency-edges");
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
    trace_stage("delayed-edge-closure");

    let loop_ids = (0..next_loop).collect::<Vec<_>>();
    orient_effect_conflicts(&loop_ids, &state, &data_edges, &mut effect_edges);
    trace_stage("effect-orientation");
    data_edges.retain(|edge| edge.consumer != edge.dependency);
    effect_edges.retain(|edge| edge.consumer != edge.dependency);

    let signals = certificate
        .records
        .iter()
        .map(|record| SignalRecord {
            signal_id: u64::from(record.signal_id),
            value_type: value_type(&record.sig_type),
            structural: record.is_symbolic_recursion_carrier,
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
    trace_stage("signal-records");

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
    trace_stage("loop-records");

    let mut transports = Vec::new();
    for (transport_id, &(signal_id, producer_loop, consumer_loop)) in cross_uses.iter().enumerate()
    {
        let record = state.records[&signal_id];
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
    trace_stage("transports");

    let fused_serial_groups =
        build_fused_serial_groups(certificate, &state, &loop_ids, &data_edges, &transports);
    for edge in &immediate_delay_edges {
        if !fused_serial_groups.iter().any(|group| {
            group
                .member_loop_ids
                .binary_search(&edge.dependency)
                .is_ok()
                && group.member_loop_ids.binary_search(&edge.consumer).is_ok()
        }) {
            return Err(VectorPlanBuildError::UnfusedImmediateDelayCrossing {
                producer: edge.dependency,
                consumer: edge.consumer,
            });
        }
    }
    trace_stage("fused-groups");
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
        fused_serial_groups,
    };
    let verification = verify_vector_plan(&plan);
    trace_stage("plan-verification");
    verification?;
    let fused_verification = verify_fused_serial_groups_after_plan(&plan, verified);
    trace_stage("fused-verification");
    fused_verification?;
    Ok(VerifiedVectorPlan { plan })
}

/// Derives a fail-closed fused-serial slice directly from certified decoration
/// facts. Every delayed read and pure transport chain belonging to one
/// top-rate recursion group is kept in a single per-sample execution unit.
fn build_fused_serial_groups(
    certificate: &super::decoration_verify::DecorationCertificate,
    state: &PlacementState<'_>,
    loop_ids: &[u64],
    data_edges: &BTreeSet<LoopEdge>,
    transports: &[TransportRecord],
) -> Vec<FusedSerialGroupRecord> {
    #[derive(Default)]
    struct Candidate {
        carrier: Option<u64>,
        recursion_group: Option<u32>,
        members: BTreeSet<u64>,
        delayed_reads: BTreeSet<u64>,
        ambiguous: bool,
    }

    let reachability = PlanReachability::new(loop_ids, data_edges);
    let mut candidates = BTreeMap::<u64, Candidate>::new();
    for dependency in certificate
        .dependencies
        .iter()
        .filter(|dependency| matches!(dependency.kind, DepKind::Delayed { amount } if amount > 0))
    {
        let read_id = dependency.from;
        let Some(read_record) = state.records.get(&read_id).copied() else {
            continue;
        };
        let Some(carrier_record) = state.records.get(&dependency.to).copied() else {
            continue;
        };
        let Some(projection) = carrier_record.recursive_projection else {
            continue;
        };
        if carrier_record.max_delay == 0
            || read_record.clock_domain.is_some()
            || carrier_record.clock_domain.is_some()
        {
            continue;
        }
        let Some(Placement::Owned(read_loop_id)) = state.placement.get(&read_id).copied() else {
            continue;
        };
        let Some(Placement::Owned(owner_loop_id)) = state.placement.get(&dependency.to).copied()
        else {
            continue;
        };
        if read_loop_id == owner_loop_id || !reachability.reaches(read_loop_id, owner_loop_id) {
            continue;
        }
        let candidate = candidates.entry(owner_loop_id).or_default();
        let carrier = u64::from(dependency.to);
        if candidate
            .recursion_group
            .is_some_and(|existing| existing != projection.group)
        {
            candidate.ambiguous = true;
            continue;
        }
        candidate.carrier = Some(
            candidate
                .carrier
                .map_or(carrier, |current| current.min(carrier)),
        );
        candidate.recursion_group = Some(projection.group);
        // Include every loop on a same-sample data path from the delayed read
        // to its recursive writer. The fused body then preserves read(n),
        // write(n), read(n+1) even when no transport directly carries the
        // delayed-read node itself.
        candidate
            .members
            .extend(loop_ids.iter().copied().filter(|&loop_id| {
                (loop_id == read_loop_id || reachability.reaches(read_loop_id, loop_id))
                    && (loop_id == owner_loop_id || reachability.reaches(loop_id, owner_loop_id))
            }));
        candidate.delayed_reads.insert(u64::from(read_id));
    }
    // Preserve the direct transported-read slice as a second, independent
    // discovery route. A delayed read may already share its recursive owner
    // loop while a consumer transport still has to remain in that same
    // physical sample loop. Direct dependency discovery above intentionally
    // skips that local-read case.
    for transport in transports {
        let read_id = u32::try_from(transport.signal_id).expect("signal id fits u32");
        let Some(read_record) = state.records.get(&read_id).copied() else {
            continue;
        };
        for dependency in certificate.dependencies.iter().filter(|dependency| {
            dependency.from == read_id
                && matches!(dependency.kind, DepKind::Delayed { amount } if amount > 0)
        }) {
            let Some(carrier_record) = state.records.get(&dependency.to).copied() else {
                continue;
            };
            let Some(projection) = carrier_record.recursive_projection else {
                continue;
            };
            if carrier_record.max_delay == 0
                || read_record.clock_domain.is_some()
                || carrier_record.clock_domain.is_some()
            {
                continue;
            }
            let Some(Placement::Owned(owner_loop_id)) =
                state.placement.get(&dependency.to).copied()
            else {
                continue;
            };
            let candidate = candidates.entry(owner_loop_id).or_default();
            let carrier = u64::from(dependency.to);
            if candidate
                .recursion_group
                .is_some_and(|existing| existing != projection.group)
            {
                candidate.ambiguous = true;
                continue;
            }
            candidate.carrier = Some(
                candidate
                    .carrier
                    .map_or(carrier, |current| current.min(carrier)),
            );
            candidate.recursion_group = Some(projection.group);
            candidate.members.insert(transport.producer_loop);
            candidate.members.insert(transport.consumer_loop);
            candidate.members.insert(owner_loop_id);
            candidate.delayed_reads.insert(transport.signal_id);
        }
    }

    let mut components = Vec::<Vec<(u64, Candidate)>>::new();
    for candidate in candidates.into_iter().filter(|(_, candidate)| {
        !candidate.ambiguous && candidate.carrier.is_some() && candidate.recursion_group.is_some()
    }) {
        let mut component = vec![candidate];
        let mut members = component[0].1.members.clone();
        while let Some(position) = components.iter().position(|existing| {
            existing
                .iter()
                .any(|(_, candidate)| !candidate.members.is_disjoint(&members))
        }) {
            let existing = components.remove(position);
            for (_, candidate) in &existing {
                members.extend(candidate.members.iter().copied());
            }
            component.extend(existing);
        }
        component.sort_by_key(|(owner_loop_id, _)| *owner_loop_id);
        components.push(component);
    }
    components.sort_by_key(|component| component[0].0);

    let mut groups = Vec::new();
    for component in components {
        let owner_loop_id = component[0].0;
        let carrier = component[0].1.carrier.expect("component carrier checked");
        let members = component
            .iter()
            .flat_map(|(_, candidate)| candidate.members.iter().copied())
            .collect::<BTreeSet<_>>();
        let delayed_reads = component
            .iter()
            .flat_map(|(_, candidate)| candidate.delayed_reads.iter().copied())
            .collect::<BTreeSet<_>>();
        let state_write_signal_ids = certificate
            .records
            .iter()
            .filter(|record| {
                record.recursive_projection.is_some()
                    && state
                        .placement
                        .get(&record.signal_id)
                        .is_some_and(|placement| {
                            matches!(placement, Placement::Owned(owner) if members.contains(owner))
                        })
            })
            .map(|record| u64::from(record.signal_id))
            .collect::<Vec<_>>();
        let output_or_transport_roots = members
            .iter()
            .flat_map(|loop_id| state.roots_by_loop.get(loop_id).into_iter().flatten())
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let internal_transport_ids = transports
            .iter()
            .filter(|transport| {
                members.contains(&transport.producer_loop)
                    && members.contains(&transport.consumer_loop)
            })
            .map(|transport| transport.transport_id)
            .collect::<Vec<_>>();
        if state_write_signal_ids.is_empty() || output_or_transport_roots.is_empty() {
            continue;
        }
        groups.push(FusedSerialGroupRecord {
            group_id: u64::try_from(groups.len()).expect("fused group count fits u64"),
            owner_loop_id,
            member_loop_ids: members.into_iter().collect(),
            recursive_carrier_signal_id: carrier,
            delayed_read_signal_ids: delayed_reads.into_iter().collect(),
            state_write_signal_ids,
            internal_transport_ids,
            output_or_transport_roots,
        });
    }
    groups
}

fn add_dependency_edges(
    dependency: &DependencyFact,
    state: &PlacementState<'_>,
    cross_uses: &mut BTreeSet<(u32, u64, u64)>,
    delayed_edges: &mut BTreeSet<LoopEdge>,
    immediate_delay_edges: &mut BTreeSet<LoopEdge>,
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
            DepKind::Immediate
                if state.records[&dependency.from].is_delay_read
                    && state
                        .delayed_pairs
                        .contains(&(dependency.from, dependency.to)) =>
            {
                let edge = LoopEdge {
                    consumer,
                    dependency: producer,
                };
                delayed_edges.insert(edge);
                immediate_delay_edges.insert(edge);
            }
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
    if std::env::var_os("FAUST_RS_VECTOR_TIMING").is_some() {
        eprintln!(
            "[vector-plan-size] loops={} data_edges={} effect_edges={}",
            loops.len(),
            data_edges.len(),
            effect_edges.len()
        );
    }
    let mut base_edges = data_edges.clone();
    base_edges.extend(effect_edges.iter().copied());
    let order = stable_topological_order(loops, &base_edges);
    let position = order
        .iter()
        .enumerate()
        .map(|(position, &loop_id)| (loop_id, position))
        .collect::<BTreeMap<_, _>>();
    let effects_by_loop = loops
        .iter()
        .map(|&loop_id| {
            (
                loop_id,
                EffectConflictSummary::new(&loop_effects(loop_id, state)),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut reachability = PlanReachability::new(loops, &base_edges);
    for (index, &left) in loops.iter().enumerate() {
        for &right in &loops[index + 1..] {
            if !effects_by_loop[&left].conflicts(&effects_by_loop[&right])
                || reachability.reaches(left, right)
                || reachability.reaches(right, left)
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
            reachability.add_edge(dependency, consumer);
        }
    }
}

#[derive(Default)]
struct EffectConflictSummary {
    any: bool,
    barrier: bool,
    state_reads: BTreeSet<StateResource>,
    state_writes: BTreeSet<StateResource>,
    table_reads: BTreeSet<u32>,
    table_writes: BTreeSet<u32>,
    ui_writes: BTreeSet<u32>,
    output_writes: BTreeSet<u32>,
}

impl EffectConflictSummary {
    fn new(effects: &[EffectAtom]) -> Self {
        let mut summary = Self {
            any: !effects.is_empty(),
            ..Self::default()
        };
        for effect in effects {
            match effect {
                EffectAtom::ReadState(resource) => {
                    summary.state_reads.insert(resource.clone());
                }
                EffectAtom::WriteState(resource) => {
                    summary.state_writes.insert(resource.clone());
                }
                EffectAtom::ReadTable(table) => {
                    summary.table_reads.insert(*table);
                }
                EffectAtom::WriteTable(table) => {
                    summary.table_writes.insert(*table);
                }
                EffectAtom::WriteUi(zone) => {
                    summary.ui_writes.insert(*zone);
                }
                EffectAtom::WriteOutput(output) => {
                    summary.output_writes.insert(*output);
                }
                EffectAtom::Foreign { purity, .. } => {
                    summary.barrier |=
                        matches!(purity, ForeignPurity::Impure | ForeignPurity::Unknown);
                }
            }
        }
        summary
    }

    fn conflicts(&self, other: &Self) -> bool {
        (self.barrier && other.any)
            || (other.barrier && self.any)
            || intersects(&self.state_writes, &other.state_reads)
            || intersects(&self.state_writes, &other.state_writes)
            || intersects(&self.state_reads, &other.state_writes)
            || intersects(&self.table_writes, &other.table_reads)
            || intersects(&self.table_writes, &other.table_writes)
            || intersects(&self.table_reads, &other.table_writes)
            || intersects(&self.ui_writes, &other.ui_writes)
            || intersects(&self.output_writes, &other.output_writes)
    }
}

fn intersects<T: Ord>(left: &BTreeSet<T>, right: &BTreeSet<T>) -> bool {
    let (small, large) = if left.len() <= right.len() {
        (left, right)
    } else {
        (right, left)
    };
    small.iter().any(|item| large.contains(item))
}

/// Compact transitive closure used while orienting effect conflicts.
///
/// The previous implementation ran one graph BFS for every conflicting loop
/// pair. Large UI DSPs have hundreds of loops and many effects propagated to
/// their output roots, making that quadratic pair scan cubic in graph size.
/// Bit rows make each query constant-time and update only predecessors of a
/// newly inserted acyclic edge.
struct PlanReachability {
    index: BTreeMap<u64, usize>,
    rows: Vec<Vec<u64>>,
}

impl PlanReachability {
    fn new(loops: &[u64], edges: &BTreeSet<LoopEdge>) -> Self {
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

    fn reaches(&self, from: u64, to: u64) -> bool {
        self.bit(self.index[&from], self.index[&to])
    }

    fn add_edge(&mut self, from: u64, to: u64) {
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

    fn bit(&self, from: usize, to: usize) -> bool {
        self.rows[from][to / u64::BITS as usize] & (1_u64 << (to % u64::BITS as usize)) != 0
    }
}

fn set_bit(bits: &mut [u64], index: usize) {
    bits[index / u64::BITS as usize] |= 1_u64 << (index % u64::BITS as usize);
}

fn or_bits(target: &mut [u64], additions: &[u64]) {
    for (target, additions) in target.iter_mut().zip(additions) {
        *target |= additions;
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
        .map(|&loop_id| (loop_id, 0_usize))
        .collect::<BTreeMap<_, _>>();
    let successors = successor_map(loops, edges);
    for edge in edges {
        *dependencies.entry(edge.consumer).or_default() += 1;
    }
    let mut ready = dependencies
        .iter()
        .filter_map(|(&loop_id, &count)| (count == 0).then_some(loop_id))
        .collect::<BTreeSet<_>>();
    let mut order = Vec::new();
    while let Some(next) = ready.pop_first() {
        order.push(next);
        for &consumer in &successors[&next] {
            let count = dependencies
                .get_mut(&consumer)
                .expect("successor is a known loop");
            *count -= 1;
            if *count == 0 {
                ready.insert(consumer);
            }
        }
    }
    let scheduled = order.iter().copied().collect::<BTreeSet<_>>();
    order.extend(
        loops
            .iter()
            .copied()
            .filter(|loop_id| !scheduled.contains(loop_id)),
    );
    order
}

fn successor_map(loops: &[u64], edges: &BTreeSet<LoopEdge>) -> BTreeMap<u64, BTreeSet<u64>> {
    let mut successors = loops
        .iter()
        .map(|&loop_id| (loop_id, BTreeSet::new()))
        .collect::<BTreeMap<_, _>>();
    for edge in edges {
        successors
            .entry(edge.dependency)
            .or_default()
            .insert(edge.consumer);
    }
    successors
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
        // Table wrappers carry the effective nature of the current signal.
        // This matters for casts around nested read-table types: recursively
        // unwrapping content would lose the outer real cast.
        CanonicalSigType::Table { nature, .. } => scalar_value_type(*nature),
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

/// Whether this signal must execute once per demanded sample even when its
/// intrinsic Faust type is `Konst` or `Block`.
///
/// C++ occurrence markup starts every output in a sample-rate use context.
/// Temporal primitives can nevertheless retain a slower intrinsic type: a
/// delayed constant, for example, still changes after the first sample because
/// its history starts cleared. The occurrence certificate identifies delay
/// carriers separately from delay amounts, while state effects retain the
/// other temporal closures. A generic sample-rate *use context* alone is
/// deliberately insufficient: the literal amount of a fixed delay is visited
/// from a sample expression but remains a pure control value.
fn requires_sample_execution(
    record: &DecorationRecord,
    is_delayed_value: bool,
    is_structural_carrier: bool,
) -> bool {
    !is_structural_carrier
        && (record.variability == Variability::Samp
            || is_delayed_value
            || record.effects.iter().any(|effect| {
                matches!(effect, EffectAtom::ReadState(_) | EffectAtom::WriteState(_))
            }))
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
    fn compact_effect_summaries_match_atom_pair_semantics() {
        use crate::signal_fir::vector_analysis::{
            ForeignResource, ForeignTypeCode, effect_sets_conflict,
        };

        let state = StateResource::Signal {
            owner: 7,
            cell: crate::signal_fir::vector_analysis::StateCell::Delay,
        };
        let atoms = vec![
            EffectAtom::ReadState(state.clone()),
            EffectAtom::WriteState(state),
            EffectAtom::ReadTable(3),
            EffectAtom::WriteTable(3),
            EffectAtom::WriteUi(4),
            EffectAtom::WriteOutput(5),
            EffectAtom::Foreign {
                resource: ForeignResource::Variable {
                    name: "unknown".to_owned(),
                    value_type: ForeignTypeCode(1),
                },
                purity: ForeignPurity::Unknown,
            },
            EffectAtom::Foreign {
                resource: ForeignResource::Variable {
                    name: "pure".to_owned(),
                    value_type: ForeignTypeCode(1),
                },
                purity: ForeignPurity::Pure,
            },
        ];
        let mut sets = vec![Vec::new(), atoms.clone()];
        sets.extend(atoms.into_iter().map(|atom| vec![atom]));
        for left in &sets {
            for right in &sets {
                assert_eq!(
                    EffectConflictSummary::new(left).conflicts(&EffectConflictSummary::new(right)),
                    effect_sets_conflict(left, right),
                    "summary mismatch for {left:?} vs {right:?}"
                );
            }
        }
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
    fn sample_demand_keeps_a_delayed_constant_in_runtime_loops() {
        let mut arena = TreeArena::new();
        let (constant, delayed) = {
            let mut builder = SigBuilder::new(&mut arena);
            let constant = builder.real(2.0);
            (constant, builder.delay1(constant))
        };
        let decorations = certify(&arena, &[delayed]);
        let plan = build_vector_plan(&decorations, 8).unwrap();
        let placement = |signal: signals::SigId| {
            plan.plan()
                .signals
                .iter()
                .find(|record| record.signal_id == u64::from(signal.as_u32()))
                .unwrap()
                .placement
        };

        assert!(matches!(placement(constant), Placement::Owned(_)));
        assert!(matches!(placement(delayed), Placement::Owned(_)));
    }

    #[test]
    fn sample_use_does_not_promote_a_pure_fixed_delay_amount() {
        let mut arena = TreeArena::new();
        let (amount, delayed) = {
            let mut builder = SigBuilder::new(&mut arena);
            let input = builder.input(0);
            let amount = builder.int(2);
            (amount, builder.delay(input, amount))
        };
        let decorations = certify(&arena, &[delayed]);
        let plan = build_vector_plan(&decorations, 8).unwrap();
        let placement = |signal: signals::SigId| {
            plan.plan()
                .signals
                .iter()
                .find(|record| record.signal_id == u64::from(signal.as_u32()))
                .unwrap()
                .placement
        };

        assert_eq!(placement(amount), Placement::Control);
        assert!(matches!(placement(delayed), Placement::Owned(_)));
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
        let writer_projection_indices = decorations
            .certificate()
            .records
            .iter()
            .filter(|record| {
                plan.signals.iter().any(|signal| {
                    signal.signal_id == u64::from(record.signal_id)
                        && signal.placement == Placement::Owned(recursive[0].loop_id)
                })
            })
            .filter_map(|record| {
                record
                    .recursive_projection
                    .map(|projection| projection.index)
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(writer_projection_indices, BTreeSet::from([0, 1]));
        let delayed_projection_indices = decorations
            .certificate()
            .dependencies
            .iter()
            .filter(
                |dependency| matches!(dependency.kind, DepKind::Delayed { amount } if amount > 0),
            )
            .filter_map(|dependency| {
                decorations
                    .certificate()
                    .records
                    .iter()
                    .find(|record| record.signal_id == dependency.to)
                    .and_then(|record| record.recursive_projection)
                    .map(|projection| projection.index)
            })
            .collect::<BTreeSet<_>>();
        assert!(!delayed_projection_indices.is_empty());
        assert!(delayed_projection_indices.is_subset(&writer_projection_indices));
        assert!(
            plan.fused_serial_groups.is_empty(),
            "a recursion already colocated in one serial loop needs no fused envelope"
        );
    }

    #[test]
    fn stateful_waveform_values_use_typed_numeric_transports() {
        let mut arena = TreeArena::new();
        let (left, right) = {
            let mut builder = SigBuilder::new(&mut arena);
            let v0 = builder.real(0.1);
            let v1 = builder.real(0.5);
            let waveform = builder.waveform(&[v0, v1]);
            let two = builder.real(2.0);
            let three = builder.real(3.0);
            (
                builder.binop(signals::BinOp::Mul, waveform, two),
                builder.binop(signals::BinOp::Mul, waveform, three),
            )
        };
        let decorations = certify(&arena, &[left, right]);
        let plan = build_vector_plan(&decorations, 8).unwrap().into_plan();
        let table_ids = decorations
            .certificate()
            .records
            .iter()
            .filter(|record| matches!(record.sig_type, CanonicalSigType::Table { .. }))
            .map(|record| u64::from(record.signal_id))
            .collect::<BTreeSet<_>>();

        assert!(plan.transports.iter().any(|transport| {
            table_ids.contains(&transport.signal_id)
                && transport.element_type == ValueType::Real
                && transport.length == 8
        }));
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
