//! Signal-forest preparation before fast-lane FIR lowering.
//!
//! # Source provenance (C++)
//! - `compiler/normalize/normalform.cpp` (`deBruijn2Sym(...)`, `typeAnnotation(...)`)
//! - `compiler/box_signal_api.cpp` (`boxesToSignalsMLIR(...)`)
//! - `compiler/signals/sigtyperules.cpp` (reduced type inference subset)
//!
//! # Stage scope
//! This module now implements the first two preparation slices:
//! - clone the output forest into a private staging arena,
//! - run forest-wide `de_bruijn_to_sym`,
//! - infer one reduced `Int / Real / Sound` type for the prepared signals,
//! - insert the reduced `SignalPromotion` cast subset needed by the fast-lane,
//! - simplify the promoted forest,
//! - canonicalize one-sample delays back to `Delay1`,
//! - and re-type the final forest.
//!
//! Reduced typing deliberately stops short of the full C++ type lattice. The
//! goal is only to support the upcoming promotion pass and to feed `signal_fir`
//! with enough information for delay/recursion/table lowering.
//!
//! # Boundary contract
//! Input signals may still contain:
//! - de Bruijn recursion groups,
//! - mixed integer/real numeric expressions,
//! - table and clock-family nodes emitted by propagation.
//!
//! Output signals returned by [`prepare_signals_for_fir`] satisfy these
//! fast-lane invariants:
//! - recursion is rewritten to symbolic `SYMREC` / `SYMREF`,
//! - one reduced prepared type is available for each reachable node,
//! - casts needed by the current FIR lowerer have already been inserted,
//! - the original source arena is left untouched.
//!
//! The module also exposes an explicit verification boundary (implemented in
//! the `verify` submodule, `signal_prepare/verify.rs`):
//! - [`PreparedSignals::verify`] checks those postconditions on an already built
//!   staging forest,
//! - [`prepare_signals_for_fir_verified`] returns a wrapper that certifies the
//!   prepared forest passed the boundary checks before reaching FIR lowering.
//!
//! The two structural canonicalization rewrites (`canonicalize_unary_rec_projections`
//! and `canonicalize_one_sample_delays`) are implemented in the `rewrites` submodule
//! (`signal_prepare/rewrites.rs`). See that module for their specifications and
//! explicit C++ parity limitations.
//!
//! # Adaptation status
//! This is an adapted Rust staging phase rather than a 1:1 copy of one single
//! C++ class:
//! - `deBruijn2Sym(...)` is applied forest-wide like the C++ normalization path,
//! - reduced typing keeps only the distinctions currently needed by the
//!   fast-lane instead of the full C++ signal type lattice,
//! - the promotion pass ports only the `SignalPromotion` subset required before
//!   `signal_fir`,
//! - algebraic simplification then runs on the promoted forest before the
//!   final type snapshot is exposed to FIR lowering,
//! - `Delay(x, 1)` is then canonicalized back to `Delay1(x)` so downstream
//!   FIR consumers see one stable unary-delay form.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use normalize::normalform::{NormalFormError, promote_signals_fastlane, simplify_signals_fastlane};
use signals::{SigId, SigMatch, match_sig};
use sigtype::{Nature, SigType, TypeAnnotator};
use tlib::{RecursionError, TreeArena, list_to_vec, vec_to_list};
use ui::UiProgram;

mod rewrites;
mod verify;
// Re-export the promotion-invariant checker so `tests.rs` can call it as
// `super::verify_promotion_invariant(...)` without knowing it lives in `verify`.
#[cfg(test)]
use verify::verify_promotion_invariant;

/// Reduced signal type domain used by the FIR-preparation pass.
///
/// Intentionally smaller than the C++ `sigtyperules` lattice: keeps only the
/// distinctions required by the `SignalPromotion` subset and FIR type selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SimpleSigType {
    /// Integer-valued signal.
    Int,
    /// Real-valued signal.
    Real,
    /// Soundfile handle payload.
    Sound,
}

