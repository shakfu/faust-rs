//! Bounded P5.3 event-order certificates for vector loop fission.
//!
//! # C++ provenance and formal boundary
//! Faust C++ performs loop fission after signal dependencies and recursive
//! state have constrained the loop DAG (`DAGInstructionsCompiler` and
//! `CodeLoop`). The port plan states the corresponding proof obligation as
//! `FissionSafe`: every dynamic dependence ordered by scalar execution must
//! remain ordered by vector execution.
//!
//! This module makes that obligation executable for small routed plans. It
//! expands each loop operation over the complete vector chunk, builds a
//! sample-major scalar order, a scheduled loop-major vector order, and a
//! conservative dependence relation. Conflicting effect events are ordered as
//! they are in the scalar reference. Consequently, cross-loop carried state is
//! rejected even when a static effect edge happens to order the two loops.
//!
//! The model is deliberately bounded: it is a structural P5 gate and not the
//! P6 state-transition simulation. Production construction and independent
//! checking both require an explicit event limit and fail closed when the
//! complete chunk expansion exceeds it.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use super::vector_analysis::{EffectAtom, effects_conflict};
use super::vector_plan::VerifiedVectorPlan;
use super::vector_route::{RoutedUseSource, VectorRegion, VerifiedRoutedFir};
use super::vector_verify::{LoopEdge, VectorPlan, VectorPlanError, verify_vector_plan};

/// Suggested upper bound for focused production and differential checks.
pub const DEFAULT_EVENT_LIMIT: usize = 4096;

/// Stable region containing a bounded dynamic event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventRegion {
    /// Fixed control computation, executed once before vector epochs.
    Control,
    /// An epoch boundary rather than a loop body operation.
    Epoch(u64),
    /// One scheduled vector-plan loop.
    Loop(u64),
}

/// Canonical source identity for a routed signal use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventUseSource {
    /// A definition in fixed control scope.
    Control,
    /// A definition in the same loop scope.
    Loop(u64),
    /// A named P4.4 chunk transport.
    Transport(u64),
}

/// One operation represented by the bounded event model.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VectorEventKind {
    /// A routed signal definition.
    Definition { signal_id: u64 },
    /// One exact signal-level effect atom attached to a definition.
    Effect {
        signal_id: u64,
        effect_index: u64,
        effect: EffectAtom,
    },
    /// Producer-side write into a named chunk transport.
    TransportStore { transport_id: u64 },
    /// Consumer-side read from a named chunk transport.
    TransportLoad { transport_id: u64 },
    /// One routed signal use. `occurrence` distinguishes repeated equal uses.
    Use {
        signal_id: u64,
        source: EventUseSource,
        occurrence: u64,
    },
    /// Fixed entry barrier for one epoch.
    EpochEnter { epoch_id: u64 },
    /// Fixed exit barrier for one epoch.
    EpochExit { epoch_id: u64 },
}

/// One canonical static or sample-indexed event.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VectorEvent {
    /// Dense canonical identity, independent of `-ss`.
    pub event_id: u64,
    /// Region that executes the event.
    pub region: EventRegion,
    /// Chunk-relative sample for loop events; absent for control/barriers.
    pub sample: Option<u64>,
    /// Exact operation identity.
    pub kind: VectorEventKind,
}

/// Directed dynamic dependence `before -> after`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventDependency {
    /// Event that must execute first.
    pub before: u64,
    /// Event that must execute second.
    pub after: u64,
}

/// Canonical bounded witness containing `<scalar`, `<vec`, and `D`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventOrderCertificate {
    sample_count: u64,
    events: Vec<VectorEvent>,
    scalar_order: Vec<u64>,
    vector_order: Vec<u64>,
    dependencies: Vec<EventDependency>,
}

impl EventOrderCertificate {
    /// Number of chunk samples expanded by the model.
    #[must_use]
    pub fn sample_count(&self) -> u64 {
        self.sample_count
    }

    /// Canonical event table.
    #[must_use]
    pub fn events(&self) -> &[VectorEvent] {
        &self.events
    }

    /// Sample-major scalar reference order.
    #[must_use]
    pub fn scalar_order(&self) -> &[u64] {
        &self.scalar_order
    }

    /// Epoch-major, scheduled-loop-major vector order.
    #[must_use]
    pub fn vector_order(&self) -> &[u64] {
        &self.vector_order
    }

    /// Complete finite dependence relation used by the checker.
    #[must_use]
    pub fn dependencies(&self) -> &[EventDependency] {
        &self.dependencies
    }
}

/// Opaque evidence that [`verify_event_order_certificate`] accepted P5.3.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedEventOrderCertificate {
    certificate: EventOrderCertificate,
}

impl VerifiedEventOrderCertificate {
    /// Returns the checked finite certificate.
    #[must_use]
    pub fn certificate(&self) -> &EventOrderCertificate {
        &self.certificate
    }

    /// Consumes the wrapper and returns the checked finite certificate.
    #[must_use]
    pub fn into_certificate(self) -> EventOrderCertificate {
        self.certificate
    }
}

