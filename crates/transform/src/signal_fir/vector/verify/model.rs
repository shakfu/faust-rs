//! Canonical `VectorPlan` DTO vocabulary (schema v4).
//!
//! Pure data: producers construct these records; the independent checker
//! re-derives every invariant from them. No construction or checking logic
//! lives here. Serialization field order and the schema version are frozen.

use super::super::analysis::EffectAtom;

/// Current in-memory vector-plan schema. Version 4 records, per signal, the
/// effects the signal performs itself alongside its transitive closure, so the
/// event model can attribute an operation to its performer rather than to every
/// carrier. Version 3 generalized fused serial groups from one recursive
/// carrier to a canonical delayed-state carrier set.
pub const VECTOR_PLAN_SCHEMA_VERSION: u32 = 4;
/// `$defs/signalType`: the v1 value-type vocabulary (matches the Lean
/// `ValueTy`). FIR widths / `FaustFloat` live in the routed-FIR layer, not
/// here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueType {
    Int,
    Real,
    /// Soundfile handle payload, derived from the signal shape exactly as the
    /// prepared boundary derives `SimpleSigType::Sound`.
    Sound,
    Tuple(Vec<ValueType>),
}
/// `signalRecord.rate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rate {
    Konst,
    Block,
    Samp,
}
/// `signalRecord.vectorability`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vectorability {
    Vect,
    Scal,
    TrueScal,
}
/// `$defs/placement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Control,
    Inline,
    Owned(u64),
}
/// `$defs/loopKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopKind {
    Vectorizable,
    Recursive(u64),
    Island(u64),
    /// One logical lane of a checked lockstep bundle. The lane loop identities
    /// remain in the plan so the verifier can re-check independence; scheduling
    /// and FIR assembly treat the bundle as one physical execution unit.
    Lockstep {
        width: u64,
    },
}
/// `vecSafeWitness.witness_kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WitnessKind {
    Pointwise,
    SerialStateInternal,
    ProvenIntrinsic,
}
/// `$defs/signalRecord`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalRecord {
    pub signal_id: u64,
    pub value_type: ValueType,
    /// True only for checked symbolic recursion carriers with no runtime FIR value.
    pub structural: bool,
    pub rate: Rate,
    pub vectorability: Vectorability,
    pub clock_id: u64,
    pub effects: Vec<EffectAtom>,
    /// Effects this signal performs itself, a sorted subset of `effects`.
    ///
    /// The event model attributes an effect operation to the signal that
    /// performs it. Attributing it to every transitive carrier instead invents
    /// conflicts between signals that only contain the performer in their
    /// subtree. `duplicable` keeps reading `effects`: a signal whose subtree
    /// has any effect must not be duplicated, which is what keeps performers
    /// unique.
    pub direct_effects: Vec<EffectAtom>,
    pub placement: Placement,
    pub duplicable: bool,
}
/// `$defs/loopRecord`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopRecord {
    pub loop_id: u64,
    pub stable_name: String,
    pub kind: LoopKind,
    pub roots: Vec<u64>,
    pub epoch_id: u64,
}
/// `$defs/epochRecord`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpochRecord {
    pub epoch_id: u64,
    pub rank: u64,
    pub loops: Vec<u64>,
}
/// `$defs/transportRecord`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportRecord {
    pub transport_id: u64,
    pub stable_name: String,
    pub signal_id: u64,
    pub producer_loop: u64,
    pub consumer_loop: u64,
    pub element_type: ValueType,
    pub length: u64,
    pub layout: TransportLayout,
}
/// Chunk-local transport layout. The external `compute` ABI remains planar;
/// `Interleaved` is legal only for a checked lockstep bundle of matching width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportLayout {
    Planar,
    Interleaved(u64),
}
/// One explicit leaf correspondence in an isomorphism witness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct IsoLeafMapping {
    pub representative_signal_id: u64,
    pub lane_signal_id: u64,
}
/// Root-level isomorphism evidence for one non-representative lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsoRootWitness {
    pub representative_root: u64,
    pub lane_root: u64,
    pub shape_hash: u64,
    pub leaf_mapping: Vec<IsoLeafMapping>,
}
/// One logical lane retained inside a lockstep bundle certificate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockstepLaneRecord {
    pub loop_id: u64,
    pub recursion_group: u64,
    pub roots: Vec<IsoRootWitness>,
}
/// Canonical lockstep bundle. `member_loop_ids` are scheduled as one unit and
/// assembled under one physical sample loop, while remaining explicit in the
/// plan for legality checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockstepBundleRecord {
    pub bundle_id: u64,
    pub representative_loop_id: u64,
    pub member_loop_ids: Vec<u64>,
    pub lanes: Vec<LockstepLaneRecord>,
}
/// `$defs/fusedSerialGroupRecord`.
///
/// A group preserves the original loop identities while requiring their
/// delayed state work to be emitted as one serial per-sample unit. Generic
/// delayed-state and recursive owners may share one group; `owner_loop_id` is
/// its canonical minimum carrier owner. `state_carrier_signal_ids` is the
/// non-empty, strictly ascending, complete set of positive-delay carriers,
/// while `state_write_signal_ids` covers every carrier and recursive member.
/// An `internal_transport_id` retains a planned transport identity whose
/// access is rematerialized as a scalar value inside that unit. All members
/// and grouped signals must have one exact clock id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FusedSerialGroupRecord {
    pub group_id: u64,
    pub owner_loop_id: u64,
    pub member_loop_ids: Vec<u64>,
    pub state_carrier_signal_ids: Vec<u64>,
    pub delayed_read_signal_ids: Vec<u64>,
    pub state_write_signal_ids: Vec<u64>,
    pub internal_transport_ids: Vec<u64>,
    pub output_or_transport_roots: Vec<u64>,
}
/// `$defs/vecSafeWitness`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VecSafeWitness {
    pub loop_id: u64,
    pub witness_kind: WitnessKind,
}
/// A `data` or `effect` edge over loop ids (`consumer -> dependency`:
/// dependency runs first).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LoopEdge {
    pub consumer: u64,
    pub dependency: u64,
}
/// `$defs/vectorPlan`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorPlan {
    pub schema_version: u32,
    pub vec_size: u64,
    pub signals: Vec<SignalRecord>,
    pub loops: Vec<LoopRecord>,
    pub epochs: Vec<EpochRecord>,
    pub transports: Vec<TransportRecord>,
    pub data_edges: Vec<LoopEdge>,
    pub effect_edges: Vec<LoopEdge>,
    pub vec_safe_witnesses: Vec<VecSafeWitness>,
    pub fused_serial_groups: Vec<FusedSerialGroupRecord>,
    pub lockstep_bundles: Vec<LockstepBundleRecord>,
}
