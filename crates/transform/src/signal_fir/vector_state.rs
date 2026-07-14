//! Verified P6.1 state-transition plans for vector delays and recursion.
//!
//! # C++ provenance and adaptation
//! The storage equations mirror
//! `DAGInstructionsCompiler::generateDlineLoop` and
//! `DAGInstructionsCompiler::generateDelayAccess` in
//! `compiler/generator/dag_instructions_compiler.cpp`: short delay lines use
//! `_tmp`/`_perm` storage with a four-sample-rounded history, while long delay
//! lines use a power-of-two ring plus `_idx`/`_idx_save`. `CodeLoop`'s
//! `fPreCode`, `fExecCode`, and `fPostCode` become explicit phase actions.
//!
//! Recursion follows the simultaneous `RecStep` rule from the vector port
//! plan: every projection of one symbolic group is owned by one serial
//! `LoopKind::Recursive` loop and advances once per sample. The artifact is
//! derived exclusively from checked P4.3b decorations and the checked P4.4
//! vector plan; it does not inspect FIR statements. Clock, reverse-time, and
//! AD state deliberately fail closed until P6.2 supplies their transition
//! semantics.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use sigtype::{Nature, Variability};

use super::decoration_verify::{CanonicalSigType, DecorationRecord, VerifiedDecorationCertificate};
use super::vector_analysis::{EffectAtom, StateCell, StateResource};
use super::vector_clock_ad::VerifiedVectorClockAdPlan;
use super::vector_plan::VerifiedVectorPlan;
use super::vector_verify::{
    LoopKind, Placement, Rate, ValueType, VectorPlan, VectorPlanError, verify_vector_plan,
};

/// Current canonical P6.1 state-plan schema.
pub const VECTOR_STATE_PLAN_VERSION: u32 = 1;

/// One `CodeLoop` execution phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VectorStatePhase {
    Pre,
    Exec,
    Post,
}

/// Exact vector-mode storage selected for one delayed carrier.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VectorDelayStorage {
    /// C++ `_tmp`/`_perm` dual-buffer representation.
    Copy {
        temporary_name: String,
        permanent_name: String,
        history_length: u64,
        temporary_length: u64,
    },
    /// C++ power-of-two ring representation.
    Ring {
        buffer_name: String,
        index_name: String,
        index_save_name: String,
        capacity: u64,
        mask: u64,
    },
}

/// Canonical phase operation. Enum order is also canonical operation order.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VectorStateAction {
    DelayCopyIn { signal_id: u64 },
    DelayRingAdvance { signal_id: u64 },
    RecursionStep { group: u64 },
    DelayWrite { signal_id: u64 },
    DelayCopyOut { signal_id: u64 },
    DelayRingSaveAdvance { signal_id: u64 },
}

impl VectorStateAction {
    /// Phase in which this action must execute.
    #[must_use]
    pub fn phase(&self) -> VectorStatePhase {
        match self {
            Self::DelayCopyIn { .. } | Self::DelayRingAdvance { .. } => VectorStatePhase::Pre,
            Self::RecursionStep { .. } | Self::DelayWrite { .. } => VectorStatePhase::Exec,
            Self::DelayCopyOut { .. } | Self::DelayRingSaveAdvance { .. } => VectorStatePhase::Post,
        }
    }
}

/// Storage transition for one signal whose certified `max_delay` is nonzero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelayTransition {
    pub signal_id: u64,
    pub loop_id: u64,
    pub value_type: ValueType,
    pub max_delay: u64,
    pub storage: VectorDelayStorage,
}

/// One projection participating in a simultaneous recursion step.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecursionProjectionTransition {
    pub index: u64,
    /// Prepared signal aliases that read this one symbolic projection.
    pub signal_ids: Vec<u64>,
}

/// One symbolic recursion group and its serial owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecursionTransition {
    pub group: u64,
    pub loop_id: u64,
    pub projections: Vec<RecursionProjectionTransition>,
}

/// Complete phase bodies for one stateful vector-plan loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopStatePhases {
    pub loop_id: u64,
    pub pre: Vec<VectorStateAction>,
    pub exec: Vec<VectorStateAction>,
    pub post: Vec<VectorStateAction>,
}

/// Canonical finite P6.1 transition artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorStatePlan {
    pub schema_version: u32,
    pub vec_size: u64,
    pub max_copy_delay: u64,
    pub loops: Vec<LoopStatePhases>,
    pub delays: Vec<DelayTransition>,
    pub recursions: Vec<RecursionTransition>,
}

/// Opaque evidence that P6.1 construction passed its independent checker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedVectorStatePlan {
    plan: VectorStatePlan,
    vector_plan: VectorPlan,
    delegated_resources: BTreeSet<StateResource>,
}

impl VerifiedVectorStatePlan {
    #[must_use]
    pub fn plan(&self) -> &VectorStatePlan {
        &self.plan
    }

    #[must_use]
    pub fn vector_plan(&self) -> &VectorPlan {
        &self.vector_plan
    }

    #[must_use]
    pub fn into_plan(self) -> VectorStatePlan {
        self.plan
    }

    /// State resources whose generic P5.3 effects are refined by this plan.
    #[must_use]
    pub fn managed_resources(&self) -> BTreeSet<StateResource> {
        let mut resources = managed_resources(&self.plan);
        resources.extend(self.delegated_resources.iter().cloned());
        resources
    }
}

