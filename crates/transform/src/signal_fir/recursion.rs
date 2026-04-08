//! Recursive-group carrier data model for the `signal_fir` fast-lane.
//!
//! This module owns the explicit recursion abstractions used by `module.rs`
//! during recursive-group lowering:
//!
//! - carrier storage strategy
//! - carrier metadata
//! - canonical resolved carrier references
//! - canonical resolved delayed recursion reads
//!
//! Group scheduling and final FIR orchestration still live in `module.rs`.

use std::collections::{HashMap, HashSet};

use fir::{AccessType, FirBuilder, FirId, FirStore, FirType};
use signals::{SigId, SigMatch, match_sig};
use tlib::{TreeArena, list_to_vec, match_sym_rec, match_sym_ref};

use super::delay::{DelayManager, pow2limit_for_delay};
use super::error::{SignalFirError, SignalFirErrorCode};

/// Storage strategy used by one recursion carrier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RecursionStorageStrategy {
    /// Two-slot carrier:
    /// - current sample in slot `0`
    /// - previous sample in slot `1`
    /// - post-output finalization copy `slot1 = slot0`
    TwoSlotShift,
    /// Circular carrier larger than 2 slots, indexed by the shared global
    /// circular cursor (`fIOTA`).
    Circular,
}

/// Carrier metadata for one output of a recursive group (`SIGPROJ(i, SYMREC(…))`).
///
/// Each output body in a multi-output recursion group gets its own array.
/// The carrier uses one of two storage strategies:
///
/// - [`RecursionStorageStrategy::TwoSlotShift`] for the default 2-slot case
/// - [`RecursionStorageStrategy::Circular`] when accumulated delay analysis
///   upsizes the carrier to serve delayed reads directly
///
/// Source provenance (C++): `signalFIRCompiler.cpp` (`generateRecProj`,
/// `generateRec`), emitted as `fRecN[2]` / `iRecN[2]`.
#[derive(Clone, Debug)]
pub(super) struct RecArrayInfo {
    /// Generated DSP-struct array variable name (e.g. `fRec7`).
    pub(super) name: String,
    /// Element type (`Int32` for integer recursion, `Float32`/`Float64` otherwise).
    pub(super) typ: FirType,
    /// Allocated circular-buffer size in elements (always a power of two).
    ///
    /// Defaults to 2 (current + previous sample). When the recursion output
    /// is consumed by delayed reads, the carrier may be upsized so those reads
    /// can be served directly from the recursion array instead of a separate
    /// delay line.
    pub(super) size: usize,
}

impl RecArrayInfo {
    pub(super) fn storage_strategy(&self) -> RecursionStorageStrategy {
        if self.size == 2 {
            RecursionStorageStrategy::TwoSlotShift
        } else {
            RecursionStorageStrategy::Circular
        }
    }
}

/// Canonically resolved recursion carrier.
#[derive(Clone, Debug)]
pub(super) struct RecursionCarrierRef {
    pub(super) info: RecArrayInfo,
    pub(super) strategy: RecursionStorageStrategy,
}

impl RecursionCarrierRef {
    pub(super) fn new(info: RecArrayInfo) -> Self {
        let strategy = info.storage_strategy();
        Self { info, strategy }
    }
}

/// Canonically resolved delayed recursion read.
#[derive(Clone, Debug)]
pub(super) struct RecursionDelayRef {
    pub(super) carrier: RecursionCarrierRef,
    pub(super) implicit_delay: usize,
}

/// Recursion lookup input recovered from a `Proj(...)` optionally wrapped in
/// `Delay1^k(...)`.
#[derive(Clone, Copy, Debug)]
pub(super) struct RecursionDelayKey {
    pub(super) proj_node: SigId,
    pub(super) proj_index: i32,
    pub(super) group: SigId,
    pub(super) implicit_delay: usize,
}

/// Lightweight active recursion-stack view used by canonical lookup helpers.
#[derive(Clone, Copy)]
pub(super) struct ActiveRecursionView<'a> {
    pub(super) recursion_stack: &'a [Vec<RecArrayInfo>],
    pub(super) recursion_vars: &'a [SigId],
}

/// Borrow bundle for recursive-group carrier allocation and zero-init
/// registration.
pub(super) struct RecursionAllocCtx<'a> {
    pub(super) arena: &'a TreeArena,
    pub(super) delay: &'a DelayManager,
    pub(super) store: &'a mut FirStore,
    pub(super) struct_declarations: &'a mut Vec<FirId>,
    pub(super) clear_statements: &'a mut Vec<FirId>,
    pub(super) clear_init_seen: &'a mut HashSet<String>,
    pub(super) next_loop_var_id: &'a mut usize,
    pub(super) rec_array_by_group_index: &'a mut HashMap<(u32, usize), RecArrayInfo>,
}

