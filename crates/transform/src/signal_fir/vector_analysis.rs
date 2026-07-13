//! Typed vector-analysis spine (P4.2).
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
//! P4.2 deliberately defers effects, the full execution-condition producer,
//! `DecorationCertificate`, and all production consumers.  The reserved
//! dependency kinds below make those later additions explicit without
//! inventing effect semantics in this phase.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::fmt;

use signals::{BinOp, SigId, SigMatch, match_sig};
use sigtype::{SigType, Variability, Vectorability, check_delay_interval};
use tlib::{TreeArena, list_to_vec, match_sym_rec, match_sym_ref};

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
    /// Projection facts when this signal is a symbolic recursion projection.
    pub recursive_projection: Option<RecursiveProjection>,
    /// Exactly `Int | Real | Input | FConst`.
    pub very_simple: bool,
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
        }
        SigMatch::Delay1(value) => {
            push_schedule(&mut result, sig, value, DepKind::Delayed { amount: 1 });
            push_occurrence(&mut result, sig, value, 1);
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
        }
        SigMatch::Prefix(init, x) => {
            push_schedule(&mut result, sig, init, DepKind::Immediate);
            push_schedule(&mut result, sig, x, DepKind::Immediate);
            push_occurrence(&mut result, sig, init, 0);
            push_occurrence(&mut result, sig, x, 1);
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
        }
        SigMatch::ZeroPad(x, h)
        | SigMatch::Pow(x, h)
        | SigMatch::Min(x, h)
        | SigMatch::Max(x, h)
        | SigMatch::Atan2(x, h)
        | SigMatch::Fmod(x, h)
        | SigMatch::Remainder(x, h)
        | SigMatch::Attach(x, h)
        | SigMatch::Enable(x, h)
        | SigMatch::Control(x, h)
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
                }
            }
        }
        SigMatch::Fir(children) => decode_fir(context, sig, children, &mut result)?,
        SigMatch::Iir(children) => decode_iir(context, sig, children, &mut result)?,
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

fn is_zero_signal(arena: &TreeArena, sig: SigId) -> bool {
    matches!(match_sig(arena, sig), SigMatch::Int(0))
        || matches!(match_sig(arena, sig), SigMatch::Real(value) if value == 0.0)
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
) -> Result<BTreeMap<u32, u32>, AnalysisError> {
    #[derive(Clone)]
    enum Frame {
        Enter {
            sig: SigId,
            env: Vec<SigId>,
        },
        Exit {
            sig: SigId,
            env: Vec<SigId>,
            child_env: Vec<SigId>,
            children: Vec<SigId>,
            binder: bool,
        },
    }

    let key = |sig: SigId, env: &[SigId]| {
        (
            sig.as_u32(),
            env.iter().map(|var| var.as_u32()).collect::<Vec<_>>(),
        )
    };
    let mut memo = BTreeMap::<(u32, Vec<u32>), u32>::new();
    let mut by_signal = BTreeMap::<u32, u32>::new();
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
                let frame_key = key(sig, &env);
                if memo.contains_key(&frame_key) {
                    continue;
                }
                if let Some(var) = match_sym_ref(context.arena, sig) {
                    let depth = env
                        .iter()
                        .position(|candidate| *candidate == var)
                        .map_or(0, |position| {
                            u32::try_from(position + 1).expect("recursion depth fits u32")
                        });
                    memo.insert(frame_key, depth);
                    by_signal.entry(sig.as_u32()).or_insert(depth);
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
                    env,
                    child_env: child_env.clone(),
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
                env,
                child_env,
                children,
                binder,
            } => {
                let maximum = children
                    .iter()
                    .map(|&child| {
                        memo.get(&key(child, &child_env))
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
                memo.insert(key(sig, &env), value);
                by_signal.entry(sig.as_u32()).or_insert(value);
            }
        }
    }
    Ok(by_signal)
}