/// Typed producer/checker failure for the P5.3 event-order gate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VectorEventError {
    /// The P4.4 plan is not independently valid.
    Plan(VectorPlanError),
    /// The routed artifact was produced from a different plan.
    RoutedPlanMismatch,
    /// The route layout does not exactly and topologically cover the plan.
    InvalidLayout { detail: &'static str, loop_id: u64 },
    /// The complete dynamic expansion is larger than the caller's bound.
    EventBoundExceeded { needed: usize, limit: usize },
    /// Event-count arithmetic exceeded the host representation.
    EventCountOverflow,
    /// The event table differs from independent reconstruction.
    EventTableMismatch,
    /// The scalar order differs from independent reconstruction.
    ScalarOrderMismatch,
    /// The vector order differs from the accepted routed layout.
    VectorOrderMismatch,
    /// The dependence relation differs from independent reconstruction.
    DependencyMismatch,
    /// An order is not a permutation of the event table.
    InvalidOrder { which: &'static str },
    /// A reconstructed dependence contradicts scalar execution.
    ScalarDependenceViolation { before: u64, after: u64 },
    /// A scalar-ordered dependence is reversed by vector execution.
    FissionSafeViolation { before: u64, after: u64 },
}

impl fmt::Display for VectorEventError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(error) => write!(f, "vector plan verification failed: {error}"),
            Self::RoutedPlanMismatch => {
                write!(
                    f,
                    "routed FIR and event certificate use different vector plans"
                )
            }
            Self::InvalidLayout { detail, loop_id } => {
                write!(f, "invalid routed layout for loop {loop_id}: {detail}")
            }
            Self::EventBoundExceeded { needed, limit } => {
                write!(
                    f,
                    "bounded event model needs {needed} events, limit is {limit}"
                )
            }
            Self::EventCountOverflow => write!(f, "bounded event count overflowed"),
            Self::EventTableMismatch => write!(f, "event table does not match routed FIR"),
            Self::ScalarOrderMismatch => write!(f, "scalar event order is not canonical"),
            Self::VectorOrderMismatch => write!(f, "vector event order does not match routing"),
            Self::DependencyMismatch => write!(f, "event dependence relation is incomplete"),
            Self::InvalidOrder { which } => {
                write!(f, "{which} order is not a permutation of the event table")
            }
            Self::ScalarDependenceViolation { before, after } => write!(
                f,
                "dependence {before} -> {after} contradicts scalar execution"
            ),
            Self::FissionSafeViolation { before, after } => write!(
                f,
                "vector execution reverses scalar dependence {before} -> {after}"
            ),
        }
    }
}

impl std::error::Error for VectorEventError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plan(error) => Some(error),
            _ => None,
        }
    }
}

impl From<VectorPlanError> for VectorEventError {
    fn from(value: VectorPlanError) -> Self {
        Self::Plan(value)
    }
}

/// Produces and independently checks the complete bounded P5.3 certificate.
///
/// The event limit applies to the full `vec_size` expansion; no prefix is
/// silently accepted as evidence for a larger chunk.
pub fn build_event_order_certificate(
    verified_plan: &VerifiedVectorPlan,
    routed: &VerifiedRoutedFir,
    event_limit: usize,
) -> Result<VerifiedEventOrderCertificate, VectorEventError> {
    let plan = verified_plan.plan();
    let certificate = derive_certificate(plan, routed, event_limit)?;
    verify_event_order_certificate(plan, routed, &certificate, event_limit)?;
    Ok(VerifiedEventOrderCertificate { certificate })
}

/// Independently reconstructs and checks event coverage, both total orders,
/// the dependence relation, and `FissionSafe`.
pub fn verify_event_order_certificate(
    plan: &VectorPlan,
    routed: &VerifiedRoutedFir,
    certificate: &EventOrderCertificate,
    event_limit: usize,
) -> Result<(), VectorEventError> {
    verify_vector_plan(plan)?;
    if routed.plan() != plan {
        return Err(VectorEventError::RoutedPlanMismatch);
    }
    validate_layout(plan, routed)?;
    if certificate.sample_count != plan.vec_size {
        return Err(VectorEventError::EventTableMismatch);
    }
    verify_event_table_independently(plan, routed, &certificate.events, event_limit)?;
    validate_order("scalar", &certificate.events, &certificate.scalar_order)?;
    validate_order("vector", &certificate.events, &certificate.vector_order)?;
    let scalar_order = independently_order_events(plan, routed, &certificate.events, true);
    if certificate.scalar_order != scalar_order {
        return Err(VectorEventError::ScalarOrderMismatch);
    }
    let vector_order = independently_order_events(plan, routed, &certificate.events, false);
    if certificate.vector_order != vector_order {
        return Err(VectorEventError::VectorOrderMismatch);
    }
    if certificate
        .dependencies
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
    {
        return Err(VectorEventError::DependencyMismatch);
    }

    let scalar_positions = positions(&certificate.scalar_order);
    let vector_positions = positions(&certificate.vector_order);
    let event_ids = certificate
        .events
        .iter()
        .map(|event| event.event_id)
        .collect::<BTreeSet<_>>();
    for dependency in &certificate.dependencies {
        if dependency.before == dependency.after
            || !event_ids.contains(&dependency.before)
            || !event_ids.contains(&dependency.after)
        {
            return Err(VectorEventError::DependencyMismatch);
        }
        if scalar_positions[&dependency.before] >= scalar_positions[&dependency.after] {
            return Err(VectorEventError::ScalarDependenceViolation {
                before: dependency.before,
                after: dependency.after,
            });
        }
        if vector_positions[&dependency.before] >= vector_positions[&dependency.after] {
            return Err(VectorEventError::FissionSafeViolation {
                before: dependency.before,
                after: dependency.after,
            });
        }
    }
    verify_required_dependencies(
        plan,
        &certificate.events,
        &certificate.dependencies,
        &scalar_positions,
    )?;
    Ok(())
}

type EventKey = (EventRegion, Option<u64>, VectorEventKind);

fn verify_event_table_independently(
    plan: &VectorPlan,
    routed: &VerifiedRoutedFir,
    events: &[VectorEvent],
    event_limit: usize,
) -> Result<(), VectorEventError> {
    let expected = independently_expected_event_keys(plan, routed)?;
    if expected.len() > event_limit {
        return Err(VectorEventError::EventBoundExceeded {
            needed: expected.len(),
            limit: event_limit,
        });
    }
    if events.len() != expected.len() {
        return Err(VectorEventError::EventTableMismatch);
    }
    for (index, (event, key)) in events.iter().zip(expected).enumerate() {
        let event_id = u64::try_from(index).map_err(|_| VectorEventError::EventCountOverflow)?;
        if event.event_id != event_id || (event.region, event.sample, event.kind.clone()) != key {
            return Err(VectorEventError::EventTableMismatch);
        }
    }
    Ok(())
}