/// Result of preparing a propagated signal list for FIR lowering.
///
/// Owns a private staging arena and prepared output roots. Callers observe that
/// staged forest through read-only accessors so the preparation boundary stays
/// encapsulated and can be explicitly re-verified when needed.
#[derive(Debug)]
pub struct PreparedSignals {
    /// Private staging arena containing the prepared signal forest.
    arena: TreeArena,
    /// Prepared output roots interned in [`Self::arena`].
    outputs: Vec<SigId>,
    /// Reduced type annotation for prepared signal nodes (for promoter + FIR lowerer).
    types: HashMap<SigId, SimpleSigType>,
    /// Full signal type annotation from the `sigtype` type system.
    /// Carries interval bounds, variability, and all other lattice qualifiers.
    sig_types: HashMap<SigId, SigType>,
}

/// Prepared-signal forest that passed the explicit postcondition verifier.
///
/// This wrapper exists so downstream code can request a stronger boundary than
/// "the constructor should have produced something valid": the checked state is
/// represented in the type system and can be threaded through lowering code
/// without re-introducing ad hoc assumptions.
#[derive(Debug)]
pub struct VerifiedPreparedSignals {
    inner: PreparedSignals,
}

impl PreparedSignals {
    /// Returns the prepared staging arena.
    #[must_use]
    pub fn arena(&self) -> &TreeArena {
        &self.arena
    }

    /// Returns the prepared output roots.
    #[must_use]
    pub fn outputs(&self) -> &[SigId] {
        &self.outputs
    }

    /// Returns the reduced prepared type for one signal node, when available.
    #[must_use]
    pub fn ty(&self, sig: SigId) -> Option<SimpleSigType> {
        self.types.get(&sig).copied()
    }

    /// Returns the full `SigType` (with interval) for one signal node.
    #[must_use]
    pub fn sig_ty(&self, sig: SigId) -> Option<&SigType> {
        self.sig_types.get(&sig)
    }

    /// Read-only view of the reduced type map for consumers that need to
    /// iterate or pass it wholesale (e.g. FIR builders).  Prefer [`Self::ty`]
    /// for single-node lookups.
    #[must_use]
    pub fn types_map(&self) -> &HashMap<SigId, SimpleSigType> {
        &self.types
    }

    /// Read-only view of the full sig-type map for consumers that need to
    /// iterate or pass it wholesale.  Prefer [`Self::sig_ty`] for
    /// single-node lookups.  Use this map (rather than `types_map`) when
    /// interval bounds, variability, or other lattice qualifiers are needed —
    /// it is a strict superset of the reduced type map.
    #[must_use]
    pub fn sig_types_map(&self) -> &HashMap<SigId, SigType> {
        &self.sig_types
    }
}

impl VerifiedPreparedSignals {
    /// Returns the verified staging arena.
    #[must_use]
    pub fn arena(&self) -> &TreeArena {
        &self.inner.arena
    }

    /// Returns the verified output roots.
    #[must_use]
    pub fn outputs(&self) -> &[SigId] {
        &self.inner.outputs
    }

    /// Returns the reduced prepared type for one signal node, when available.
    #[must_use]
    pub fn ty(&self, sig: SigId) -> Option<SimpleSigType> {
        self.inner.ty(sig)
    }

    /// Returns the full `SigType` for one signal node.
    #[must_use]
    pub fn sig_ty(&self, sig: SigId) -> Option<&SigType> {
        self.inner.sig_ty(sig)
    }

    /// Read-only view of the reduced type map.
    #[must_use]
    pub fn types_map(&self) -> &HashMap<SigId, SimpleSigType> {
        self.inner.types_map()
    }

    /// Read-only view of the full type map.
    #[must_use]
    pub fn sig_types_map(&self) -> &HashMap<SigId, SigType> {
        self.inner.sig_types_map()
    }

    /// Releases the verified wrapper and returns the inner prepared forest.
    #[must_use]
    pub fn into_inner(self) -> PreparedSignals {
        self.inner
    }
}

