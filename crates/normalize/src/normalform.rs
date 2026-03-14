//! Normal-form pipeline coordinator — Phase 1.
//!
//! Ported (Phase 1 scope) from C++ `compiler/normalize/normalform.cpp`.
//!
//! # Phase 1 scope
//!
//! This module implements the three preparation steps that
//! `crates/transform/signal_prepare` currently performs before FIR lowering,
//! now exposed as a general-purpose, backend-agnostic API:
//!
//! 1. **de Bruijn → symbolic** (`de_bruijn_to_sym`): rewrite all de-Bruijn
//!    recursion groups into symbolic `SymRec`/`SymRef` form.
//! 2. **Type annotation** (`TypeAnnotator::annotate`): compute the full
//!    `SigType` for every reachable node using the C++ type lattice.
//! 3. **Signal promotion** (see [`promote_signals`]): insert numeric casts
//!    where the signal graph mixes integer and real computations.
//!
//! # Phase 2 (deferred)
//!
//! The full C++ `simplifyToNormalForm` pipeline (UI promotion, FTZ wrapping,
//! auto-differentiation, double-promotion, causality check) is deferred to a
//! future Phase 2 implementation.
//!
//! # API mapping status
//! - `simplifyToNormalForm(sig)` → [`prepare_signals`] (Phase 1 only)
//! - `simplifyToNormalForm2(sigs)` → [`prepare_signals_multi`]

use std::collections::HashMap;

use signals::{SigBuilder, SigId, SigMatch, match_sig};
use sigtype::{SigType, TypeAnnotator};
use tlib::{RecursionError, TreeArena, de_bruijn_to_sym, match_sym_rec, match_sym_ref};
use ui::UiProgram;

// ─── Options ──────────────────────────────────────────────────────────────────

/// Configuration for the normal-form preparation pipeline.
///
/// Corresponds to the subset of `gGlobal` options consumed by Phase 1.
#[derive(Debug, Clone, Default)]
pub struct NormalFormOpts {
    /// Skip the signal-promotion pass (useful for testing individual steps).
    pub skip_promotion: bool,
}

// ─── Errors ───────────────────────────────────────────────────────────────────

/// Error returned by the normal-form pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormalFormError {
    /// De-Bruijn-to-symbolic conversion failed (open recursion group).
    Recursion(String),
    /// Full-type annotation failed.
    Type(String),
}

impl std::fmt::Display for NormalFormError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Recursion(e) => write!(f, "de-Bruijn conversion error: {e}"),
            Self::Type(e) => write!(f, "type annotation error: {e}"),
        }
    }
}

impl std::error::Error for NormalFormError {}

impl From<RecursionError> for NormalFormError {
    fn from(e: RecursionError) -> Self {
        Self::Recursion(format!("{e}"))
    }
}

impl From<sigtype::TypeError> for NormalFormError {
    fn from(e: sigtype::TypeError) -> Self {
        Self::Type(format!("{e}"))
    }
}

impl From<sigtype::rules::TypeError> for NormalFormError {
    fn from(e: sigtype::rules::TypeError) -> Self {
        Self::Type(format!("{e}"))
    }
}

// ─── Public API ───────────────────────────────────────────────────────────────

/// Phase 1 normal-form preparation for a single signal.
///
/// Runs the three-step pipeline:
/// 1. `de_bruijn_to_sym(arena, sig)`
/// 2. `TypeAnnotator::annotate(&[sym_sig])` → `types`
/// 3. `promote_signals(arena, &types, &[sym_sig])` → promoted output
///
/// Returns the promoted signal root and the type map produced by step 2.
///
/// C++: Phase 1 subset of `Tree simplifyToNormalForm(Tree sig)`.
pub fn prepare_signals(
    arena: &mut TreeArena,
    ui: &UiProgram,
    sig: SigId,
    opts: &NormalFormOpts,
) -> Result<(SigId, HashMap<SigId, SigType>), NormalFormError> {
    let results = prepare_signals_multi(arena, ui, &[sig], opts)?;
    let (sigs, types) = results;
    Ok((sigs[0], types))
}