/// Builds deterministic P4.2 occurrence facts from a verified prepared forest.
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
    let recursiveness = compute_recursiveness(analysis, roots)?;
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
                recursive_projection,
                very_simple,
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
mod tests {
    use propagate::ClockDomainTable;
    use signals::SigBuilder;
    use tlib::{TreeArena, sym_rec, vec_to_list};

    use super::*;
    use crate::clk_env::annotate;
    use crate::signal_prepare::prepare_signals_for_fir_verified;

    fn dep_targets(deps: &[AnalysisDependency]) -> Vec<(u32, DepKind)> {
        deps.iter().map(|dep| (dep.to.as_u32(), dep.kind)).collect()
    }

    fn occurrence_targets(deps: &[OccurrenceUse]) -> Vec<(u32, u32)> {
        deps.iter()
            .map(|dep| (dep.to.as_u32(), dep.delay))
            .collect()
    }

    fn decode(
        arena: &TreeArena,
        roots: &[SigId],
        sig: SigId,
    ) -> Result<SignalDependencies, AnalysisError> {
        let types = sigtype::TypeAnnotator::new(arena, &ui::UiProgram::empty())
            .annotate(roots)
            .unwrap();
        let context = SignalAnalysisContext::new(arena, &types, roots)?;
        signal_dependencies(&context, sig)
    }

    #[test]
    fn delay_prefix_and_seq_have_distinct_scheduling_and_occurrence_rules() {
        let mut arena = TreeArena::new();
        let (x, one, three, delay1, fixed, bounded_zero, bounded_one, prefix, seq) = {
            let mut b = SigBuilder::new(&mut arena);
            let x = b.input(0);
            let zero = b.int(0);
            let one = b.int(1);
            let three = b.int(3);
            let raw_dynamic = b.input(1);
            let dynamic = b.mul(raw_dynamic, three);
            let zero_to_three = b.assert_bounds(zero, three, dynamic);
            let one_to_three = b.assert_bounds(one, three, dynamic);
            let delay1 = b.delay1(x);
            let fixed = b.delay(x, three);
            let bounded_zero = b.delay(x, zero_to_three);
            let bounded_one = b.delay(x, one_to_three);
            let prefix = b.prefix(one, x);
            let seq = b.seq(x, dynamic);
            (
                x,
                one,
                three,
                delay1,
                fixed,
                bounded_zero,
                bounded_one,
                prefix,
                seq,
            )
        };
        let roots = [delay1, fixed, bounded_zero, bounded_one, prefix, seq];

        let delay1_deps = decode(&arena, &roots, delay1).unwrap();
        assert_eq!(
            dep_targets(delay1_deps.scheduling()),
            vec![(x.as_u32(), DepKind::Delayed { amount: 1 })]
        );
        assert_eq!(
            occurrence_targets(delay1_deps.occurrences()),
            vec![(x.as_u32(), 1)]
        );

        let fixed_deps = decode(&arena, &roots, fixed).unwrap();
        assert_eq!(
            dep_targets(fixed_deps.scheduling()),
            vec![
                (x.as_u32(), DepKind::Delayed { amount: 3 }),
                (three.as_u32(), DepKind::Immediate)
            ]
        );
        assert_eq!(
            occurrence_targets(fixed_deps.occurrences()),
            vec![(x.as_u32(), 3), (three.as_u32(), 0)]
        );

        let bounded_zero_deps = decode(&arena, &roots, bounded_zero).unwrap();
        assert_eq!(
            bounded_zero_deps.scheduling()[0].kind,
            DepKind::Immediate,
            "[0, N] may read the current sample"
        );
        assert_eq!(bounded_zero_deps.occurrences()[0].delay, 3);

        let bounded_one_deps = decode(&arena, &roots, bounded_one).unwrap();
        assert_eq!(
            bounded_one_deps.scheduling()[0].kind,
            DepKind::Delayed { amount: 3 },
            "[1, N] is causally delayed"
        );

        let prefix_deps = decode(&arena, &roots, prefix).unwrap();
        assert_eq!(
            dep_targets(prefix_deps.scheduling()),
            vec![
                (one.as_u32(), DepKind::Immediate),
                (x.as_u32(), DepKind::Immediate)
            ]
        );
        assert_eq!(
            occurrence_targets(prefix_deps.occurrences()),
            vec![(one.as_u32(), 0), (x.as_u32(), 1)]
        );

        let seq_deps = decode(&arena, &roots, seq).unwrap();
        assert_eq!(
            dep_targets(seq_deps.scheduling()),
            vec![(x.as_u32(), DepKind::Immediate)]
        );
        assert_eq!(seq_deps.occurrences().len(), 2);
    }

