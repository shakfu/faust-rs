//! Assembly artifact DTOs, the verified wrapper, the error taxonomy,
//! and total helper conversions shared by producer and checker
//! (plan §4.6: vocabulary and total conversions are shareable).

use crate::signal_fir::vector::clock_ad::{ClockGuard, ClockIsland};
use crate::signal_fir::vector::route::VerifiedRoutedFir;
use crate::signal_fir::vector::state::VectorStateAction;
use crate::signal_fir::vector::verify::VectorPlan;
use fir::FirId;
use std::collections::BTreeSet;
use std::fmt;
/// Current canonical P6.3b/P6.5 assembly schema.
pub const VECTOR_FIR_ASSEMBLY_VERSION: u32 = 3;
/// Already-lowered non-state statements owned by one checked P4 loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorLoopFirInput {
    pub loop_id: u64,
    pub statements: Vec<FirId>,
}
/// One top-rate output store whose value is produced by a held clock island.
///
/// This is an adapted Rust representation of the C++ post-island output write:
/// ownership is explicit rather than inferred from a mutable `CodeLoop` tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorClockOutputStore {
    pub owner_loop_id: u64,
    pub statement: FirId,
}
/// Concrete statement implementing one accepted P6.1 action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorStateFirAction {
    pub action: VectorStateAction,
    pub statement: FirId,
    /// Statements inserted into the enclosing sample scope. Recursion-step
    /// declarations are flattened so subsequent delay writes can read them.
    pub execution_statements: Vec<FirId>,
}
/// One loop body after state-phase materialization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssembledVectorLoop {
    pub loop_id: u64,
    pub pre: Vec<VectorStateFirAction>,
    pub exec: Vec<FirId>,
    pub exec_actions: Vec<VectorStateFirAction>,
    pub post: Vec<VectorStateFirAction>,
    /// Complete outer-chunk execution: `pre; for i0; post`.
    pub chunk_statement: FirId,
    /// One serial iteration used when this loop is nested below a clock guard.
    pub iteration_statement: FirId,
}
/// One nested serial clock domain after guard construction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssembledClockIsland {
    pub domain_id: u64,
    pub parent_domain: Option<u64>,
    pub guard: ClockGuard,
    pub nested_loop_ids: Vec<u64>,
    /// P6.2 `IslandScalar` declarations whose lifetime begins below this guard.
    pub local_declarations: Vec<FirId>,
    /// Optional shared P6.6 state-cursor advance inside this domain guard.
    pub state_cursor_advance: Option<FirId>,
    pub statement: FirId,
}
/// Finite FIR assembly accepted before final module placement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorFirAssembly {
    pub schema_version: u32,
    pub local_declarations: Vec<FirId>,
    pub state_declarations: Vec<FirId>,
    pub clear_statements: Vec<FirId>,
    pub loops: Vec<AssembledVectorLoop>,
    pub islands: Vec<AssembledClockIsland>,
    pub clock_output_stores: Vec<VectorClockOutputStore>,
    pub top_level_statement: FirId,
}
/// Opaque evidence that the P6.3b checker accepted an assembly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedVectorFirAssembly {
    pub(super) assembly: VectorFirAssembly,
    pub(super) vector_plan: VectorPlan,
}
impl VerifiedVectorFirAssembly {
    #[must_use]
    pub fn assembly(&self) -> &VectorFirAssembly {
        &self.assembly
    }

    #[must_use]
    pub fn vector_plan(&self) -> &VectorPlan {
        &self.vector_plan
    }

