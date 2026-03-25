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
//! - infer one reduced `Int / Real / Sound` type for the prepared signals.
//! - insert the reduced `SignalPromotion` cast subset needed by the fast-lane
//!   and re-type the promoted forest
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
//! # Adaptation status
//! This is an adapted Rust staging phase rather than a 1:1 copy of one single
//! C++ class:
//! - `deBruijn2Sym(...)` is applied forest-wide like the C++ normalization path,
//! - reduced typing keeps only the distinctions currently needed by the
//!   fast-lane instead of the full C++ signal type lattice,
//! - the promotion pass ports only the `SignalPromotion` subset required before
//!   `signal_fir`, without additional simplification or normalization.
//!
//! # Explicit Limitation
//! The unary-recursion canonicalization performed here is **not** a 1:1 port of
//! the C++ `inlineDegenerateRecursions(...)` pass.
//!
//! Concretely, the Rust fast-lane currently does **not**:
//! - build the recursive dependency graph,
//! - detect degenerate recursive projections through the C++ graph analysis,
//! - rewrite projections through `hasProjDefinition(...)` / `setProjDefinition(...)`,
//! - or inline recursive projection definitions under delays the way the C++
//!   rewrite rules do.
//!
//! Instead, this stage performs a smaller compatibility normalization tailored
//! to the FIR preparation contract: when a symbolic recursion group has one
//! physical slot, any logical projection index targeting that group is
//! canonicalized to slot `0`. This is sufficient for the current fast-lane
//! consumers, but it should not be mistaken for a full Rust port of the C++
//! degenerate-recursion elimination machinery.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

use normalize::normalform::{NormalFormError, promote_signals_fastlane};
use signals::{SigBuilder, SigId, SigMatch, match_sig};
use sigtype::{Nature, SigType, TypeAnnotator};
use tlib::{
    RecursionError, TreeArena, list_to_vec, match_sym_rec, match_sym_ref, sym_rec, vec_to_list,
};
use ui::UiProgram;

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
/// Owns a private staging arena so preparation passes can rewrite the signal
/// forest without mutating the original parse/eval arena.
#[derive(Debug)]
pub struct PreparedSignals {
    /// Private staging arena containing the prepared signal forest.
    pub arena: TreeArena,
    /// Prepared output roots interned in [`Self::arena`].
    pub outputs: Vec<SigId>,
    /// Reduced type annotation for prepared signal nodes (for promoter + FIR lowerer).
    pub(crate) types: HashMap<SigId, SimpleSigType>,
    /// Full signal type annotation from the `sigtype` type system.
    /// Carries interval bounds, variability, and all other lattice qualifiers.
    pub(crate) sig_types: HashMap<SigId, SigType>,
}

impl PreparedSignals {
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

/// Typed errors returned while preparing signals for FIR lowering.
#[derive(Debug)]
pub enum SignalPrepareError {
    /// The output forest contains malformed or open de Bruijn recursion.
    Recursion(RecursionError),
    /// Structural type-inference or validation error (e.g. malformed recursion body).
    Typing(String),
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
            Self::Promotion(err) => write!(f, "signal preparation promotion failed: {err}"),
        }
    }
}

impl Error for SignalPrepareError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Recursion(e) => Some(e),
            Self::Promotion(e) => Some(e),
            Self::Typing(_) => None,
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
/// and re-types the promoted forest.
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
    let mut arena = TreeArena::new();
    let cloned_outputs = arena.clone_forest_from(src_arena, outputs);
    let cloned_list = vec_to_list(&mut arena, &cloned_outputs);
    let symbolic_list = tlib::de_bruijn_to_sym(&mut arena, cloned_list)?;
    let symbolic_list = canonicalize_unary_rec_projections(&mut arena, symbolic_list)?;
    let outputs = list_to_vec(&arena, symbolic_list)
        .expect("prepare_signals_for_fir rebuilds a proper cons list");
    let sig_types_before = infer_full_types(&arena, &outputs, ui)?;
    let outputs = promote_signals_fastlane(&mut arena, &sig_types_before, &outputs)
        .map_err(SignalPrepareError::Promotion)?;
    let sig_types = infer_full_types(&arena, &outputs, ui)?;
    let types = derive_simple_types(&arena, &sig_types);
    Ok(PreparedSignals {
        arena,
        outputs,
        types,
        sig_types,
    })
}