    #[test]
    fn projection_schedules_its_selected_definition_and_marks_the_group() {
        let mut arena = TreeArena::new();
        let (first, second) = {
            let mut b = SigBuilder::new(&mut arena);
            (b.input(0), b.input(1))
        };
        let var = arena.symbol("r");
        let body = vec_to_list(&mut arena, &[first, second]);
        let group = sym_rec(&mut arena, var, body);
        let (projection, back_reference) = {
            let reference = tlib::sym_ref(&mut arena, var);
            let mut b = SigBuilder::new(&mut arena);
            (b.proj(1, group), b.proj(1, reference))
        };
        let empty_types = HashMap::new();
        let context =
            SignalAnalysisContext::new(&arena, &empty_types, &[projection, back_reference])
                .unwrap();

        let projection_deps = signal_dependencies(&context, projection).unwrap();
        assert_eq!(
            dep_targets(projection_deps.scheduling()),
            vec![(second.as_u32(), DepKind::Immediate)]
        );
        assert_eq!(
            occurrence_targets(projection_deps.occurrences()),
            vec![(group.as_u32(), 0)]
        );
        assert_eq!(
            occurrence_targets(signal_dependencies(&context, group).unwrap().occurrences()),
            vec![(first.as_u32(), 0), (second.as_u32(), 0)]
        );
        assert_eq!(
            dep_targets(
                signal_dependencies(&context, back_reference)
                    .unwrap()
                    .scheduling()
            ),
            vec![(second.as_u32(), DepKind::Delayed { amount: 1 })]
        );
    }

    #[test]
    fn fir_rules_preserve_tap_delays_and_zero_coefficient_fallback() {
        let mut arena = TreeArena::new();
        let (input, zero, c1, c3, sparse, all_zero) = {
            let mut b = SigBuilder::new(&mut arena);
            let input = b.input(0);
            let zero = b.int(0);
            let c1 = b.real(0.5);
            let c3 = b.real(0.25);
            let sparse = b.fir(&[input, zero, c1, zero, c3]);
            let all_zero = b.fir(&[input, zero, zero]);
            (input, zero, c1, c3, sparse, all_zero)
        };
        let empty_types = HashMap::new();
        let context =
            SignalAnalysisContext::new(&arena, &empty_types, &[sparse, all_zero]).unwrap();

        let sparse_deps = signal_dependencies(&context, sparse).unwrap();
        assert_eq!(
            dep_targets(sparse_deps.scheduling()),
            vec![
                (zero.as_u32(), DepKind::Immediate),
                (c1.as_u32(), DepKind::Immediate),
                (zero.as_u32(), DepKind::Immediate),
                (c3.as_u32(), DepKind::Immediate),
                (input.as_u32(), DepKind::Delayed { amount: 1 }),
            ]
        );
        assert_eq!(
            occurrence_targets(sparse_deps.occurrences()),
            vec![
                (zero.as_u32(), 0),
                (c1.as_u32(), 0),
                (zero.as_u32(), 0),
                (c3.as_u32(), 0),
                (input.as_u32(), 1),
                (input.as_u32(), 3),
            ]
        );

        let all_zero_deps = signal_dependencies(&context, all_zero).unwrap();
        assert_eq!(
            occurrence_targets(all_zero_deps.occurrences()).last(),
            Some(&(input.as_u32(), 0))
        );
    }

