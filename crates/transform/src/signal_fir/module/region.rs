//! Compute-region tree for `compute()` assembly (roadmap P2).
//!
//! # Design note (P2.1)
//!
//! The FIR `compute()` body is assembled from a tree of **regions**. A region
//! is a lexical execution scope with its own statement lists; the full model
//! (from `porting/ondemand-vec-fad-interleave-synthesis-2026-07-07-en.md` §6
//! and the 2026-06-10 roadmap §5) is:
//!
//! - **scalar mode**: `SampleLoop ⊃ GuardedBlock(OD/US/DS, nested)` — one
//!   guarded region per clocked-wrapper instance, strictly nested like the
//!   clock domains themselves (roadmap P3);
//! - **vector mode**: `ChunkLoop ⊃ { LoopNode | Island ⊃ GuardedBlock }`
//!   (roadmap P6/P7).
//!
//! **The visibility rule** (single rule for CSE / occurrences / placement):
//! *a value computed in region `R` is reusable only in `R` and its
//! descendants; cross-region reuse goes through named storage* (stack locals,
//! struct fields, chunk buffers). While the tree holds exactly one region per
//! sample loop (the P2.2 state), per-bucket CSE (`cse.rs`) and
//! variability-driven placement (`placement.rs`) already respect the rule
//! trivially — bucket and region coincide. Guarded child regions (P3) extend
//! the rule by *narrowing* reuse, never widening it.
//!
//! **FIR-vocabulary decision** (P2.1 checklist): guarded blocks reuse the
//! existing generic `If` / `SimpleForLoop` / `Block` FIR statements (the
//! vector-doc §4 finding) rather than introducing dedicated block nodes;
//! this module therefore stays a pure *assembly-side* structure with no new
//! FIR node kinds.
//!
//! # Current state (P2.2)
//!
//! [`RegionTree`] is instantiated with exactly one [`RegionKind::SampleLoop`]
//! region, plus the reverse-time loop as a sibling region opened by
//! `reset_sample_loop_state`. The tree API is the **only** way lowering code
//! appends compute statements (the P2 exit criterion): the former flat
//! `SamplePhases` accumulator lives on as [`RegionPhases`], owned by the
//! current region.

use fir::FirId;

/// Kind of one compute region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RegionKind {
    /// The forward per-sample loop body.
    SampleLoop,
    /// The reverse-time per-sample loop body (public RAD outputs).
    ReverseSampleLoop,
}

/// Explicit execution phases inside one region iteration.
///
/// The region body is assembled in this fixed order:
///
/// 1. `immediate`: ordinary per-sample work and writes that must happen before
///    outputs are finalized
/// 2. `post_output`: updates that must observe the current sample's outputs
///    before shifting/finalizing state
/// 3. `sample_end`: generic subsystem maintenance such as delay counter bumps
#[derive(Default)]
pub(super) struct RegionPhases {
    pub(super) immediate: Vec<FirId>,
    pub(super) post_output: Vec<FirId>,
    pub(super) sample_end: Vec<FirId>,
}

impl RegionPhases {
    /// Concatenates the three lifecycle phases into a single statement list,
    /// preserving execution order: `immediate`, then `post_output`, then
    /// `sample_end`.
    pub(super) fn flattened(&self) -> Vec<FirId> {
        let mut all = Vec::with_capacity(
            self.immediate.len() + self.post_output.len() + self.sample_end.len(),
        );
        all.extend(self.immediate.iter().copied());
        all.extend(self.post_output.iter().copied());
        all.extend(self.sample_end.iter().copied());
        all
    }
}

/// One compute region: a lexical scope with its own phased statement lists.
///
/// Child regions (guarded OD/US/DS blocks) arrive with P3; until then a
/// region is a leaf.
struct Region {
    #[allow(dead_code, reason = "read once guarded blocks (P3) emit per-kind")]
    kind: RegionKind,
    phases: RegionPhases,
}

/// The compute-region tree, with a cursor designating the region lowering
/// code currently appends into.
///
/// Regions at the top level are the sibling per-loop slices (forward sample
/// loop, reverse-time loop) emitted in creation order by `build_module`.
pub(super) struct RegionTree {
    regions: Vec<Region>,
    current: usize,
}

impl RegionTree {
    /// Creates a tree holding one open region of the given kind.
    pub(super) fn new(kind: RegionKind) -> Self {
        Self {
            regions: vec![Region {
                kind,
                phases: RegionPhases::default(),
            }],
            current: 0,
        }
    }

    /// Phased statement lists of the current region — the single append
    /// surface for all lowering code.
    pub(super) fn current_phases_mut(&mut self) -> &mut RegionPhases {
        &mut self.regions[self.current].phases
    }

    /// Flattens the current region's phases into one ordered statement list.
    pub(super) fn current_flattened(&self) -> Vec<FirId> {
        self.regions[self.current].phases.flattened()
    }

    /// Closes the current region and opens a fresh sibling of `kind`,
    /// pointing the cursor at it.
    pub(super) fn begin_sibling(&mut self, kind: RegionKind) {
        self.regions.push(Region {
            kind,
            phases: RegionPhases::default(),
        });
        self.current = self.regions.len() - 1;
    }
}
