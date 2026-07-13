//! Strategy-independent `VectorPlan` DTO and its independent verifier
//! (`verify_vector_plan`).
//!
//! Vectorization port plan phases P4.4/P5 (formal gate: "before emission,
//! `verify_vector_plan` establishes `L-*`, typed transports, region
//! visibility, `VecSafe`…") and certified plan "R3 - Vector plan certificate
//! at L2/L3". This is the vector-plan analogue of the R1 schedule certificate
//! (`crate::schedule::certificate`): a canonical DTO mirroring the
//! `vectorPlan` shape of
//! `porting/schemas/vector-verification-certificate-v1.schema.json`, plus a
//! checker that re-derives every invariant from the plan's own fields.
//!
//! The checks mechanize the Lean `VectorPlanCertificate` obligations
//! (`porting/vector-mode-scheduling-formal-spec.lean`): unique ids, exact
//! epoch coverage with unique ranks, ownership/root agreement, inline
//! duplicability, complete non-self loop edges, an acyclic induced graph per
//! epoch, well-typed transports, monotone cross-epoch barriers, serial
//! recursion/island loops, and a `VecSafe` witness for every vectorizable
//! loop.
//!
//! # Scope, deliberately bounded (first P5 slice)
//! Additive and **not wired into FIR emission**. P4.4 constructs accepted plans
//! from verified decorations; P5 will route FIR through those plans. Deferred,
//! matching the certified plan's own staging:
//! - **effect commutation** (`L-Effects` for incomparable loops): the DTO
//!   retains P4.3a's exact effect identities and the verifier derives
//!   duplicability and local `VecSafe` instead of trusting producer booleans,
//!   but it does not yet prove pairwise commutation of independent effectful
//!   loops (the plan calls this the hard case; effect edges are
//!   producer-supplied here);
//! - **JSON (de)serialization / `plan_hash`** (R2 canonical-boundary work): a
//!   plan is identified by its Rust type, not a runtime tag or hash.

use ahash::{AHashMap, AHashSet};
use std::fmt;

pub use super::vector_analysis::EffectAtom;
use super::vector_analysis::{ForeignPurity, effect_sets_conflict};

/// `$defs/signalType`: the v1 value-type vocabulary (matches the Lean
/// `ValueTy`). FIR widths / `FaustFloat` live in the routed-FIR layer, not
/// here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueType {
    Int,
    Real,
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
    pub rate: Rate,
    pub vectorability: Vectorability,
    pub clock_id: u64,
    pub effects: Vec<EffectAtom>,
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
    pub vec_size: u64,
    pub signals: Vec<SignalRecord>,
    pub loops: Vec<LoopRecord>,
    pub epochs: Vec<EpochRecord>,
    pub transports: Vec<TransportRecord>,
    pub data_edges: Vec<LoopEdge>,
    pub effect_edges: Vec<LoopEdge>,
    pub vec_safe_witnesses: Vec<VecSafeWitness>,
}

/// Why [`verify_vector_plan`] rejected a plan. One variant per checked
/// obligation, so each has a demonstrated rejecting mutation (plan §8).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VectorPlanError {
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
}

impl fmt::Display for VectorPlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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
        }
    }
}

impl std::error::Error for VectorPlanError {}

fn strictly_ascending<T: Ord>(items: &[T]) -> Result<(), usize> {
    for i in 1..items.len() {
        if items[i - 1] >= items[i] {
            return Err(i);
        }
    }
    Ok(())
}

/// Concrete Lean `duplicableEffectsB`: only an empty effect set or pure
/// foreign calls can be recomputed in several loop regions.
#[must_use]
pub(crate) fn effects_duplicable(effects: &[EffectAtom]) -> bool {
    effects.iter().all(|effect| {
        matches!(
            effect,
            EffectAtom::Foreign {
                purity: ForeignPurity::Pure,
                ..
            }
        )
    })
}