#[cfg(test)]
pub(crate) fn verified_vector_state_plan_for_test(
    plan: VectorStatePlan,
    vector_plan: &VerifiedVectorPlan,
) -> VerifiedVectorStatePlan {
    verify_phases(&plan).expect("test vector state plan must have canonical phases");
    VerifiedVectorStatePlan {
        plan,
        vector_plan: vector_plan.plan().clone(),
        delegated_resources: BTreeSet::new(),
    }
}

/// Typed producer/checker failure at the P6.1 boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VectorStateError {
    Plan(VectorPlanError),
    UnsupportedSchema { found: u32 },
    VecSizeMismatch { declared: u64, actual: u64 },
    SignalCoverageMismatch { signal_id: u64 },
    SignalFactMismatch { signal_id: u64 },
    MissingLoopOwner { signal_id: u64 },
    DelayOwnerNotVectorLoop { signal_id: u64, loop_id: u64 },
    RecursionLoopMismatch { group: u64, loop_id: u64 },
    UnsupportedClockState { signal_id: u64, clock_id: u32 },
    UnsupportedStateResource { resource: StateResource },
    ArithmeticOverflow { signal_id: u64 },
    NotCanonical { what: &'static str, at: usize },
    DelayCoverageMismatch,
    RecursionCoverageMismatch,
    LoopPhaseMismatch { loop_id: u64 },
    SimulationGeometryMismatch,
    SimulationDelayOutOfRange { delay: usize, max_delay: usize },
    SimulationChunkTooLarge { count: usize, vec_size: usize },
    RecursionArityMismatch { state: usize, next: usize },
}

impl fmt::Display for VectorStateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(error) => write!(f, "vector plan verification failed: {error}"),
            Self::UnsupportedSchema { found } => {
                write!(f, "unsupported vector-state schema {found}")
            }
            Self::VecSizeMismatch { declared, actual } => write!(
                f,
                "state-plan vector size {declared} differs from vector plan {actual}"
            ),
            Self::SignalCoverageMismatch { signal_id } => {
                write!(f, "state-plan source facts do not cover signal {signal_id}")
            }
            Self::SignalFactMismatch { signal_id } => {
                write!(f, "state-plan source facts disagree for signal {signal_id}")
            }
            Self::MissingLoopOwner { signal_id } => {
                write!(f, "stateful signal {signal_id} has no owned loop")
            }
            Self::DelayOwnerNotVectorLoop { signal_id, loop_id } => write!(
                f,
                "delayed signal {signal_id} is owned by non-vector loop {loop_id}"
            ),
            Self::RecursionLoopMismatch { group, loop_id } => write!(
                f,
                "recursion group {group} is not owned by recursive loop {loop_id}"
            ),
            Self::UnsupportedClockState {
                signal_id,
                clock_id,
            } => write!(
                f,
                "clocked state on signal {signal_id} (domain {clock_id}) requires P6.2"
            ),
            Self::UnsupportedStateResource { resource } => {
                write!(
                    f,
                    "state resource {resource:?} requires a later P6 transition model"
                )
            }
            Self::ArithmeticOverflow { signal_id } => {
                write!(f, "delay geometry overflow for signal {signal_id}")
            }
            Self::NotCanonical { what, at } => {
                write!(f, "{what} is not canonical at index {at}")
            }
            Self::DelayCoverageMismatch => write!(f, "delay transitions do not cover source facts"),
            Self::RecursionCoverageMismatch => {
                write!(f, "recursion transitions do not cover source facts")
            }
            Self::LoopPhaseMismatch { loop_id } => {
                write!(f, "pre/exec/post actions are invalid for loop {loop_id}")
            }
            Self::SimulationGeometryMismatch => write!(f, "delay simulation geometry mismatch"),
            Self::SimulationDelayOutOfRange { delay, max_delay } => {
                write!(f, "delay {delay} exceeds certified maximum {max_delay}")
            }
            Self::SimulationChunkTooLarge { count, vec_size } => write!(
                f,
                "chunk length {count} exceeds configured vector size {vec_size}"
            ),
            Self::RecursionArityMismatch { state, next } => write!(
                f,
                "recursion step has arity {next}, expected current state arity {state}"
            ),
        }
    }
}

impl std::error::Error for VectorStateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plan(error) => Some(error),
            _ => None,
        }
    }
}

impl From<VectorPlanError> for VectorStateError {
    fn from(value: VectorPlanError) -> Self {
        Self::Plan(value)
    }
}

/// Builds and independently checks the P6.1 transition plan.
pub fn build_vector_state_plan(
    decorations: &VerifiedDecorationCertificate,
    vector_plan: &VerifiedVectorPlan,
    max_copy_delay: u64,
) -> Result<VerifiedVectorStatePlan, VectorStateError> {
    build_vector_state_plan_with_resources(
        decorations,
        vector_plan,
        max_copy_delay,
        &BTreeSet::new(),
    )
}

