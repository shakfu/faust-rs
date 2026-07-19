//! `VectorPlanError`: the stable error taxonomy of the plan checker.

use super::model::*;
use std::fmt;

/// Why [`verify_vector_plan`] rejected a plan. One variant per checked
/// obligation, so each has a demonstrated rejecting mutation (plan §8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VectorPlanError {
    /// The verifier accepts only the exact v2 schema.
    UnsupportedSchema { found: u32 },
    /// `vec_size` must be positive.
    VecSizeZero,
    /// A set-like array is not in its required canonical order (also catches
    /// duplicates, since canonical order is *strictly* ascending).
    NotCanonical { what: &'static str, at: usize },
    /// A loop appears in more than one epoch, or a plan loop is in none.
    EpochCoverageMismatch { loop_id: u64 },
    /// An epoch lists a loop id that is not a plan loop.
    EpochLoopUnknown { epoch_id: u64, loop_id: u64 },
    /// A signal placed `Owned(l)` is absent from `l`'s roots.
    OwnedSignalNotRoot { signal_id: u64, loop_id: u64 },
    /// A root of loop `l` is not placed `Owned(l)`.
    RootWithoutOwnership { signal_id: u64, loop_id: u64 },
    /// A root references a signal id that is not a plan signal.
    RootUnknownSignal { loop_id: u64, signal_id: u64 },
    /// An `Inline`-placed signal is not `duplicable`.
    InlineNotDuplicable { signal_id: u64 },
    /// The producer-supplied `duplicable` bit disagrees with the effect facts.
    DuplicabilityMismatch { signal_id: u64 },
    /// A loop's redundant `epoch_id` disagrees with canonical epoch membership.
    LoopEpochMismatch {
        loop_id: u64,
        declared: u64,
        actual: u64,
    },
    /// A data/effect edge references a loop id that is not a plan loop.
    EdgeEndpointUnknown { edge: LoopEdge, missing: u64 },
    /// A loop depends on itself (an instantaneous self-edge).
    LoopSelfEdge { loop_id: u64 },
    /// The induced graph of one epoch contains a cycle.
    EpochNotAcyclic { epoch_id: u64, remaining: Vec<u64> },
    /// Two loops have conflicting effects but neither is ordered before the
    /// other by the combined data/effect relation.
    UnorderedEffectConflict { left: u64, right: u64 },
    /// A transport's producer and consumer loops are the same.
    TransportSelfLoop { transport_id: u64 },
    /// A transport's element type does not equal its signal's value type.
    TransportTypeMismatch { transport_id: u64 },
    /// A transport's array length does not equal `vec_size`.
    TransportLengthMismatch { transport_id: u64 },
    /// An interleaved transport has no matching lockstep width.
    TransportLayoutMismatch { transport_id: u64 },
    /// A lockstep bundle has fewer than two lanes or inconsistent width.
    LockstepWidthMismatch { bundle_id: u64 },
    /// A lockstep bundle references a missing loop or repeats a member.
    LockstepMemberMismatch { bundle_id: u64, loop_id: u64 },
    /// A lane record does not correspond exactly to one bundle member.
    LockstepLaneMismatch { bundle_id: u64, loop_id: u64 },
    /// Two candidate lanes are connected in the epoch dependence graph.
    LockstepDependentLanes {
        bundle_id: u64,
        left: u64,
        right: u64,
    },
    /// Two candidate lanes do not share the same epoch and clock.
    LockstepDomainMismatch {
        bundle_id: u64,
        left: u64,
        right: u64,
    },
    /// Two candidate lanes have non-commuting effects.
    LockstepEffectConflict {
        bundle_id: u64,
        left: u64,
        right: u64,
    },
    /// A root/leaf witness is not canonical or references signals outside its
    /// declared lane roots. Prepared-tree shape is checked by the second gate.
    LockstepIsoWitnessMismatch { bundle_id: u64, loop_id: u64 },
    /// A cross-epoch edge whose dependency epoch has a strictly greater rank
    /// than its consumer epoch (a barrier run backwards).
    BarrierViolation { edge: LoopEdge },
    /// A `Recursive`/`Island` loop carries a `pointwise` witness, or is
    /// otherwise asserted vector-safe in a way that contradicts its serial
    /// kind.
    SerialLoopNotSerial { loop_id: u64 },
    /// A `Vectorizable` loop has no `VecSafe` witness.
    VectorizableWithoutWitness { loop_id: u64 },
    /// A vectorizable loop's roots do not satisfy the concrete `VecSafe` rule.
    VectorizableNotSafe { loop_id: u64 },
    /// A `VecSafe` witness references a loop id that is not a plan loop.
    WitnessUnknownLoop { loop_id: u64 },
    /// A transport references a signal or loop id that is not in the plan.
    TransportUnknownRef { transport_id: u64, missing: u64 },
    /// A required set-like fused-group field is empty.
    FusedGroupEmpty { group_id: u64, what: &'static str },
    /// A fused group references a loop absent from the plan.
    FusedGroupUnknownLoop { group_id: u64, loop_id: u64 },
    /// The owner is not included in the group's members.
    FusedGroupOwnerNotMember { group_id: u64, owner_loop_id: u64 },
    /// One loop belongs to two fused serial groups.
    FusedGroupLoopOverlap { loop_id: u64 },
    /// A fused group references a signal absent from the plan.
    FusedGroupUnknownSignal { group_id: u64, signal_id: u64 },
    /// A grouped signal is not owned by one of the group's member loops.
    FusedGroupSignalOutside { group_id: u64, signal_id: u64 },
    /// A rematerialized transport id is absent from the plan.
    FusedGroupUnknownTransport { group_id: u64, transport_id: u64 },
    /// A rematerialized transport does not stay within its group.
    FusedGroupTransportOutside { group_id: u64, transport_id: u64 },
    /// A rematerialized transport does not carry one of the certified delayed
    /// reads.
    FusedGroupTransportNotDelayedRead { group_id: u64, transport_id: u64 },
    /// The selected owner is not the canonical owner of a certified carrier.
    FusedGroupOwnerNotStateCarrier { group_id: u64, owner_loop_id: u64 },
    /// The decoration facts do not identify the declared carrier as delayed
    /// state.
    FusedGroupCarrierNotDelayedState { group_id: u64, signal_id: u64 },
    /// A delayed read lacks the declared `DepKind::Delayed` carrier edge.
    FusedGroupDelayedDependencyMissing { group_id: u64, signal_id: u64 },
    /// A same-sample path from a delayed read to its recursive writer leaves
    /// the fused serial group.
    FusedGroupPathIncomplete { group_id: u64, loop_id: u64 },
    /// A declared state writer does not match its recursive member loop.
    FusedGroupStateWriterMismatch { group_id: u64, signal_id: u64 },
    /// A recursive member loop has no matching projection writer in the group.
    FusedGroupRecursiveMemberMissingWriter { group_id: u64, loop_id: u64 },
    /// Grouped signals or loops cross incompatible clock domains.
    FusedGroupClockMismatch { group_id: u64 },
    /// An active chunk transport still materializes a delayed read internally.
    FusedGroupDangerousTransportPresent { group_id: u64, transport_id: u64 },
    /// An independently reconstructed immediate-delay crossing is uncovered.
    FusedGroupDangerousCrossingMissing { producer: u64, consumer: u64 },
}
impl fmt::Display for VectorPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchema { found } => {
                write!(f, "unsupported vector-plan schema {found}")
            }
            Self::VecSizeZero => write!(f, "vec_size must be positive"),
            Self::NotCanonical { what, at } => {
                write!(f, "{what} is not strictly ascending at index {at}")
            }
            Self::EpochCoverageMismatch { loop_id } => {
                write!(f, "loop {loop_id} is not owned by exactly one epoch")
            }
            Self::EpochLoopUnknown { epoch_id, loop_id } => {
                write!(f, "epoch {epoch_id} lists unknown loop {loop_id}")
            }
            Self::OwnedSignalNotRoot { signal_id, loop_id } => write!(
                f,
                "signal {signal_id} is Owned({loop_id}) but not a root of that loop"
            ),
            Self::RootWithoutOwnership { signal_id, loop_id } => write!(
                f,
                "signal {signal_id} is a root of loop {loop_id} but not placed Owned({loop_id})"
            ),
            Self::RootUnknownSignal { loop_id, signal_id } => {
                write!(f, "loop {loop_id} root {signal_id} is not a plan signal")
            }
            Self::InlineNotDuplicable { signal_id } => {
                write!(f, "Inline signal {signal_id} is not duplicable")
            }
            Self::DuplicabilityMismatch { signal_id } => write!(
                f,
                "signal {signal_id} duplicable bit disagrees with its effects"
            ),
            Self::LoopEpochMismatch {
                loop_id,
                declared,
                actual,
            } => write!(
                f,
                "loop {loop_id} declares epoch {declared} but belongs to epoch {actual}"
            ),
            Self::EdgeEndpointUnknown { edge, missing } => {
                write!(f, "edge {edge:?} references unknown loop {missing}")
            }
            Self::LoopSelfEdge { loop_id } => write!(f, "loop {loop_id} depends on itself"),
            Self::EpochNotAcyclic {
                epoch_id,
                remaining,
            } => write!(
                f,
                "epoch {epoch_id} induced graph has a cycle: {remaining:?}"
            ),
            Self::UnorderedEffectConflict { left, right } => write!(
                f,
                "loops {left} and {right} have conflicting unordered effects"
            ),
            Self::TransportSelfLoop { transport_id } => {
                write!(f, "transport {transport_id} producer == consumer")
            }
            Self::TransportTypeMismatch { transport_id } => write!(
                f,
                "transport {transport_id} element type != its signal's value type"
            ),
            Self::TransportLengthMismatch { transport_id } => {
                write!(f, "transport {transport_id} length != vec_size")
            }
            Self::TransportLayoutMismatch { transport_id } => {
                write!(f, "transport {transport_id} has an invalid lockstep layout")
            }
            Self::LockstepWidthMismatch { bundle_id } => {
                write!(f, "lockstep bundle {bundle_id} has an inconsistent width")
            }
            Self::LockstepMemberMismatch { bundle_id, loop_id } => write!(
                f,
                "lockstep bundle {bundle_id} has invalid member loop {loop_id}"
            ),
            Self::LockstepLaneMismatch { bundle_id, loop_id } => write!(
                f,
                "lockstep bundle {bundle_id} has invalid lane loop {loop_id}"
            ),
            Self::LockstepDependentLanes {
                bundle_id,
                left,
                right,
            } => write!(
                f,
                "lockstep bundle {bundle_id} lanes {left} and {right} are dependency-connected"
            ),
            Self::LockstepDomainMismatch {
                bundle_id,
                left,
                right,
            } => write!(
                f,
                "lockstep bundle {bundle_id} lanes {left} and {right} disagree on epoch or clock"
            ),
            Self::LockstepEffectConflict {
                bundle_id,
                left,
                right,
            } => write!(
                f,
                "lockstep bundle {bundle_id} lanes {left} and {right} have conflicting effects"
            ),
            Self::LockstepIsoWitnessMismatch { bundle_id, loop_id } => write!(
                f,
                "lockstep bundle {bundle_id} lane {loop_id} has an invalid isomorphism witness"
            ),
            Self::BarrierViolation { edge } => {
                write!(f, "cross-epoch edge {edge:?} runs a barrier backwards")
            }
            Self::SerialLoopNotSerial { loop_id } => {
                write!(
                    f,
                    "serial (recursive/island) loop {loop_id} asserted vector-safe"
                )
            }
            Self::VectorizableWithoutWitness { loop_id } => {
                write!(f, "vectorizable loop {loop_id} has no VecSafe witness")
            }
            Self::VectorizableNotSafe { loop_id } => {
                write!(f, "vectorizable loop {loop_id} does not satisfy VecSafe")
            }
            Self::WitnessUnknownLoop { loop_id } => {
                write!(f, "VecSafe witness references unknown loop {loop_id}")
            }
            Self::TransportUnknownRef {
                transport_id,
                missing,
            } => write!(
                f,
                "transport {transport_id} references unknown id {missing}"
            ),
            Self::FusedGroupEmpty { group_id, what } => {
                write!(f, "fused group {group_id} has empty {what}")
            }
            Self::FusedGroupUnknownLoop { group_id, loop_id } => {
                write!(
                    f,
                    "fused group {group_id} references unknown loop {loop_id}"
                )
            }
            Self::FusedGroupOwnerNotMember {
                group_id,
                owner_loop_id,
            } => write!(
                f,
                "fused group {group_id} owner loop {owner_loop_id} is not a member"
            ),
            Self::FusedGroupLoopOverlap { loop_id } => {
                write!(f, "loop {loop_id} belongs to more than one fused group")
            }
            Self::FusedGroupUnknownSignal {
                group_id,
                signal_id,
            } => write!(
                f,
                "fused group {group_id} references unknown signal {signal_id}"
            ),
            Self::FusedGroupSignalOutside {
                group_id,
                signal_id,
            } => write!(
                f,
                "fused group {group_id} signal {signal_id} is outside its member loops"
            ),
            Self::FusedGroupUnknownTransport {
                group_id,
                transport_id,
            } => write!(
                f,
                "fused group {group_id} references unknown transport {transport_id}"
            ),
            Self::FusedGroupTransportOutside {
                group_id,
                transport_id,
            } => write!(
                f,
                "fused group {group_id} transport {transport_id} crosses its member boundary"
            ),
            Self::FusedGroupTransportNotDelayedRead {
                group_id,
                transport_id,
            } => write!(
                f,
                "fused group {group_id} transport {transport_id} is not a delayed read"
            ),
            Self::FusedGroupOwnerNotStateCarrier {
                group_id,
                owner_loop_id,
            } => write!(
                f,
                "fused group {group_id} owner loop {owner_loop_id} is not the canonical state-carrier owner"
            ),
            Self::FusedGroupCarrierNotDelayedState {
                group_id,
                signal_id,
            } => write!(
                f,
                "fused group {group_id} carrier {signal_id} is not certified delayed state"
            ),
            Self::FusedGroupDelayedDependencyMissing {
                group_id,
                signal_id,
            } => write!(
                f,
                "fused group {group_id} read {signal_id} lacks its delayed carrier dependency"
            ),
            Self::FusedGroupPathIncomplete { group_id, loop_id } => {
                write!(f, "fused group {group_id} omits data-path loop {loop_id}")
            }
            Self::FusedGroupStateWriterMismatch {
                group_id,
                signal_id,
            } => write!(
                f,
                "fused group {group_id} writer {signal_id} does not match a recursive member loop"
            ),
            Self::FusedGroupRecursiveMemberMissingWriter { group_id, loop_id } => write!(
                f,
                "fused group {group_id} recursive member loop {loop_id} has no state writer"
            ),
            Self::FusedGroupClockMismatch { group_id } => {
                write!(f, "fused group {group_id} crosses clock domains")
            }
            Self::FusedGroupDangerousTransportPresent {
                group_id,
                transport_id,
            } => write!(
                f,
                "fused group {group_id} still materializes delayed read transport {transport_id}"
            ),
            Self::FusedGroupDangerousCrossingMissing { producer, consumer } => write!(
                f,
                "state-mediated immediate delay crossing {producer} -> {consumer} is not covered by a fused group"
            ),
        }
    }
}
impl std::error::Error for VectorPlanError {}