    #[test]
    fn compact_iir_rules_ignore_state_as_a_child_and_mark_feedback_delays() {
        let mut arena = TreeArena::new();
        let (state, input, c0, c1, c2, iir) = {
            let mut b = SigBuilder::new(&mut arena);
            let state = b.input(0);
            let input = b.input(1);
            let c0 = b.int(0);
            let c1 = b.real(-0.5);
            let c2 = b.real(0.25);
            let iir = b.iir(&[state, input, c0, c1, c2]);
            (state, input, c0, c1, c2, iir)
        };
        let empty_types = HashMap::new();
        let context = SignalAnalysisContext::new(&arena, &empty_types, &[iir]).unwrap();
        let deps = signal_dependencies(&context, iir).unwrap();

        assert_eq!(
            dep_targets(deps.scheduling()),
            vec![
                (input.as_u32(), DepKind::Immediate),
                (c0.as_u32(), DepKind::Immediate),
                (c1.as_u32(), DepKind::Immediate),
                (c2.as_u32(), DepKind::Immediate),
            ]
        );
        assert_eq!(
            occurrence_targets(deps.occurrences()),
            vec![
                (input.as_u32(), 0),
                (c1.as_u32(), 0),
                (iir.as_u32(), 1),
                (c2.as_u32(), 0),
                (iir.as_u32(), 2),
            ]
        );
        assert!(!deps.occurrences().iter().any(|usage| usage.to == state));
    }

    #[test]
    fn table_and_clock_wrapper_rules_match_cpp_child_selection() {
        let mut arena = TreeArena::new();
        let (size, generator, write_index, write_value, read_index, table, read, wrapper) = {
            let mut b = SigBuilder::new(&mut arena);
            let size = b.int(128);
            let generator = b.input(0);
            let write_index = b.input(1);
            let write_value = b.input(2);
            let read_index = b.input(3);
            let clock = b.int(2);
            let payload = b.input(4);
            let table = b.wrtbl(size, generator, write_index, write_value);
            let read = b.rdtbl(table, read_index);
            let wrapper = b.on_demand(&[clock, payload]);
            (
                size,
                generator,
                write_index,
                write_value,
                read_index,
                table,
                read,
                wrapper,
            )
        };
        let empty_types = HashMap::new();
        let context = SignalAnalysisContext::new(&arena, &empty_types, &[read, wrapper]).unwrap();

        let table_deps = signal_dependencies(&context, table).unwrap();
        assert_eq!(table_deps.scheduling().len(), 4);
        assert_eq!(table_deps.scheduling()[0].to, size);
        assert_eq!(table_deps.scheduling()[1].to, generator);

        let read_deps = signal_dependencies(&context, read).unwrap();
        assert_eq!(
            dep_targets(read_deps.scheduling()),
            vec![
                (read_index.as_u32(), DepKind::Immediate),
                (write_index.as_u32(), DepKind::Immediate),
                (write_value.as_u32(), DepKind::Immediate),
            ]
        );
        assert_eq!(
            occurrence_targets(read_deps.occurrences()),
            vec![(table.as_u32(), 0), (read_index.as_u32(), 0)]
        );

        let wrapper_deps = signal_dependencies(&context, wrapper).unwrap();
        assert_eq!(wrapper_deps.scheduling()[0].kind, DepKind::ClockBoundary);
        assert_eq!(wrapper_deps.scheduling()[1].kind, DepKind::Immediate);
        assert_eq!(wrapper_deps.occurrences().len(), 2);
    }

