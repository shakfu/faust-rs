//! Single-pass delay planner: builds a [`DelayPlan`] from the prepared signal
//! DAG without any FIR side-effects.
//!
//! Provides [`DelayPlan`], [`DelayAnalysisEntry`], [`plan_delays`], the
//! [`DelayPlanner`] visitor, and the `is_recursion_delay_chain_static` helper.

use std::collections::{HashMap, HashSet};

use signals::{SigId, SigMatch, match_sig};
use sigtype::SigType;
use tlib::{TreeArena, list_to_vec, match_sym_rec, match_sym_ref};

use super::options::DelayOptions;
use super::sizing::delay_size_for_amount;
use super::{SignalFirError, SignalFirErrorCode};

// ─── DelayAnalysisEntry ───────────────────────────────────────────────────────

/// Read-only delay-analysis metadata for one signal carrier.
///
/// This is the first Rust-side equivalent of the C++ occurrence/delay analysis:
/// it records the maximum accumulated delay observed on a signal and how many
/// delayed accesses reached that carrier during the scan.
///
/// The data is intentionally kept separate from FIR resource allocation so
/// future planning steps can consume it without side effects.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DelayAnalysisEntry {
    /// Largest accumulated delayed access observed on this carrier.
    pub(crate) max_delay: i32,
    /// Number of delayed accesses observed on this carrier.
    pub(crate) delay_count: u32,
}

// ─── DelayPlan ────────────────────────────────────────────────────────────────

/// The complete delay decision for one module, produced by a single DAG walk.
///
/// `DelayPlan` is a pure-data value with no FIR side-effects, collecting two maps:
///
/// - `lines` — the per-carrier maximum owned delay (the standalone delay lines
///   to allocate).
/// - `rec_outputs` — the recursion-output sizing metadata.
///
/// Produced by [`plan_delays`]; consumed by `prepare_delay_lines` and
/// `ensure_recursion_array_for_group`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DelayPlan {
    /// Standalone delay lines to allocate: carried signal → required max delay.
    pub(crate) lines: HashMap<SigId, i32>,
    /// Recursion-output sizing metadata: `(rec_var_id, proj_index)` → entry.
    ///
    /// Stored into `DelayManager::rec_output_analysis` by `prepare_delay_lines`.
    pub(crate) rec_outputs: HashMap<(u32, usize), DelayAnalysisEntry>,
}

// ─── plan_delays ──────────────────────────────────────────────────────────────

/// Unified single-pass delay planner: one traversal of the prepared signal DAG
/// producing both [`DelayPlan`] maps, with no FIR side-effects.
///
/// # Algorithm
///
/// An *accumulating* traversal tracks path-accumulated delay (memoised by
/// `best_seen_delay`, so a node is re-visited when reached with a strictly larger
/// accumulated delay) to fill `rec_outputs`.  On the FIRST visit to each node
/// (tracked by `scanned: HashSet<SigId>`), it also records the per-carrier maximum
/// owned delay into `lines`, under these guards:
///
/// - zero-delay nodes are skipped,
/// - `!is_recursion_delay_chain` guard for both `Delay` and `Delay1`,
/// - `max_copy_delay >= 1` gate for `Delay1`.
///
/// This is correct because per-carrier max-delay recording does not depend on
/// the accumulated delay — it only depends on the delay amount at the `Delay`
/// node itself and on whether the carried value is a recursion chain.
pub(crate) fn plan_delays(
    arena: &TreeArena,
    sig_types: &HashMap<SigId, SigType>,
    signals: &[SigId],
    options: &DelayOptions,
) -> Result<DelayPlan, SignalFirError> {
    DelayPlanner::new(arena, sig_types, options).run(signals)
}

/// Pure-function equivalent of `DelayManager::is_recursion_delay_chain` that
/// does not need `&self`.
fn is_recursion_delay_chain_static(arena: &TreeArena, value: SigId) -> bool {
    let mut current = value;
    while let SigMatch::Delay1(inner) = match_sig(arena, current) {
        current = inner;
    }
    let SigMatch::Proj(_, group) = match_sig(arena, current) else {
        return false;
    };
    match_sym_ref(arena, group).is_some() || match_sym_rec(arena, group).map(|_| ()).is_some()
}

// ─── DelayPlanner ─────────────────────────────────────────────────────────────

/// Single-pass visitor that builds a [`DelayPlan`] without threading 8
/// arguments through every recursive call.
///
/// The shared state (`arena`, `sig_types`, `options`, `plan`,
/// `best_seen_delay`, `scanned`) is held on the struct, so recursive calls
/// reduce to `self.node(sig, accum)` / `self.child(child)`.
struct DelayPlanner<'a> {
    arena: &'a TreeArena,
    sig_types: &'a HashMap<SigId, SigType>,
    options: &'a DelayOptions,
    plan: DelayPlan,
    best_seen_delay: HashMap<SigId, i32>,
    scanned: HashSet<SigId>,
}

impl<'a> DelayPlanner<'a> {
    fn new(
        arena: &'a TreeArena,
        sig_types: &'a HashMap<SigId, SigType>,
        options: &'a DelayOptions,
    ) -> Self {
        Self {
            arena,
            sig_types,
            options,
            plan: DelayPlan::default(),
            best_seen_delay: HashMap::new(),
            scanned: HashSet::new(),
        }
    }