    #[must_use]
    pub fn into_assembly(self) -> VectorFirAssembly {
        self.assembly
    }
}
/// Typed producer/checker failure at the P6.3b boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum VectorFirAssemblyError {
    PlanMismatch {
        artifact: &'static str,
    },
    ReverseAdRequiresScalar {
        signal_id: u64,
    },
    LoopInputCoverage {
        loop_id: u64,
    },
    DuplicateLoopInput {
        loop_id: u64,
    },
    MissingDefinition {
        signal_id: u64,
        loop_id: u64,
    },
    MissingRecursionProjection {
        group: u64,
        index: u64,
    },
    LoopStateCoverage {
        loop_id: u64,
    },
    ClockLoopOwnership {
        loop_id: u64,
    },
    MissingClockValue {
        domain_id: u64,
        signal_id: u64,
    },
    MissingClockParent {
        domain_id: u64,
        parent_id: u64,
    },
    ArithmeticOverflow {
        what: &'static str,
        value: u64,
    },
    UnsupportedValueType {
        signal_id: u64,
    },
    DeclarationShape {
        name: String,
    },
    ActionShape {
        loop_id: u64,
        action: VectorStateAction,
    },
    IslandShape {
        domain_id: u64,
    },
    FusedGroupShape {
        group_id: u64,
    },
    /// A verified lockstep bundle was not emitted as one physical sample loop.
    LockstepBundleShape {
        bundle_id: u64,
    },
    TopLevelShape,
}
impl fmt::Display for VectorFirAssemblyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlanMismatch { artifact } => {
                write!(f, "{artifact} does not belong to the routed vector plan")
            }
            Self::ReverseAdRequiresScalar { signal_id } => write!(
                f,
                "signal {signal_id} requires the certified scalar reverse-AD window"
            ),
            Self::LoopInputCoverage { loop_id } => {
                write!(f, "loop FIR inputs do not cover loop {loop_id} exactly")
            }
            Self::DuplicateLoopInput { loop_id } => {
                write!(f, "loop FIR input {loop_id} is duplicated")
            }
            Self::MissingDefinition { signal_id, loop_id } => write!(
                f,
                "stateful signal {signal_id} has no routed definition in loop {loop_id}"
            ),
            Self::MissingRecursionProjection { group, index } => write!(
                f,
                "recursion group {group} projection {index} has no routed definition"
            ),
            Self::LoopStateCoverage { loop_id } => {
                write!(f, "assembled state actions disagree for loop {loop_id}")
            }
            Self::ClockLoopOwnership { loop_id } => {
                write!(f, "loop {loop_id} belongs to more than one clock island")
            }
            Self::MissingClockValue {
                domain_id,
                signal_id,
            } => write!(
                f,
                "clock island {domain_id} cannot resolve clock signal {signal_id}"
            ),
            Self::MissingClockParent {
                domain_id,
                parent_id,
            } => write!(
                f,
                "clock island {domain_id} references missing parent {parent_id}"
            ),
            Self::ArithmeticOverflow { what, value } => {
                write!(f, "{what} value {value} does not fit FIR i32 arithmetic")
            }
            Self::UnsupportedValueType { signal_id } => {
                write!(f, "signal {signal_id} has tuple-valued state storage")
            }
            Self::DeclarationShape { name } => {
                write!(f, "assembled declaration {name} has an invalid FIR shape")
            }
            Self::ActionShape { loop_id, action } => {
                write!(f, "loop {loop_id} action {action:?} has invalid FIR")
            }
            Self::IslandShape { domain_id } => {
                write!(f, "clock island {domain_id} has an invalid FIR guard")
            }
            Self::FusedGroupShape { group_id } => {
                write!(f, "fused serial group {group_id} has an invalid FIR shape")
            }
            Self::LockstepBundleShape { bundle_id } => {
                write!(f, "lockstep bundle {bundle_id} has an invalid FIR shape")
            }
            Self::TopLevelShape => write!(f, "assembled top-level FIR is not a block"),
        }
    }
}
impl std::error::Error for VectorFirAssemblyError {}
pub(super) fn scheduled_island_loop_ids(
    routed: &VerifiedRoutedFir,
    island: &ClockIsland,
) -> Vec<u64> {
    let members = island
        .nested_loop_ids
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    routed
        .layout()
        .loops()
        .iter()
        .filter_map(|region| members.contains(&region.loop_id).then_some(region.loop_id))
        .collect()
}
pub(super) fn recursion_name(group: u64, index: u64) -> String {
    format!("vrec_g{group}_p{index}_next")
}