    #[test]
    fn generators_are_analysis_leaves_and_malformed_iir_is_rejected() {
        let mut arena = TreeArena::new();
        let (generator, malformed_iir) = {
            let mut b = SigBuilder::new(&mut arena);
            let input = b.input(0);
            let generator = b.generate(input);
            let zero = b.int(0);
            let malformed_iir = b.iir(&[input, input, zero]);
            (generator, malformed_iir)
        };
        let empty_types = HashMap::new();
        let context =
            SignalAnalysisContext::new(&arena, &empty_types, &[generator, malformed_iir]).unwrap();

        let generator_deps = signal_dependencies(&context, generator).unwrap();
        assert!(generator_deps.scheduling().is_empty());
        assert!(generator_deps.occurrences().is_empty());
        assert!(matches!(
            signal_dependencies(&context, malformed_iir),
            Err(AnalysisError::Malformed { .. })
        ));
    }

    #[test]
    fn repeated_minus_one_product_propagates_sharing_to_its_rhs() {
        let mut arena = TreeArena::new();
        let (input, root) = {
            let mut b = SigBuilder::new(&mut arena);
            let input = b.input(0);
            let minus_one = b.int(-1);
            let negated = b.mul(minus_one, input);
            let root = b.fir(&[negated, negated]);
            (input, root)
        };
        let types = sigtype::TypeAnnotator::new(&arena, &ui::UiProgram::empty())
            .annotate(&[root])
            .unwrap();
        let context = SignalAnalysisContext::new(&arena, &types, &[root]).unwrap();
        let table = analyze_forest(
            &context,
            &[root],
            |_| Some(None),
            &ConstantExecutionConditions::default(),
        )
        .unwrap();

        assert_eq!(
            table.get(input).unwrap().occurrences.per_context[0].count,
            2
        );
        assert!(table.get(input).unwrap().occurrences.multi);
    }

    fn analyze(
        arena: &TreeArena,
        roots: &[SigId],
        conditions: &impl ExecutionConditions,
    ) -> (VerifiedPreparedSignals, SignalUseTable) {
        let prepared =
            prepare_signals_for_fir_verified(arena, roots, &ui::UiProgram::empty()).unwrap();
        let domains = ClockDomainTable::new();
        let clocks = annotate(prepared.arena(), &domains, prepared.outputs()).unwrap();
        let table = analyze_signal_uses(&prepared, &clocks, conditions).unwrap();
        (prepared, table)
    }

    #[test]
    fn table_is_deterministic_and_marks_duplicate_same_context() {
        let mut arena = TreeArena::new();
        let root = {
            let mut b = SigBuilder::new(&mut arena);
            let input = b.input(0);
            let shared = b.sin(input);
            b.fir(&[shared, shared])
        };
        let conditions = ConstantExecutionConditions::default();
        let (first_prepared, first) = analyze(&arena, &[root], &conditions);
        let (_, second) = analyze(&arena, &[root], &conditions);
        assert_eq!(first, second);
        let analysis = SignalAnalysisContext::new(
            first_prepared.arena(),
            first_prepared.sig_types_map(),
            first_prepared.outputs(),
        )
        .unwrap();
        let root_dependencies =
            signal_dependencies(&analysis, first_prepared.outputs()[0]).unwrap();
        assert_eq!(root_dependencies.occurrences().len(), 2);
        assert_eq!(
            root_dependencies.occurrences()[0].to,
            root_dependencies.occurrences()[1].to
        );
        let shared_info = first.get(root_dependencies.occurrences()[0].to).unwrap();
        assert_eq!(shared_info.occurrences.per_context[0].count, 2);
        assert!(shared_info.occurrences.multi);
    }