/// Concrete Lean `sampleReorderableB`: loop-carried state is the local
/// per-sample vectorization blocker. Other effect conflicts are ordered or
/// co-located by the plan's separate effect relation.
#[must_use]
pub(crate) fn effects_sample_reorderable(effects: &[EffectAtom]) -> bool {
    !effects
        .iter()
        .any(|effect| matches!(effect, EffectAtom::ReadState(_) | EffectAtom::WriteState(_)))
}

/// Independent verifier for a [`VectorPlan`] (plan §5.5/§5.10
/// `verify_vector_plan`; Lean `VectorPlanCertificate`). Re-derives every
/// invariant from the plan's own fields; never runs a planner.
///
/// # Errors
/// The first [`VectorPlanError`] found (checks ordered so identity/coverage
/// problems surface before the graph/transport checks that assume them).
pub fn verify_vector_plan(plan: &VectorPlan) -> Result<(), VectorPlanError> {
    if plan.vec_size == 0 {
        return Err(VectorPlanError::VecSizeZero);
    }

    // ── Canonical set orders (also enforce uniqueness). ──────────────────
    let signal_ids: Vec<u64> = plan.signals.iter().map(|s| s.signal_id).collect();
    strictly_ascending(&signal_ids).map_err(|at| VectorPlanError::NotCanonical {
        what: "signals",
        at,
    })?;
    let loop_ids: Vec<u64> = plan.loops.iter().map(|l| l.loop_id).collect();
    strictly_ascending(&loop_ids)
        .map_err(|at| VectorPlanError::NotCanonical { what: "loops", at })?;
    let epoch_keys: Vec<(u64, u64)> = plan.epochs.iter().map(|e| (e.rank, e.epoch_id)).collect();
    strictly_ascending(&epoch_keys)
        .map_err(|at| VectorPlanError::NotCanonical { what: "epochs", at })?;
    let transport_ids: Vec<u64> = plan.transports.iter().map(|t| t.transport_id).collect();
    strictly_ascending(&transport_ids).map_err(|at| VectorPlanError::NotCanonical {
        what: "transports",
        at,
    })?;
    strictly_ascending(&plan.data_edges).map_err(|at| VectorPlanError::NotCanonical {
        what: "data_edges",
        at,
    })?;
    strictly_ascending(&plan.effect_edges).map_err(|at| VectorPlanError::NotCanonical {
        what: "effect_edges",
        at,
    })?;
    let witness_ids: Vec<u64> = plan
        .vec_safe_witnesses
        .iter()
        .map(|witness| witness.loop_id)
        .collect();
    strictly_ascending(&witness_ids).map_err(|at| VectorPlanError::NotCanonical {
        what: "vec_safe_witnesses",
        at,
    })?;

    let signal_set: AHashSet<u64> = signal_ids.iter().copied().collect();
    let loop_set: AHashSet<u64> = loop_ids.iter().copied().collect();
    let signal_by_id: AHashMap<u64, &SignalRecord> =
        plan.signals.iter().map(|s| (s.signal_id, s)).collect();
    let loop_by_id: AHashMap<u64, &LoopRecord> =
        plan.loops.iter().map(|l| (l.loop_id, l)).collect();

    // ── Epoch coverage: every loop in exactly one epoch, epoch loops known.
    let mut epoch_of_loop: AHashMap<u64, u64> = AHashMap::new();
    for epoch in &plan.epochs {
        strictly_ascending(&epoch.loops).map_err(|at| VectorPlanError::NotCanonical {
            what: "epoch.loops",
            at,
        })?;
        for &l in &epoch.loops {
            if !loop_set.contains(&l) {
                return Err(VectorPlanError::EpochLoopUnknown {
                    epoch_id: epoch.epoch_id,
                    loop_id: l,
                });
            }
            if epoch_of_loop.insert(l, epoch.epoch_id).is_some() {
                return Err(VectorPlanError::EpochCoverageMismatch { loop_id: l });
            }
        }
    }

    for &l in &loop_ids {
        if !epoch_of_loop.contains_key(&l) {
            return Err(VectorPlanError::EpochCoverageMismatch { loop_id: l });
        }
    }
    for lp in &plan.loops {
        let actual = epoch_of_loop[&lp.loop_id];
        if lp.epoch_id != actual {
            return Err(VectorPlanError::LoopEpochMismatch {
                loop_id: lp.loop_id,
                declared: lp.epoch_id,
                actual,
            });
        }
    }

    // ── Placement / roots agreement (P-Unique, P-Root, P-Duplicate). ─────
    for sig in &plan.signals {
        let derived_duplicable = effects_duplicable(&sig.effects);
        if sig.duplicable != derived_duplicable {
            return Err(VectorPlanError::DuplicabilityMismatch {
                signal_id: sig.signal_id,
            });
        }
        if sig.placement == Placement::Inline && !derived_duplicable {
            return Err(VectorPlanError::InlineNotDuplicable {
                signal_id: sig.signal_id,
            });
        }
        if let Placement::Owned(l) = sig.placement {
            let owner = loop_by_id
                .get(&l)
                .ok_or(VectorPlanError::OwnedSignalNotRoot {
                    signal_id: sig.signal_id,
                    loop_id: l,
                })?;
            if !owner.roots.contains(&sig.signal_id) {
                return Err(VectorPlanError::OwnedSignalNotRoot {
                    signal_id: sig.signal_id,
                    loop_id: l,
                });
            }
        }
    }
    for lp in &plan.loops {
        // Roots must be unique within a loop (Lean `rootsNodup`) but need not
        // be ascending (deterministic materialization order, not a set-like
        // canonical array). Uniqueness and ownership are checked together.
        let mut seen = AHashSet::new();
        for &r in &lp.roots {
            if !signal_set.contains(&r) {
                return Err(VectorPlanError::RootUnknownSignal {
                    loop_id: lp.loop_id,
                    signal_id: r,
                });
            }
            if !seen.insert(r) {
                return Err(VectorPlanError::RootWithoutOwnership {
                    signal_id: r,
                    loop_id: lp.loop_id,
                });
            }
            match signal_by_id.get(&r).map(|s| s.placement) {
                Some(Placement::Owned(owner)) if owner == lp.loop_id => {}
                _ => {
                    return Err(VectorPlanError::RootWithoutOwnership {
                        signal_id: r,
                        loop_id: lp.loop_id,
                    });
                }
            }
        }
    }

    // ── Edges: endpoints exist, no self-edge, barriers monotone. ─────────
    for edge in plan.data_edges.iter().chain(plan.effect_edges.iter()) {
        if !loop_set.contains(&edge.consumer) {
            return Err(VectorPlanError::EdgeEndpointUnknown {
                edge: *edge,
                missing: edge.consumer,
            });
        }
        if !loop_set.contains(&edge.dependency) {
            return Err(VectorPlanError::EdgeEndpointUnknown {
                edge: *edge,
                missing: edge.dependency,
            });
        }
        if edge.consumer == edge.dependency {
            return Err(VectorPlanError::LoopSelfEdge {
                loop_id: edge.consumer,
            });
        }
        // Barrier: dependency epoch rank ≤ consumer epoch rank.
        let dep_rank = rank_of(plan, epoch_of_loop[&edge.dependency]);
        let con_rank = rank_of(plan, epoch_of_loop[&edge.consumer]);
        if dep_rank > con_rank {
            return Err(VectorPlanError::BarrierViolation { edge: *edge });
        }
    }

    // ── Per-epoch induced-graph acyclicity (L-DAG). ──────────────────────
    for epoch in &plan.epochs {
        let members: AHashSet<u64> = epoch.loops.iter().copied().collect();
        if let Some(remaining) = induced_cycle(plan, &members) {
            return Err(VectorPlanError::EpochNotAcyclic {
                epoch_id: epoch.epoch_id,
                remaining,
            });
        }
    }

    // ── Effect conflicts: every conflicting loop pair is comparable. ──
    // Root identities and graph endpoints are known valid at this point, so
    // this check cannot panic when presented with a hostile DTO.
    for (index, left) in plan.loops.iter().enumerate() {
        for right in &plan.loops[index + 1..] {
            let left_effects = loop_effects(plan, left);
            let right_effects = loop_effects(plan, right);
            if effect_sets_conflict(&left_effects, &right_effects)
                && !loop_reaches(plan, left.loop_id, right.loop_id)
                && !loop_reaches(plan, right.loop_id, left.loop_id)
            {
                return Err(VectorPlanError::UnorderedEffectConflict {
                    left: left.loop_id,
                    right: right.loop_id,
                });
            }
        }
    }

    // ── Transports well-typed (T-TRANSPORT). ─────────────────────────────
    for t in &plan.transports {
        if !signal_set.contains(&t.signal_id) {
            return Err(VectorPlanError::TransportUnknownRef {
                transport_id: t.transport_id,
                missing: t.signal_id,
            });
        }
        for l in [t.producer_loop, t.consumer_loop] {
            if !loop_set.contains(&l) {
                return Err(VectorPlanError::TransportUnknownRef {
                    transport_id: t.transport_id,
                    missing: l,
                });
            }
        }
        if t.producer_loop == t.consumer_loop {
            return Err(VectorPlanError::TransportSelfLoop {
                transport_id: t.transport_id,
            });
        }
        if signal_by_id[&t.signal_id].value_type != t.element_type {
            return Err(VectorPlanError::TransportTypeMismatch {
                transport_id: t.transport_id,
            });
        }
        if t.length != plan.vec_size {
            return Err(VectorPlanError::TransportLengthMismatch {
                transport_id: t.transport_id,
            });
        }
    }

    // ── VecSafe witnesses vs loop kinds. ─────────────────────────────────
    let mut witness_of: AHashMap<u64, WitnessKind> = AHashMap::new();
    for w in &plan.vec_safe_witnesses {
        if !loop_set.contains(&w.loop_id) {
            return Err(VectorPlanError::WitnessUnknownLoop { loop_id: w.loop_id });
        }
        witness_of.insert(w.loop_id, w.witness_kind);
    }
    for lp in &plan.loops {
        match lp.kind {
            LoopKind::Vectorizable => {
                let vec_safe = lp.roots.iter().all(|root| {
                    let signal = signal_by_id[root];
                    signal.vectorability == Vectorability::Vect
                        && effects_sample_reorderable(&signal.effects)
                });
                if !vec_safe {
                    return Err(VectorPlanError::VectorizableNotSafe {
                        loop_id: lp.loop_id,
                    });
                }
                let Some(kind) = witness_of.get(&lp.loop_id) else {
                    return Err(VectorPlanError::VectorizableWithoutWitness {
                        loop_id: lp.loop_id,
                    });
                };
                // A vectorizable loop's witness must be a *vectorizing* one.
                if !matches!(kind, WitnessKind::Pointwise | WitnessKind::ProvenIntrinsic) {
                    return Err(VectorPlanError::VectorizableWithoutWitness {
                        loop_id: lp.loop_id,
                    });
                }
            }
            LoopKind::Recursive(_) | LoopKind::Island(_) => {
                // Serial loops must not claim a pointwise (per-lane
                // parallel) witness; only a serial-state witness is
                // consistent with their kind.
                if matches!(witness_of.get(&lp.loop_id), Some(WitnessKind::Pointwise)) {
                    return Err(VectorPlanError::SerialLoopNotSerial {
                        loop_id: lp.loop_id,
                    });
                }
            }
        }
    }

    Ok(())
}