fn independently_expected_event_keys(
    plan: &VectorPlan,
    routed: &VerifiedRoutedFir,
) -> Result<Vec<EventKey>, VectorEventError> {
    let signals = plan
        .signals
        .iter()
        .map(|signal| (signal.signal_id, signal))
        .collect::<BTreeMap<_, _>>();
    let transports = plan
        .transports
        .iter()
        .map(|transport| (transport.transport_id, transport))
        .collect::<BTreeMap<_, _>>();
    let mut loop_kinds = BTreeMap::<u64, Vec<VectorEventKind>>::new();
    let mut keys = Vec::new();

    for definition in routed.trace().definitions() {
        let kinds = match definition.region {
            VectorRegion::Control => None,
            VectorRegion::Loop(loop_id) => Some(loop_kinds.entry(loop_id).or_default()),
        };
        let definition_kind = VectorEventKind::Definition {
            signal_id: definition.signal_id,
        };
        if let Some(kinds) = kinds {
            kinds.push(definition_kind);
        } else {
            keys.push((EventRegion::Control, None, definition_kind));
        }
        for (effect_index, effect) in signals[&definition.signal_id].effects.iter().enumerate() {
            let kind = VectorEventKind::Effect {
                signal_id: definition.signal_id,
                effect_index: u64::try_from(effect_index)
                    .map_err(|_| VectorEventError::EventCountOverflow)?,
                effect: effect.clone(),
            };
            match definition.region {
                VectorRegion::Control => keys.push((EventRegion::Control, None, kind)),
                VectorRegion::Loop(loop_id) => loop_kinds.entry(loop_id).or_default().push(kind),
            }
        }
    }
    for routed_transport in routed.trace().transports() {
        let transport = transports[&routed_transport.transport_id];
        loop_kinds.entry(transport.producer_loop).or_default().push(
            VectorEventKind::TransportStore {
                transport_id: transport.transport_id,
            },
        );
        loop_kinds.entry(transport.consumer_loop).or_default().push(
            VectorEventKind::TransportLoad {
                transport_id: transport.transport_id,
            },
        );
    }
    let mut uses = routed
        .trace()
        .uses()
        .iter()
        .map(|routed_use| {
            (
                routed_use.consumer_loop,
                routed_use.signal_id,
                event_use_source(routed_use.source),
            )
        })
        .collect::<Vec<_>>();
    uses.sort_unstable();
    let mut occurrences = BTreeMap::new();
    for (consumer_loop, signal_id, source) in uses {
        let occurrence = occurrences
            .entry((consumer_loop, signal_id, source))
            .or_insert(0_u64);
        loop_kinds
            .entry(consumer_loop)
            .or_default()
            .push(VectorEventKind::Use {
                signal_id,
                source,
                occurrence: *occurrence,
            });
        *occurrence = occurrence
            .checked_add(1)
            .ok_or(VectorEventError::EventCountOverflow)?;
    }
    for epoch in &plan.epochs {
        keys.push((
            EventRegion::Epoch(epoch.epoch_id),
            None,
            VectorEventKind::EpochEnter {
                epoch_id: epoch.epoch_id,
            },
        ));
        keys.push((
            EventRegion::Epoch(epoch.epoch_id),
            None,
            VectorEventKind::EpochExit {
                epoch_id: epoch.epoch_id,
            },
        ));
    }
    for record in &plan.loops {
        let kinds = loop_kinds.remove(&record.loop_id).unwrap_or_default();
        for sample in 0..plan.vec_size {
            for kind in &kinds {
                keys.push((
                    EventRegion::Loop(record.loop_id),
                    Some(sample),
                    kind.clone(),
                ));
            }
        }
    }
    keys.sort();
    Ok(keys)
}

fn independently_order_events(
    plan: &VectorPlan,
    routed: &VerifiedRoutedFir,
    events: &[VectorEvent],
    sample_major: bool,
) -> Vec<u64> {
    let epochs = sorted_epochs(plan);
    let epoch_position = epochs
        .iter()
        .enumerate()
        .map(|(position, epoch)| (epoch.epoch_id, position + 1))
        .collect::<BTreeMap<_, _>>();
    let loop_epoch = plan
        .loops
        .iter()
        .map(|record| (record.loop_id, record.epoch_id))
        .collect::<BTreeMap<_, _>>();
    let canonical_loops = canonical_scalar_loops(plan);
    let scalar_loop_position = canonical_loops
        .values()
        .flat_map(|loops| loops.iter().enumerate())
        .map(|(position, loop_id)| (*loop_id, position))
        .collect::<BTreeMap<_, _>>();
    let vector_loop_position = routed
        .layout()
        .loops()
        .iter()
        .enumerate()
        .map(|(position, region)| (region.loop_id, position))
        .collect::<BTreeMap<_, _>>();

    let mut order = events.iter().collect::<Vec<_>>();
    order.sort_unstable_by_key(|event| match event.region {
        EventRegion::Control => (0, 0, 0, 0, event.event_id),
        EventRegion::Epoch(epoch_id) => {
            let phase = match event.kind {
                VectorEventKind::EpochEnter { .. } => 0,
                VectorEventKind::EpochExit { .. } => 2,
                _ => unreachable!("event-table checker restricts epoch events"),
            };
            (epoch_position[&epoch_id], phase, 0, 0, event.event_id)
        }
        EventRegion::Loop(loop_id) => {
            let epoch = epoch_position[&loop_epoch[&loop_id]];
            let sample = usize::try_from(event.sample.expect("loop event has a sample"))
                .expect("event table is bounded by usize");
            let loop_position = if sample_major {
                scalar_loop_position[&loop_id]
            } else {
                vector_loop_position[&loop_id]
            };
            let (outer, inner) = if sample_major {
                (sample, loop_position)
            } else {
                (loop_position, sample)
            };
            (epoch, 1, outer, inner, event.event_id)
        }
    });
    order.into_iter().map(|event| event.event_id).collect()
}