/// Typed errors returned while preparing signals for FIR lowering.
#[derive(Debug)]
pub enum SignalPrepareError {
    /// The output forest contains malformed or open de Bruijn recursion.
    Recursion(RecursionError),
    /// Structural type-inference or validation error (e.g. malformed recursion body).
    Typing(String),
    /// Explicit prepared-forest contract validation failed.
    Validation(String),
    /// The signal promotion pass failed (type-driven cast insertion).
    Promotion(NormalFormError),
}

impl fmt::Display for SignalPrepareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recursion(err) => write!(
                f,
                "signal preparation failed during de_bruijn_to_sym: {err}"
            ),
            Self::Typing(msg) => write!(f, "signal preparation typing failed: {msg}"),
            Self::Validation(msg) => {
                write!(f, "signal preparation postcondition failed: {msg}")
            }
            Self::Promotion(err) => write!(f, "signal preparation promotion failed: {err}"),
        }
    }
}

impl Error for SignalPrepareError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Recursion(e) => Some(e),
            Self::Promotion(e) => Some(e),
            Self::Typing(_) | Self::Validation(_) => None,
        }
    }
}

impl From<RecursionError> for SignalPrepareError {
    fn from(value: RecursionError) -> Self {
        Self::Recursion(value)
    }
}

impl From<NormalFormError> for SignalPrepareError {
    fn from(value: NormalFormError) -> Self {
        Self::Promotion(value)
    }
}

/// Clones one output forest into a private arena, converts de Bruijn recursion
/// to symbolic recursion with forest-wide sharing preserved, then infers one
/// reduced type per prepared signal node, applies the reduced promotion pass,
/// simplifies the promoted forest, then re-runs typing and promotion so the
/// final forest still satisfies the FIR lowerer's explicit cast invariants.
///
/// C++ parity note: both `deBruijn2Sym(...)` and the later type/promotion flow
/// conceptually operate on the whole output list, not independently per root.
/// This function mirrors that contract by cloning all outputs through one memo
/// table, converting one list root, and then typing the prepared forest.
///
/// Type-source note: recursive and arithmetic typing now come from the
/// canonical `sigtype::TypeAnnotator` / `normalize` path. `transform` no longer
/// runs a second local recursive typer. The remaining `SimpleSigType` map is a
/// reduced view derived from canonical `SigType` results for FIR type
/// selection.
///
/// Additional fast-lane note: after `de_bruijn_to_sym`, the staging forest is
/// normalized so degenerate symbolic recursion groups use a canonical physical
/// projection index (`0`). This mirrors the intent of the classic C++ pipeline,
/// where degenerate recursive projections are later normalized through
/// projection-definition rewriting (`inlineDegenerateRecursions(...)`), but it
/// does so earlier so the FIR preparer and lowerer can reason on dense slot
/// vectors only.
///
/// Limitation: this is a narrower compatibility step, not a full Rust port of
/// `inlineDegenerateRecursions(...)`. In particular, it does not analyze
/// recursive dependency graphs or rewrite projection definitions under delay
/// operators; it only canonicalizes logical projection indices once symbolic
/// recursion has already been built.
pub fn prepare_signals_for_fir(
    src_arena: &TreeArena,
    outputs: &[SigId],
    ui: &UiProgram,
) -> Result<PreparedSignals, SignalPrepareError> {
    let prepared = prepare_signals_for_fir_unverified(src_arena, outputs, ui)?;
    verify::verify_prepared_output_arity(outputs.len(), prepared.outputs.len())?;
    prepared.verify(ui)?;
    Ok(prepared)
}

/// Like [`prepare_signals_for_fir`], but returns a wrapper certifying that the
/// postcondition verifier has already run successfully.
pub fn prepare_signals_for_fir_verified(
    src_arena: &TreeArena,
    outputs: &[SigId],
    ui: &UiProgram,
) -> Result<VerifiedPreparedSignals, SignalPrepareError> {
    let prepared = prepare_signals_for_fir_unverified(src_arena, outputs, ui)?;
    verify::verify_prepared_output_arity(outputs.len(), prepared.outputs.len())?;
    prepared.into_verified(ui)
}

