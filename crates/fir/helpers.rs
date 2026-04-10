//! Reusable FIR loop-construction helpers.
//!
//! # Source provenance (Rust port)
//! - Moved from `crates/transform/src/signal_fir/loops.rs`
//!
//! These helpers centralize low-level loop-shape emission that can be reused by
//! multiple lowering or transformation passes while preserving deterministic FIR
//! structure.

use crate::{AccessType, FirBinOp, FirBuilder, FirId, FirStore, FirType};

/// Generates a fresh loop-variable name using a shared monotonic counter.
#[must_use]
pub fn fresh_loop_var(next_loop_var_id: &mut usize, prefix: &str) -> String {
    let name = format!("{prefix}{}", *next_loop_var_id);
    *next_loop_var_id += 1;
    name
}

/// Emits a reverse `ForLoop` shift:
///
/// ```text
/// for (int j = delay; j > 0; j = j + -1)
///     array[j] = array[j - 1];
/// ```
///
/// This helper preserves the canonical FIR loop shape currently used by
/// `signal_fir` delay-line and recursion-carrier lowering.
pub fn emit_reverse_array_shift_loop(
    store: &mut FirStore,
    next_loop_var_id: &mut usize,
    loop_prefix: &str,
    array_name: &str,
    delay: i32,
    elem_ty: FirType,
    access_type: AccessType,
) -> FirId {
    let loop_var = fresh_loop_var(next_loop_var_id, loop_prefix);
    let init = {
        let delay_val = {
            let mut b = FirBuilder::new(store);
            b.int32(delay)
        };
        let mut b = FirBuilder::new(store);
        b.declare_var(
            loop_var.clone(),
            FirType::Int32,
            AccessType::Loop,
            Some(delay_val),
        )
    };
    let end = {
        let mut b = FirBuilder::new(store);
        b.int32(0)
    };
    let step = {
        let mut b = FirBuilder::new(store);
        b.int32(-1)
    };
    let body = {
        let dst_index = {
            let mut b = FirBuilder::new(store);
            b.load_var(loop_var.clone(), AccessType::Loop, FirType::Int32)
        };
        let src_index = {
            let j = {
                let mut b = FirBuilder::new(store);
                b.load_var(loop_var.clone(), AccessType::Loop, FirType::Int32)
            };
            let one = {
                let mut b = FirBuilder::new(store);
                b.int32(1)
            };
            let mut b = FirBuilder::new(store);
            b.binop(FirBinOp::Sub, j, one, FirType::Int32)
        };
        let src_value = {
            let mut b = FirBuilder::new(store);
            b.load_table(
                array_name.to_owned(),
                access_type,
                src_index,
                elem_ty.clone(),
            )
        };
        let shift_store = {
            let mut b = FirBuilder::new(store);
            b.store_table(array_name.to_owned(), access_type, dst_index, src_value)
        };
        let mut b = FirBuilder::new(store);
        b.block(&[shift_store])
    };
    let mut b = FirBuilder::new(store);
    b.for_loop(loop_var, init, end, step, body, true)
}