    /// Entry point: walk every root signal and return the finished plan.
    fn run(mut self, signals: &[SigId]) -> Result<DelayPlan, SignalFirError> {
        for &sig in signals {
            self.node(sig, 0)?;
        }
        Ok(self.plan)
    }

    /// Core recursive visitor.
    ///
    /// Combines the accumulating logic of `analyze_node` (tracking
    /// `accumulated_delay` along paths through `Delay` / `Delay1` / `Prefix`)
    /// with the first-visit scan-recording logic of `scan_node`.
    fn node(&mut self, sig: SigId, accumulated_delay: i32) -> Result<(), SignalFirError> {
        // Accumulating-pass memoisation: skip if already visited with >= delay.
        if let Some(prev) = self.best_seen_delay.get(&sig)
            && *prev >= accumulated_delay
        {
            return Ok(());
        }
        self.best_seen_delay.insert(sig, accumulated_delay);

        // Accumulating pass: record rec-output analysis.
        if accumulated_delay > 0 {
            self.record_rec_output(sig, accumulated_delay);
        }

        // First-visit scan pass: record per-carrier max owned delay.
        if self.scanned.insert(sig) {
            self.scan_once(sig)?;
        }

        match match_sig(self.arena, sig) {
            SigMatch::Delay(value, amount) => {
                let Some(delay) = delay_size_for_amount(self.arena, self.sig_types, amount)? else {
                    return Err(SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "SIGDELAY requires a constant integer amount or a signal with a bounded non-negative interval",
                    ));
                };
                self.node(value, accumulated_delay.saturating_add(delay))?;
                self.node(amount, 0)?;
                return Ok(());
            }
            SigMatch::Delay1(value) => {
                self.node(value, accumulated_delay.saturating_add(1))?;
                return Ok(());
            }
            SigMatch::Prefix(init, value) => {
                self.node(value, accumulated_delay.saturating_add(1))?;
                self.node(init, 0)?;
                return Ok(());
            }
            SigMatch::Proj(_, group) => {
                if let Some((_var, body_list)) = match_sym_rec(self.arena, group) {
                    let bodies = list_to_vec(self.arena, body_list).ok_or_else(|| {
                        SignalFirError::new(
                            SignalFirErrorCode::UnsupportedSignalNode,
                            "malformed symbolic recursion body list during delay planning",
                        )
                    })?;
                    for body in bodies {
                        self.node(body, 0)?;
                    }
                    return Ok(());
                }
            }
            _ => {}
        }

        let node = self.arena.node(sig).ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("missing prepared signal node {}", sig.as_u32()),
            )
        })?;
        let children: Vec<SigId> = node.children.as_slice().to_vec();
        for child in children {
            self.child(child)?;
        }
        Ok(())
    }

    /// Walks a child node, handling list children the same way as `analyze_child`
    /// and `scan_child`.
    fn child(&mut self, child: SigId) -> Result<(), SignalFirError> {
        if self.arena.is_list(child) {
            let mut list = child;
            while !self.arena.is_nil(list) {
                let head = self.arena.hd(list).ok_or_else(|| {
                    SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "malformed prepared signal list during delay planning",
                    )
                })?;
                self.node(head, 0)?;
                list = self.arena.tl(list).ok_or_else(|| {
                    SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "malformed prepared signal list during delay planning",
                    )
                })?;
            }
            Ok(())
        } else {
            self.node(child, 0)
        }
    }

    /// Records per-carrier scan information on the first visit to `sig`.
    ///
    /// Mirrors the body of `scan_node`, but operates on `plan.lines` instead of
    /// a local `max_delays` map.
    fn scan_once(&mut self, sig: SigId) -> Result<(), SignalFirError> {
        if let SigMatch::Delay(value, amount) = match_sig(self.arena, sig) {
            match delay_size_for_amount(self.arena, self.sig_types, amount)? {
                Some(0) => {}
                Some(delay) => {
                    if !is_recursion_delay_chain_static(self.arena, value) {
                        let entry = self.plan.lines.entry(value).or_insert(0);
                        if delay > *entry {
                            *entry = delay;
                        }
                    }
                }
                None => {
                    return Err(SignalFirError::new(
                        SignalFirErrorCode::UnsupportedSignalNode,
                        "SIGDELAY requires a constant integer amount or a signal with a bounded non-negative interval",
                    ));
                }
            }
        }
        if let SigMatch::Delay1(value) = match_sig(self.arena, sig)
            && self.options.max_copy_delay >= 1
            && !is_recursion_delay_chain_static(self.arena, value)
        {
            let entry = self.plan.lines.entry(value).or_insert(0);
            if 1 > *entry {
                *entry = 1;
            }
        }
        Ok(())
    }

    /// Records recursion-output delay analysis for `sig` at `accumulated_delay`,
    /// mirroring `DelayManager::record_rec_output_delay_analysis`.
    fn record_rec_output(&mut self, sig: SigId, accumulated_delay: i32) {
        let SigMatch::Proj(index, group) = match_sig(self.arena, sig) else {
            return;
        };
        let rec_var = match match_sym_ref(self.arena, group) {
            Some(var) => Some(var),
            None => match_sym_rec(self.arena, group).map(|(var, _)| var),
        };
        let Some(var) = rec_var else {
            return;
        };
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let entry = self
            .plan
            .rec_outputs
            .entry((var.as_u32(), index))
            .or_default();
        entry.max_delay = entry.max_delay.max(accumulated_delay);
        entry.delay_count = entry.delay_count.saturating_add(1);
    }
}