fn prepare_signals_for_fir_unverified(
    src_arena: &TreeArena,
    outputs: &[SigId],
    ui: &UiProgram,
) -> Result<PreparedSignals, SignalPrepareError> {
    let mut arena = TreeArena::new();
    let cloned_outputs = arena.clone_forest_from(src_arena, outputs);
    let cloned_list = vec_to_list(&mut arena, &cloned_outputs);
    let symbolic_list = tlib::de_bruijn_to_sym(&mut arena, cloned_list)?;
    let symbolic_list = rewrites::canonicalize_unary_rec_projections(&mut arena, symbolic_list)?;
    let outputs = list_to_vec(&arena, symbolic_list)
        .expect("prepare_signals_for_fir rebuilds a proper cons list");
    // Inter-pass contract (W4): `de_bruijn_to_sym` establishes the symbolic
    // recursion form. Checking it in debug builds turns the otherwise implicit
    // staging order into a testable postcondition, localized to this pass rather
    // than surfacing later at the `verify` boundary.
    debug_assert!(
        !forest_has_de_bruijn(&arena, &outputs),
        "de_bruijn_to_sym must eliminate every de Bruijn recursion form before typing"
    );
    let sig_types_before = infer_full_types(&arena, &outputs, ui)?;
    let outputs = promote_signals_fastlane(&mut arena, &sig_types_before, &outputs)
        .map_err(SignalPrepareError::Promotion)?;
    let sig_types_after_promotion = infer_full_types(&arena, &outputs, ui)?;
    let outputs = simplify_signals_fastlane(&mut arena, &sig_types_after_promotion, &outputs);

    let outputs = normalize::merge_isomorphic_symrec_groups(&mut arena, &outputs);
    let sig_types_after_merge = infer_full_types(&arena, &outputs, ui)?;
    let outputs = simplify_signals_fastlane(&mut arena, &sig_types_after_merge, &outputs);
    let outputs = rewrites::canonicalize_one_sample_delays(&mut arena, &outputs)?;
    // Inter-pass contract (W4): `canonicalize_one_sample_delays` establishes D1.
    debug_assert!(
        !forest_has_delay_of_one(&arena, &outputs),
        "canonicalize_one_sample_delays must rewrite every Delay(_, 1) to Delay1"
    );
    let sig_types_after_canonicalize = infer_full_types(&arena, &outputs, ui)?;
    let outputs = promote_signals_fastlane(&mut arena, &sig_types_after_canonicalize, &outputs)
        .map_err(SignalPrepareError::Promotion)?;
    // Inter-pass contract (W4): the tail of the pipeline (typing / promotion /
    // simplification / merge) must preserve the structural invariants the prepared
    // forest hands to lowering. `P` itself is enforced for the final forest by
    // `verify`; these debug checks pin the two structural forms (`Sym`, `D1`) at
    // the point promotion #2 hands them off, so a regression is attributed here.
    debug_assert!(
        !forest_has_de_bruijn(&arena, &outputs),
        "the staging pipeline must preserve the symbolic-recursion form (Sym)"
    );
    debug_assert!(
        !forest_has_delay_of_one(&arena, &outputs),
        "promotion must preserve the one-sample-delay canonical form (D1)"
    );
    let sig_types = infer_full_types(&arena, &outputs, ui)?;
    let types = derive_simple_types(&arena, &sig_types);
    Ok(PreparedSignals {
        arena,
        outputs,
        types,
        sig_types,
    })
}