/// Rewrites symbolic recursion projections so unary groups always use slot `0`.
///
/// C++ parity note: the classic pipeline can still carry logical projection
/// indices on degenerate recursive groups and resolves them through projection
/// identity later on. The fast-lane uses physical slot vectors, so it
/// canonicalizes `proj(k, group)` to `proj(0, group)` when `group` has one body.
///
/// This is intentionally a preparation-level normalization:
/// - downstream reduced typing only sees dense slot indices,
/// - FIR lowering can keep using `Vec<slot>` recursion carriers,
/// - the behavior stays stable even if different frontends expose the same
///   degenerate recursive projection through different logical indices.
///
/// Explicit limitation: the pass does not decide whether a projection is
/// degenerate from recursive dependency analysis. It only observes the already
/// materialized symbolic shape and canonicalizes projections targeting groups
/// whose body list has arity `1`.
fn canonicalize_unary_rec_projections(
    arena: &mut TreeArena,
    root: SigId,
) -> Result<SigId, SignalPrepareError> {
    let mut unary_groups = HashMap::new();
    let mut visited = HashSet::new();
    collect_unary_sym_groups(arena, root, &mut unary_groups, &mut visited)?;
    let mut memo = HashMap::new();
    rewrite_unary_rec_projections(arena, root, &unary_groups, &mut memo)
}

/// Collects symbolic recursion variables whose body list has exactly one slot.
///
/// The collected set drives [`rewrite_unary_rec_projections`].  The traversal
/// uses a `visited` set to avoid re-visiting shared DAG nodes (signal forests
/// use structural sharing so the same sub-tree can appear under multiple
/// parents).  `cons`-encoded child lists are expanded explicitly rather than
/// treated as opaque signal nodes, because the arena represents list spines as
/// regular nodes and `match_sig` would not recurse into them otherwise.
fn collect_unary_sym_groups(
    arena: &TreeArena,
    sig: SigId,
    unary_groups: &mut HashMap<SigId, usize>,
    visited: &mut HashSet<SigId>,
) -> Result<(), SignalPrepareError> {
    if !visited.insert(sig) {
        return Ok(());
    }

    if let Some((var, body_list)) = match_sym_rec(arena, sig) {
        let bodies = list_to_vec(arena, body_list).ok_or_else(|| {
            SignalPrepareError::Typing("malformed symbolic recursion body list".to_owned())
        })?;
        if bodies.len() == 1 {
            unary_groups.insert(var, 1);
        }
        for body in bodies {
            collect_unary_sym_groups(arena, body, unary_groups, visited)?;
        }
        return Ok(());
    }

    if arena.is_nil(sig) {
        return Ok(());
    }

    let node = arena.node(sig).ok_or_else(|| {
        SignalPrepareError::Typing(format!(
            "missing node {} during unary recursion canonicalization",
            sig.as_u32()
        ))
    })?;
    for child in node.children.as_slice() {
        if arena.is_list(*child) {
            let items = list_to_vec(arena, *child).ok_or_else(|| {
                SignalPrepareError::Typing(
                    "malformed list during unary recursion canonicalization".to_owned(),
                )
            })?;
            for item in items {
                collect_unary_sym_groups(arena, item, unary_groups, visited)?;
            }
        } else {
            collect_unary_sym_groups(arena, *child, unary_groups, visited)?;
        }
    }
    Ok(())
}