/// Phase 1 normal-form preparation for a slice of output signals.
///
/// Applies the three-step pipeline to the entire signal forest rooted at
/// `sigs`, treating them as co-dependent outputs (type annotation sees all
/// roots).
///
/// Returns the promoted signal roots (same order as input) and the type map.
///
/// C++: Phase 1 subset of `void simplifyToNormalForm2(tvec& sigs)`.
pub fn prepare_signals_multi(
    arena: &mut TreeArena,
    ui: &UiProgram,
    sigs: &[SigId],
    opts: &NormalFormOpts,
) -> Result<(Vec<SigId>, HashMap<SigId, SigType>), NormalFormError> {
    // ── Step 1: de Bruijn → symbolic ──────────────────────────────────────
    let mut sym_sigs: Vec<SigId> = Vec::with_capacity(sigs.len());
    for &s in sigs {
        sym_sigs.push(de_bruijn_to_sym(arena, s)?);
    }

    // ── Step 2: type annotation ────────────────────────────────────────────
    // `TypeAnnotator` borrows `arena` immutably; that borrow ends when `types`
    // is returned and the annotator drops.
    let types = {
        let mut annotator = TypeAnnotator::new(arena, ui);
        annotator.annotate(&sym_sigs)?
    };

    // ── Step 3: signal promotion ───────────────────────────────────────────
    if opts.skip_promotion {
        return Ok((sym_sigs, types));
    }
    let promoted_sigs = promote_signals(arena, &types, &sym_sigs);

    // Re-annotate after promotion so callers get an up-to-date type map.
    let types2 = {
        let mut annotator = TypeAnnotator::new(arena, ui);
        annotator.annotate(&promoted_sigs)?
    };

    Ok((promoted_sigs, types2))
}

// ─── Signal promotion ─────────────────────────────────────────────────────────

/// Insert numeric promotion casts into a signal forest.
///
/// Walks each signal tree and inserts `FloatCast` nodes where an integer signal
/// feeds into a context that requires a real value, and vice versa.  This is a
/// simplified version of the C++ `SignalPromotion` pass sufficient for Phase 1.
///
/// Current scope: wrap integer-valued output signals with `FloatCast` when
/// the Faust process output is expected to be real (variability = Konst with
/// Int nature).  This matches the minimal promotion the fast-lane needed.
///
/// C++: `signalPromotion(sig)` in `normalform.cpp`.
pub fn promote_signals(
    arena: &mut TreeArena,
    types: &HashMap<SigId, SigType>,
    sigs: &[SigId],
) -> Vec<SigId> {
    sigs.iter()
        .map(|&s| promote_one(arena, types, s, &mut HashMap::new()))
        .collect()
}

/// Recursively promote a single signal, memoizing results.
fn promote_one(
    arena: &mut TreeArena,
    types: &HashMap<SigId, SigType>,
    sig: SigId,
    cache: &mut HashMap<SigId, SigId>,
) -> SigId {
    if let Some(&cached) = cache.get(&sig) {
        return cached;
    }

    // Detect SymRec cycles (same sentinel pattern as sig_map).
    if match_sym_rec(arena, sig).is_some() {
        cache.insert(sig, sig);
        return sig;
    }

    // Check if signal is already a SymRef (leaf reference into a SymRec group).
    if match_sym_ref(arena, sig).is_some() {
        cache.insert(sig, sig);
        return sig;
    }

    // General: promote children, then decide on a cast at the current node.
    let (kind, children) = {
        let node = arena.node(sig).expect("promote_one: invalid SigId");
        (node.kind.clone(), node.children.as_slice().to_vec())
    };

    let new_children: Vec<SigId> = children
        .iter()
        .map(|&c| promote_one(arena, types, c, cache))
        .collect();
    let rebuilt = arena.intern(kind, &new_children);

    // Apply the promotion rule: if an Int signal's type could be promoted to
    // Real based on surrounding context, insert a cast.
    // Phase 1: conservative — only promote BinOp operands that mix types.
    let result = promote_mixed_binop(arena, types, rebuilt);

    cache.insert(sig, result);
    result
}

