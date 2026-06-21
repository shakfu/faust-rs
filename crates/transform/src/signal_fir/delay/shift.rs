//! Shift/copy delay-line strategy.
//!
//! Each sample all buffer elements are shifted one slot toward the high end,
//! and the new value is placed at index 0.  Read is a direct load at index
//! equal to the delay amount.  No `fIOTA` is used.
//!
//! ```text
//! buf[size-1] = buf[size-2]; ... buf[1] = buf[0];  (shift loop)
//! buf[0] = current_value;
//! read:  buf[N]
//! ```

use fir::helpers::emit_reverse_array_shift_loop;
use fir::{AccessType, FirBuilder, FirId, FirStore, FirType};

use super::{DelayLineInfo, DelayLoweringCtx};

// ─── buffer_size ─────────────────────────────────────────────────────────────

/// Minimum buffer size for the Shift strategy: `delay + 1` (exact).
pub(super) fn buffer_size(max_delay: i32) -> Result<usize, super::SignalFirError> {
    usize::try_from(max_delay).map(|d| d + 1).map_err(|_| {
        super::SignalFirError::new(
            super::SignalFirErrorCode::UnsupportedSignalNode,
            format!("SIGDELAY amount overflow: {max_delay}"),
        )
    })
}

// ─── Emission ─────────────────────────────────────────────────────────────────

/// Emits `buf[0] = new_value` — the immediate write for the Shift strategy.
pub(super) fn emit_store_at_zero(store: &mut FirStore, name: &str, new_value: FirId) -> FirId {
    let zero = {
        let mut b = FirBuilder::new(store);
        b.int32(0)
    };
    let mut b = FirBuilder::new(store);
    b.store_table(name, AccessType::Struct, zero, new_value)
}

/// Emits unrolled shift copies for a Shift delay line with `delay ≤ 2`.
///
/// Returns individual store instructions in high-to-low order:
/// - delay=1: `[buf[1] = buf[0]]`
/// - delay=2: `[buf[2] = buf[1], buf[1] = buf[0]]`
pub(super) fn emit_unrolled_shift_copies(
    store: &mut FirStore,
    name: &str,
    delay: i32,
    elem_ty: FirType,
) -> Vec<FirId> {
    let delay_usize = usize::try_from(delay).unwrap_or(0);
    let mut copies = Vec::with_capacity(delay_usize);
    for j in (1..=delay_usize).rev() {
        let j_idx = {
            let mut b = FirBuilder::new(store);
            b.int32(i32::try_from(j).unwrap_or(i32::MAX))
        };
        let j_minus_1_idx = {
            let mut b = FirBuilder::new(store);
            b.int32(i32::try_from(j - 1).unwrap_or(i32::MAX))
        };
        let loaded = {
            let mut b = FirBuilder::new(store);
            b.load_table(name, AccessType::Struct, j_minus_1_idx, elem_ty.clone())
        };
        let stored = {
            let mut b = FirBuilder::new(store);
            b.store_table(name, AccessType::Struct, j_idx, loaded)
        };
        copies.push(stored);
    }
    copies
}

/// Emits a reverse `ForLoop` shift for a Shift delay line with `delay ≥ 3`.
///
/// Generates:
/// ```text
/// for (int j = delay; j > 0; j = j + -1)
///     buf[j] = buf[j - 1];
/// ```
pub(super) fn emit_shift_loop(
    ctx: &mut DelayLoweringCtx<'_>,
    name: &str,
    delay: i32,
    elem_ty: FirType,
) -> FirId {
    emit_reverse_array_shift_loop(
        ctx.store,
        ctx.next_loop_var_id,
        "j",
        name,
        delay,
        elem_ty,
        AccessType::Struct,
    )
}

// ─── Strategy emit functions ─────────────────────────────────────────────────

/// Emits one `SIGDELAY(value, amount)` read/write sequence for the Shift strategy.
pub(super) fn emit_fixed_delay(
    ctx: &mut DelayLoweringCtx<'_>,
    line: &DelayLineInfo,
    current: FirId,
    amount: FirId,
    read_ty: FirType,
    schedule_write: bool,
) -> FirId {
    if schedule_write {
        let store_0 = emit_store_at_zero(ctx.store, &line.name, current);
        ctx.immediate_statements.push(store_0);
        let delay_n = i32::try_from(line.size).unwrap_or(i32::MAX) - 1;
        if delay_n <= 2 {
            let copies =
                emit_unrolled_shift_copies(ctx.store, &line.name, delay_n, read_ty.clone());
            ctx.post_output_statements.extend(copies);
        } else {
            let s = emit_shift_loop(ctx, &line.name, delay_n, read_ty.clone());
            ctx.post_output_statements.push(s);
        }
    }
    let mut b = FirBuilder::new(ctx.store);
    b.load_table(line.name.clone(), AccessType::Struct, amount, read_ty)
}

/// Emits one `Delay1(value)` read/write sequence for the Shift strategy.
///
/// Reads from index 1 (one sample behind index 0).
pub(super) fn emit_delay1(
    ctx: &mut DelayLoweringCtx<'_>,
    line: &DelayLineInfo,
    current: FirId,
    read_ty: FirType,
    schedule_write: bool,
) -> FirId {
    if schedule_write {
        let store_0 = emit_store_at_zero(ctx.store, &line.name, current);
        ctx.immediate_statements.push(store_0);
        let delay_n = i32::try_from(line.size).unwrap_or(i32::MAX) - 1;
        if delay_n <= 2 {
            let copies =
                emit_unrolled_shift_copies(ctx.store, &line.name, delay_n, read_ty.clone());
            ctx.post_output_statements.extend(copies);
        } else {
            let s = emit_shift_loop(ctx, &line.name, delay_n, read_ty.clone());
            ctx.post_output_statements.push(s);
        }
    }
    let one = {
        let mut b = FirBuilder::new(ctx.store);
        b.int32(1)
    };
    let mut b = FirBuilder::new(ctx.store);
    b.load_table(line.name.clone(), AccessType::Struct, one, read_ty)
}