/// Builds P6.1 state transitions while delegating clock/hold resources to an
/// independently accepted P6.2 artifact.
pub fn build_vector_state_plan_with_clock(
    decorations: &VerifiedDecorationCertificate,
    vector_plan: &VerifiedVectorPlan,
    clock_plan: &VerifiedVectorClockAdPlan,
    max_copy_delay: u64,
) -> Result<VerifiedVectorStatePlan, VectorStateError> {
    if clock_plan.vector_plan() != vector_plan.plan() {
        return Err(VectorStateError::SignalCoverageMismatch {
            signal_id: u64::MAX,
        });
    }
    build_vector_state_plan_with_resources(
        decorations,
        vector_plan,
        max_copy_delay,
        &clock_plan.managed_state_resources(),
    )
}

fn build_vector_state_plan_with_resources(
    decorations: &VerifiedDecorationCertificate,
    vector_plan: &VerifiedVectorPlan,
    max_copy_delay: u64,
    external_resources: &BTreeSet<StateResource>,
) -> Result<VerifiedVectorStatePlan, VectorStateError> {
    let source = decorations.certificate();
    let plan = vector_plan.plan();
    verify_source_alignment(source.records.as_slice(), plan)?;

    let loops_by_id = plan
        .loops
        .iter()
        .map(|record| (record.loop_id, record))
        .collect::<BTreeMap<_, _>>();
    let signals_by_id = plan
        .signals
        .iter()
        .map(|record| (record.signal_id, record))
        .collect::<BTreeMap<_, _>>();
    let mut phases = BTreeMap::<u64, LoopStatePhases>::new();
    let mut delays = Vec::new();

    for record in source.records.iter().filter(|record| record.max_delay > 0) {
        let signal_id = u64::from(record.signal_id);
        let signal = signals_by_id[&signal_id];
        let Placement::Owned(loop_id) = signal.placement else {
            return Err(VectorStateError::MissingLoopOwner { signal_id });
        };
        let loop_record = loops_by_id[&loop_id];
        if !matches!(
            loop_record.kind,
            LoopKind::Vectorizable | LoopKind::Recursive(_)
        ) {
            return Err(VectorStateError::DelayOwnerNotVectorLoop { signal_id, loop_id });
        }
        let max_delay = u64::from(record.max_delay);
        let storage = delay_storage(signal_id, max_delay, plan.vec_size, max_copy_delay)?;
        let loop_phases = phases
            .entry(loop_id)
            .or_insert_with(|| empty_phases(loop_id));
        match storage {
            VectorDelayStorage::Copy { .. } => {
                loop_phases
                    .pre
                    .push(VectorStateAction::DelayCopyIn { signal_id });
                loop_phases
                    .post
                    .push(VectorStateAction::DelayCopyOut { signal_id });
            }
            VectorDelayStorage::Ring { .. } => {
                loop_phases
                    .pre
                    .push(VectorStateAction::DelayRingAdvance { signal_id });
                loop_phases
                    .post
                    .push(VectorStateAction::DelayRingSaveAdvance { signal_id });
            }
        }
        loop_phases
            .exec
            .push(VectorStateAction::DelayWrite { signal_id });
        delays.push(DelayTransition {
            signal_id,
            loop_id,
            value_type: signal.value_type.clone(),
            max_delay,
            storage,
        });
    }

    let mut recursion_groups = BTreeMap::<u64, BTreeMap<u64, Vec<u64>>>::new();
    for record in source.records.iter().filter_map(|record| {
        record
            .recursive_projection
            .map(|projection| (record, projection))
    }) {
        recursion_groups
            .entry(u64::from(record.1.group))
            .or_default()
            .entry(record.1.index)
            .or_default()
            .push(u64::from(record.0.signal_id));
    }
    let mut recursions = Vec::new();
    for (group, projections) in recursion_groups {
        let projections = projections
            .into_iter()
            .map(|(index, signal_ids)| RecursionProjectionTransition { index, signal_ids })
            .collect();
        let loop_id = recursion_loop(plan, group)?;
        phases
            .entry(loop_id)
            .or_insert_with(|| empty_phases(loop_id))
            .exec
            .push(VectorStateAction::RecursionStep { group });
        recursions.push(RecursionTransition {
            group,
            loop_id,
            projections,
        });
    }

    for loop_phases in phases.values_mut() {
        loop_phases.pre.sort();
        loop_phases.exec.sort();
        loop_phases.post.sort();
    }
    let state_plan = VectorStatePlan {
        schema_version: VECTOR_STATE_PLAN_VERSION,
        vec_size: plan.vec_size,
        max_copy_delay,
        loops: phases.into_values().collect(),
        delays,
        recursions,
    };
    verify_vector_state_plan_with_resources(decorations, plan, &state_plan, external_resources)?;
    Ok(VerifiedVectorStatePlan {
        plan: state_plan,
        vector_plan: plan.clone(),
        delegated_resources: external_resources.clone(),
    })
}

/// Independently checks source alignment, storage equations, coverage, and phases.
pub fn verify_vector_state_plan(
    decorations: &VerifiedDecorationCertificate,
    vector_plan: &VectorPlan,
    state_plan: &VectorStatePlan,
) -> Result<(), VectorStateError> {
    verify_vector_state_plan_with_resources(decorations, vector_plan, state_plan, &BTreeSet::new())
}