impl RecursionAllocCtx<'_> {
    fn fresh_loop_var(&mut self, prefix: &str) -> String {
        let name = format!("{prefix}{}", *self.next_loop_var_id);
        *self.next_loop_var_id += 1;
        name
    }

    fn register_clear_recursion_array(&mut self, name: String, init: FirId, size: usize) {
        if !self.clear_init_seen.insert(name.clone()) {
            return;
        }
        let loop_var = self.fresh_loop_var("lRec");
        let upper = {
            let mut b = FirBuilder::new(self.store);
            b.int32(i32::try_from(size).unwrap_or(i32::MAX))
        };
        let body = {
            let index = {
                let mut b = FirBuilder::new(self.store);
                b.load_var(loop_var.clone(), AccessType::Loop, FirType::Int32)
            };
            let store_node = {
                let mut b = FirBuilder::new(self.store);
                b.store_table(name, AccessType::Struct, index, init)
            };
            let mut b = FirBuilder::new(self.store);
            b.block(&[store_node])
        };
        let mut b = FirBuilder::new(self.store);
        self.clear_statements
            .push(b.simple_for_loop(loop_var, upper, body, false));
    }

    /// Declares a circular-buffer recursion array for output slot `index` of
    /// recursion `group`, idempotent.
    ///
    /// The buffer is sized to `pow2limit(max_delay + 1)` when the accumulated
    /// delay analysis recorded delayed reads on this group output, or to 2
    /// otherwise.
    pub(super) fn ensure_recursion_array_for_group(
        &mut self,
        group: SigId,
        index: usize,
        typ: FirType,
        init: FirId,
    ) -> Result<RecArrayInfo, SignalFirError> {
        let key = (group.as_u32(), index);
        if let Some(info) = self.rec_array_by_group_index.get(&key) {
            return Ok(info.clone());
        }
        let prefix = if typ == FirType::Int32 {
            "iRec"
        } else {
            "fRec"
        };
        let name = if index == 0 {
            format!("{prefix}{}", group.as_u32())
        } else {
            format!("{prefix}{}_{}", group.as_u32(), index)
        };
        let size = match match_sym_rec(self.arena, group) {
            Some((var, _body)) => match self.delay.rec_output_analysis(var.as_u32(), index) {
                Some(analysis) => pow2limit_for_delay(analysis.max_delay)?,
                None => 2,
            },
            None => 2,
        };
        let array_ty = FirType::Array(Box::new(typ.clone()), size);
        let mut b = FirBuilder::new(self.store);
        let decl = b.declare_var(name.clone(), array_ty, AccessType::Struct, None);
        self.struct_declarations.push(decl);
        self.register_clear_recursion_array(name.clone(), init, size);
        let info = RecArrayInfo { name, typ, size };
        self.rec_array_by_group_index.insert(key, info.clone());
        Ok(info)
    }
}

/// Decodes a `SYMREC(var, body_list)` group to all its payload body signals.
pub(super) fn decode_symbolic_group_bodies(
    arena: &TreeArena,
    group: SigId,
) -> Option<(SigId, Vec<SigId>)> {
    let (var, body_list) = match_sym_rec(arena, group)?;
    let bodies = list_to_vec(arena, body_list)?;
    Some((var, bodies))
}

/// Returns the canonical output index for one recursion projection.
pub(super) fn canonical_group_index(
    arena: &TreeArena,
    group: SigId,
    index: usize,
) -> Option<usize> {
    let (_var, bodies) = decode_symbolic_group_bodies(arena, group)?;
    Some(if bodies.len() == 1 { 0 } else { index })
}

/// Resolves a symbolic recursion group reference to its active carrier at a
/// given projection index.
pub(super) fn resolve_active_recursion_carrier(
    arena: &TreeArena,
    view: ActiveRecursionView<'_>,
    group: SigId,
    proj_index: usize,
) -> Result<Option<RecursionCarrierRef>, SignalFirError> {
    let Some(var) = match_sym_ref(arena, group) else {
        return Ok(None);
    };
    let depth = view
        .recursion_vars
        .iter()
        .rposition(|bound| *bound == var)
        .map(|slot| view.recursion_vars.len() - slot)
        .ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!("unbound symbolic recursion variable {}", var.as_u32()),
            )
        })?;
    let group_arrays = &view.recursion_stack[view.recursion_stack.len() - depth];
    let canonical_index = if group_arrays.len() == 1 {
        0
    } else {
        proj_index
    };
    group_arrays
        .get(canonical_index)
        .cloned()
        .ok_or_else(|| {
            SignalFirError::new(
                SignalFirErrorCode::UnsupportedSignalNode,
                format!(
                    "projection index {proj_index} out of bounds for recursion group with {} outputs",
                    group_arrays.len()
                ),
            )
        })
        .map(RecursionCarrierRef::new)
        .map(Some)
}

/// Matches `Proj(i, group)` optionally wrapped in `Delay1^k(...)`.
pub(super) fn match_recursion_delay_key(
    arena: &TreeArena,
    value: SigId,
) -> Option<RecursionDelayKey> {
    let mut current = value;
    let mut carried_delay = 0usize;
    while let SigMatch::Delay1(inner) = match_sig(arena, current) {
        carried_delay = carried_delay.saturating_add(1);
        current = inner;
    }
    let SigMatch::Proj(index, group) = match_sig(arena, current) else {
        return None;
    };
    Some(RecursionDelayKey {
        proj_node: current,
        proj_index: index,
        group,
        implicit_delay: carried_delay,
    })
}
