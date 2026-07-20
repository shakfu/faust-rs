//! State-plan vocabulary (storage, transitions, phases), the verified
//! wrapper, and the error taxonomy (schema v4).

use super::check::managed_resources;
#[cfg(test)]
use super::check::verify_phases;
use crate::signal_fir::vector::analysis::StateResource;
#[cfg(test)]
use crate::signal_fir::vector::plan::VerifiedVectorPlan;
use crate::signal_fir::vector::verify::{ValueType, VectorPlan, VectorPlanError};
use std::collections::BTreeSet;
use std::fmt;

/// Current canonical P6.1 state-plan schema.
pub const VECTOR_STATE_PLAN_VERSION: u32 = 4;
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
    /// Delay-one state carried in a scalar local for one certified lockstep
    /// lane. Eligibility is bound by [`LockstepRegisterBundle`] rather than
    /// inferred from these generated names.
    Register {
        local_name: String,
        persistent_name: String,
        bundle_id: u64,
        lane: u64,
    },
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
    /// Persistent ring indexed by one shared clock-domain fire-time cursor.
    ClockRing {
        buffer_name: String,
        cursor_name: String,
        domain_id: u64,
        capacity: u64,
        mask: u64,
    },
}
/// Canonical phase operation. Enum order is also canonical operation order.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VectorStateAction {
    DelayRegisterLoad { signal_id: u64 },
    DelayCopyIn { signal_id: u64 },
    DelayRingAdvance { signal_id: u64 },
    RecursionStep { group: u64 },
    DelayWrite { signal_id: u64 },
    PrefixWrite { signal_id: u64 },
    WaveformAdvance { signal_id: u64 },
    DelayRegisterStore { signal_id: u64 },
    DelayCopyOut { signal_id: u64 },
    DelayRingSaveAdvance { signal_id: u64 },
}
impl VectorStateAction {
    /// Phase in which this action must execute.
    #[must_use]
    pub fn phase(&self) -> VectorStatePhase {
        match self {
            Self::DelayRegisterLoad { .. }
            | Self::DelayCopyIn { .. }
            | Self::DelayRingAdvance { .. } => VectorStatePhase::Pre,
            Self::RecursionStep { .. }
            | Self::DelayWrite { .. }
            | Self::PrefixWrite { .. }
            | Self::WaveformAdvance { .. } => VectorStatePhase::Exec,
            Self::DelayRegisterStore { .. }
            | Self::DelayCopyOut { .. }
            | Self::DelayRingSaveAdvance { .. } => VectorStatePhase::Post,
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
    /// `None` for top-rate chunk time, `Some(d)` for domain fire time.
    pub clock_domain: Option<u64>,
    pub storage: VectorDelayStorage,
}
/// Canonical lifecycle initializer for one scalar state cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VectorStateInitialValue {
    Int(i32),
    RealBits(u64),
    Zero,
}
/// One `prefix(init, value)` previous-sample state cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrefixTransition {
    pub signal_id: u64,
    pub loop_id: u64,
    pub value_signal_id: u64,
    pub state_name: String,
    pub value_type: ValueType,
    pub initial: VectorStateInitialValue,
}
/// One cycling direct waveform read index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaveformTransition {
    pub signal_id: u64,
    pub loop_id: u64,
    pub index_name: String,
    pub length: u64,
    pub value_type: ValueType,
}
/// One projection participating in a simultaneous recursion step.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecursionProjectionTransition {
    pub index: u64,
    /// Prepared signal aliases that read this one symbolic projection.
    pub signal_ids: Vec<u64>,
    /// Signal computing the next value. This is the first visible projection
    /// alias when one exists, otherwise the recursive body itself.
    pub value_signal_id: u64,
}
/// One symbolic recursion group and its serial owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecursionTransition {
    pub group: u64,
    pub loop_id: u64,
    pub projections: Vec<RecursionProjectionTransition>,
}
/// One ordered register-carried lane in a certified lockstep bundle.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LockstepRegisterLane {
    pub lane: u64,
    pub loop_id: u64,
    pub recursion_group: u64,
    pub signal_id: u64,
    pub local_name: String,
    pub persistent_name: String,
}
/// Checked bundle-level identity for register-carried delay-one state.
///
/// This record is an adapted Rust representation of the scalar state carried
/// by Faust C++ `CodeLoop` execution. It prevents assembly from deciding
/// eligibility from variable names and keeps lane order, state identity, and
/// persistent boundaries co-located.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LockstepRegisterBundle {
    pub bundle_id: u64,
    pub lanes: Vec<LockstepRegisterLane>,
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
    pub lockstep_register_bundles: Vec<LockstepRegisterBundle>,
    pub prefixes: Vec<PrefixTransition>,
    pub waveforms: Vec<WaveformTransition>,
    /// Conservative delay effects proven to have no positive-delay use.
    /// They require no storage or runtime action (`x @ 0 == x`).
    pub no_op_resources: Vec<StateResource>,
}
/// Opaque evidence that P6.1 construction passed its independent checker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedVectorStatePlan {
    pub(super) plan: VectorStatePlan,
    pub(super) vector_plan: VectorPlan,
    pub(super) delegated_resources: BTreeSet<StateResource>,
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
    UnsupportedSchema {
        found: u32,
    },
    VecSizeMismatch {
        declared: u64,
        actual: u64,
    },
    SignalCoverageMismatch {
        signal_id: u64,
    },
    SignalFactMismatch {
        signal_id: u64,
    },
    MissingLoopOwner {
        signal_id: u64,
    },
    DelayOwnerNotVectorLoop {
        signal_id: u64,
        loop_id: u64,
    },
    RecursionLoopMismatch {
        group: u64,
        loop_id: u64,
    },
    UnsupportedClockState {
        signal_id: u64,
        clock_id: u32,
    },
    ClockPlanRequired {
        signal_id: u64,
        clock_id: u32,
    },
    ClockLoopMismatch {
        signal_id: u64,
        clock_id: u64,
        loop_id: u64,
    },
    UnsupportedStateResource {
        resource: StateResource,
    },
    ArithmeticOverflow {
        signal_id: u64,
    },
    NotCanonical {
        what: &'static str,
        at: usize,
    },
    DelayCoverageMismatch,
    RecursionCoverageMismatch,
    LoopPhaseMismatch {
        loop_id: u64,
    },
    SimulationGeometryMismatch,
    SimulationDelayOutOfRange {
        delay: usize,
        max_delay: usize,
    },
    SimulationChunkTooLarge {
        count: usize,
        vec_size: usize,
    },
    RecursionArityMismatch {
        state: usize,
        next: usize,
    },
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
            Self::ClockPlanRequired {
                signal_id,
                clock_id,
            } => write!(
                f,
                "clocked state on signal {signal_id} (domain {clock_id}) requires a checked P6.2 plan"
            ),
            Self::ClockLoopMismatch {
                signal_id,
                clock_id,
                loop_id,
            } => write!(
                f,
                "clocked state signal {signal_id} is owned by loop {loop_id}, outside domain {clock_id}"
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
