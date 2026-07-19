//! Vector analysis: execution conditions, dependencies, occurrences,
//! effects, and use tables over the verified prepared forest.
//!
//! # C++ provenance
//! The dependency projection centralizes the dependency rules previously
//! embedded in the Rust Hgraph adapter. Its C++ references are
//! `compiler/Dependencies/DependenciesUtils.cpp::getSignalDependencies` and
//! `compiler/generator/occurrences.cpp::OccMarkup`. One decoded signal shape
//! produces distinct scheduling and occurrence views because the C++ rules
//! intentionally differ for FIR/IIR carriers, tables, `seq`, generators, and
//! clock wrappers.
//!
//! P4.3a adds the C++ `conditionAnnotation` DNF producer and conservative
//! effect decoration without activating either scalar or vector scheduling.
//! Effects in this table describe compute-time behavior. `Gen` remains a
//! lifecycle boundary, so table-initialization effects require a separate
//! decoration before a certificate can establish full lifecycle coverage.
//! The compute-scoped `DecorationCertificate` lives in the adjacent
//! `decoration_verify` module; the production vector plan consumes only
//! independently certified decorations.
//!
//! Development history: P4.2/P4.3a slices of
//! `porting/vector-mode-signal-level-analysis-cpp-port-plan-2026-07-10-en.md`.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt;

use signals::{BinOp, SigId, SigMatch, match_sig};
use sigtype::{SigType, Variability, Vectorability, check_delay_interval};
use tlib::{
    NodeKind, TreeArena, list_to_vec, match_sym_rec, match_sym_ref, tree_to_int, tree_to_str,
};

use crate::clk_env::{ClkEnv, ClkEnvMap};
use crate::signal_prepare::VerifiedPreparedSignals;

/// Stable identity of an execution condition supplied by an analysis client.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CondId(pub u64);

/// A signal use's rate and execution condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UseContext {
    /// Rate at which this use is demanded.
    pub variability: Variability,
    /// Recursive depth of the consumer context, saturated to one by the C++
    /// extended-variability rule.
    pub recursiveness: u32,
    /// Execution condition supplied by [`ExecutionConditions`].
    pub condition: CondId,
}

/// Supplies execution-condition identities without guessing control semantics.
pub trait ExecutionConditions {
    /// Canonical condition attached to one signal itself.
    fn signal_condition(&self, sig: SigId) -> CondId;

    /// Condition at which one output root is demanded.
    fn root_condition(&self, root: SigId) -> CondId;
}

/// Canonical positive DNF used by C++ `dcond` condition annotation.
///
/// An empty `clauses` vector denotes `true`, matching the C++ use of `nil`.
/// Every non-empty inner vector is a sorted conjunction of signal identities;
/// the outer vector is a sorted disjunction with absorbed supersets removed.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ExecutionCondition {
    clauses: Vec<Vec<u32>>,
}

impl ExecutionCondition {
    /// Unconditional execution (`true`).
    #[must_use]
    pub const fn unconditional() -> Self {
        Self {
            clauses: Vec::new(),
        }
    }

    /// Returns whether this condition is unconditional.
    #[must_use]
    pub fn is_unconditional(&self) -> bool {
        self.clauses.is_empty()
    }

    /// Canonical DNF clauses as numeric prepared-signal identities.
    #[must_use]
    pub fn clauses(&self) -> &[Vec<u32>] {
        &self.clauses
    }

    fn atom(sig: SigId) -> Self {
        Self {
            clauses: vec![vec![sig.as_u32()]],
        }
    }

    fn or(&self, other: &Self) -> Self {
        if self.is_unconditional() || other.is_unconditional() {
            return Self::unconditional();
        }
        Self::normalize(self.clauses.iter().chain(&other.clauses).cloned())
    }

    fn and(&self, other: &Self) -> Self {
        if self.is_unconditional() {
            return other.clone();
        }
        if other.is_unconditional() {
            return self.clone();
        }
        Self::normalize(self.clauses.iter().flat_map(|left| {
            other.clauses.iter().map(move |right| {
                let mut clause = left.clone();
                clause.extend(right);
                clause
            })
        }))
    }

    fn normalize(clauses: impl IntoIterator<Item = Vec<u32>>) -> Self {
        let mut clauses = clauses
            .into_iter()
            .map(|mut clause| {
                clause.sort_unstable();
                clause.dedup();
                clause
            })
            .collect::<Vec<_>>();
        clauses.sort();
        clauses.dedup();
        let mut minimal = Vec::<Vec<u32>>::new();
        for clause in clauses {
            if minimal
                .iter()
                .any(|candidate| is_sorted_subset(candidate, &clause))
            {
                continue;
            }
            minimal.retain(|candidate| !is_sorted_subset(&clause, candidate));
            minimal.push(clause);
            minimal.sort();
        }
        Self { clauses: minimal }
    }
}

fn is_sorted_subset(left: &[u32], right: &[u32]) -> bool {
    let mut right_index = 0;
    for &item in left {
        while right.get(right_index).is_some_and(|&other| other < item) {
            right_index += 1;
        }
        if right.get(right_index) != Some(&item) {
            return false;
        }
        right_index += 1;
    }
    true
}

/// Deterministic forest-scoped execution-condition interning table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionConditionTable {
    conditions: Vec<ExecutionCondition>,
    by_signal: BTreeMap<u32, CondId>,
    unconditional: CondId,
}

impl ExecutionConditionTable {
    /// Builds the C++ `conditionAnnotation` fixed point for a prepared forest.
    pub fn build(prepared: &VerifiedPreparedSignals) -> Result<Self, AnalysisError> {
        let analysis = SignalAnalysisContext::new(
            prepared.arena(),
            prepared.sig_types_map(),
            prepared.outputs(),
        )?;
        build_execution_conditions(&analysis, prepared.outputs())
    }

    /// Returns the canonical expression interned at `id`.
    #[must_use]
    pub fn condition(&self, id: CondId) -> Option<&ExecutionCondition> {
        self.conditions.get(usize::try_from(id.0).ok()?)
    }

    /// All canonical conditions in deterministic identity order.
    #[must_use]
    pub fn conditions(&self) -> &[ExecutionCondition] {
        &self.conditions
    }
}

impl ExecutionConditions for ExecutionConditionTable {
    fn signal_condition(&self, sig: SigId) -> CondId {
        self.by_signal
            .get(&sig.as_u32())
            .copied()
            .expect("execution-condition table queried outside its prepared forest")
    }

    fn root_condition(&self, _root: SigId) -> CondId {
        self.unconditional
    }
}

/// Constant condition provider for unconditioned prepared forests.
#[derive(Clone, Copy, Debug, Default)]
pub struct ConstantExecutionConditions {
    condition: CondId,
}

impl ConstantExecutionConditions {
    /// Uses `condition` for every root and dependency use.
    #[must_use]
    pub const fn new(condition: CondId) -> Self {
        Self { condition }
    }
}

impl ExecutionConditions for ConstantExecutionConditions {
    fn signal_condition(&self, _sig: SigId) -> CondId {
        self.condition
    }

    fn root_condition(&self, _root: SigId) -> CondId {
        self.condition
    }
}

/// Temporal/deferred semantic class of one dependency edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepKind {
    /// Same-tick value dependency.
    Immediate,
    /// State read with a known positive delay amount.
    Delayed { amount: u32 },
    /// Reserved for the P4.3 control-dependency distinction.
    Control,
    /// Boundary between a wrapper and its outer-domain clock.
    ClockBoundary,
    /// Reserved for P4.3 effect dependencies.
    Effect,
}

/// One decoded dependency, keyed by source-local child order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AnalysisDependency {
    /// Signal whose decoded shape owns this edge.
    pub from: SigId,
    /// Dependency target.
    pub to: SigId,
    /// Temporal class.
    pub kind: DepKind,
    /// Stable source-local edge key (decoded child order).
    pub edge_key: usize,
}