    #[test]
    fn faster_context_and_distinct_conditions_mark_multi() {
        let mut arena = TreeArena::new();
        let ty = arena.symbol("float");
        let name = arena.symbol("k");
        let file = arena.symbol("f");
        let root = {
            let mut b = SigBuilder::new(&mut arena);
            let constant = b.fconst(ty, name, file);
            let shared = b.input(0);
            let left = b.add(shared, constant);
            let right = b.mul(shared, constant);
            b.fir(&[left, right])
        };
        struct Branches;
        impl ExecutionConditions for Branches {
            fn signal_condition(&self, sig: SigId) -> CondId {
                CondId(u64::from(sig.as_u32()))
            }

            fn root_condition(&self, _root: SigId) -> CondId {
                CondId(0)
            }
        }
        let (_, table) = analyze(&arena, &[root], &Branches);
        assert!(
            table
                .records()
                .iter()
                .filter(|record| record.info.occurrences.multi)
                .flat_map(|record| &record.info.occurrences.per_context)
                .any(|occ| occ.context.variability > Variability::Konst),
            "a constant-rate node used by sample-rate code must be multi"
        );
        assert!(
            table.records().iter().any(|record| {
                let conditions = record
                    .info
                    .occurrences
                    .per_context
                    .iter()
                    .map(|occ| occ.context.condition)
                    .collect::<BTreeSet<_>>();
                conditions.len() > 1 && record.info.occurrences.multi
            }),
            "uses under distinct execution conditions must be multi"
        );

        let mut aggregate = table.records()[0].info.clone();
        aggregate.variability = Variability::Samp;
        aggregate.recursiveness = 0;
        aggregate.execution_condition = CondId(7);
        aggregate.occurrences = OccInfo {
            per_context: vec![
                ContextOccurrence {
                    context: UseContext {
                        variability: Variability::Block,
                        recursiveness: 1,
                        condition: CondId(7),
                    },
                    count: 1,
                },
                ContextOccurrence {
                    context: UseContext {
                        variability: Variability::Samp,
                        recursiveness: 0,
                        condition: CondId(7),
                    },
                    count: 1,
                },
            ],
            multi: false,
        };
        finalize_occurrences(&mut aggregate);
        assert!(
            aggregate.occurrences.multi,
            "C++ aggregates both contexts in extended-variability bucket 2"
        );
    }

    #[test]
    fn a_second_use_context_does_not_reexpand_shared_children() {
        let mut arena = TreeArena::new();
        let root = {
            let mut b = SigBuilder::new(&mut arena);
            let input = b.input(0);
            let shared = b.sin(input);
            let left = b.cos(shared);
            let right = b.exp(shared);
            b.fir(&[left, right])
        };
        struct ParentConditions;
        impl ExecutionConditions for ParentConditions {
            fn signal_condition(&self, sig: SigId) -> CondId {
                CondId(u64::from(sig.as_u32()))
            }

            fn root_condition(&self, _root: SigId) -> CondId {
                CondId(0)
            }
        }

        let (prepared, table) = analyze(&arena, &[root], &ParentConditions);
        let analysis = SignalAnalysisContext::new(
            prepared.arena(),
            prepared.sig_types_map(),
            prepared.outputs(),
        )
        .unwrap();
        let branches = signal_dependencies(&analysis, prepared.outputs()[0]).unwrap();
        assert_eq!(branches.occurrences().len(), 2);
        let left_dependencies =
            signal_dependencies(&analysis, branches.occurrences()[0].to).unwrap();
        let right_dependencies =
            signal_dependencies(&analysis, branches.occurrences()[1].to).unwrap();
        assert_eq!(left_dependencies.occurrences().len(), 1);
        assert_eq!(right_dependencies.occurrences().len(), 1);
        assert_eq!(
            left_dependencies.occurrences()[0].to,
            right_dependencies.occurrences()[0].to
        );
        let shared = left_dependencies.occurrences()[0].to;
        let shared_dependencies = signal_dependencies(&analysis, shared).unwrap();
        assert_eq!(shared_dependencies.occurrences().len(), 1);

        assert_eq!(table.get(shared).unwrap().occurrences.per_context.len(), 2);
        assert_eq!(
            table
                .get(shared_dependencies.occurrences()[0].to)
                .unwrap()
                .occurrences
                .per_context
                .len(),
            1
        );
    }