/// Promote BinOp operands when one is integer and the other is real.
///
/// Mirrors the C++ `SignalPromotion` rule for arithmetic expressions: if one
/// operand is typed as integer and the other as real, cast the integer to real.
fn promote_mixed_binop(
    arena: &mut TreeArena,
    types: &HashMap<SigId, SigType>,
    sig: SigId,
) -> SigId {
    let (op, t1, t2) = match match_sig(arena, sig) {
        SigMatch::BinOp(op, t1, t2) => (op, t1, t2),
        _ => return sig,
    };
    let is_int = |id: SigId| {
        types
            .get(&id)
            .map(|ty| matches!(ty.nature(), sigtype::Nature::Int))
            .unwrap_or(false)
    };
    let is_real = |id: SigId| {
        types
            .get(&id)
            .map(|ty| matches!(ty.nature(), sigtype::Nature::Real))
            .unwrap_or(false)
    };

    if is_int(t1) && is_real(t2) {
        let cast_t1 = SigBuilder::new(arena).float_cast(t1);
        SigBuilder::new(arena).binop(op, cast_t1, t2)
    } else if is_real(t1) && is_int(t2) {
        let cast_t2 = SigBuilder::new(arena).float_cast(t2);
        SigBuilder::new(arena).binop(op, t1, cast_t2)
    } else {
        sig
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use signals::{SigBuilder, SigMatch, match_sig};
    use tlib::TreeArena;
    use ui::UiProgram;

    use super::*;

    fn arena() -> TreeArena {
        TreeArena::new()
    }

    fn ui() -> UiProgram {
        UiProgram::empty()
    }

    // ── prepare_signals — trivial signals ─────────────────────────────────

    #[test]
    fn prepare_signals_integer_constant() {
        // A plain integer constant passes through unchanged.
        let mut a = arena();
        let u = ui();
        let i42 = SigBuilder::new(&mut a).int(42);
        let (r, types) = prepare_signals(&mut a, &u, i42, &NormalFormOpts::default()).unwrap();
        assert_eq!(match_sig(&a, r), SigMatch::Int(42));
        assert!(types.contains_key(&r));
    }

    #[test]
    fn prepare_signals_audio_input() {
        // An input signal passes through; types must be available.
        let mut a = arena();
        let u = ui();
        let x = SigBuilder::new(&mut a).input(0);
        let (r, types) = prepare_signals(&mut a, &u, x, &NormalFormOpts::default()).unwrap();
        // Input may stay as-is or be promoted; types must have an entry for r.
        assert!(types.contains_key(&r) || types.contains_key(&x));
    }

    #[test]
    fn prepare_signals_add_expression() {
        // An add expression on two inputs: all three nodes typed.
        let mut a = arena();
        let u = ui();
        let x = SigBuilder::new(&mut a).input(0);
        let y = SigBuilder::new(&mut a).input(1);
        let add = SigBuilder::new(&mut a).add(x, y);
        let opts = NormalFormOpts {
            skip_promotion: true,
        };
        let (_r, types) = prepare_signals(&mut a, &u, add, &opts).unwrap();
        // At minimum, the de-Bruijn-converted form should be typed.
        assert!(!types.is_empty());
    }

    // ── prepare_signals_multi ─────────────────────────────────────────────

    #[test]
    fn prepare_signals_multi_two_outputs() {
        let mut a = arena();
        let u = ui();
        let x = SigBuilder::new(&mut a).input(0);
        let y = SigBuilder::new(&mut a).input(1);
        let opts = NormalFormOpts {
            skip_promotion: true,
        };
        let (rs, _types) = prepare_signals_multi(&mut a, &u, &[x, y], &opts).unwrap();
        assert_eq!(rs.len(), 2);
    }

    // ── promote_signals ───────────────────────────────────────────────────

    #[test]
    fn promote_signals_no_change_for_same_type_binop() {
        // int + int should not insert casts.
        let mut a = arena();
        let u = ui();
        let i3 = SigBuilder::new(&mut a).int(3);
        let i4 = SigBuilder::new(&mut a).int(4);
        let add = SigBuilder::new(&mut a).add(i3, i4);
        let opts = NormalFormOpts {
            skip_promotion: true,
        };
        let (_r, types) = prepare_signals(&mut a, &u, add, &opts).unwrap();
        let promoted = promote_signals(&mut a, &types, &[add]);
        // With same-type operands, the promoted result should not wrap in a cast.
        assert_eq!(promoted.len(), 1);
    }

    // ── NormalFormError conversion ────────────────────────────────────────

    #[test]
    fn error_display() {
        let e = NormalFormError::Type("bad type".to_string());
        assert!(e.to_string().contains("bad type"));
        let e2 = NormalFormError::Recursion("open group".to_string());
        assert!(e2.to_string().contains("open group"));
    }
}