/// Checks P6.1 while accepting only the clock/hold resources named by P6.2.
pub fn verify_vector_state_plan_with_clock(
    decorations: &VerifiedDecorationCertificate,
    vector_plan: &VectorPlan,
    clock_plan: &VerifiedVectorClockAdPlan,
    state_plan: &VectorStatePlan,
) -> Result<(), VectorStateError> {
    if clock_plan.vector_plan() != vector_plan {
        return Err(VectorStateError::SignalCoverageMismatch {
            signal_id: u64::MAX,
        });
    }
    verify_vector_state_plan_with_resources(
        decorations,
        vector_plan,
        state_plan,
        &clock_plan.managed_state_resources(),
    )
}

fn verify_vector_state_plan_with_resources(
    decorations: &VerifiedDecorationCertificate,
    vector_plan: &VectorPlan,
    state_plan: &VectorStatePlan,
    external_resources: &BTreeSet<StateResource>,
) -> Result<(), VectorStateError> {
    verify_vector_plan(vector_plan)?;
    if state_plan.schema_version != VECTOR_STATE_PLAN_VERSION {
        return Err(VectorStateError::UnsupportedSchema {
            found: state_plan.schema_version,
        });
    }
    if state_plan.vec_size != vector_plan.vec_size {
        return Err(VectorStateError::VecSizeMismatch {
            declared: state_plan.vec_size,
            actual: vector_plan.vec_size,
        });
    }
    let records = &decorations.certificate().records;
    verify_source_alignment(records, vector_plan)?;
    verify_supported_state(records, state_plan, external_resources)?;
    verify_delays(records, vector_plan, state_plan)?;
    verify_recursions(records, vector_plan, state_plan)?;
    verify_phases(state_plan)?;
    Ok(())
}

fn verify_source_alignment(
    records: &[DecorationRecord],
    plan: &VectorPlan,
) -> Result<(), VectorStateError> {
    if records.len() != plan.signals.len() {
        return Err(VectorStateError::SignalCoverageMismatch {
            signal_id: u64::MAX,
        });
    }
    for (record, signal) in records.iter().zip(&plan.signals) {
        let signal_id = u64::from(record.signal_id);
        if signal.signal_id != signal_id {
            return Err(VectorStateError::SignalCoverageMismatch { signal_id });
        }
        if signal.value_type != value_type(&record.sig_type)
            || signal.rate != rate(record.variability)
            || signal.clock_id != record.clock_domain.map_or(0, |clock| u64::from(clock) + 1)
            || signal.effects != record.effects
        {
            return Err(VectorStateError::SignalFactMismatch { signal_id });
        }
    }
    Ok(())
}

fn verify_supported_state(
    records: &[DecorationRecord],
    state_plan: &VectorStatePlan,
    external_resources: &BTreeSet<StateResource>,
) -> Result<(), VectorStateError> {
    let resources = managed_resources(state_plan);
    for record in records {
        for effect in &record.effects {
            let Some(resource) = state_resource(effect) else {
                continue;
            };
            if resources.contains(resource) {
                if let Some(clock_id) = record.clock_domain {
                    return Err(VectorStateError::UnsupportedClockState {
                        signal_id: u64::from(record.signal_id),
                        clock_id,
                    });
                }
            } else if !external_resources.contains(resource) {
                return Err(VectorStateError::UnsupportedStateResource {
                    resource: resource.clone(),
                });
            }
        }
    }
    Ok(())
}

fn verify_delays(
    records: &[DecorationRecord],
    vector_plan: &VectorPlan,
    state_plan: &VectorStatePlan,
) -> Result<(), VectorStateError> {
    check_strict_by(&state_plan.delays, "delay transitions", |delay| {
        delay.signal_id
    })?;
    let expected = records
        .iter()
        .filter(|record| record.max_delay > 0)
        .map(|record| u64::from(record.signal_id))
        .collect::<Vec<_>>();
    if state_plan
        .delays
        .iter()
        .map(|delay| delay.signal_id)
        .collect::<Vec<_>>()
        != expected
    {
        return Err(VectorStateError::DelayCoverageMismatch);
    }
    let signals = vector_plan
        .signals
        .iter()
        .map(|signal| (signal.signal_id, signal))
        .collect::<BTreeMap<_, _>>();
    let loops = vector_plan
        .loops
        .iter()
        .map(|record| (record.loop_id, record))
        .collect::<BTreeMap<_, _>>();
    for (transition, record) in state_plan
        .delays
        .iter()
        .zip(records.iter().filter(|record| record.max_delay > 0))
    {
        let signal = signals[&transition.signal_id];
        let Placement::Owned(loop_id) = signal.placement else {
            return Err(VectorStateError::MissingLoopOwner {
                signal_id: transition.signal_id,
            });
        };
        if transition.loop_id != loop_id
            || transition.value_type != signal.value_type
            || transition.max_delay != u64::from(record.max_delay)
        {
            return Err(VectorStateError::DelayCoverageMismatch);
        }
        if !matches!(
            loops[&loop_id].kind,
            LoopKind::Vectorizable | LoopKind::Recursive(_)
        ) {
            return Err(VectorStateError::DelayOwnerNotVectorLoop {
                signal_id: transition.signal_id,
                loop_id,
            });
        }
        let expected_storage = delay_storage(
            transition.signal_id,
            transition.max_delay,
            state_plan.vec_size,
            state_plan.max_copy_delay,
        )?;
        if transition.storage != expected_storage {
            return Err(VectorStateError::DelayCoverageMismatch);
        }
    }
    Ok(())
}

