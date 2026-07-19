//! Execution-condition DNF vocabulary and construction (C++
//! `conditionAnnotation` counterpart).

use super::AnalysisError;
use super::dependencies::*;
use crate::signal_prepare::VerifiedPreparedSignals;
use signals::{SigId, SigMatch, match_sig};
use std::collections::{BTreeMap, VecDeque};

/// Stable identity of an execution condition supplied by an analysis client.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CondId(pub u64);
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