fn rank_of(plan: &VectorPlan, epoch_id: u64) -> u64 {
    plan.epochs
        .iter()
        .find(|e| e.epoch_id == epoch_id)
        .map_or(u64::MAX, |e| e.rank)
}

fn loop_effects(plan: &VectorPlan, loop_record: &LoopRecord) -> Vec<EffectAtom> {
    let signal_by_id = plan
        .signals
        .iter()
        .map(|signal| (signal.signal_id, signal))
        .collect::<AHashMap<_, _>>();
    let mut effects = loop_record
        .roots
        .iter()
        .flat_map(|root| signal_by_id[root].effects.iter().cloned())
        .collect::<Vec<_>>();
    effects.sort();
    effects.dedup();
    effects
}

/// Whether `dependency` is ordered before `consumer` by one or more combined
/// edges. Edges are stored consumer -> dependency, so traversal follows the
/// reverse adjacency.
fn loop_reaches(plan: &VectorPlan, dependency: u64, consumer: u64) -> bool {
    let mut pending = vec![dependency];
    let mut seen = AHashSet::new();
    while let Some(node) = pending.pop() {
        if !seen.insert(node) {
            continue;
        }
        for edge in plan
            .data_edges
            .iter()
            .chain(plan.effect_edges.iter())
            .filter(|edge| edge.dependency == node)
        {
            if edge.consumer == consumer {
                return true;
            }
            pending.push(edge.consumer);
        }
    }
    false
}