/// One occurrence use with the C++ delay amount kept separate from scheduling
/// immediacy. A bounded delay `[0, n]`, for example, is an immediate scheduling
/// dependency and an occurrence use with delay `n`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OccurrenceUse {
    /// Signal whose decoded shape owns this use.
    pub from: SigId,
    /// Used signal.
    pub to: SigId,
    /// Maximum fixed delay attached to this use (`0` means outside a delay).
    pub delay: u32,
    /// Stable source-local use key.
    pub edge_key: usize,
}

/// The two dependency projections produced by one decoded signal shape.
///
/// Scheduling follows C++ `getSignalDependencies`; occurrences follow
/// `OccMarkup::incOcc`. Keeping both in one value preserves a single owner for
/// `SigMatch` child enumeration without conflating the two semantics.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SignalDependencies {
    scheduling: Vec<AnalysisDependency>,
    occurrences: Vec<OccurrenceUse>,
    condition_children: Vec<SigId>,
}

impl SignalDependencies {
    /// Dependencies constraining Hgraph scheduling and placement.
    #[must_use]
    pub fn scheduling(&self) -> &[AnalysisDependency] {
        &self.scheduling
    }

    /// Uses consumed by C++-compatible occurrence analysis.
    #[must_use]
    pub fn occurrences(&self) -> &[OccurrenceUse] {
        &self.occurrences
    }

    /// Children receiving execution conditions through C++
    /// `conditionAnnotation`; generators intentionally have no children here.
    #[must_use]
    pub fn condition_children(&self) -> &[SigId] {
        &self.condition_children
    }
}

/// Forest-scoped inputs required by typed dependency decoding.
#[derive(Debug)]
pub struct SignalAnalysisContext<'a> {
    arena: &'a TreeArena,
    sig_types: &'a HashMap<SigId, SigType>,
    rec_groups: BTreeMap<u32, SigId>,
}

impl<'a> SignalAnalysisContext<'a> {
    /// Builds the symbolic-recursion index once for the reachable forest.
    pub fn new(
        arena: &'a TreeArena,
        sig_types: &'a HashMap<SigId, SigType>,
        roots: &[SigId],
    ) -> Result<Self, AnalysisError> {
        let mut rec_groups = BTreeMap::new();
        let mut visited = BTreeSet::new();
        let mut stack = roots.to_vec();
        while let Some(sig) = stack.pop() {
            if !visited.insert(sig.as_u32()) {
                continue;
            }
            if let Some((var, _)) = match_sym_rec(arena, sig)
                && let Some(previous) = rec_groups.insert(var.as_u32(), sig)
                && previous != sig
            {
                return Err(AnalysisError::Malformed {
                    sig,
                    detail: format!(
                        "symbolic recursion variable {} names groups {} and {}",
                        var.as_u32(),
                        previous.as_u32(),
                        sig.as_u32()
                    ),
                });
            }
            if let Some(children) = arena.children(sig) {
                stack.extend(children.iter().copied());
            }
        }
        Ok(Self {
            arena,
            sig_types,
            rec_groups,
        })
    }

    /// Prepared signal arena.
    #[must_use]
    pub fn arena(&self) -> &TreeArena {
        self.arena
    }

    fn sig_type(&self, sig: SigId) -> Result<&SigType, AnalysisError> {
        self.sig_types
            .get(&sig)
            .ok_or(AnalysisError::MissingType { sig })
    }

    fn resolve_rec_group(&self, sig: SigId) -> Option<SigId> {
        if match_sym_rec(self.arena, sig).is_some() {
            return Some(sig);
        }
        if let Some(var) = match_sym_ref(self.arena, sig) {
            return self.rec_groups.get(&var.as_u32()).copied();
        }
        if let SigMatch::ReverseTimeRec(inner) = match_sig(self.arena, sig) {
            return self.resolve_rec_group(inner);
        }
        None
    }

    fn projection_dependency(
        &self,
        projection: SigId,
        index: i32,
        group_ref: SigId,
    ) -> Result<(SigId, DepKind), AnalysisError> {
        if index < 0 {
            return Err(AnalysisError::InvalidRecursiveProjection {
                sig: projection,
                index,
                group: group_ref,
            });
        }
        let back_reference = match_sym_ref(self.arena, group_ref).is_some();
        let group = if match_sym_rec(self.arena, group_ref).is_some() {
            Some(group_ref)
        } else if let Some(var) = match_sym_ref(self.arena, group_ref) {
            self.rec_groups.get(&var.as_u32()).copied()
        } else {
            None
        };
        let Some(group) = group else {
            // Rust-only tuple carriers such as BlockReverseAD and
            // ReverseTimeRec also use Proj. They retain the pre-P4 dependency
            // on the carrier itself; only symbolic recursion selects a body.
            return Ok((group_ref, DepKind::Immediate));
        };
        let (_, body_list) = match_sym_rec(self.arena, group).expect("resolved SYMREC group");
        let definitions =
            list_to_vec(self.arena, body_list).ok_or_else(|| AnalysisError::Malformed {
                sig: group,
                detail: "malformed SYMREC body list".to_owned(),
            })?;
        let definition = definitions
            .get(usize::try_from(index).expect("nonnegative i32 fits usize"))
            .copied()
            .ok_or_else(|| AnalysisError::Malformed {
                sig: projection,
                detail: format!(
                    "projection index {index} is outside group {} arity {}",
                    group.as_u32(),
                    definitions.len()
                ),
            })?;
        let kind = if back_reference {
            // In the acyclic Rust encoding, SYMREF denotes the implicit
            // one-sample recursion back-edge that C++ represents cyclically.
            DepKind::Delayed { amount: 1 }
        } else {
            DepKind::Immediate
        };
        Ok((definition, kind))
    }
}

/// One use count grouped by a [`UseContext`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContextOccurrence {
    /// Context in which this signal is referenced.
    pub context: UseContext,
    /// Number of references in that context.
    pub count: u32,
}

/// Canonical context-sensitive counterpart of C++ `Occurrences`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OccInfo {
    /// Counts sorted by [`UseContext`].
    pub per_context: Vec<ContextOccurrence>,
    /// Whether any use prevents simple single-use treatment.
    pub multi: bool,
}

/// Checked symbolic recursive projection metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecursiveProjection {
    /// Nonnegative projection index.
    pub index: usize,
    /// Symbolic recursion group read by the projection.
    pub group: SigId,
}

/// Stable cell discriminator for signal-owned persistent state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StateCell {
    Delay,
    Prefix,
    Fir,
    Iir,
    WaveformIndex,
    Hold,
    Clock,
    ReverseTime,
    ReverseAd,
}

/// Stable abstract identity of one persistent state resource.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StateResource {
    /// State owned by one prepared signal plus a semantic cell discriminator.
    Signal { owner: u32, cell: StateCell },
    /// State owned by one symbolic recursion projection.
    Recursion { group: u32, projection: u32 },
}

/// Raw Faust foreign type code preserved independently from backend precision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForeignTypeCode(pub i64);

/// Stable identity of one declared foreign function signature.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForeignSignature {
    pub names: Vec<String>,
    pub return_type: ForeignTypeCode,
    pub arguments: Vec<ForeignTypeCode>,
}

/// Stable foreign resource identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ForeignResource {
    Function(ForeignSignature),
    Variable {
        name: String,
        value_type: ForeignTypeCode,
    },
}

/// Declared foreign purity. Faust currently supplies no declaration, so
/// analysis-produced foreign effects use [`ForeignPurity::Unknown`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ForeignPurity {
    Pure,
    Impure,
    Unknown,
}

