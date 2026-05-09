//! Signal-level helpers for `SigBlockReverseAD` FIR lowering (Phase B3).
//!
//! This module provides pure signal-tree utilities used by `module.rs` when
//! lowering `SigBlockReverseAD` carriers into FIR reverse-mode adjoint code.
//! All functions here are `pub(super)` and are called exclusively from
//! `module.rs`; the FIR-emitting logic itself lives there as `&mut self`
//! methods on `SignalToFirLower`.
//!
//! # Scope
//!
//! Phase B3 supports the TBPTT(BS, BS) backward sweep without a tape:
//! body signals must be **trivially reverse-evaluable** (i.e. their forward
//! value can be correctly reconstructed in the reverse sample loop without
//! storing a tape).  Non-trivially-evaluable signals (e.g. `Delay1(Delay1(x))`)
//! cause a `SignalFirError::UnsupportedSignalNode` at lowering time.  Full
//! tape support is planned for Phase B4.
//!
//! # Trivial reverse-evaluability
//!
//! A signal is *trivially reverse-evaluable* if it is either:
//! - a leaf (`Real`, `Int`, `Input`, `HSlider`, `VSlider`, `NumEntry`,
//!   `Button`, `Checkbox`), or
//! - a pure stateless combinator whose children are all trivially
//!   reverse-evaluable (`BinOp`, `IntCast`, `FloatCast`, unary math).
//!
//! `Delay1`, `Delay`, `Prefix`, and recursive carriers are **not** trivially
//! re-evaluable: their forward value at a given sample depends on prior state
//! that is not available in the reverse loop without a tape.

use std::collections::HashSet;

use signals::{SigId, SigMatch, match_sig};
use tlib::TreeArena;

/// Returns `true` if `sig` can be correctly re-evaluated in the **reverse**
/// sample loop without accessing a recorded tape.
#[allow(dead_code)]
///
/// A trivially reverse-evaluable signal is either a stateless leaf (constant,
/// audio input, UI control) or a pure stateless combinator of such nodes.
///
/// `Delay1`, `Delay`, `Prefix`, and recursive carriers return `false`:
/// their forward value depends on prior state that is absent in the reverse
/// loop unless a tape is recorded (Phase B4).
pub(super) fn is_trivially_reverse_evaluable(arena: &TreeArena, sig: SigId) -> bool {
    match match_sig(arena, sig) {
        SigMatch::Real(_) | SigMatch::Int(_) => true,
        SigMatch::Input(_) => true,
        SigMatch::HSlider(_) | SigMatch::VSlider(_) | SigMatch::NumEntry(_) => true,
        SigMatch::Button(_) | SigMatch::Checkbox(_) => true,
        SigMatch::IntCast(x) | SigMatch::FloatCast(x) | SigMatch::BitCast(x) => {
            is_trivially_reverse_evaluable(arena, x)
        }
        SigMatch::BinOp(_, lhs, rhs) => {
            is_trivially_reverse_evaluable(arena, lhs) && is_trivially_reverse_evaluable(arena, rhs)
        }
        SigMatch::Sin(x)
        | SigMatch::Cos(x)
        | SigMatch::Tan(x)
        | SigMatch::Asin(x)
        | SigMatch::Acos(x)
        | SigMatch::Atan(x)
        | SigMatch::Exp(x)
        | SigMatch::Log(x)
        | SigMatch::Log10(x)
        | SigMatch::Sqrt(x)
        | SigMatch::Abs(x)
        | SigMatch::Floor(x)
        | SigMatch::Ceil(x)
        | SigMatch::Rint(x)
        | SigMatch::Round(x) => is_trivially_reverse_evaluable(arena, x),
        _ => false,
    }
}

/// Performs a DFS postorder traversal of the signal sub-tree rooted at `root`.
///
/// Each signal ID is visited at most once; `visited` guards against re-entry
/// in DAG-shared patterns (e.g. `x * x` where `x` is a shared leaf).
///
/// The traversal is intentionally shallow at `Delay1` children: the child
/// signal `x` in `Delay1(x)` **is** included in the postorder because the
/// backward adjoint for `x` at the previous reverse step must be tracked, but
/// the traversal stops at any node kind not covered by the match below (an
/// unsupported kind will be caught with a proper error in
/// [`crate::signal_fir::module::SignalToFirLower::propagate_bra_adj`]).
pub(super) fn collect_bra_postorder(
    arena: &TreeArena,
    root: SigId,
    visited: &mut HashSet<SigId>,
    order: &mut Vec<SigId>,
) {
    if !visited.insert(root) {
        return;
    }
    match match_sig(arena, root) {
        // Leaves — no children.
        SigMatch::Real(_)
        | SigMatch::Int(_)
        | SigMatch::Input(_)
        | SigMatch::HSlider(_)
        | SigMatch::VSlider(_)
        | SigMatch::NumEntry(_)
        | SigMatch::Button(_)
        | SigMatch::Checkbox(_) => {}
        // Unary casts and unary math — one child.
        SigMatch::IntCast(x)
        | SigMatch::FloatCast(x)
        | SigMatch::BitCast(x)
        | SigMatch::Sin(x)
        | SigMatch::Cos(x)
        | SigMatch::Tan(x)
        | SigMatch::Asin(x)
        | SigMatch::Acos(x)
        | SigMatch::Atan(x)
        | SigMatch::Exp(x)
        | SigMatch::Log(x)
        | SigMatch::Log10(x)
        | SigMatch::Sqrt(x)
        | SigMatch::Abs(x)
        | SigMatch::Floor(x)
        | SigMatch::Ceil(x)
        | SigMatch::Rint(x)
        | SigMatch::Round(x) => {
            collect_bra_postorder(arena, x, visited, order);
        }
        // Binary operations — two children.
        SigMatch::BinOp(_, lhs, rhs) | SigMatch::Pow(lhs, rhs) | SigMatch::Atan2(lhs, rhs) => {
            collect_bra_postorder(arena, lhs, visited, order);
            collect_bra_postorder(arena, rhs, visited, order);
        }
        // Delay1: recurse into the value child so its adjoint can be tracked.
        SigMatch::Delay1(x) => {
            collect_bra_postorder(arena, x, visited, order);
        }
        // Any other kind: include in postorder; the backward pass will
        // return an unsupported-node error when it encounters it.
        _ => {}
    }
    order.push(root);
}