/// Returns `true` if any node reachable from `outputs` satisfies `pred`.
///
/// A small structural DAG scan (memoized via `visited`, list children expanded)
/// backing the debug-only inter-pass contract assertions in
/// [`prepare_signals_for_fir_unverified`]. Malformed structures are ignored
/// here; the explicit [`PreparedSignals::verify`] boundary reports them with
/// precise diagnostics.
fn forest_any_node<P>(arena: &TreeArena, outputs: &[SigId], pred: P) -> bool
where
    P: Fn(&TreeArena, SigId) -> bool,
{
    fn walk<P: Fn(&TreeArena, SigId) -> bool>(
        arena: &TreeArena,
        sig: SigId,
        pred: &P,
        visited: &mut HashSet<SigId>,
    ) -> bool {
        if !visited.insert(sig) {
            return false;
        }
        if pred(arena, sig) {
            return true;
        }
        if arena.is_nil(sig) {
            return false;
        }
        let Some(node) = arena.node(sig) else {
            return false;
        };
        for &child in node.children.as_slice() {
            if arena.is_list(child) {
                if let Some(items) = list_to_vec(arena, child)
                    && items.iter().any(|&item| walk(arena, item, pred, visited))
                {
                    return true;
                }
            } else if walk(arena, child, pred, visited) {
                return true;
            }
        }
        false
    }

    let mut visited = HashSet::new();
    outputs
        .iter()
        .any(|&sig| walk(arena, sig, &pred, &mut visited))
}

/// Debug-contract predicate: any residual de Bruijn recursion form (`Sym` broken).
fn forest_has_de_bruijn(arena: &TreeArena, outputs: &[SigId]) -> bool {
    forest_any_node(arena, outputs, |a, s| {
        tlib::match_de_bruijn_rec(a, s).is_some() || tlib::match_de_bruijn_ref(a, s).is_some()
    })
}

/// Debug-contract predicate: any non-canonical one-sample delay `Delay(_, 1)` (`D1` broken).
fn forest_has_delay_of_one(arena: &TreeArena, outputs: &[SigId]) -> bool {
    forest_any_node(
        arena,
        outputs,
        |a, s| matches!(match_sig(a, s), SigMatch::Delay(_, amount) if matches!(match_sig(a, amount), SigMatch::Int(1))),
    )
}

/// Runs the full `TypeAnnotator` (sigtype crate) on the prepared output forest.
///
/// This produces interval bounds, variability, and all lattice qualifiers for
/// each node. Called in [`prepare_signals_for_fir`] before the first
/// `promote_signals_fastlane` (to guide cast-insertion decisions), again after
/// that promotion (to drive algebraic simplification on the promoted graph),
/// again after simplification (to re-establish canonical types before the
/// second promotion pass), and once more after the second promotion so the
/// final map reflects the forest that reaches FIR lowering. That last result
/// is what ends up in [`PreparedSignals::sig_types`].
fn infer_full_types(
    arena: &TreeArena,
    outputs: &[SigId],
    ui: &UiProgram,
) -> Result<HashMap<SigId, SigType>, SignalPrepareError> {
    let mut annotator = TypeAnnotator::new(arena, ui);
    annotator
        .annotate(outputs)
        .map_err(|e| SignalPrepareError::Typing(e.0))
}

/// Reduces canonical `SigType` annotations to the smaller fast-lane domain.
///
/// `normalize`/`sigtype` are the single source of truth for recursion and
/// arithmetic typing. The fast-lane still keeps a smaller `Int / Real / Sound`
/// view for FIR type selection and assertions, derived directly and
/// homomorphically from the canonical map: no second fixpoint and no per-node
/// override, so the reduced type never contradicts the canonical nature. In
/// particular, a fully unconstrained self-recursion (`x = x`) closes to `Int`,
/// matching the C++ `TREC` result — the fast-lane no longer forces it to `Real`.
fn derive_simple_types(
    arena: &TreeArena,
    sig_types: &HashMap<SigId, SigType>,
) -> HashMap<SigId, SimpleSigType> {
    sig_types
        .iter()
        .map(|(sig, ty)| {
            let reduced = if matches!(match_sig(arena, *sig), SigMatch::Soundfile(_)) {
                SimpleSigType::Sound
            } else {
                match ty.nature() {
                    Nature::Int => SimpleSigType::Int,
                    Nature::Real | Nature::Any => SimpleSigType::Real,
                }
            };
            (*sig, reduced)
        })
        .collect()
}

#[cfg(test)]
mod tests;