/// Rebuilds one prepared signal/list tree with canonical unary recursion indices.
///
/// For every `proj(k, group)` where `group` resolves to a symbolic recursion
/// binder with one body, the rebuilt node becomes `proj(0, group)`. The pass is
/// memoized, so shared subtrees remain shared in the staging arena.
fn rewrite_unary_rec_projections(
    arena: &mut TreeArena,
    sig: SigId,
    unary_groups: &HashMap<SigId, usize>,
    memo: &mut HashMap<SigId, SigId>,
) -> Result<SigId, SignalPrepareError> {
    if let Some(mapped) = memo.get(&sig) {
        return Ok(*mapped);
    }

    let rewritten = if arena.is_nil(sig) {
        sig
    } else if arena.is_list(sig) {
        let head = arena.hd(sig).ok_or_else(|| {
            SignalPrepareError::Typing(
                "malformed list during unary recursion canonicalization".to_owned(),
            )
        })?;
        let tail = arena.tl(sig).ok_or_else(|| {
            SignalPrepareError::Typing(
                "malformed list during unary recursion canonicalization".to_owned(),
            )
        })?;
        let head = rewrite_unary_rec_projections(arena, head, unary_groups, memo)?;
        let tail = rewrite_unary_rec_projections(arena, tail, unary_groups, memo)?;
        arena.cons(head, tail)
    } else if let Some((var, body_list)) = match_sym_rec(arena, sig) {
        let body_list = rewrite_unary_rec_projections(arena, body_list, unary_groups, memo)?;
        sym_rec(arena, var, body_list)
    } else if let SigMatch::Proj(index, group) = match_sig(arena, sig) {
        let group = rewrite_unary_rec_projections(arena, group, unary_groups, memo)?;
        let canonical_index = if let Some(var) = match_sym_ref(arena, group) {
            if unary_groups.contains_key(&var) {
                0
            } else {
                index
            }
        } else if let Some((var, body_list)) = match_sym_rec(arena, group) {
            if unary_groups.contains_key(&var) {
                0
            } else {
                let bodies = list_to_vec(arena, body_list).ok_or_else(|| {
                    SignalPrepareError::Typing("malformed symbolic recursion body list".to_owned())
                })?;
                if bodies.len() == 1 { 0 } else { index }
            }
        } else {
            index
        };
        let mut b = SigBuilder::new(arena);
        b.proj(canonical_index, group)
    } else {
        let node = arena.node(sig).cloned().ok_or_else(|| {
            SignalPrepareError::Typing(format!(
                "missing node {} during unary recursion canonicalization",
                sig.as_u32()
            ))
        })?;
        let mut children = Vec::with_capacity(node.children.len());
        for child in node.children.as_slice() {
            children.push(rewrite_unary_rec_projections(
                arena,
                *child,
                unary_groups,
                memo,
            )?);
        }
        arena.intern(node.kind, &children)
    };

    memo.insert(sig, rewritten);
    Ok(rewritten)
}

/// Runs the full `TypeAnnotator` (sigtype crate) on the prepared output forest.
///
/// This produces interval bounds, variability, and all lattice qualifiers for
/// each node.  Called **twice** in [`prepare_signals_for_fir`]: once before
/// `promote_signals_fastlane` (to guide cast-insertion decisions) and once
/// after (so the final map reflects the promoted forest, including the newly
/// inserted `IntCast`/`FloatCast` nodes).  The second result is what ends up
/// in [`PreparedSignals::sig_types`].
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
/// view for FIR type selection and assertions, which is derived directly from
/// the canonical map rather than recomputed by a second fixpoint engine.
fn derive_simple_types(
    arena: &TreeArena,
    sig_types: &HashMap<SigId, SigType>,
) -> HashMap<SigId, SimpleSigType> {
    sig_types
        .iter()
        .map(|(sig, ty)| {
            let reduced = if is_unresolved_recursive_projection(arena, *sig) {
                SimpleSigType::Real
            } else if matches!(match_sig(arena, *sig), SigMatch::Soundfile(_)) {
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

/// Fast-lane compatibility fallback for recursion groups with no constraining body.
///
/// The canonical `sigtype` recursion port now starts from the C++ `TREC`
/// approximation, which keeps integer-preserving feedback groups such as
/// `min`/`abs` in `Int`. For the FIR fast-lane we still keep the historical
/// fallback that a completely unconstrained self-recursion closes to `Real`.
///
/// This is intentionally a fast-lane-specific adaptation layered on top of the
/// canonical `sigtype` result, not part of the shared normalization/type system.
fn is_unresolved_recursive_projection(arena: &TreeArena, sig: SigId) -> bool {
    let SigMatch::Proj(_, group) = match_sig(arena, sig) else {
        return false;
    };
    let Some((var, body_list)) = match_sym_rec(arena, group) else {
        return false;
    };
    let Some(body) = arena.hd(body_list) else {
        return false;
    };
    if !arena.is_nil(arena.tl(body_list).unwrap_or_else(|| arena.nil())) {
        return false;
    }
    matches!(match_sig(arena, body), SigMatch::Proj(_, target) if match_sym_ref(arena, target) == Some(var))
}

#[cfg(test)]
mod tests;
