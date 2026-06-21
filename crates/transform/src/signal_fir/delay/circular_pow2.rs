//! Power-of-two circular delay-line strategy.
//!
//! Every active delay line is backed by one DSP-struct array (`fVec*` or
//! `iVec*`) of size `S = next_power_of_two(max_delay + 1)`.  A shared
//! integer counter `fIOTA` advances by 1 each sample and serves as the write
//! pointer.  Reads use a masked offset: `array[(fIOTA - N) & (S - 1)]`.
//!
//! ```text
//! write: array[fIOTA & (S-1)]  = current_value;
//! read:  array[(fIOTA - N) & (S-1)]
//! end-of-sample: fIOTA = fIOTA + 1;
//! ```

use fir::{AccessType, FirBinOp, FirBuilder, FirId, FirStore, FirType};

use super::DelayFirCtx;

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

// ─── masked_delay_index ───────────────────────────────────────────────────────

/// Applies the power-of-two ring-buffer mask: `index & (size - 1)`.
pub(crate) fn masked_delay_index(store: &mut FirStore, index: FirId, size: usize) -> FirId {
    let mut e = DelayArith(store);
    let mask = e.i32c(i32::try_from(size.saturating_sub(1)).unwrap_or(i32::MAX));
    e.and_mask(index, mask)
}

// ─── GlobalCircularCursor ─────────────────────────────────────────────────────

/// Shared runtime cursor used by all global masked circular-storage paths.
///
/// Today this is materialized as the persistent struct field `fIOTA`. It is
/// shared by `CircularPow2` delay lines and by circular recursion carriers
/// lowered from `module.rs`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct GlobalCircularCursor;

impl GlobalCircularCursor {
    /// Declares and clears the shared `fIOTA` state, idempotent.
    pub(crate) fn ensure_state(self, ctx: &mut DelayFirCtx<'_>) {
        if *ctx.uses_iota {
            return;
        }
        *ctx.uses_iota = true;
        let zero = {
            let mut b = FirBuilder::new(ctx.store);
            b.int32(0)
        };
        let decl = {
            let mut b = FirBuilder::new(ctx.store);
            b.declare_var("fIOTA", FirType::Int32, AccessType::Struct, None)
        };
        ctx.struct_declarations.push(decl);
        if ctx.clear_init_seen.insert("fIOTA".to_owned()) {
            let mut b = FirBuilder::new(ctx.store);
            ctx.clear_statements
                .push(b.store_var("fIOTA", AccessType::Struct, zero));
        }
    }

    /// Loads the current cursor value from the DSP struct.
    pub(crate) fn load(self, store: &mut FirStore) -> FirId {
        let mut b = FirBuilder::new(store);
        b.load_var("fIOTA", AccessType::Struct, FirType::Int32)
    }

    /// Computes the masked current write index `fIOTA & (size - 1)`.
    pub(crate) fn current_index(self, store: &mut FirStore, size: usize) -> FirId {
        let iota = self.load(store);
        masked_delay_index(store, iota, size)
    }

    /// Computes the masked delayed read index `(fIOTA - amount) & (size - 1)`.
    pub(crate) fn delayed_index(self, store: &mut FirStore, amount: FirId, size: usize) -> FirId {
        let iota = self.load(store);
        let raw = {
            let mut b = FirBuilder::new(store);
            b.binop(FirBinOp::Sub, iota, amount, FirType::Int32)
        };
        masked_delay_index(store, raw, size)
    }

    /// Emits `fIOTA = fIOTA + 1` to advance the cursor by one sample.
    pub(crate) fn emit_advance(self, store: &mut FirStore) -> FirId {
        let next = {
            let iota = self.load(store);
            let one = {
                let mut b = FirBuilder::new(store);
                b.int32(1)
            };
            let mut b = FirBuilder::new(store);
            b.binop(FirBinOp::Add, iota, one, FirType::Int32)
        };
        let mut b = FirBuilder::new(store);
        b.store_var("fIOTA", AccessType::Struct, next)
    }
}