fn verify_required_dependencies(
    plan: &VectorPlan,
    events: &[VectorEvent],
    dependencies: &[EventDependency],
    scalar_positions: &BTreeMap<u64, usize>,
) -> Result<(), VectorEventError> {
    let present = dependencies.iter().copied().collect::<BTreeSet<_>>();
    let contexts = context_events(events);
    let require = |before, after| {
        if before == after || present.contains(&EventDependency { before, after }) {
            Ok(())
        } else {
            Err(VectorEventError::DependencyMismatch)
        }
    };

    for local in contexts.values() {
        for pair in local.windows(2) {
            require(pair[0], pair[1])?;
        }
    }
    let epochs = sorted_epochs(plan);
    for (index, epoch) in epochs.iter().enumerate() {
        let boundaries = &contexts[&(EventRegion::Epoch(epoch.epoch_id), None)];
        let enter = boundaries[0];
        let exit = boundaries[1];
        if index == 0 {
            if let Some(control) = contexts.get(&(EventRegion::Control, None))
                && let Some(last) = control.last()
            {
                require(*last, enter)?;
            }
        } else {
            let previous = &contexts[&(EventRegion::Epoch(epochs[index - 1].epoch_id), None)];
            require(previous[1], enter)?;
        }
        for &loop_id in &epoch.loops {
            for sample in 0..plan.vec_size {
                if let Some(local) = contexts.get(&(EventRegion::Loop(loop_id), Some(sample)))
                    && let (Some(first), Some(last)) = (local.first(), local.last())
                {
                    require(enter, *first)?;
                    require(*last, exit)?;
                }
            }
        }
    }
    for edge in plan.data_edges.iter().chain(&plan.effect_edges) {
        for sample in 0..plan.vec_size {
            let producer = contexts
                .get(&(EventRegion::Loop(edge.dependency), Some(sample)))
                .and_then(|local| local.last());
            let consumer = contexts
                .get(&(EventRegion::Loop(edge.consumer), Some(sample)))
                .and_then(|local| local.first());
            if let (Some(producer), Some(consumer)) = (producer, consumer) {
                require(*producer, *consumer)?;
            }
        }
    }

    let definitions = events
        .iter()
        .filter_map(|event| match event.kind {
            VectorEventKind::Definition { signal_id } => {
                Some(((event.region, signal_id, event.sample), event.event_id))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let loads = event_ids_by_transport(events, false);
    let transports = plan
        .transports
        .iter()
        .map(|transport| (transport.transport_id, transport))
        .collect::<BTreeMap<_, _>>();
    for event in events {
        match event.kind {
            VectorEventKind::TransportStore { transport_id } => {
                let transport = transports[&transport_id];
                require(
                    definitions[&(
                        EventRegion::Loop(transport.producer_loop),
                        transport.signal_id,
                        event.sample,
                    )],
                    event.event_id,
                )?;
                require(event.event_id, loads[&(transport_id, event.sample)])?;
            }
            VectorEventKind::Use {
                signal_id, source, ..
            } => {
                let source_event = match source {
                    EventUseSource::Control => {
                        definitions[&(EventRegion::Control, signal_id, None)]
                    }
                    EventUseSource::Loop(loop_id) => {
                        definitions[&(EventRegion::Loop(loop_id), signal_id, event.sample)]
                    }
                    EventUseSource::Transport(transport_id) => loads[&(transport_id, event.sample)],
                };
                require(source_event, event.event_id)?;
            }
            _ => {}
        }
    }

    let effects = events
        .iter()
        .filter_map(|event| match &event.kind {
            VectorEventKind::Effect { effect, .. } => Some((event.event_id, effect)),
            _ => None,
        })
        .collect::<Vec<_>>();
    for left_index in 0..effects.len() {
        for right_index in (left_index + 1)..effects.len() {
            let (left_id, left) = effects[left_index];
            let (right_id, right) = effects[right_index];
            if effects_conflict(left, right) {
                let (before, after) = if scalar_positions[&left_id] < scalar_positions[&right_id] {
                    (left_id, right_id)
                } else {
                    (right_id, left_id)
                };
                require(before, after)?;
            }
        }
    }
    Ok(())
}

fn derive_certificate(
    plan: &VectorPlan,
    routed: &VerifiedRoutedFir,
    event_limit: usize,
) -> Result<EventOrderCertificate, VectorEventError> {
    verify_vector_plan(plan)?;
    if routed.plan() != plan {
        return Err(VectorEventError::RoutedPlanMismatch);
    }
    validate_layout(plan, routed)?;

    let templates = event_templates(plan, routed)?;
    let needed = expanded_event_count(plan, &templates)?;
    if needed > event_limit {
        return Err(VectorEventError::EventBoundExceeded {
            needed,
            limit: event_limit,
        });
    }
    let events = expand_events(plan, templates)?;
    debug_assert_eq!(events.len(), needed);
    let contexts = context_events(&events);
    let scalar_loops = canonical_scalar_loops(plan);
    let vector_loops = routed_layout_loops(plan, routed);
    let scalar_order = build_order(plan, &contexts, &scalar_loops, true);
    let vector_order = build_order(plan, &contexts, &vector_loops, false);
    let dependencies = build_dependencies(plan, &events, &contexts, &scalar_order);

    Ok(EventOrderCertificate {
        sample_count: plan.vec_size,
        events,
        scalar_order,
        vector_order,
        dependencies,
    })
}

fn validate_layout(plan: &VectorPlan, routed: &VerifiedRoutedFir) -> Result<(), VectorEventError> {
    let loop_by_id = plan
        .loops
        .iter()
        .map(|record| (record.loop_id, record))
        .collect::<BTreeMap<_, _>>();
    let epoch_by_id = plan
        .epochs
        .iter()
        .map(|epoch| (epoch.epoch_id, epoch))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut positions = BTreeMap::new();
    let mut previous_rank = None;
    for (index, region) in routed.layout().loops().iter().enumerate() {
        let Some(record) = loop_by_id.get(&region.loop_id) else {
            return Err(VectorEventError::InvalidLayout {
                detail: "unknown loop",
                loop_id: region.loop_id,
            });
        };
        let Some(epoch) = epoch_by_id.get(&record.epoch_id) else {
            return Err(VectorEventError::InvalidLayout {
                detail: "unknown epoch",
                loop_id: region.loop_id,
            });
        };
        if !seen.insert(region.loop_id) {
            return Err(VectorEventError::InvalidLayout {
                detail: "duplicate loop",
                loop_id: region.loop_id,
            });
        }
        if region.epoch_id != record.epoch_id
            || region.epoch_rank != epoch.rank
            || region.kind != record.kind
            || region.roots != record.roots
        {
            return Err(VectorEventError::InvalidLayout {
                detail: "metadata mismatch",
                loop_id: region.loop_id,
            });
        }
        if previous_rank.is_some_and(|rank| rank > region.epoch_rank) {
            return Err(VectorEventError::InvalidLayout {
                detail: "epoch rank reversal",
                loop_id: region.loop_id,
            });
        }
        previous_rank = Some(region.epoch_rank);
        positions.insert(region.loop_id, index);
    }
    if seen.len() != plan.loops.len() {
        let missing = plan
            .loops
            .iter()
            .find(|record| !seen.contains(&record.loop_id))
            .map_or(u64::MAX, |record| record.loop_id);
        return Err(VectorEventError::InvalidLayout {
            detail: "missing loop",
            loop_id: missing,
        });
    }
    for edge in plan.data_edges.iter().chain(&plan.effect_edges) {
        if positions[&edge.dependency] >= positions[&edge.consumer] {
            return Err(VectorEventError::InvalidLayout {
                detail: "dependency reversal",
                loop_id: edge.consumer,
            });
        }
    }
    Ok(())
}

fn event_templates(
    plan: &VectorPlan,
    routed: &VerifiedRoutedFir,
) -> Result<BTreeMap<EventRegion, Vec<VectorEventKind>>, VectorEventError> {
    let signals = plan
        .signals
        .iter()
        .map(|signal| (signal.signal_id, signal))
        .collect::<BTreeMap<_, _>>();
    let mut templates = BTreeMap::<EventRegion, Vec<VectorEventKind>>::new();
    for definition in routed.trace().definitions() {
        let region = event_region(definition.region);
        templates
            .entry(region)
            .or_default()
            .push(VectorEventKind::Definition {
                signal_id: definition.signal_id,
            });
        for (index, effect) in signals[&definition.signal_id].effects.iter().enumerate() {
            let effect_index =
                u64::try_from(index).map_err(|_| VectorEventError::EventCountOverflow)?;
            templates
                .entry(region)
                .or_default()
                .push(VectorEventKind::Effect {
                    signal_id: definition.signal_id,
                    effect_index,
                    effect: effect.clone(),
                });
        }
    }
    let transports = plan
        .transports
        .iter()
        .map(|transport| (transport.transport_id, transport))
        .collect::<BTreeMap<_, _>>();
    for routed_transport in routed.trace().transports() {
        let transport = transports[&routed_transport.transport_id];
        templates
            .entry(EventRegion::Loop(transport.producer_loop))
            .or_default()
            .push(VectorEventKind::TransportStore {
                transport_id: transport.transport_id,
            });
        templates
            .entry(EventRegion::Loop(transport.consumer_loop))
            .or_default()
            .push(VectorEventKind::TransportLoad {
                transport_id: transport.transport_id,
            });
    }

    let mut uses = routed
        .trace()
        .uses()
        .iter()
        .map(|routed_use| {
            (
                routed_use.consumer_loop,
                routed_use.signal_id,
                event_use_source(routed_use.source),
            )
        })
        .collect::<Vec<_>>();
    uses.sort_unstable();
    let mut occurrences = BTreeMap::new();
    for (consumer_loop, signal_id, source) in uses {
        let occurrence = occurrences
            .entry((consumer_loop, signal_id, source))
            .or_insert(0_u64);
        templates
            .entry(EventRegion::Loop(consumer_loop))
            .or_default()
            .push(VectorEventKind::Use {
                signal_id,
                source,
                occurrence: *occurrence,
            });
        *occurrence = occurrence
            .checked_add(1)
            .ok_or(VectorEventError::EventCountOverflow)?;
    }
    for events in templates.values_mut() {
        events.sort();
    }
    Ok(templates)
}

fn expanded_event_count(
    plan: &VectorPlan,
    templates: &BTreeMap<EventRegion, Vec<VectorEventKind>>,
) -> Result<usize, VectorEventError> {
    let mut total = plan
        .epochs
        .len()
        .checked_mul(2)
        .ok_or(VectorEventError::EventCountOverflow)?;
    total = total
        .checked_add(templates.get(&EventRegion::Control).map_or(0, Vec::len))
        .ok_or(VectorEventError::EventCountOverflow)?;
    let samples =
        usize::try_from(plan.vec_size).map_err(|_| VectorEventError::EventCountOverflow)?;
    for record in &plan.loops {
        let count = templates
            .get(&EventRegion::Loop(record.loop_id))
            .map_or(0, Vec::len);
        total = total
            .checked_add(
                count
                    .checked_mul(samples)
                    .ok_or(VectorEventError::EventCountOverflow)?,
            )
            .ok_or(VectorEventError::EventCountOverflow)?;
    }
    Ok(total)
}

fn expand_events(
    plan: &VectorPlan,
    templates: BTreeMap<EventRegion, Vec<VectorEventKind>>,
) -> Result<Vec<VectorEvent>, VectorEventError> {
    let mut keys = Vec::new();
    if let Some(control) = templates.get(&EventRegion::Control) {
        for kind in control {
            keys.push((EventRegion::Control, None, kind.clone()));
        }
    }
    for epoch in &plan.epochs {
        keys.push((
            EventRegion::Epoch(epoch.epoch_id),
            None,
            VectorEventKind::EpochEnter {
                epoch_id: epoch.epoch_id,
            },
        ));
        keys.push((
            EventRegion::Epoch(epoch.epoch_id),
            None,
            VectorEventKind::EpochExit {
                epoch_id: epoch.epoch_id,
            },
        ));
    }
    for record in &plan.loops {
        if let Some(loop_templates) = templates.get(&EventRegion::Loop(record.loop_id)) {
            for sample in 0..plan.vec_size {
                for kind in loop_templates {
                    keys.push((
                        EventRegion::Loop(record.loop_id),
                        Some(sample),
                        kind.clone(),
                    ));
                }
            }
        }
    }
    keys.sort();
    keys.into_iter()
        .enumerate()
        .map(|(event_id, (region, sample, kind))| {
            Ok(VectorEvent {
                event_id: u64::try_from(event_id)
                    .map_err(|_| VectorEventError::EventCountOverflow)?,
                region,
                sample,
                kind,
            })
        })
        .collect()
}

fn context_events(events: &[VectorEvent]) -> BTreeMap<(EventRegion, Option<u64>), Vec<u64>> {
    let mut contexts = BTreeMap::<_, Vec<_>>::new();
    for event in events {
        contexts
            .entry((event.region, event.sample))
            .or_default()
            .push(event.event_id);
    }
    contexts
}

fn canonical_scalar_loops(plan: &VectorPlan) -> BTreeMap<u64, Vec<u64>> {
    let mut result = BTreeMap::new();
    for epoch in sorted_epochs(plan) {
        let members = epoch.loops.iter().copied().collect::<BTreeSet<_>>();
        let mut remaining = members.clone();
        let mut order = Vec::with_capacity(members.len());
        while !remaining.is_empty() {
            let next = remaining
                .iter()
                .copied()
                .find(|candidate| {
                    plan.data_edges
                        .iter()
                        .chain(&plan.effect_edges)
                        .filter(|edge| {
                            edge.consumer == *candidate && members.contains(&edge.dependency)
                        })
                        .all(|edge| !remaining.contains(&edge.dependency))
                })
                .expect("verified epoch DAG has a ready loop");
            remaining.remove(&next);
            order.push(next);
        }
        result.insert(epoch.epoch_id, order);
    }
    result
}

fn routed_layout_loops(plan: &VectorPlan, routed: &VerifiedRoutedFir) -> BTreeMap<u64, Vec<u64>> {
    let mut result = plan
        .epochs
        .iter()
        .map(|epoch| (epoch.epoch_id, Vec::new()))
        .collect::<BTreeMap<_, _>>();
    for region in routed.layout().loops() {
        result
            .get_mut(&region.epoch_id)
            .expect("layout validation checked epoch")
            .push(region.loop_id);
    }
    result
}

fn build_order(
    plan: &VectorPlan,
    contexts: &BTreeMap<(EventRegion, Option<u64>), Vec<u64>>,
    loops_by_epoch: &BTreeMap<u64, Vec<u64>>,
    sample_major: bool,
) -> Vec<u64> {
    let mut order = contexts
        .get(&(EventRegion::Control, None))
        .cloned()
        .unwrap_or_default();
    for epoch in sorted_epochs(plan) {
        order.extend(
            contexts[&(EventRegion::Epoch(epoch.epoch_id), None)]
                .iter()
                .copied()
                .filter(|event_id| is_epoch_enter(*event_id, contexts, epoch.epoch_id)),
        );
        let loop_order = &loops_by_epoch[&epoch.epoch_id];
        append_epoch_events(plan, contexts, loop_order, &mut order, sample_major);
        order.extend(
            contexts[&(EventRegion::Epoch(epoch.epoch_id), None)]
                .iter()
                .copied()
                .filter(|event_id| !is_epoch_enter(*event_id, contexts, epoch.epoch_id)),
        );
    }
    order
}

fn append_epoch_events(
    plan: &VectorPlan,
    contexts: &BTreeMap<(EventRegion, Option<u64>), Vec<u64>>,
    loops: &[u64],
    order: &mut Vec<u64>,
    sample_major: bool,
) {
    if sample_major {
        for sample in 0..plan.vec_size {
            for &loop_id in loops {
                order.extend(
                    contexts
                        .get(&(EventRegion::Loop(loop_id), Some(sample)))
                        .into_iter()
                        .flatten()
                        .copied(),
                );
            }
        }
    } else {
        for &loop_id in loops {
            for sample in 0..plan.vec_size {
                order.extend(
                    contexts
                        .get(&(EventRegion::Loop(loop_id), Some(sample)))
                        .into_iter()
                        .flatten()
                        .copied(),
                );
            }
        }
    }
}

fn is_epoch_enter(
    event_id: u64,
    contexts: &BTreeMap<(EventRegion, Option<u64>), Vec<u64>>,
    epoch_id: u64,
) -> bool {
    contexts[&(EventRegion::Epoch(epoch_id), None)]
        .first()
        .is_some_and(|first| *first == event_id)
}

fn build_dependencies(
    plan: &VectorPlan,
    events: &[VectorEvent],
    contexts: &BTreeMap<(EventRegion, Option<u64>), Vec<u64>>,
    scalar_order: &[u64],
) -> Vec<EventDependency> {
    let mut dependencies = BTreeSet::new();
    for event_ids in contexts.values() {
        for pair in event_ids.windows(2) {
            add_dependency(&mut dependencies, pair[0], pair[1]);
        }
    }

    let epochs = sorted_epochs(plan);
    for (index, epoch) in epochs.iter().enumerate() {
        let boundaries = &contexts[&(EventRegion::Epoch(epoch.epoch_id), None)];
        let enter = boundaries[0];
        let exit = boundaries[1];
        if index == 0 {
            if let Some(control) = contexts.get(&(EventRegion::Control, None))
                && let Some(last) = control.last()
            {
                add_dependency(&mut dependencies, *last, enter);
            }
        } else {
            let previous = &contexts[&(EventRegion::Epoch(epochs[index - 1].epoch_id), None)];
            add_dependency(&mut dependencies, previous[1], enter);
        }
        for &loop_id in &epoch.loops {
            for sample in 0..plan.vec_size {
                if let Some(local) = contexts.get(&(EventRegion::Loop(loop_id), Some(sample)))
                    && let (Some(first), Some(last)) = (local.first(), local.last())
                {
                    add_dependency(&mut dependencies, enter, *first);
                    add_dependency(&mut dependencies, *last, exit);
                }
            }
        }
    }

    for edge in plan.data_edges.iter().chain(&plan.effect_edges) {
        add_loop_edge_dependencies(plan, contexts, *edge, &mut dependencies);
    }

    let by_id = events
        .iter()
        .map(|event| (event.event_id, event))
        .collect::<BTreeMap<_, _>>();
    let definitions = events
        .iter()
        .filter_map(|event| match event.kind {
            VectorEventKind::Definition { signal_id } => {
                Some(((event.region, signal_id, event.sample), event.event_id))
            }
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();
    let stores = event_ids_by_transport(events, true);
    let loads = event_ids_by_transport(events, false);
    let transports = plan
        .transports
        .iter()
        .map(|transport| (transport.transport_id, transport))
        .collect::<BTreeMap<_, _>>();

    for event in events {
        match event.kind {
            VectorEventKind::TransportStore { transport_id } => {
                let transport = transports[&transport_id];
                let definition = definitions[&(
                    EventRegion::Loop(transport.producer_loop),
                    transport.signal_id,
                    event.sample,
                )];
                add_dependency(&mut dependencies, definition, event.event_id);
                add_dependency(
                    &mut dependencies,
                    event.event_id,
                    loads[&(transport_id, event.sample)],
                );
            }
            VectorEventKind::Use {
                signal_id, source, ..
            } => {
                let source_event = match source {
                    EventUseSource::Control => {
                        definitions[&(EventRegion::Control, signal_id, None)]
                    }
                    EventUseSource::Loop(loop_id) => {
                        definitions[&(EventRegion::Loop(loop_id), signal_id, event.sample)]
                    }
                    EventUseSource::Transport(transport_id) => loads[&(transport_id, event.sample)],
                };
                add_dependency(&mut dependencies, source_event, event.event_id);
            }
            _ => {}
        }
    }
    for ((transport_id, sample), store) in stores {
        add_dependency(&mut dependencies, store, loads[&(transport_id, sample)]);
    }

    let scalar_positions = positions(scalar_order);
    let effects = scalar_order
        .iter()
        .filter_map(|event_id| match &by_id[event_id].kind {
            VectorEventKind::Effect { effect, .. } => Some((*event_id, effect)),
            _ => None,
        })
        .collect::<Vec<_>>();
    for left_index in 0..effects.len() {
        for right_index in (left_index + 1)..effects.len() {
            let (left_id, left) = effects[left_index];
            let (right_id, right) = effects[right_index];
            if effects_conflict(left, right) {
                debug_assert!(scalar_positions[&left_id] < scalar_positions[&right_id]);
                add_dependency(&mut dependencies, left_id, right_id);
            }
        }
    }
    dependencies.into_iter().collect()
}

fn add_loop_edge_dependencies(
    plan: &VectorPlan,
    contexts: &BTreeMap<(EventRegion, Option<u64>), Vec<u64>>,
    edge: LoopEdge,
    dependencies: &mut BTreeSet<EventDependency>,
) {
    for sample in 0..plan.vec_size {
        let producer = contexts
            .get(&(EventRegion::Loop(edge.dependency), Some(sample)))
            .and_then(|events| events.last());
        let consumer = contexts
            .get(&(EventRegion::Loop(edge.consumer), Some(sample)))
            .and_then(|events| events.first());
        if let (Some(producer), Some(consumer)) = (producer, consumer) {
            add_dependency(dependencies, *producer, *consumer);
        }
    }
}

fn event_ids_by_transport(
    events: &[VectorEvent],
    stores: bool,
) -> BTreeMap<(u64, Option<u64>), u64> {
    events
        .iter()
        .filter_map(|event| {
            let transport_id = match event.kind {
                VectorEventKind::TransportStore { transport_id } if stores => transport_id,
                VectorEventKind::TransportLoad { transport_id } if !stores => transport_id,
                _ => return None,
            };
            Some(((transport_id, event.sample), event.event_id))
        })
        .collect()
}

fn add_dependency(dependencies: &mut BTreeSet<EventDependency>, before: u64, after: u64) {
    if before != after {
        dependencies.insert(EventDependency { before, after });
    }
}

fn validate_order(
    which: &'static str,
    events: &[VectorEvent],
    order: &[u64],
) -> Result<(), VectorEventError> {
    let expected = events
        .iter()
        .map(|event| event.event_id)
        .collect::<BTreeSet<_>>();
    let actual = order.iter().copied().collect::<BTreeSet<_>>();
    if order.len() != events.len() || actual != expected {
        Err(VectorEventError::InvalidOrder { which })
    } else {
        Ok(())
    }
}

fn positions(order: &[u64]) -> BTreeMap<u64, usize> {
    order
        .iter()
        .enumerate()
        .map(|(position, event_id)| (*event_id, position))
        .collect()
}

fn sorted_epochs(plan: &VectorPlan) -> Vec<&super::vector_verify::EpochRecord> {
    let mut epochs = plan.epochs.iter().collect::<Vec<_>>();
    epochs.sort_unstable_by_key(|epoch| (epoch.rank, epoch.epoch_id));
    epochs
}

fn event_region(region: VectorRegion) -> EventRegion {
    match region {
        VectorRegion::Control => EventRegion::Control,
        VectorRegion::Loop(loop_id) => EventRegion::Loop(loop_id),
    }
}

fn event_use_source(source: RoutedUseSource) -> EventUseSource {
    match source {
        RoutedUseSource::Direct(VectorRegion::Control) => EventUseSource::Control,
        RoutedUseSource::Direct(VectorRegion::Loop(loop_id)) => EventUseSource::Loop(loop_id),
        RoutedUseSource::Transport(transport_id) => EventUseSource::Transport(transport_id),
    }
}

#[cfg(test)]
mod tests {
    use fir::{FirBuilder, FirStore, FirType};

    use super::*;
    use crate::schedule::SchedulingStrategy;
    use crate::signal_fir::vector_analysis::{StateCell, StateResource};
    use crate::signal_fir::vector_plan::verified_vector_plan_for_test;
    use crate::signal_fir::vector_route::{RouteResolution, VectorRouteSession};
    use crate::signal_fir::vector_verify::{
        EpochRecord, LoopKind, LoopRecord, Placement, Rate, SignalRecord, TransportRecord,
        ValueType, VecSafeWitness, Vectorability, WitnessKind,
    };

    const ALL_STRATEGIES: [SchedulingStrategy; 4] = [
        SchedulingStrategy::DepthFirst,
        SchedulingStrategy::BreadthFirst,
        SchedulingStrategy::Special,
        SchedulingStrategy::ReverseBreadthFirst,
    ];

    fn pure_transport_plan() -> VerifiedVectorPlan {
        verified_vector_plan_for_test(VectorPlan {
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
        verified_vector_plan_for_test(VectorPlan {
            vec_size: 3,
            signals: vec![
                signal(0, Placement::Owned(0), vec![left]),
                signal(1, Placement::Owned(1), vec![right]),
            ],
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
        })
    }

    fn colocated_state_plan() -> VerifiedVectorPlan {
        let resource = StateResource::Signal {
            owner: 7,
            cell: StateCell::Delay,
        };
        verified_vector_plan_for_test(VectorPlan {
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
        })
    }

    fn signal(signal_id: u64, placement: Placement, effects: Vec<EffectAtom>) -> SignalRecord {
        SignalRecord {
            signal_id,
            value_type: ValueType::Real,
            rate: Rate::Samp,
            vectorability: if effects.is_empty() {
                Vectorability::Vect
            } else {
                Vectorability::Scal
            },
            clock_id: 0,
            duplicable: effects.is_empty(),
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

    #[test]
    fn pure_transport_is_fission_safe_for_all_scheduling_strategies() {
        let plan = pure_transport_plan();
        for strategy in ALL_STRATEGIES {
            let routed = route(&plan, strategy, true);
            let verified =
                build_event_order_certificate(&plan, &routed, DEFAULT_EVENT_LIMIT).unwrap();
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
        let verified = build_event_order_certificate(&plan, &routed, DEFAULT_EVENT_LIMIT).unwrap();

        let mut order_mutation = verified.certificate().clone();
        order_mutation.vector_order.swap(2, 3);
        assert_eq!(
            verify_event_order_certificate(
                plan.plan(),
                &routed,
                &order_mutation,
                DEFAULT_EVENT_LIMIT
            ),
            Err(VectorEventError::VectorOrderMismatch)
        );

        let mut dependency_mutation = verified.into_certificate();
        dependency_mutation.dependencies.pop();
        assert_eq!(
            verify_event_order_certificate(
                plan.plan(),
                &routed,
                &dependency_mutation,
                DEFAULT_EVENT_LIMIT
            ),
            Err(VectorEventError::DependencyMismatch)
        );
    }

    #[test]
    fn complete_chunk_expansion_obeys_the_explicit_bound() {
        let plan = pure_transport_plan();
        let routed = route(&plan, SchedulingStrategy::DepthFirst, true);
        assert_eq!(
            build_event_order_certificate(&plan, &routed, 16),
            Err(VectorEventError::EventBoundExceeded {
                needed: 17,
                limit: 16,
            })
        );
    }

    #[test]
    fn cross_loop_carried_state_is_rejected_despite_an_effect_edge() {
        let plan = split_state_plan();
        let routed = route(&plan, SchedulingStrategy::DepthFirst, false);
        assert!(matches!(
            build_event_order_certificate(&plan, &routed, DEFAULT_EVENT_LIMIT),
            Err(VectorEventError::FissionSafeViolation { .. })
        ));
    }

    #[test]
    fn conflicting_observable_effects_in_separate_loops_are_rejected() {
        let plan = split_effect_plan(EffectAtom::WriteOutput(0), EffectAtom::WriteOutput(0));
        let routed = route(&plan, SchedulingStrategy::DepthFirst, false);
        assert!(matches!(
            build_event_order_certificate(&plan, &routed, DEFAULT_EVENT_LIMIT),
            Err(VectorEventError::FissionSafeViolation { .. })
        ));
    }

    #[test]
    fn conflicting_state_colocated_in_one_serial_loop_is_accepted() {
        let plan = colocated_state_plan();
        let routed = route(&plan, SchedulingStrategy::DepthFirst, false);
        build_event_order_certificate(&plan, &routed, DEFAULT_EVENT_LIMIT).unwrap();
    }
}