/// Kahn peeling on the induced graph of `members` (edges from both edge
/// families whose endpoints are both members). Returns the unschedulable set
/// (stable-sorted) if a cycle remains, else `None`.
fn induced_cycle(plan: &VectorPlan, members: &AHashSet<u64>) -> Option<Vec<u64>> {
    let mut pending: AHashMap<u64, AHashSet<u64>> = AHashMap::new();
    let mut successors: AHashMap<u64, Vec<u64>> = AHashMap::new();
    for &m in members {
        pending.insert(m, AHashSet::new());
    }
    for edge in plan.data_edges.iter().chain(plan.effect_edges.iter()) {
        if members.contains(&edge.consumer) && members.contains(&edge.dependency) {
            pending
                .get_mut(&edge.consumer)
                .expect("consumer is a member")
                .insert(edge.dependency);
            successors
                .entry(edge.dependency)
                .or_default()
                .push(edge.consumer);
        }
    }
    let mut ready: Vec<u64> = pending
        .iter()
        .filter(|(_, deps)| deps.is_empty())
        .map(|(&n, _)| n)
        .collect();
    let mut removed: AHashSet<u64> = AHashSet::new();
    while let Some(n) = ready.pop() {
        if !removed.insert(n) {
            continue;
        }
        if let Some(succs) = successors.get(&n) {
            for &s in succs {
                if let Some(set) = pending.get_mut(&s) {
                    set.remove(&n);
                    if set.is_empty() && !removed.contains(&s) {
                        ready.push(s);
                    }
                }
            }
        }
    }
    if removed.len() == members.len() {
        None
    } else {
        let mut remaining: Vec<u64> = members.difference(&removed).copied().collect();
        remaining.sort_unstable();
        Some(remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal_fir::vector_analysis::{StateCell, StateResource};

    /// A minimal valid two-loop plan mirroring the PV DSP shape: loop 0 owns
    /// `x` (a vectorizable producer), loop 1 consumes it (vectorizable), one
    /// typed transport, both in the single forward epoch.
    fn valid_plan() -> VectorPlan {
        VectorPlan {
            vec_size: 16,
            signals: vec![
                SignalRecord {
                    signal_id: 10,
                    value_type: ValueType::Real,
                    rate: Rate::Samp,
                    vectorability: Vectorability::Vect,
                    clock_id: 0,
                    effects: vec![],
                    placement: Placement::Owned(0),
                    duplicable: true,
                },
                SignalRecord {
                    signal_id: 11,
                    value_type: ValueType::Real,
                    rate: Rate::Samp,
                    vectorability: Vectorability::Vect,
                    clock_id: 0,
                    effects: vec![],
                    placement: Placement::Owned(1),
                    duplicable: true,
                },
            ],
            loops: vec![
                LoopRecord {
                    loop_id: 0,
                    stable_name: "loop_owns_x".to_owned(),
                    kind: LoopKind::Vectorizable,
                    roots: vec![10],
                    epoch_id: 0,
                },
                LoopRecord {
                    loop_id: 1,
                    stable_name: "loop_consumes".to_owned(),
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
                stable_name: "transportX".to_owned(),
                signal_id: 10,
                producer_loop: 0,
                consumer_loop: 1,
                element_type: ValueType::Real,
                length: 16,
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
        }
    }

    #[test]
    fn the_reference_plan_verifies() {
        verify_vector_plan(&valid_plan()).expect("reference plan is valid");
    }

    // ── one rejecting mutation per obligation (plan §8) ──────────────────

    #[test]
    fn rejects_zero_vec_size() {
        let mut p = valid_plan();
        p.vec_size = 0;
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::VecSizeZero)
        ));
    }

    #[test]
    fn rejects_noncanonical_loops() {
        let mut p = valid_plan();
        p.loops.reverse();
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::NotCanonical { what: "loops", .. })
        ));
    }

    #[test]
    fn rejects_a_loop_in_two_epochs() {
        let mut p = valid_plan();
        p.epochs = vec![
            EpochRecord {
                epoch_id: 0,
                rank: 0,
                loops: vec![0, 1],
            },
            EpochRecord {
                epoch_id: 1,
                rank: 1,
                loops: vec![1],
            },
        ];
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::EpochCoverageMismatch { loop_id: 1 })
        ));
    }

    #[test]
    fn rejects_a_loop_covered_by_no_epoch() {
        let mut p = valid_plan();
        p.epochs[0].loops = vec![0];
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::EpochCoverageMismatch { loop_id: 1 })
        ));
    }

    #[test]
    fn rejects_owned_signal_absent_from_roots() {
        let mut p = valid_plan();
        p.loops[0].roots.clear();
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::OwnedSignalNotRoot {
                signal_id: 10,
                loop_id: 0
            })
        ));
    }

    #[test]
    fn rejects_unknown_root_before_inspecting_its_effects() {
        let mut p = valid_plan();
        p.loops[1].roots.push(99);
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::RootUnknownSignal {
                loop_id: 1,
                signal_id: 99
            })
        ));
    }

    #[test]
    fn rejects_inline_signal_not_duplicable() {
        let mut p = valid_plan();
        // Detach signal 10 from loop 0 ownership and make it a non-duplicable
        // inline signal.
        p.loops[0].roots = vec![];
        p.loops[0].kind = LoopKind::Vectorizable;
        p.signals[0].placement = Placement::Inline;
        p.signals[0].duplicable = false;
        p.signals[0].effects = vec![EffectAtom::WriteState(StateResource::Signal {
            owner: 10,
            cell: StateCell::Delay,
        })];
        // Give loop 0 a different owned root so it stays valid otherwise.
        p.signals.insert(
            0,
            SignalRecord {
                signal_id: 5,
                value_type: ValueType::Real,
                rate: Rate::Samp,
                vectorability: Vectorability::Vect,
                clock_id: 0,
                effects: vec![],
                placement: Placement::Owned(0),
                duplicable: true,
            },
        );
        p.loops[0].roots = vec![5];
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::InlineNotDuplicable { signal_id: 10 })
        ));
    }

    #[test]
    fn rejects_a_duplicability_bit_not_derived_from_effects() {
        let mut p = valid_plan();
        p.signals[0].duplicable = false;
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::DuplicabilityMismatch { signal_id: 10 })
        ));
    }

    #[test]
    fn rejects_a_loop_epoch_field_not_matching_membership() {
        let mut p = valid_plan();
        p.loops[1].epoch_id = 7;
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::LoopEpochMismatch {
                loop_id: 1,
                declared: 7,
                actual: 0
            })
        ));
    }

    #[test]
    fn rejects_noncanonical_vec_safe_witnesses() {
        let mut p = valid_plan();
        p.vec_safe_witnesses.reverse();
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::NotCanonical {
                what: "vec_safe_witnesses",
                ..
            })
        ));
    }

    #[test]
    fn rejects_vectorizable_loop_whose_root_is_not_vec_safe() {
        let mut p = valid_plan();
        p.signals[1].vectorability = Vectorability::Scal;
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::VectorizableNotSafe { loop_id: 1 })
        ));
    }

    #[test]
    fn rejects_unordered_conflicting_effects() {
        let mut p = valid_plan();
        let effect = EffectAtom::WriteOutput(0);
        for signal in &mut p.signals {
            signal.effects = vec![effect.clone()];
            signal.duplicable = false;
        }
        p.data_edges.clear();
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::UnorderedEffectConflict { left: 0, right: 1 })
        ));
    }

    #[test]
    fn rejects_edge_to_unknown_loop() {
        let mut p = valid_plan();
        p.data_edges = vec![LoopEdge {
            consumer: 1,
            dependency: 99,
        }];
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::EdgeEndpointUnknown { missing: 99, .. })
        ));
    }

    #[test]
    fn rejects_loop_self_edge() {
        let mut p = valid_plan();
        p.data_edges = vec![LoopEdge {
            consumer: 1,
            dependency: 1,
        }];
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::LoopSelfEdge { loop_id: 1 })
        ));
    }

    #[test]
    fn rejects_a_cyclic_epoch() {
        let mut p = valid_plan();
        // Canonical ascending order by (consumer, dependency), so the cycle
        // 0 -> 1 -> 0 reaches the acyclicity check rather than tripping the
        // canonical-order check first.
        p.data_edges = vec![
            LoopEdge {
                consumer: 0,
                dependency: 1,
            },
            LoopEdge {
                consumer: 1,
                dependency: 0,
            },
        ];
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::EpochNotAcyclic { epoch_id: 0, .. })
        ));
    }

    #[test]
    fn rejects_transport_self_loop() {
        let mut p = valid_plan();
        p.transports[0].consumer_loop = 0;
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::TransportSelfLoop { transport_id: 0 })
        ));
    }

    #[test]
    fn rejects_transport_type_mismatch() {
        let mut p = valid_plan();
        p.transports[0].element_type = ValueType::Int;
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::TransportTypeMismatch { transport_id: 0 })
        ));
    }

    #[test]
    fn rejects_transport_length_mismatch() {
        let mut p = valid_plan();
        p.transports[0].length = 8;
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::TransportLengthMismatch { transport_id: 0 })
        ));
    }

    #[test]
    fn rejects_a_backwards_barrier() {
        let mut p = valid_plan();
        // Two epochs: rank 0 = {0}, rank 1 = {1}. An edge 0 -> 1 makes the
        // rank-0 consumer depend on a rank-1 dependency: a backwards barrier.
        p.epochs = vec![
            EpochRecord {
                epoch_id: 0,
                rank: 0,
                loops: vec![0],
            },
            EpochRecord {
                epoch_id: 1,
                rank: 1,
                loops: vec![1],
            },
        ];
        p.loops[0].epoch_id = 0;
        p.loops[1].epoch_id = 1;
        p.data_edges = vec![LoopEdge {
            consumer: 0,
            dependency: 1,
        }];
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::BarrierViolation { .. })
        ));
    }

    #[test]
    fn rejects_vectorizable_loop_without_witness() {
        let mut p = valid_plan();
        p.vec_safe_witnesses.retain(|w| w.loop_id != 1);
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::VectorizableWithoutWitness { loop_id: 1 })
        ));
    }

    #[test]
    fn rejects_serial_loop_claiming_pointwise_witness() {
        let mut p = valid_plan();
        p.loops[1].kind = LoopKind::Recursive(0);
        // loop 1's witness is still Pointwise from the reference plan.
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::SerialLoopNotSerial { loop_id: 1 })
        ));
    }

    #[test]
    fn recursive_loop_with_serial_witness_is_accepted() {
        let mut p = valid_plan();
        p.loops[1].kind = LoopKind::Recursive(0);
        for w in &mut p.vec_safe_witnesses {
            if w.loop_id == 1 {
                w.witness_kind = WitnessKind::SerialStateInternal;
            }
        }
        verify_vector_plan(&p).expect("a serial loop with a serial witness is valid");
    }

    #[test]
    fn changing_only_edge_order_is_rejected_as_noncanonical_not_accepted_silently() {
        // The verifier rejects a noncanonical *equivalent* set, so a plan is
        // identified by canonical bytes (P-Strategy support): reordering edges
        // is not silently accepted.
        let mut p = valid_plan();
        p.effect_edges = vec![
            LoopEdge {
                consumer: 1,
                dependency: 0,
            },
            LoopEdge {
                consumer: 1,
                dependency: 0,
            },
        ];
        assert!(matches!(
            verify_vector_plan(&p),
            Err(VectorPlanError::NotCanonical {
                what: "effect_edges",
                ..
            })
        ));
    }
}
