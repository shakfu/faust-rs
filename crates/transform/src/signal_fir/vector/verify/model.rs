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
    /// Integer scalar value.
    Int,
    /// Real (floating-point) scalar value; its concrete width is chosen in the
    /// routed-FIR layer.
    Real,
    /// Soundfile handle payload, derived from the signal shape exactly as the
    /// prepared boundary derives `SimpleSigType::Sound`.
    Sound,
    /// Tuple of component value types, one per element.
    Tuple(Vec<ValueType>),
}
/// `signalRecord.rate`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rate {
    /// Constant: computed once at initialization.
    Konst,
    /// Block rate: computed once per compute block.
    Block,
    /// Sample rate: computed once per sample.
    Samp,
}
/// `signalRecord.vectorability`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vectorability {
    /// Vectorizable: may be evaluated element-wise over a whole chunk.
    Vect,
    /// Scalar: must be evaluated one sample at a time inside its loop.
    Scal,
    /// Truly scalar: inherently sequential regardless of surrounding context.
    TrueScal,
}
/// `$defs/placement`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Computed on the control path, outside every sample loop.
    Control,
    /// Recomputed at each use site instead of being materialized.
    Inline,
    /// Materialized by the identified owning loop.
    Owned(u64),
}
/// `$defs/loopKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopKind {
    /// A loop whose roots may legally be computed chunk-wise.
    Vectorizable,
    /// A serial loop carrying the recursion group with the given id.
    Recursive(u64),
    /// A serial effect/clock island identified by the given tag.
    Island(u64),
    /// One logical lane of a checked lockstep bundle. The lane loop identities
    /// remain in the plan so the verifier can re-check independence; scheduling
    /// and FIR assembly treat the bundle as one physical execution unit.
    Lockstep {
        /// Number of lanes executed together in the bundle.
        width: u64,
    },
}
/// `vecSafeWitness.witness_kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WitnessKind {
    /// The loop's roots are pointwise over the chunk, so lanes are independent.
    Pointwise,
    /// Serial state stays internal to the loop, so chunk execution is safe.
    SerialStateInternal,
    /// Vector safety follows from a proven intrinsic property.
    ProvenIntrinsic,
}
/// `$defs/signalRecord`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalRecord {
    /// Stable plan-wide identity of the signal.
    pub signal_id: u64,
    /// Value type of the signal, from the v1 vocabulary.
    pub value_type: ValueType,
    /// True only for checked symbolic recursion carriers with no runtime FIR value.
    pub structural: bool,
    /// Evaluation rate of the signal.
    pub rate: Rate,
    /// Vectorability classification of the signal.
    pub vectorability: Vectorability,
    /// Clock domain the signal is evaluated in.
    pub clock_id: u64,
    /// Sorted transitive closure of effects reachable from the signal.
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
    /// Where the signal's value is computed (control, inline, or owned).
    pub placement: Placement,
    /// True if the computation may be replicated at several use sites.
    pub duplicable: bool,
}
/// `$defs/loopRecord`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopRecord {
    /// Stable plan-wide identity of the loop.
    pub loop_id: u64,
    /// Deterministic human-readable name used in emitted code and diagnostics.
    pub stable_name: String,
    /// Execution kind of the loop (vectorizable, recursive, island, lockstep).
    pub kind: LoopKind,
    /// Strictly ascending ids of the signals this loop materializes.
    pub roots: Vec<u64>,
    /// Epoch that owns the loop (redundant with the epoch's member list).
    pub epoch_id: u64,
}
/// `$defs/epochRecord`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpochRecord {
    /// Stable plan-wide identity of the epoch.
    pub epoch_id: u64,
    /// Barrier position: lower ranks run before higher ranks.
    pub rank: u64,
    /// Strictly ascending ids of the loops owned by this epoch.
    pub loops: Vec<u64>,
}
/// `$defs/transportRecord`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportRecord {
    /// Stable plan-wide identity of the transport.
    pub transport_id: u64,
    /// Deterministic human-readable name of the backing buffer.
    pub stable_name: String,
    /// Signal whose values the transport carries.
    pub signal_id: u64,
    /// Loop that writes the transport.
    pub producer_loop: u64,
    /// Loop that reads the transport.
    pub consumer_loop: u64,
    /// Element type of the buffer; must equal the signal's value type.
    pub element_type: ValueType,
    /// Buffer length in elements; must equal `vec_size`.
    pub length: u64,
    /// Memory layout of the buffer within the chunk.
    pub layout: TransportLayout,
}
/// Chunk-local transport layout. The external `compute` ABI remains planar;
/// `Interleaved` is legal only for a checked lockstep bundle of matching width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportLayout {
    /// One contiguous run per lane (the default chunk layout).
    Planar,
    /// Lane-interleaved storage with the given lane count.
    Interleaved(u64),
}
/// One explicit leaf correspondence in an isomorphism witness.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct IsoLeafMapping {
    /// Leaf signal on the representative lane's side.
    pub representative_signal_id: u64,
    /// Corresponding leaf signal on the witnessed lane's side.
    pub lane_signal_id: u64,
}
/// Root-level isomorphism evidence for one non-representative lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsoRootWitness {
    /// Root signal of the representative lane.
    pub representative_root: u64,
    /// Corresponding root signal of the witnessed lane.
    pub lane_root: u64,
    /// Hash of the shared prepared-tree shape; must be non-zero and equal
    /// across the pair.
    pub shape_hash: u64,
    /// Canonical leaf-by-leaf correspondence between the two roots.
    pub leaf_mapping: Vec<IsoLeafMapping>,
}
/// One logical lane retained inside a lockstep bundle certificate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockstepLaneRecord {
    /// Plan loop this lane corresponds to.
    pub loop_id: u64,
    /// Recursion group of the lane's loop.
    pub recursion_group: u64,
    /// Isomorphism witnesses tying each lane root to a representative root.
    pub roots: Vec<IsoRootWitness>,
}
/// Canonical lockstep bundle. `member_loop_ids` are scheduled as one unit and
/// assembled under one physical sample loop, while remaining explicit in the
/// plan for legality checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockstepBundleRecord {
    /// Stable plan-wide identity of the bundle.
    pub bundle_id: u64,
    /// Member loop whose body stands for every lane during assembly.
    pub representative_loop_id: u64,
    /// Strictly ascending ids of all member (lane) loops.
    pub member_loop_ids: Vec<u64>,
    /// Per-lane certificates for the non-representative members.
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
    /// Stable plan-wide identity of the group.
    pub group_id: u64,
    /// Canonical minimum carrier-owning loop of the group.
    pub owner_loop_id: u64,
    /// Strictly ascending ids of every loop fused into the group.
    pub member_loop_ids: Vec<u64>,
    /// Complete strictly ascending set of positive-delay state carriers.
    pub state_carrier_signal_ids: Vec<u64>,
    /// Signals that read a carrier at a positive delay inside the group.
    pub delayed_read_signal_ids: Vec<u64>,
    /// State-writing signals covering every carrier and recursive member.
    pub state_write_signal_ids: Vec<u64>,
    /// Planned transports rematerialized as scalar values inside the group.
    pub internal_transport_ids: Vec<u64>,
    /// Group roots that still feed outputs or transports outside the group.
    pub output_or_transport_roots: Vec<u64>,
}
/// `$defs/vecSafeWitness`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VecSafeWitness {
    /// Vectorizable loop the witness certifies.
    pub loop_id: u64,
    /// Rule under which the loop is claimed vector-safe.
    pub witness_kind: WitnessKind,
}
/// A `data` or `effect` edge over loop ids (`consumer -> dependency`:
/// dependency runs first).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct LoopEdge {
    /// Loop that must run after the dependency.
    pub consumer: u64,
    /// Loop that must run first.
    pub dependency: u64,
}
/// `$defs/vectorPlan`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorPlan {
    /// Schema version the plan was produced under.
    pub schema_version: u32,
    /// Positive chunk length every transport buffer is sized to.
    pub vec_size: u64,
    /// All plan signals, strictly ascending by id.
    pub signals: Vec<SignalRecord>,
    /// All plan loops, strictly ascending by id.
    pub loops: Vec<LoopRecord>,
    /// All epochs, strictly ascending by id, partitioning the loops.
    pub epochs: Vec<EpochRecord>,
    /// All chunk transports between loops, strictly ascending by id.
    pub transports: Vec<TransportRecord>,
    /// Canonical value-dependency edges between loops.
    pub data_edges: Vec<LoopEdge>,
    /// Canonical effect-ordering edges between loops.
    pub effect_edges: Vec<LoopEdge>,
    /// Vector-safety witnesses, one per vectorizable loop.
    pub vec_safe_witnesses: Vec<VecSafeWitness>,
    /// Fused serial groups covering all dangerous delayed-state crossings.
    pub fused_serial_groups: Vec<FusedSerialGroupRecord>,
    /// Checked lockstep bundles executed as single physical loops.
    pub lockstep_bundles: Vec<LockstepBundleRecord>,
}