fn verify_recursions(
    records: &[DecorationRecord],
    vector_plan: &VectorPlan,
    state_plan: &VectorStatePlan,
) -> Result<(), VectorStateError> {
    check_strict_by(&state_plan.recursions, "recursion transitions", |rec| {
        rec.group
    })?;
    let mut expected = BTreeMap::<u64, BTreeMap<u64, Vec<u64>>>::new();
    for record in records {
        if let Some(projection) = record.recursive_projection {
            expected
                .entry(u64::from(projection.group))
                .or_default()
                .entry(projection.index)
                .or_default()
                .push(u64::from(record.signal_id));
        }
    }
    let expected = expected
        .into_iter()
        .map(|(group, projections)| {
            (
                group,
                projections
                    .into_iter()
                    .map(|(index, signal_ids)| RecursionProjectionTransition { index, signal_ids })
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    if state_plan.recursions.len() != expected.len() {
        return Err(VectorStateError::RecursionCoverageMismatch);
    }
    for transition in &state_plan.recursions {
        if expected.get(&transition.group) != Some(&transition.projections) {
            return Err(VectorStateError::RecursionCoverageMismatch);
        }
        let loop_id = recursion_loop(vector_plan, transition.group)?;
        if transition.loop_id != loop_id {
            return Err(VectorStateError::RecursionLoopMismatch {
                group: transition.group,
                loop_id: transition.loop_id,
            });
        }
        for projection in &transition.projections {
            check_strict_by(
                &projection.signal_ids,
                "recursion projection aliases",
                |id| *id,
            )?;
            for &signal_id in &projection.signal_ids {
                let signal = vector_plan
                    .signals
                    .iter()
                    .find(|signal| signal.signal_id == signal_id)
                    .ok_or(VectorStateError::SignalCoverageMismatch { signal_id })?;
                if signal.placement != Placement::Owned(loop_id) {
                    return Err(VectorStateError::RecursionLoopMismatch {
                        group: transition.group,
                        loop_id,
                    });
                }
            }
        }
    }
    Ok(())
}

fn verify_phases(state_plan: &VectorStatePlan) -> Result<(), VectorStateError> {
    check_strict_by(&state_plan.loops, "stateful loops", |phases| phases.loop_id)?;
    let mut expected = BTreeMap::<u64, LoopStatePhases>::new();
    for delay in &state_plan.delays {
        let phases = expected
            .entry(delay.loop_id)
            .or_insert_with(|| empty_phases(delay.loop_id));
        match delay.storage {
            VectorDelayStorage::Copy { .. } => {
                phases.pre.push(VectorStateAction::DelayCopyIn {
                    signal_id: delay.signal_id,
                });
                phases.post.push(VectorStateAction::DelayCopyOut {
                    signal_id: delay.signal_id,
                });
            }
            VectorDelayStorage::Ring { .. } => {
                phases.pre.push(VectorStateAction::DelayRingAdvance {
                    signal_id: delay.signal_id,
                });
                phases.post.push(VectorStateAction::DelayRingSaveAdvance {
                    signal_id: delay.signal_id,
                });
            }
        }
        phases.exec.push(VectorStateAction::DelayWrite {
            signal_id: delay.signal_id,
        });
    }
    for recursion in &state_plan.recursions {
        expected
            .entry(recursion.loop_id)
            .or_insert_with(|| empty_phases(recursion.loop_id))
            .exec
            .push(VectorStateAction::RecursionStep {
                group: recursion.group,
            });
    }
    for phases in expected.values_mut() {
        phases.pre.sort();
        phases.exec.sort();
        phases.post.sort();
    }
    let expected = expected.into_values().collect::<Vec<_>>();
    if state_plan.loops != expected {
        let loop_id = state_plan
            .loops
            .iter()
            .zip(&expected)
            .find(|(left, right)| left != right)
            .map_or(u64::MAX, |(left, _)| left.loop_id);
        return Err(VectorStateError::LoopPhaseMismatch { loop_id });
    }
    for phases in &state_plan.loops {
        if phases
            .pre
            .iter()
            .any(|action| action.phase() != VectorStatePhase::Pre)
            || phases
                .exec
                .iter()
                .any(|action| action.phase() != VectorStatePhase::Exec)
            || phases
                .post
                .iter()
                .any(|action| action.phase() != VectorStatePhase::Post)
        {
            return Err(VectorStateError::LoopPhaseMismatch {
                loop_id: phases.loop_id,
            });
        }
    }
    Ok(())
}

fn delay_storage(
    signal_id: u64,
    max_delay: u64,
    vec_size: u64,
    max_copy_delay: u64,
) -> Result<VectorDelayStorage, VectorStateError> {
    let base = format!("vstate_s{signal_id}");
    if max_delay < max_copy_delay {
        let history_length = max_delay
            .checked_add(3)
            .map(|value| value & !3)
            .ok_or(VectorStateError::ArithmeticOverflow { signal_id })?;
        let temporary_length = history_length
            .checked_add(vec_size)
            .ok_or(VectorStateError::ArithmeticOverflow { signal_id })?;
        Ok(VectorDelayStorage::Copy {
            temporary_name: format!("{base}_tmp"),
            permanent_name: format!("{base}_perm"),
            history_length,
            temporary_length,
        })
    } else {
        let required = max_delay
            .checked_add(vec_size)
            .ok_or(VectorStateError::ArithmeticOverflow { signal_id })?;
        let capacity = required
            .checked_next_power_of_two()
            .ok_or(VectorStateError::ArithmeticOverflow { signal_id })?;
        Ok(VectorDelayStorage::Ring {
            buffer_name: base.clone(),
            index_name: format!("{base}_idx"),
            index_save_name: format!("{base}_idx_save"),
            capacity,
            mask: capacity - 1,
        })
    }
}

fn recursion_loop(plan: &VectorPlan, group: u64) -> Result<u64, VectorStateError> {
    let matches = plan
        .loops
        .iter()
        .filter(|record| record.kind == LoopKind::Recursive(group))
        .map(|record| record.loop_id)
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(VectorStateError::RecursionLoopMismatch {
            group,
            loop_id: matches.first().copied().unwrap_or(u64::MAX),
        });
    }
    Ok(matches[0])
}

fn empty_phases(loop_id: u64) -> LoopStatePhases {
    LoopStatePhases {
        loop_id,
        pre: Vec::new(),
        exec: Vec::new(),
        post: Vec::new(),
    }
}

fn managed_resources(plan: &VectorStatePlan) -> BTreeSet<StateResource> {
    let mut result = plan
        .delays
        .iter()
        .map(|delay| StateResource::Signal {
            owner: u32::try_from(delay.signal_id).expect("decorated signal id fits u32"),
            cell: StateCell::Delay,
        })
        .collect::<BTreeSet<_>>();
    for recursion in &plan.recursions {
        for projection in &recursion.projections {
            result.insert(StateResource::Recursion {
                group: u32::try_from(recursion.group).expect("decorated recursion group fits u32"),
                projection: u32::try_from(projection.index)
                    .expect("decorated recursion projection fits u32"),
            });
        }
    }
    result
}

fn state_resource(effect: &EffectAtom) -> Option<&StateResource> {
    match effect {
        EffectAtom::ReadState(resource) | EffectAtom::WriteState(resource) => Some(resource),
        _ => None,
    }
}

fn check_strict_by<T, K: Ord>(
    values: &[T],
    what: &'static str,
    key: impl Fn(&T) -> K,
) -> Result<(), VectorStateError> {
    if let Some(at) = values
        .windows(2)
        .position(|pair| key(&pair[0]) >= key(&pair[1]))
    {
        return Err(VectorStateError::NotCanonical { what, at: at + 1 });
    }
    Ok(())
}

fn rate(variability: Variability) -> Rate {
    match variability {
        Variability::Konst => Rate::Konst,
        Variability::Block => Rate::Block,
        Variability::Samp => Rate::Samp,
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

/// Abstract newest-first history transition from the formal port plan.
pub fn history_step<T: Clone>(history: &mut Vec<T>, current: T) {
    if history.is_empty() {
        return;
    }
    history.pop();
    history.insert(0, current);
}

/// Abstract `delayRead`: delay zero is current, delay `n>0` is history `n-1`.
#[must_use]
pub fn delay_read<'a, T>(history: &'a [T], current: &'a T, delay: usize) -> Option<&'a T> {
    if delay == 0 {
        Some(current)
    } else {
        history.get(delay - 1)
    }
}

/// C++ short-delay concrete state used by bounded `DelaySim` checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyDelayState<T> {
    max_delay: usize,
    vec_size: usize,
    permanent: Vec<T>,
}

impl<T: Clone> CopyDelayState<T> {
    pub fn new(
        storage: &VectorDelayStorage,
        max_delay: usize,
        fill: T,
    ) -> Result<Self, VectorStateError> {
        let VectorDelayStorage::Copy {
            history_length,
            temporary_length,
            ..
        } = storage
        else {
            return Err(VectorStateError::SimulationGeometryMismatch);
        };
        let history_length = usize::try_from(*history_length)
            .map_err(|_| VectorStateError::SimulationGeometryMismatch)?;
        let temporary_length = usize::try_from(*temporary_length)
            .map_err(|_| VectorStateError::SimulationGeometryMismatch)?;
        let vec_size = temporary_length
            .checked_sub(history_length)
            .ok_or(VectorStateError::SimulationGeometryMismatch)?;
        if history_length < max_delay {
            return Err(VectorStateError::SimulationGeometryMismatch);
        }
        Ok(Self {
            max_delay,
            vec_size,
            permanent: vec![fill; history_length],
        })
    }

    pub fn process_chunk(
        &mut self,
        input: &[T],
        delays: &[usize],
    ) -> Result<Vec<Vec<T>>, VectorStateError> {
        validate_simulation_request(input.len(), self.vec_size, delays, self.max_delay)?;
        let history_length = self.permanent.len();
        let mut temporary = self.permanent.clone();
        if let Some(fill) = self.permanent.first().cloned() {
            temporary.resize(history_length + self.vec_size, fill);
        } else {
            return Err(VectorStateError::SimulationGeometryMismatch);
        }
        let mut output = Vec::with_capacity(input.len());
        for (sample, value) in input.iter().enumerate() {
            let write = history_length + sample;
            temporary[write] = value.clone();
            output.push(
                delays
                    .iter()
                    .map(|delay| temporary[write - delay].clone())
                    .collect(),
            );
        }
        self.permanent
            .clone_from_slice(&temporary[input.len()..input.len() + history_length]);
        Ok(output)
    }

    /// Abstraction function `alpha`: newest-first semantic history.
    #[must_use]
    pub fn abstract_history(&self) -> Vec<T> {
        self.permanent[self.permanent.len() - self.max_delay..]
            .iter()
            .rev()
            .cloned()
            .collect()
    }
}

/// C++ long-delay concrete state used by bounded `DelaySim` checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RingDelayState<T> {
    max_delay: usize,
    vec_size: usize,
    memory: Vec<T>,
    index: usize,
    index_save: usize,
}

impl<T: Clone> RingDelayState<T> {
    pub fn new(
        storage: &VectorDelayStorage,
        max_delay: usize,
        vec_size: usize,
        fill: T,
    ) -> Result<Self, VectorStateError> {
        let VectorDelayStorage::Ring { capacity, mask, .. } = storage else {
            return Err(VectorStateError::SimulationGeometryMismatch);
        };
        let capacity =
            usize::try_from(*capacity).map_err(|_| VectorStateError::SimulationGeometryMismatch)?;
        if capacity == 0
            || !capacity.is_power_of_two()
            || *mask != u64::try_from(capacity - 1).expect("capacity fits u64")
            || capacity < max_delay.saturating_add(vec_size)
        {
            return Err(VectorStateError::SimulationGeometryMismatch);
        }
        Ok(Self {
            max_delay,
            vec_size,
            memory: vec![fill; capacity],
            index: 0,
            index_save: 0,
        })
    }

    pub fn process_chunk(
        &mut self,
        input: &[T],
        delays: &[usize],
    ) -> Result<Vec<Vec<T>>, VectorStateError> {
        validate_simulation_request(input.len(), self.vec_size, delays, self.max_delay)?;
        let mask = self.memory.len() - 1;
        self.index = (self.index + self.index_save) & mask;
        let mut output = Vec::with_capacity(input.len());
        for (sample, value) in input.iter().enumerate() {
            let write = (self.index + sample) & mask;
            self.memory[write] = value.clone();
            output.push(
                delays
                    .iter()
                    .map(|delay| self.memory[write.wrapping_sub(*delay) & mask].clone())
                    .collect(),
            );
        }
        self.index_save = input.len();
        Ok(output)
    }

    /// Abstraction function `alpha`: newest-first semantic history.
    #[must_use]
    pub fn abstract_history(&self) -> Vec<T> {
        let mask = self.memory.len() - 1;
        let next = (self.index + self.index_save) & mask;
        (1..=self.max_delay)
            .map(|delay| self.memory[next.wrapping_sub(delay) & mask].clone())
            .collect()
    }
}

fn validate_simulation_request(
    count: usize,
    vec_size: usize,
    delays: &[usize],
    max_delay: usize,
) -> Result<(), VectorStateError> {
    if count > vec_size {
        return Err(VectorStateError::SimulationChunkTooLarge { count, vec_size });
    }
    if let Some(&delay) = delays.iter().find(|&&delay| delay > max_delay) {
        return Err(VectorStateError::SimulationDelayOutOfRange { delay, max_delay });
    }
    Ok(())
}

/// Commits one simultaneous recursion transition and returns the old tuple.
pub fn commit_recursion_step<T>(
    state: &mut Vec<T>,
    next: Vec<T>,
) -> Result<Vec<T>, VectorStateError> {
    if state.len() != next.len() {
        return Err(VectorStateError::RecursionArityMismatch {
            state: state.len(),
            next: next.len(),
        });
    }
    Ok(std::mem::replace(state, next))
}

#[cfg(test)]
mod tests {
    use propagate::ClockDomainTable;
    use signals::SigBuilder;
    use tlib::TreeArena;

    use super::*;
    use crate::clk_env::annotate;
    use crate::signal_fir::decoration_verify::certify_decorations;
    use crate::signal_fir::pv_slice::build_pv_signals;
    use crate::signal_fir::vector_plan::build_vector_plan;
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
    fn production_delay_geometry_matches_cpp_threshold_and_rounding() {
        assert!(matches!(
            delay_storage(1, 5, 8, 16).unwrap(),
            VectorDelayStorage::Copy {
                history_length: 8,
                temporary_length: 16,
                ..
            }
        ));
        let (arena, y, z) = build_pv_signals(20);
        let decorations = certify(&arena, &[y, z]);
        let vector_plan = build_vector_plan(&decorations, 8).unwrap();
        let ring = build_vector_state_plan(&decorations, &vector_plan, 16).unwrap();
        let delayed = ring
            .plan()
            .delays
            .iter()
            .find(|delay| delay.max_delay == 20)
            .unwrap();
        assert_eq!(
            delayed.storage,
            VectorDelayStorage::Ring {
                buffer_name: format!("vstate_s{}", delayed.signal_id),
                index_name: format!("vstate_s{}_idx", delayed.signal_id),
                index_save_name: format!("vstate_s{}_idx_save", delayed.signal_id),
                capacity: 32,
                mask: 31,
            }
        );

        let boundary = build_vector_state_plan(&decorations, &vector_plan, 20).unwrap();
        assert!(matches!(
            boundary
                .plan()
                .delays
                .iter()
                .find(|delay| delay.max_delay == 20)
                .unwrap()
                .storage,
            VectorDelayStorage::Ring { .. }
        ));

        let copy = build_vector_state_plan(&decorations, &vector_plan, 32).unwrap();
        let delayed = copy
            .plan()
            .delays
            .iter()
            .find(|delay| delay.max_delay == 20)
            .unwrap();
        assert!(matches!(
            delayed.storage,
            VectorDelayStorage::Copy {
                history_length: 20,
                temporary_length: 28,
                ..
            }
        ));
    }

    #[test]
    fn recursive_projections_share_one_simultaneous_serial_step() {
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
        let vector_plan = build_vector_plan(&decorations, 8).unwrap();
        let state = build_vector_state_plan(&decorations, &vector_plan, 16).unwrap();
        assert_eq!(state.plan().recursions.len(), 1);
        let recursion = &state.plan().recursions[0];
        assert_eq!(
            recursion
                .projections
                .iter()
                .map(|projection| projection.index)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );
        assert_eq!(
            vector_plan
                .plan()
                .loops
                .iter()
                .find(|record| record.loop_id == recursion.loop_id)
                .unwrap()
                .kind,
            LoopKind::Recursive(recursion.group)
        );
        let phases = state
            .plan()
            .loops
            .iter()
            .find(|phases| phases.loop_id == recursion.loop_id)
            .unwrap();
        assert!(phases.exec.contains(&VectorStateAction::RecursionStep {
            group: recursion.group
        }));
    }

    #[test]
    fn independent_checker_rejects_geometry_and_phase_mutations() {
        let (arena, y, z) = build_pv_signals(20);
        let decorations = certify(&arena, &[y, z]);
        let vector_plan = build_vector_plan(&decorations, 8).unwrap();
        let verified = build_vector_state_plan(&decorations, &vector_plan, 16).unwrap();

        let mut geometry = verified.plan().clone();
        let transition = geometry
            .delays
            .iter_mut()
            .find(|delay| delay.max_delay == 20)
            .unwrap();
        if let VectorDelayStorage::Ring { capacity, .. } = &mut transition.storage {
            *capacity = 64;
        }
        assert_eq!(
            verify_vector_state_plan(&decorations, vector_plan.plan(), &geometry),
            Err(VectorStateError::DelayCoverageMismatch)
        );

        let mut phase = verified.into_plan();
        phase.loops[0].exec.clear();
        assert!(matches!(
            verify_vector_state_plan(&decorations, vector_plan.plan(), &phase),
            Err(VectorStateError::LoopPhaseMismatch { .. })
        ));
    }

    #[test]
    fn copy_and_ring_refine_newest_first_history_exhaustively() {
        const CHUNKINGS: [[usize; 4]; 3] = [[4, 4, 0, 0], [1, 3, 2, 2], [3, 4, 1, 0]];
        for max_delay in 1_u64..=5 {
            let vec_size = 4_u64;
            let copy_storage = delay_storage(1, max_delay, vec_size, max_delay + 1).unwrap();
            let ring_storage = delay_storage(1, max_delay, vec_size, 0).unwrap();
            for input_code in 0..3_usize.pow(8) {
                let values = ternary_values(input_code, 8);
                let delays = (0..=max_delay as usize).collect::<Vec<_>>();
                for chunking in CHUNKINGS {
                    let mut abstract_history = vec![0_i32; max_delay as usize];
                    let mut copy =
                        CopyDelayState::new(&copy_storage, max_delay as usize, 0).unwrap();
                    let mut ring = RingDelayState::new(
                        &ring_storage,
                        max_delay as usize,
                        vec_size as usize,
                        0,
                    )
                    .unwrap();
                    let mut start = 0;
                    for count in chunking.into_iter().filter(|count| *count > 0) {
                        let chunk = &values[start..start + count];
                        start += count;
                        let expected = abstract_chunk(&mut abstract_history, chunk, &delays);
                        assert_eq!(copy.process_chunk(chunk, &delays).unwrap(), expected);
                        assert_eq!(ring.process_chunk(chunk, &delays).unwrap(), expected);
                        assert_eq!(copy.abstract_history(), abstract_history);
                        assert_eq!(ring.abstract_history(), abstract_history);
                    }
                    assert_eq!(start, values.len());
                }
            }
        }
    }

    #[test]
    fn recursion_commit_is_simultaneous_and_checks_arity() {
        let mut state = vec![1, 2];
        let next = vec![state[1] + 1, state[0] + 10];
        assert_eq!(commit_recursion_step(&mut state, next).unwrap(), vec![1, 2]);
        assert_eq!(state, vec![3, 11]);
        assert_eq!(
            commit_recursion_step(&mut state, vec![4]),
            Err(VectorStateError::RecursionArityMismatch { state: 2, next: 1 })
        );
    }

    fn ternary_values(mut code: usize, count: usize) -> Vec<i32> {
        (0..count)
            .map(|_| {
                let value = (code % 3) as i32 - 1;
                code /= 3;
                value
            })
            .collect()
    }

    fn abstract_chunk(history: &mut Vec<i32>, input: &[i32], delays: &[usize]) -> Vec<Vec<i32>> {
        input
            .iter()
            .map(|value| {
                let output = delays
                    .iter()
                    .map(|delay| *delay_read(history, value, *delay).unwrap())
                    .collect();
                history_step(history, *value);
                output
            })
            .collect()
    }
}