/// Conservative signal-level effect atom with stable resource identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EffectAtom {
    ReadState(StateResource),
    WriteState(StateResource),
    ReadTable(u32),
    WriteTable(u32),
    WriteUi(u32),
    WriteOutput(u32),
    Foreign {
        resource: ForeignResource,
        purity: ForeignPurity,
    },
}

/// Returns whether a `WrTbl` has no live writer port.
///
/// `rdtable` lowers to `WrTbl(size, generator, nil, nil)`: its content is
/// produced once by its generator before `compute` and is immutable for the
/// whole of it. `rwtable` binds both ports and stays mutable. Every component
/// that classifies table effects or admits a read-only generator must decide
/// this from one definition: reading a read-only table as a writer costs
/// coverage, and reading a mutable table as read-only would admit an unsound
/// program.
#[must_use]
pub fn wrtbl_is_readonly(arena: &TreeArena, write_index: SigId, write_value: SigId) -> bool {
    arena.is_nil(write_index) && arena.is_nil(write_value)
}

/// Returns whether two atoms cannot be freely reordered.
#[must_use]
pub fn effects_conflict(left: &EffectAtom, right: &EffectAtom) -> bool {
    use EffectAtom::{Foreign, ReadState, ReadTable, WriteOutput, WriteState, WriteTable, WriteUi};

    let foreign_barrier = |effect: &EffectAtom| {
        matches!(
            effect,
            Foreign {
                purity: ForeignPurity::Impure | ForeignPurity::Unknown,
                ..
            }
        )
    };
    if foreign_barrier(left) || foreign_barrier(right) {
        return true;
    }
    match (left, right) {
        (ReadState(a), WriteState(b))
        | (WriteState(a), ReadState(b))
        | (WriteState(a), WriteState(b)) => a == b,
        (ReadTable(a), WriteTable(b))
        | (WriteTable(a), ReadTable(b))
        | (WriteTable(a), WriteTable(b)) => a == b,
        (WriteUi(a), WriteUi(b)) | (WriteOutput(a), WriteOutput(b)) => a == b,
        _ => false,
    }
}

/// Returns whether any pair of atoms in two effect sets conflicts.
#[must_use]
pub fn effect_sets_conflict(left: &[EffectAtom], right: &[EffectAtom]) -> bool {
    left.iter()
        .any(|a| right.iter().any(|b| effects_conflict(a, b)))
}

/// P4.2 facts for one reachable prepared signal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalUseInfo {
    /// Full type copied from the verified preparation boundary.
    pub sig_type: SigType,
    /// Cached `sig_type.variability()`.
    pub variability: Variability,
    /// Cached `sig_type.vectorability()`.
    pub vectorability: Vectorability,
    /// Inferred clock environment copied from [`ClkEnvMap`].
    pub clk_env: ClkEnv,
    /// Recursive depth used by C++ extended variability.
    pub recursiveness: u32,
    /// Canonical execution condition attached to this signal.
    pub execution_condition: CondId,
    /// Deterministic uses grouped by context.
    pub occurrences: OccInfo,
    /// Largest fixed delay amount of a delayed reader.
    pub max_delay: u32,
    /// Number of delayed reads of this signal.
    pub delay_reads: u32,
    /// Whether at least one use is outside a delay.
    pub has_out_delay_occurrence: bool,
    /// Whether this node is itself a general `sigDelay` read.
    pub is_delay_read: bool,
    /// Whether this node is a structural `SYMREC`/`SYMREF` tuple carrier.
    pub is_symbolic_recursion_carrier: bool,
    /// Projection facts when this signal is a symbolic recursion projection.
    pub recursive_projection: Option<RecursiveProjection>,
    /// Exactly `Int | Real | Input | FConst`.
    pub very_simple: bool,
    /// Sorted conservative compute-time effects, including non-`Gen` children.
    pub effects: Vec<EffectAtom>,
    /// Sorted effects performed by this node itself, excluding child effects.
    ///
    /// This projection lets scalar scheduling and the vector event model
    /// orient actual effect operations without paying a quadratic cost over
    /// every transitive effect carrier in the signal graph. It is always a
    /// sorted subset of `effects`.
    pub direct_effects: Vec<EffectAtom>,
}

/// Deterministic record pairing a `SigId` with its P4.2 facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalUseRecord {
    /// Signal identity.
    pub sig: SigId,
    /// Its analysis facts.
    pub info: SignalUseInfo,
}

/// Deterministic P4.2 output: records by numeric `SigId`, dependencies by
/// numeric source `SigId` then source-local `edge_key`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalUseTable {
    records: Vec<SignalUseRecord>,
    dependencies: Vec<AnalysisDependency>,
    occurrence_dependencies: Vec<OccurrenceUse>,
}

impl SignalUseTable {
    /// Records in increasing numeric `SigId` order.
    #[must_use]
    pub fn records(&self) -> &[SignalUseRecord] {
        &self.records
    }

    /// Decoded dependencies in deterministic source/edge-key order.
    #[must_use]
    pub fn dependencies(&self) -> &[AnalysisDependency] {
        &self.dependencies
    }

    /// Decoded occurrence uses in deterministic source/edge-key order.
    #[must_use]
    pub fn occurrence_dependencies(&self) -> &[OccurrenceUse] {
        &self.occurrence_dependencies
    }

    /// Looks up one record without requiring `SigId: Ord` in the public API.
    #[must_use]
    pub fn get(&self, sig: SigId) -> Option<&SignalUseInfo> {
        self.records
            .binary_search_by_key(&sig.as_u32(), |record| record.sig.as_u32())
            .ok()
            .map(|index| &self.records[index].info)
    }
}

/// Canonical P4.3a result coupling real execution conditions with decorated
/// signal-use facts from the same prepared forest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorSignalAnalysis {
    pub conditions: ExecutionConditionTable,
    pub uses: SignalUseTable,
}

/// Typed P4.2 analysis errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnalysisError {
    /// A required list-shaped signal payload was malformed, or legacy `SIGREC`
    /// reached the symbolic-recursion-only analysis boundary.
    Malformed { sig: SigId, detail: String },
    /// The verified preparation map unexpectedly lacks a reachable signal type.
    MissingType { sig: SigId },
    /// Clock inference did not annotate a reachable signal.
    MissingClock { sig: SigId },
    /// A projection index was negative.
    InvalidRecursiveProjection {
        sig: SigId,
        index: i32,
        group: SigId,
    },
    /// A delay amount type violates the bounded nonnegative C++ contract.
    InvalidDelayInterval {
        sig: SigId,
        amount: SigId,
        detail: String,
    },
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed { sig, detail } => {
                write!(f, "malformed signal {}: {detail}", sig.as_u32())
            }
            Self::MissingType { sig } => write!(f, "signal {} has no prepared type", sig.as_u32()),
            Self::MissingClock { sig } => write!(
                f,
                "signal {} has no inferred clock environment",
                sig.as_u32()
            ),
            Self::InvalidRecursiveProjection { sig, index, group } => write!(
                f,
                "projection {} has invalid negative index {index} for group {}",
                sig.as_u32(),
                group.as_u32()
            ),
            Self::InvalidDelayInterval {
                sig,
                amount,
                detail,
            } => write!(
                f,
                "signal {} has invalid delay amount {}: {detail}",
                sig.as_u32(),
                amount.as_u32()
            ),
        }
    }
}

impl std::error::Error for AnalysisError {}

