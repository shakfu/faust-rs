//! Shared arithmetic helper for delay-line index computations.
//!
//! [`DelayArith`] is used by both `circular_pow2` and `if_wrapping` strategies
//! to emit the simple integer FIR nodes that make up their index formulas.

use fir::{AccessType, FirBinOp, FirBuilder, FirId, FirStore, FirType};

// ─── DelayArith ──────────────────────────────────────────────────────────────

/// Thin arithmetic layer so index-formula functions read like the doc-comments.
///
/// Each method creates one FIR node via `FirBuilder` and returns its `FirId`.
/// The emitted nodes are byte-for-byte identical to what raw `FirBuilder` calls
/// produce — this is a legibility-only wrapper, not an optimization.
pub(super) struct DelayArith<'a>(pub(super) &'a mut FirStore);

impl<'a> DelayArith<'a> {
    pub(super) fn i32c(&mut self, v: i32) -> FirId {
        FirBuilder::new(self.0).int32(v)
    }
    pub(super) fn load_counter(&mut self, name: &str) -> FirId {
        FirBuilder::new(self.0).load_var(name, AccessType::Struct, FirType::Int32)
    }
    pub(super) fn add(&mut self, a: FirId, b: FirId) -> FirId {
        FirBuilder::new(self.0).binop(FirBinOp::Add, a, b, FirType::Int32)
    }
    pub(super) fn sub(&mut self, a: FirId, b: FirId) -> FirId {
        FirBuilder::new(self.0).binop(FirBinOp::Sub, a, b, FirType::Int32)
    }
    pub(super) fn ge(&mut self, a: FirId, b: FirId) -> FirId {
        FirBuilder::new(self.0).binop(FirBinOp::Ge, a, b, FirType::Int32)
    }
    pub(super) fn and_mask(&mut self, idx: FirId, mask: FirId) -> FirId {
        FirBuilder::new(self.0).binop(FirBinOp::And, idx, mask, FirType::Int32)
    }
    pub(super) fn select2(&mut self, cond: FirId, then_: FirId, else_: FirId) -> FirId {
        FirBuilder::new(self.0).select2(cond, then_, else_, FirType::Int32)
    }
    pub(super) fn store_counter(&mut self, name: &str, val: FirId) -> FirId {
        FirBuilder::new(self.0).store_var(name, AccessType::Struct, val)
    }
}