    #[test]
    fn delay_projection_and_very_simple_facts_are_recorded() {
        let mut arena = TreeArena::new();
        let (delayed, int, real, input, rec_input) = {
            let mut b = SigBuilder::new(&mut arena);
            let x = b.input(0);
            let amount = b.int(3);
            let delayed = b.delay(x, amount);
            let int = b.int(1);
            let real = b.real(1.0);
            let input = b.input(1);
            let rec_input = b.input(2);
            (delayed, int, real, input, rec_input)
        };
        let var = arena.symbol("r");
        let body = vec_to_list(&mut arena, &[rec_input]);
        let group = sym_rec(&mut arena, var, body);
        let projection = SigBuilder::new(&mut arena).proj(0, group);
        let fconst = {
            let ty = arena.symbol("float");
            let name = arena.symbol("k");
            let file = arena.symbol("f");
            SigBuilder::new(&mut arena).fconst(ty, name, file)
        };
        let (prepared, table) = analyze(
            &arena,
            &[delayed, projection, int, real, input, fconst],
            &ConstantExecutionConditions::default(),
        );
        let prepared_delayed = prepared.outputs()[0];
        let analysis = SignalAnalysisContext::new(
            prepared.arena(),
            prepared.sig_types_map(),
            prepared.outputs(),
        )
        .unwrap();
        let delayed_dependencies = signal_dependencies(&analysis, prepared_delayed).unwrap();
        let delayed_value = delayed_dependencies
            .scheduling()
            .iter()
            .find_map(|dependency| match dependency.kind {
                DepKind::Delayed { amount: 3 } => Some(dependency.to),
                _ => None,
            })
            .expect("prepared fixed delay has one delayed value dependency");
        let x_info = table.get(delayed_value).unwrap();
        assert_eq!(
            (x_info.max_delay, x_info.delay_reads, x_info.is_delay_read),
            (3, 1, false)
        );
        assert!(table.get(prepared_delayed).unwrap().is_delay_read);
        assert_eq!(
            table
                .get(prepared.outputs()[1])
                .unwrap()
                .recursive_projection
                .unwrap()
                .index,
            0
        );
        for &sig in &prepared.outputs()[2..] {
            assert!(table.get(sig).unwrap().very_simple);
        }
        assert!(!table.get(prepared_delayed).unwrap().very_simple);
    }

    #[test]
    fn missing_clock_type_and_invalid_delay_interval_are_typed_errors() {
        let mut arena = TreeArena::new();
        let (root, dynamic_delay, amount) = {
            let mut b = SigBuilder::new(&mut arena);
            let x = b.input(0);
            let root = b.sin(x);
            let minus_three = b.int(-3);
            let minus_one = b.int(-1);
            let amount_input = b.input(1);
            let amount = b.assert_bounds(minus_three, minus_one, amount_input);
            let dynamic_delay = b.delay(x, amount);
            (root, dynamic_delay, amount)
        };
        let conditions = ConstantExecutionConditions::default();
        let empty_types = HashMap::new();
        let analysis = SignalAnalysisContext::new(&arena, &empty_types, &[root]).unwrap();
        assert_eq!(
            analyze_forest(&analysis, &[root], |_| Some(None), &conditions),
            Err(AnalysisError::MissingType { sig: root })
        );
        let prepared =
            prepare_signals_for_fir_verified(&arena, &[root], &ui::UiProgram::empty()).unwrap();
        let prepared_root = prepared.outputs()[0];
        let analysis = SignalAnalysisContext::new(
            prepared.arena(),
            prepared.sig_types_map(),
            prepared.outputs(),
        )
        .unwrap();
        assert_eq!(
            analyze_forest(&analysis, &[prepared_root], |_| None, &conditions),
            Err(AnalysisError::MissingClock { sig: prepared_root })
        );

        let amount_types = sigtype::TypeAnnotator::new(&arena, &ui::UiProgram::empty())
            .annotate(&[amount])
            .unwrap();
        let analysis = SignalAnalysisContext::new(&arena, &amount_types, &[dynamic_delay]).unwrap();
        assert!(matches!(
            signal_dependencies(&analysis, dynamic_delay),
            Err(AnalysisError::InvalidDelayInterval { .. })
        ));
    }
}