/// Canonically decodes the scheduling dependencies and occurrence uses of one
/// typed signal. This is the sole `SigMatch` child enumeration for Hgraph,
/// LoopGraph, PV, and [`analyze_signal_uses`].
pub fn signal_dependencies(
    context: &SignalAnalysisContext<'_>,
    sig: SigId,
) -> Result<SignalDependencies, AnalysisError> {
    let arena = context.arena;
    let mut result = SignalDependencies::default();

    if let Some((_, body_list)) = match_sym_rec(arena, sig) {
        let definitions =
            list_to_vec(arena, body_list).ok_or_else(|| AnalysisError::Malformed {
                sig,
                detail: "malformed SYMREC body list".to_owned(),
            })?;
        for definition in definitions {
            push_occurrence(&mut result, sig, definition, 0);
            push_condition(&mut result, definition);
        }
        return Ok(result);
    }
    if match_sym_ref(arena, sig).is_some() {
        return Ok(result);
    }

    match match_sig(arena, sig) {
        SigMatch::Int(_)
        | SigMatch::Real(_)
        | SigMatch::Input(_)
        | SigMatch::Button(_)
        | SigMatch::Checkbox(_)
        | SigMatch::VSlider(_)
        | SigMatch::HSlider(_)
        | SigMatch::NumEntry(_)
        | SigMatch::Soundfile(_)
        | SigMatch::FConst(_, _, _)
        | SigMatch::FVar(_, _, _)
        | SigMatch::ClockEnvToken(_)
        | SigMatch::Unknown => {}
        SigMatch::Waveform(children) => {
            push_both_many(&mut result, sig, children.iter().copied());
        }
        SigMatch::Seq(block, held) => {
            push_schedule(&mut result, sig, block, DepKind::Immediate);
            push_occurrence(&mut result, sig, block, 0);
            push_occurrence(&mut result, sig, held, 0);
            push_condition(&mut result, block);
            push_condition(&mut result, held);
        }
        SigMatch::Delay1(value) => {
            push_schedule(&mut result, sig, value, DepKind::Delayed { amount: 1 });
            push_occurrence(&mut result, sig, value, 1);
            push_condition(&mut result, value);
        }
        SigMatch::Delay(x, amount) => {
            let amount_type = context.sig_type(amount)?;
            let max_delay = check_delay_interval(amount_type).map_err(|error| {
                AnalysisError::InvalidDelayInterval {
                    sig,
                    amount,
                    detail: error.to_string(),
                }
            })?;
            let max_delay =
                u32::try_from(max_delay).map_err(|_| AnalysisError::InvalidDelayInterval {
                    sig,
                    amount,
                    detail: format!("negative maximum delay {max_delay}"),
                })?;
            // The validated lower bound is non-negative. C++ converts it to
            // `int`, so exactly the interval [0, 1) can still schedule the
            // value as an immediate dependency.
            let schedule_kind = if amount_type.interval().lo() < 1.0 {
                DepKind::Immediate
            } else {
                DepKind::Delayed {
                    amount: max_delay.max(1),
                }
            };
            push_schedule(&mut result, sig, x, schedule_kind);
            push_schedule(&mut result, sig, amount, DepKind::Immediate);
            push_occurrence(&mut result, sig, x, max_delay);
            push_occurrence(&mut result, sig, amount, 0);
            push_condition(&mut result, x);
            push_condition(&mut result, amount);
        }
        SigMatch::Prefix(init, x) => {
            push_schedule(&mut result, sig, init, DepKind::Immediate);
            push_schedule(&mut result, sig, x, DepKind::Immediate);
            push_occurrence(&mut result, sig, init, 0);
            push_occurrence(&mut result, sig, x, 1);
            push_condition(&mut result, init);
            push_condition(&mut result, x);
        }
        SigMatch::Clocked(_, y)
        | SigMatch::TempVar(y)
        | SigMatch::PermVar(y)
        | SigMatch::Output(_, y)
        | SigMatch::IntCast(y)
        | SigMatch::BitCast(y)
        | SigMatch::FloatCast(y)
        | SigMatch::Lowest(y)
        | SigMatch::Highest(y)
        | SigMatch::Acos(y)
        | SigMatch::Asin(y)
        | SigMatch::Atan(y)
        | SigMatch::Cos(y)
        | SigMatch::Sin(y)
        | SigMatch::Tan(y)
        | SigMatch::Exp(y)
        | SigMatch::Exp10(y)
        | SigMatch::Log(y)
        | SigMatch::Log10(y)
        | SigMatch::Sqrt(y)
        | SigMatch::Abs(y)
        | SigMatch::Floor(y)
        | SigMatch::Ceil(y)
        | SigMatch::Rint(y)
        | SigMatch::Round(y)
        | SigMatch::VBargraph(_, y)
        | SigMatch::HBargraph(_, y)
        | SigMatch::ReverseTimeRec(y) => push_both(&mut result, sig, y),
        // Both `getSubSignals(..., false)` and C++ `OccMarkup` deliberately
        // stop at generators. Their contents are compiled in table context.
        SigMatch::Gen(_) => {}
        SigMatch::Proj(index, group) => {
            let (definition, kind) = context.projection_dependency(sig, index, group)?;
            push_schedule(&mut result, sig, definition, kind);
            push_occurrence(&mut result, sig, group, 0);
            push_condition(&mut result, group);
        }
        SigMatch::RdTbl(table, read_index) => {
            push_schedule(&mut result, sig, read_index, DepKind::Immediate);
            if let SigMatch::WrTbl(_, _, write_index, write_value) = match_sig(arena, table)
                && !arena.is_nil(write_index)
            {
                push_schedule(&mut result, sig, write_index, DepKind::Immediate);
                push_schedule(&mut result, sig, write_value, DepKind::Immediate);
            }
            push_occurrence(&mut result, sig, table, 0);
            push_occurrence(&mut result, sig, read_index, 0);
            push_condition(&mut result, table);
            push_condition(&mut result, read_index);
        }
        SigMatch::Control(value, gate) => {
            push_both(&mut result, sig, value);
            push_schedule(&mut result, sig, gate, DepKind::Control);
            push_occurrence(&mut result, sig, gate, 0);
            push_condition(&mut result, gate);
        }
        SigMatch::Attach(x, h) => {
            push_both(&mut result, sig, x);
            // `attach(x, h)` returns `x` and only forces `h`'s computation.
            // The `h` schedule edge is pure ordering (`Effect`), never a value
            // use: the `SIGATTACH` lowering does not load the attached value,
            // so planning a transport for it produces an orphan the body
            // checker rejects. The delay-0 occurrence stays: record coverage
            // and the scalar occurrence facts derive from occurrences, and the
            // plan suppresses occurrence-driven transports for pairs whose
            // schedule edge is `Effect`.
            push_schedule(&mut result, sig, h, DepKind::Effect);
            push_occurrence(&mut result, sig, h, 0);
            push_condition(&mut result, h);
        }
        SigMatch::ZeroPad(x, h)
        | SigMatch::Pow(x, h)
        | SigMatch::Min(x, h)
        | SigMatch::Max(x, h)
        | SigMatch::Atan2(x, h)
        | SigMatch::Fmod(x, h)
        | SigMatch::Remainder(x, h)
        | SigMatch::Enable(x, h)
        | SigMatch::SoundfileLength(x, h)
        | SigMatch::SoundfileRate(x, h)
        | SigMatch::BinOp(_, x, h) => {
            push_both(&mut result, sig, x);
            push_both(&mut result, sig, h);
        }
        SigMatch::Select2(a, b, c) | SigMatch::AssertBounds(a, b, c) => {
            push_both_many(&mut result, sig, [a, b, c]);
        }
        SigMatch::SoundfileBuffer(a, b, c, d) => {
            push_both_many(&mut result, sig, [a, b, c, d]);
        }
        SigMatch::WrTbl(size, generator, write_index, write_value) => {
            push_both(&mut result, sig, size);
            push_both(&mut result, sig, generator);
            for child in [write_index, write_value] {
                if !arena.is_nil(child) {
                    push_both(&mut result, sig, child);
                }
            }
        }
        SigMatch::OnDemand(children)
        | SigMatch::Upsampling(children)
        | SigMatch::Downsampling(children) => {
            if let Some((&clock, payload)) = children.split_first() {
                push_schedule(&mut result, sig, clock, DepKind::ClockBoundary);
                for &child in payload {
                    push_schedule(&mut result, sig, child, DepKind::Immediate);
                }
                for &child in children {
                    push_occurrence(&mut result, sig, child, 0);
                    push_condition(&mut result, child);
                }
            }
        }
        SigMatch::Fir(children) => {
            decode_fir(context, sig, children, &mut result)?;
            for &child in children {
                push_condition(&mut result, child);
            }
        }
        SigMatch::Iir(children) => {
            decode_iir(context, sig, children, &mut result)?;
            for &child in children {
                if !arena.is_nil(child) {
                    push_condition(&mut result, child);
                }
            }
        }
        SigMatch::FFun(_, args) => {
            let args = list_to_vec(arena, args).ok_or_else(|| AnalysisError::Malformed {
                sig,
                detail: "malformed FFUN argument list".to_owned(),
            })?;
            push_both_many(&mut result, sig, args);
        }
        SigMatch::BlockReverseAD {
            body,
            seeds,
            cotangents,
            ..
        } => {
            for list in [body, seeds, cotangents] {
                let items = list_to_vec(arena, list).ok_or_else(|| AnalysisError::Malformed {
                    sig,
                    detail: "malformed BlockReverseAD child list".to_owned(),
                })?;
                push_both_many(&mut result, sig, items);
            }
        }
        SigMatch::Rec(_) => {
            return Err(AnalysisError::Malformed {
                sig,
                detail: "legacy SIGREC form is not supported by hgraph".to_owned(),
            });
        }
    }
    Ok(result)
}

fn build_execution_conditions(
    analysis: &SignalAnalysisContext<'_>,
    roots: &[SigId],
) -> Result<ExecutionConditionTable, AnalysisError> {
    let unconditional = ExecutionCondition::unconditional();
    let mut by_signal = BTreeMap::<u32, ExecutionCondition>::new();
    let mut work = VecDeque::<(SigId, ExecutionCondition)>::new();
    for &root in roots {
        work.push_back((root, unconditional.clone()));
    }

    while let Some((sig, incoming)) = work.pop_front() {
        let condition = if let Some(current) = by_signal.get(&sig.as_u32()) {
            let joined = current.or(&incoming);
            if joined == *current {
                continue;
            }
            joined
        } else {
            incoming
        };
        by_signal.insert(sig.as_u32(), condition.clone());

        let dependencies = signal_dependencies(analysis, sig)?;
        if let SigMatch::Control(value, gate) = match_sig(analysis.arena, sig) {
            work.push_back((gate, condition.clone()));
            work.push_back((value, condition.and(&ExecutionCondition::atom(gate))));
        } else {
            for &child in dependencies.condition_children() {
                work.push_back((child, condition.clone()));
            }
        }
    }

    let mut conditions = by_signal.values().cloned().collect::<Vec<_>>();
    conditions.push(unconditional.clone());
    conditions.sort();
    conditions.dedup();
    let ids = conditions
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, condition)| {
            (
                condition,
                CondId(u64::try_from(index).expect("condition count fits u64")),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let unconditional = ids[&unconditional];
    let by_signal = by_signal
        .into_iter()
        .map(|(sig, condition)| (sig, ids[&condition]))
        .collect();

    Ok(ExecutionConditionTable {
        conditions,
        by_signal,
        unconditional,
    })
}

fn push_schedule(result: &mut SignalDependencies, from: SigId, to: SigId, kind: DepKind) {
    let edge_key = result.scheduling.len();
    result.scheduling.push(AnalysisDependency {
        from,
        to,
        kind,
        edge_key,
    });
}

fn push_occurrence(result: &mut SignalDependencies, from: SigId, to: SigId, delay: u32) {
    let edge_key = result.occurrences.len();
    result.occurrences.push(OccurrenceUse {
        from,
        to,
        delay,
        edge_key,
    });
}

fn push_both(result: &mut SignalDependencies, from: SigId, to: SigId) {
    push_schedule(result, from, to, DepKind::Immediate);
    push_occurrence(result, from, to, 0);
    push_condition(result, to);
}

fn push_both_many(
    result: &mut SignalDependencies,
    from: SigId,
    children: impl IntoIterator<Item = SigId>,
) {
    for child in children {
        push_both(result, from, child);
    }
}

fn push_condition(result: &mut SignalDependencies, child: SigId) {
    result.condition_children.push(child);
}

fn is_zero_signal(arena: &TreeArena, sig: SigId) -> bool {
    matches!(match_sig(arena, sig), SigMatch::Int(0))
        || matches!(match_sig(arena, sig), SigMatch::Real(value) if value == 0.0)
}

fn state_effects(sig: SigId, cell: StateCell) -> BTreeSet<EffectAtom> {
    let resource = StateResource::Signal {
        owner: sig.as_u32(),
        cell,
    };
    BTreeSet::from([
        EffectAtom::ReadState(resource.clone()),
        EffectAtom::WriteState(resource),
    ])
}

fn match_ffunction_descriptor(arena: &TreeArena, id: SigId) -> Option<(SigId, SigId, SigId)> {
    let node = arena.node(id)?;
    let NodeKind::Tag(tag_id) = node.kind else {
        return None;
    };
    if arena.tag_name(tag_id)? != "FFUN" {
        return None;
    }
    let [signature, include_file, library_file] = node.children.as_slice() else {
        return None;
    };
    Some((*signature, *include_file, *library_file))
}

fn decode_foreign_signature(
    arena: &TreeArena,
    owner: SigId,
    descriptor: SigId,
) -> Result<ForeignSignature, AnalysisError> {
    let Some((signature, _, _)) = match_ffunction_descriptor(arena, descriptor) else {
        return Err(AnalysisError::Malformed {
            sig: owner,
            detail: "FFUN call has a malformed foreign-function descriptor".to_owned(),
        });
    };
    let items = list_to_vec(arena, signature).ok_or_else(|| AnalysisError::Malformed {
        sig: owner,
        detail: "FFUN signature is not a list".to_owned(),
    })?;
    if items.len() < 2 {
        return Err(AnalysisError::Malformed {
            sig: owner,
            detail: "FFUN signature needs a return type and name list".to_owned(),
        });
    }
    let return_type = tree_to_int(arena, items[0]).ok_or_else(|| AnalysisError::Malformed {
        sig: owner,
        detail: "FFUN return type is not an integer code".to_owned(),
    })?;
    let names = list_to_vec(arena, items[1])
        .ok_or_else(|| AnalysisError::Malformed {
            sig: owner,
            detail: "FFUN names are not a list".to_owned(),
        })?
        .into_iter()
        .map(|name| {
            tree_to_str(arena, name)
                .map(str::to_owned)
                .ok_or_else(|| AnalysisError::Malformed {
                    sig: owner,
                    detail: "FFUN name slot is not a symbol".to_owned(),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let arguments = items[2..]
        .iter()
        .map(|&item| {
            tree_to_int(arena, item)
                .map(ForeignTypeCode)
                .ok_or_else(|| AnalysisError::Malformed {
                    sig: owner,
                    detail: "FFUN argument type is not an integer code".to_owned(),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ForeignSignature {
        names,
        return_type: ForeignTypeCode(return_type),
        arguments,
    })
}

fn direct_effects(
    analysis: &SignalAnalysisContext<'_>,
    sig: SigId,
) -> Result<BTreeSet<EffectAtom>, AnalysisError> {
    let arena = analysis.arena;
    if let Some((_, body)) = match_sym_rec(arena, sig) {
        let definitions = list_to_vec(arena, body).ok_or_else(|| AnalysisError::Malformed {
            sig,
            detail: "malformed SYMREC body list while deriving effects".to_owned(),
        })?;
        let mut effects = BTreeSet::new();
        for index in 0..definitions.len() {
            let projection = u32::try_from(index).map_err(|_| AnalysisError::Malformed {
                sig,
                detail: "SYMREC projection index does not fit u32".to_owned(),
            })?;
            let resource = StateResource::Recursion {
                group: sig.as_u32(),
                projection,
            };
            effects.insert(EffectAtom::ReadState(resource.clone()));
            effects.insert(EffectAtom::WriteState(resource));
        }
        return Ok(effects);
    }

    let effects = match match_sig(arena, sig) {
        // Delay storage is allocated for the carried signal and shared by all
        // of its readers, regardless of the requested history depth.
        SigMatch::Delay1(value) | SigMatch::Delay(value, _) => {
            state_effects(value, StateCell::Delay)
        }
        SigMatch::Prefix(_, _) => state_effects(sig, StateCell::Prefix),
        SigMatch::Fir(_) => state_effects(sig, StateCell::Fir),
        SigMatch::Iir(_) => state_effects(sig, StateCell::Iir),
        SigMatch::Waveform(_) => state_effects(sig, StateCell::WaveformIndex),
        SigMatch::Seq(_, _) => state_effects(sig, StateCell::Hold),
        SigMatch::Clocked(_, _)
        | SigMatch::OnDemand(_)
        | SigMatch::Upsampling(_)
        | SigMatch::Downsampling(_) => state_effects(sig, StateCell::Clock),
        SigMatch::ReverseTimeRec(_) => state_effects(sig, StateCell::ReverseTime),
        SigMatch::BlockReverseAD { .. } => state_effects(sig, StateCell::ReverseAd),
        SigMatch::Proj(index, group_ref) if index >= 0 => {
            let group = analysis.resolve_rec_group(group_ref).unwrap_or(group_ref);
            let resource = StateResource::Recursion {
                group: group.as_u32(),
                projection: u32::try_from(index).expect("nonnegative i32 fits u32"),
            };
            BTreeSet::from([
                EffectAtom::ReadState(resource.clone()),
                EffectAtom::WriteState(resource),
            ])
        }
        // A read-only table has no live writer port, so it contributes no
        // compute-time write. Its generator subtree keeps its own effects, and
        // fill-before-read stays carried by the data edge from every `RdTbl` to
        // its table operand.
        SigMatch::WrTbl(_, _, write_index, write_value)
            if wrtbl_is_readonly(arena, write_index, write_value) =>
        {
            BTreeSet::new()
        }
        SigMatch::WrTbl(_, _, _, _) => BTreeSet::from([EffectAtom::WriteTable(sig.as_u32())]),
        SigMatch::RdTbl(table, _) => BTreeSet::from([EffectAtom::ReadTable(table.as_u32())]),
        SigMatch::VBargraph(control, _) | SigMatch::HBargraph(control, _) => {
            BTreeSet::from([EffectAtom::WriteUi(control)])
        }
        SigMatch::Output(channel, _) if channel >= 0 => BTreeSet::from([EffectAtom::WriteOutput(
            u32::try_from(channel).expect("nonnegative i32 fits u32"),
        )]),
        SigMatch::Output(channel, _) => {
            return Err(AnalysisError::Malformed {
                sig,
                detail: format!("negative output channel {channel}"),
            });
        }
        SigMatch::FFun(descriptor, _) => BTreeSet::from([EffectAtom::Foreign {
            resource: ForeignResource::Function(decode_foreign_signature(arena, sig, descriptor)?),
            purity: ForeignPurity::Unknown,
        }]),
        SigMatch::FVar(value_type, name, _) => {
            let name = tree_to_str(arena, name).ok_or_else(|| AnalysisError::Malformed {
                sig,
                detail: "foreign variable name is not a symbol".to_owned(),
            })?;
            if name == "count" {
                BTreeSet::new()
            } else {
                let value_type =
                    tree_to_int(arena, value_type).ok_or_else(|| AnalysisError::Malformed {
                        sig,
                        detail: "foreign variable type is not an integer code".to_owned(),
                    })?;
                BTreeSet::from([EffectAtom::Foreign {
                    resource: ForeignResource::Variable {
                        name: name.to_owned(),
                        value_type: ForeignTypeCode(value_type),
                    },
                    purity: ForeignPurity::Unknown,
                }])
            }
        }
        _ => BTreeSet::new(),
    };
    Ok(effects)
}

fn decorate_effects(
    analysis: &SignalAnalysisContext<'_>,
    records: &mut BTreeMap<u32, SignalUseRecord>,
    dependencies: &BTreeMap<u32, SignalDependencies>,
) -> Result<(), AnalysisError> {
    let mut direct = BTreeMap::<u32, BTreeSet<EffectAtom>>::new();
    for record in records.values() {
        direct.insert(record.sig.as_u32(), direct_effects(analysis, record.sig)?);
    }
    // The former whole-map fixed point cloned every signal's complete effect
    // set once per graph-depth iteration. Deep UI-bearing DSPs therefore paid
    // roughly O(depth * signals * effects). Propagate only changed child sets
    // to their parents instead; this computes the same least union fixed point
    // and converges over cycles without rescanning unrelated records.
    let mut parents = BTreeMap::<u32, BTreeSet<u32>>::new();
    for (&parent, projection) in dependencies {
        for child in projection.condition_children() {
            if direct.contains_key(&child.as_u32()) {
                parents.entry(child.as_u32()).or_default().insert(parent);
            }
        }
    }
    let (accumulated, _) = propagate_effect_sets(&direct, &parents);
    for (&sig, effects) in &accumulated {
        let info = &mut records
            .get_mut(&sig)
            .expect("effect record has matching signal record")
            .info;
        info.effects = effects.iter().cloned().collect();
        info.direct_effects = direct[&sig].iter().cloned().collect();
    }
    Ok(())
}

fn propagate_effect_sets(
    direct: &BTreeMap<u32, BTreeSet<EffectAtom>>,
    parents: &BTreeMap<u32, BTreeSet<u32>>,
) -> (BTreeMap<u32, BTreeSet<EffectAtom>>, usize) {
    let mut accumulated = direct.clone();
    let mut pending = accumulated.keys().copied().collect::<BTreeSet<_>>();
    let mut updates = 0_usize;
    while let Some(child) = pending.pop_first() {
        let child_effects = accumulated
            .remove(&child)
            .expect("pending effect child has an accumulated set");
        for &parent in parents.get(&child).into_iter().flatten() {
            if parent == child {
                continue;
            }
            let parent_effects = accumulated
                .get_mut(&parent)
                .expect("effect parent has a signal record");
            let previous_len = parent_effects.len();
            parent_effects.extend(child_effects.iter().cloned());
            if parent_effects.len() != previous_len {
                updates += 1;
                pending.insert(parent);
            }
        }
        accumulated.insert(child, child_effects);
    }
    (accumulated, updates)
}

fn decode_fir(
    context: &SignalAnalysisContext<'_>,
    sig: SigId,
    children: &[SigId],
    result: &mut SignalDependencies,
) -> Result<(), AnalysisError> {
    let Some((&input, coefficients)) = children.split_first() else {
        return Err(AnalysisError::Malformed {
            sig,
            detail: "FIR carrier has no input".to_owned(),
        });
    };
    if coefficients.is_empty() {
        return Err(AnalysisError::Malformed {
            sig,
            detail: "FIR carrier has no coefficient".to_owned(),
        });
    }

    for &coefficient in coefficients {
        push_schedule(result, sig, coefficient, DepKind::Immediate);
        push_occurrence(result, sig, coefficient, 0);
    }
    let first_nonzero = coefficients
        .iter()
        .position(|&coefficient| !is_zero_signal(context.arena, coefficient));
    let schedule_delay = first_nonzero.unwrap_or(1);
    let schedule_kind = if schedule_delay == 0 {
        DepKind::Immediate
    } else {
        DepKind::Delayed {
            amount: u32::try_from(schedule_delay).expect("FIR tap index fits u32"),
        }
    };
    push_schedule(result, sig, input, schedule_kind);

    if first_nonzero.is_some() {
        for (delay, &coefficient) in coefficients.iter().enumerate() {
            if !is_zero_signal(context.arena, coefficient) {
                push_occurrence(
                    result,
                    sig,
                    input,
                    u32::try_from(delay).expect("FIR tap index fits u32"),
                );
            }
        }
    } else {
        push_occurrence(result, sig, input, 0);
    }
    Ok(())
}

fn decode_iir(
    _context: &SignalAnalysisContext<'_>,
    sig: SigId,
    children: &[SigId],
    result: &mut SignalDependencies,
) -> Result<(), AnalysisError> {
    if children.len() < 4 {
        return Err(AnalysisError::Malformed {
            sig,
            detail: "compact IIR carrier requires state, input, C0, and one feedback coefficient"
                .to_owned(),
        });
    }
    let input = children[1];
    for &dependency in &children[1..] {
        push_schedule(result, sig, dependency, DepKind::Immediate);
    }
    push_occurrence(result, sig, input, 0);
    // V[2] is the structural C0 term and is zero in canonical IIR carriers.
    // OccMarkup starts at V[3], whose self-use is delayed by one sample.
    for (index, &coefficient) in children.iter().enumerate().skip(3) {
        push_occurrence(result, sig, coefficient, 0);
        push_occurrence(
            result,
            sig,
            sig,
            u32::try_from(index - 2).expect("IIR tap index fits u32"),
        );
    }
    Ok(())
}

fn compute_recursiveness(
    context: &SignalAnalysisContext<'_>,
    roots: &[SigId],
) -> Result<(BTreeMap<u32, u32>, usize), AnalysisError> {
    #[derive(Clone)]
    enum Frame {
        Enter {
            sig: SigId,
            env: Vec<SigId>,
        },
        Exit {
            sig: SigId,
            children: Vec<SigId>,
            binder: bool,
        },
    }

    // C++ `recursiveness.cpp::annotate` stores `RECURSIVNESS` directly on the
    // signal tree and returns it on every later visit, independently of the
    // current recursive environment. Keep the same first-visit memoization:
    // keying by the whole environment makes shared recursive DAGs expand once
    // per binder combination and can grow exponentially.
    let mut memo = BTreeMap::<u32, u32>::new();
    let mut expanded_signals = 0;
    let mut stack = roots
        .iter()
        .rev()
        .copied()
        .map(|sig| Frame::Enter {
            sig,
            env: Vec::new(),
        })
        .collect::<Vec<_>>();

    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Enter { sig, env } => {
                let signal_key = sig.as_u32();
                if memo.contains_key(&signal_key) {
                    continue;
                }
                expanded_signals += 1;
                if let Some(var) = match_sym_ref(context.arena, sig) {
                    let depth = env
                        .iter()
                        .position(|candidate| *candidate == var)
                        .map_or(0, |position| {
                            u32::try_from(position + 1).expect("recursion depth fits u32")
                        });
                    memo.insert(signal_key, depth);
                    continue;
                }

                let dependencies = signal_dependencies(context, sig)?;
                let children = dependencies
                    .occurrences()
                    .iter()
                    .filter_map(|dependency| (dependency.to != sig).then_some(dependency.to))
                    .collect::<Vec<_>>();
                let mut child_env = env.clone();
                let binder = if let Some((var, _)) = match_sym_rec(context.arena, sig) {
                    child_env.insert(0, var);
                    true
                } else {
                    false
                };
                stack.push(Frame::Exit {
                    sig,
                    children: children.clone(),
                    binder,
                });
                for child in children.into_iter().rev() {
                    stack.push(Frame::Enter {
                        sig: child,
                        env: child_env.clone(),
                    });
                }
            }
            Frame::Exit {
                sig,
                children,
                binder,
            } => {
                let maximum = children
                    .iter()
                    .map(|&child| {
                        memo.get(&child.as_u32())
                            .copied()
                            .expect("child recursiveness computed before parent")
                    })
                    .max()
                    .unwrap_or(0);
                let value = if binder {
                    maximum.saturating_sub(1)
                } else {
                    maximum
                };
                memo.insert(sig.as_u32(), value);
            }
        }
    }
    Ok((memo, expanded_signals))
}

/// Builds the canonical P4.3a condition/effect analysis for a verified forest.
pub fn analyze_vector_signals(
    prepared: &VerifiedPreparedSignals,
    clk_envs: &ClkEnvMap,
) -> Result<VectorSignalAnalysis, AnalysisError> {
    let timing_enabled = std::env::var_os("FAUST_RS_VECTOR_TIMING").is_some();
    let started = std::time::Instant::now();
    let conditions = ExecutionConditionTable::build(prepared)?;
    if timing_enabled {
        eprintln!(
            "[vector-analysis-stage] conditions: {:.3}s",
            started.elapsed().as_secs_f64()
        );
    }
    let started = std::time::Instant::now();
    let uses = analyze_signal_uses(prepared, clk_envs, &conditions)?;
    if timing_enabled {
        eprintln!(
            "[vector-analysis-stage] uses: {:.3}s",
            started.elapsed().as_secs_f64()
        );
    }
    Ok(VectorSignalAnalysis { conditions, uses })
}

/// Direct effect facts needed by scalar scheduling, keyed by prepared signal.
///
/// Unlike [`VectorSignalAnalysis`], this intentionally contains neither
/// occurrence facts nor execution conditions: scalar conflict orientation only
/// needs the effects performed by each node itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScalarSchedulingEffects {
    direct: BTreeMap<u32, Vec<EffectAtom>>,
}

impl ScalarSchedulingEffects {
    #[must_use]
    pub(crate) fn direct_effects(&self, sig: SigId) -> &[EffectAtom] {
        self.direct.get(&sig.as_u32()).map_or(&[], Vec::as_slice)
    }
}

/// Builds only the compute-time effect facts required by scalar scheduling.
///
/// Scalar [`crate::hgraph::orient_effect_conflicts`] consumes direct effects
/// and never inspects occurrence facts, clock environments, or execution
/// conditions. Avoiding those vector-oriented analyses keeps `-ss` from
/// paying for certification data it cannot observe.
pub fn analyze_scalar_scheduling_effects(
    prepared: &VerifiedPreparedSignals,
) -> Result<ScalarSchedulingEffects, AnalysisError> {
    let analysis = SignalAnalysisContext::new(
        prepared.arena(),
        prepared.sig_types_map(),
        prepared.outputs(),
    )?;
    let mut direct = BTreeMap::new();
    for &sig in prepared.sig_types_map().keys() {
        let effects = direct_effects(&analysis, sig)?.into_iter().collect();
        direct.insert(sig.as_u32(), effects);
    }
    Ok(ScalarSchedulingEffects { direct })
}

/// Builds deterministic occurrence/effect facts with an injected condition
/// provider. Production clients should prefer [`analyze_vector_signals`]; this
/// lower-level entry point remains useful for rule tests and formal mutations.
pub fn analyze_signal_uses(
    prepared: &VerifiedPreparedSignals,
    clk_envs: &ClkEnvMap,
    conditions: &impl ExecutionConditions,
) -> Result<SignalUseTable, AnalysisError> {
    let context = SignalAnalysisContext::new(
        prepared.arena(),
        prepared.sig_types_map(),
        prepared.outputs(),
    )?;
    analyze_forest(
        &context,
        prepared.outputs(),
        |sig| clk_envs.env(sig),
        conditions,
    )
}

fn analyze_forest(
    analysis: &SignalAnalysisContext<'_>,
    roots: &[SigId],
    clk_env: impl Fn(SigId) -> Option<ClkEnv>,
    conditions: &impl ExecutionConditions,
) -> Result<SignalUseTable, AnalysisError> {
    let timing_enabled = std::env::var_os("FAUST_RS_VECTOR_TIMING").is_some();
    let mut stage_started = std::time::Instant::now();
    let mut trace_stage = |stage: &str| {
        if timing_enabled {
            eprintln!(
                "[vector-uses-stage] {stage}: {:.3}s",
                stage_started.elapsed().as_secs_f64()
            );
        }
        stage_started = std::time::Instant::now();
    };
    let (recursiveness, _) = compute_recursiveness(analysis, roots)?;
    trace_stage("recursiveness");
    let mut records = BTreeMap::<u32, SignalUseRecord>::new();
    let mut dependency_cache = BTreeMap::<u32, SignalDependencies>::new();
    let mut expanded_signals = BTreeSet::<u32>::new();
    let mut work = VecDeque::<(SigId, UseContext, u32)>::new();

    for &root in roots {
        work.push_back((
            root,
            UseContext {
                variability: Variability::Samp,
                recursiveness: 0,
                condition: conditions.root_condition(root),
            },
            0,
        ));
    }

    while let Some((sig, use_context, delay)) = work.pop_front() {
        ensure_record(
            &mut records,
            analysis,
            sig,
            &recursiveness,
            &clk_env,
            conditions,
        )?;
        increment_occurrence(
            records.get_mut(&sig.as_u32()).expect("record inserted"),
            use_context,
            delay,
        );

        // C++ OccMarkup increments every use but recursively marks children
        // only on the signal's first visit. In particular, a second context on
        // a shared signal does not leak into all of that signal's descendants.
        let first_visit = expanded_signals.insert(sig.as_u32());
        if !first_visit {
            if let SigMatch::BinOp(BinOp::Mul, left, right) = match_sig(analysis.arena, sig)
                && matches!(match_sig(analysis.arena, left), SigMatch::Int(-1))
            {
                // C++ propagates repeated `-1 * y` uses because codegen ignores
                // sharing of the negation wrapper itself.
                work.push_back((right, use_context, delay));
            }
            continue;
        }
        if let std::collections::btree_map::Entry::Vacant(entry) =
            dependency_cache.entry(sig.as_u32())
        {
            entry.insert(signal_dependencies(analysis, sig)?);
        }
        let dependencies = dependency_cache
            .get(&sig.as_u32())
            .expect("dependencies inserted")
            .clone();
        let parent = &records.get(&sig.as_u32()).expect("record inserted").info;
        let child_context = UseContext {
            // C++ OccMarkup passes the current node's inferred variability
            // and recursiveness, not those inherited by this use.
            variability: parent.variability,
            recursiveness: parent.recursiveness,
            condition: conditions.signal_condition(sig),
        };

        for occurrence in dependencies.occurrences() {
            work.push_back((occurrence.to, child_context, occurrence.delay));
        }
    }
    trace_stage("occurrences-and-dependencies");

    decorate_effects(analysis, &mut records, &dependency_cache)?;
    trace_stage("effects");

    let mut dependencies = dependency_cache
        .values()
        .flat_map(|projection| projection.scheduling.iter().copied())
        .collect::<Vec<_>>();
    dependencies.sort_by_key(|dependency| (dependency.from.as_u32(), dependency.edge_key));
    let mut occurrence_dependencies = dependency_cache
        .into_values()
        .flat_map(|projection| projection.occurrences)
        .collect::<Vec<_>>();
    occurrence_dependencies
        .sort_by_key(|dependency| (dependency.from.as_u32(), dependency.edge_key));
    for record in records.values_mut() {
        finalize_occurrences(&mut record.info);
    }
    trace_stage("canonicalization");
    Ok(SignalUseTable {
        records: records.into_values().collect(),
        dependencies,
        occurrence_dependencies,
    })
}

fn ensure_record(
    records: &mut BTreeMap<u32, SignalUseRecord>,
    analysis: &SignalAnalysisContext<'_>,
    sig: SigId,
    recursiveness: &BTreeMap<u32, u32>,
    clk_env: &impl Fn(SigId) -> Option<ClkEnv>,
    conditions: &impl ExecutionConditions,
) -> Result<(), AnalysisError> {
    if records.contains_key(&sig.as_u32()) {
        return Ok(());
    }
    let sig_type = analysis.sig_type(sig)?.clone();
    let clk_env = clk_env(sig).ok_or(AnalysisError::MissingClock { sig })?;
    let recursive_projection = match match_sig(analysis.arena, sig) {
        SigMatch::Proj(index, group) if index < 0 => {
            return Err(AnalysisError::InvalidRecursiveProjection { sig, index, group });
        }
        SigMatch::Proj(index, group_ref) => {
            let group = analysis.resolve_rec_group(group_ref).unwrap_or(group_ref);
            Some(RecursiveProjection {
                index: usize::try_from(index).expect("nonnegative i32 fits usize"),
                group,
            })
        }
        _ => None,
    };
    let very_simple = matches!(
        match_sig(analysis.arena, sig),
        SigMatch::Int(_) | SigMatch::Real(_) | SigMatch::Input(_) | SigMatch::FConst(_, _, _)
    );
    records.insert(
        sig.as_u32(),
        SignalUseRecord {
            sig,
            info: SignalUseInfo {
                variability: sig_type.variability(),
                vectorability: sig_type.vectorability(),
                sig_type,
                clk_env,
                recursiveness: recursiveness.get(&sig.as_u32()).copied().unwrap_or(0),
                execution_condition: conditions.signal_condition(sig),
                occurrences: OccInfo::default(),
                max_delay: 0,
                delay_reads: 0,
                has_out_delay_occurrence: false,
                is_delay_read: matches!(match_sig(analysis.arena, sig), SigMatch::Delay(_, _)),
                is_symbolic_recursion_carrier: match_sym_rec(analysis.arena, sig).is_some()
                    || match_sym_ref(analysis.arena, sig).is_some(),
                recursive_projection,
                very_simple,
                effects: Vec::new(),
                direct_effects: Vec::new(),
            },
        },
    );
    Ok(())
}

fn increment_occurrence(record: &mut SignalUseRecord, context: UseContext, delay: u32) {
    if delay == 0 {
        record.info.has_out_delay_occurrence = true;
    } else {
        record.info.max_delay = record.info.max_delay.max(delay);
        record.info.delay_reads = record.info.delay_reads.saturating_add(1);
    }
    if let Some(occurrence) = record
        .info
        .occurrences
        .per_context
        .iter_mut()
        .find(|occurrence| occurrence.context == context)
    {
        occurrence.count = occurrence.count.saturating_add(1);
        return;
    }
    record
        .info
        .occurrences
        .per_context
        .push(ContextOccurrence { context, count: 1 });
}

fn finalize_occurrences(info: &mut SignalUseInfo) {
    info.occurrences
        .per_context
        .sort_by_key(|occurrence| occurrence.context);
    let own_variability = extended_variability(info.variability, info.recursiveness);
    let mut counts = [0_u32; 4];
    for occurrence in &info.occurrences.per_context {
        let context_variability = extended_variability(
            occurrence.context.variability,
            occurrence.context.recursiveness,
        );
        counts[usize::from(context_variability)] =
            counts[usize::from(context_variability)].saturating_add(occurrence.count);
        if context_variability > own_variability
            || occurrence.context.condition != info.execution_condition
        {
            info.occurrences.multi = true;
        }
    }
    info.occurrences.multi |= counts.into_iter().any(|count| count > 1);
}

fn extended_variability(variability: Variability, recursiveness: u32) -> u8 {
    let variability = match variability {
        Variability::Konst => 0,
        Variability::Block => 1,
        Variability::Samp => 2,
    };
    (variability + u8::from(recursiveness > 0)).min(3)
}

#[cfg(test)]
mod tests;
