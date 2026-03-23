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

use normalize::normalform::promote_signals_fastlane;
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
    pub types: HashMap<SigId, SimpleSigType>,
    /// Full signal type annotation from the `sigtype` type system.
    /// Carries interval bounds, variability, and all other lattice qualifiers.
    pub sig_types: HashMap<SigId, SigType>,
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
}

/// Typed errors returned while preparing signals for FIR lowering.
#[derive(Debug)]
pub enum SignalPrepareError {
    /// The output forest contains malformed or open de Bruijn recursion.
    Recursion(RecursionError),
    /// Reduced type inference failed on the prepared signal forest.
    Typing(String),
}

impl fmt::Display for SignalPrepareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Recursion(err) => write!(
                f,
                "signal preparation failed during de_bruijn_to_sym: {err}"
            ),
            Self::Typing(msg) => write!(f, "signal preparation typing failed: {msg}"),
        }
    }
}

impl Error for SignalPrepareError {}

impl From<RecursionError> for SignalPrepareError {
    fn from(value: RecursionError) -> Self {
        Self::Recursion(value)
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
        .map_err(|err| SignalPrepareError::Typing(err.to_string()))?;
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
/// The collected set drives [`rewrite_unary_rec_projections`]. The traversal is
/// structural and preserves list payload semantics, so `cons`-encoded child
/// lists are expanded rather than treated as opaque signal nodes.
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
/// each node.  The resulting map is stored alongside the reduced `SimpleSigType`
/// map so that downstream consumers (e.g. `signal_fir`) can read either.
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
mod tests {
    use signals::{BinOp, SigBuilder, SigMatch, match_sig};
    use tlib::{de_bruijn_rec, de_bruijn_ref, match_sym_rec, match_sym_ref};

    use super::{SimpleSigType, prepare_signals_for_fir};

    #[test]
    fn prepare_signals_for_fir_converts_shared_debruijn_group_once_per_forest() {
        let mut arena = tlib::TreeArena::new();
        let self_ref = de_bruijn_ref(&mut arena, 1);
        let body = {
            let mut b = SigBuilder::new(&mut arena);
            let in0 = b.input(0);
            let feedback = b.proj(0, self_ref);
            b.add(feedback, in0)
        };
        let body_list = arena.cons(body, arena.nil());
        let group = de_bruijn_rec(&mut arena, body_list);
        let (proj0, proj1) = {
            let mut b = SigBuilder::new(&mut arena);
            let proj0 = b.proj(0, group);
            let proj1 = b.proj(0, group);
            (proj0, proj1)
        };

        let prepared = prepare_signals_for_fir(&arena, &[proj0, proj1], &ui::UiProgram::empty())
            .expect("closed recursion group");

        assert_eq!(prepared.outputs.len(), 2);
        let SigMatch::Proj(_, left_group) = match_sig(&prepared.arena, prepared.outputs[0]) else {
            panic!("expected left projection");
        };
        let SigMatch::Proj(_, right_group) = match_sig(&prepared.arena, prepared.outputs[1]) else {
            panic!("expected right projection");
        };
        assert_eq!(
            left_group, right_group,
            "forest preparation should keep one symbolic group identity across outputs"
        );

        let (var, body_list) =
            match_sym_rec(&prepared.arena, left_group).expect("symbolic recursion expected");
        let body = prepared
            .arena
            .hd(body_list)
            .expect("symbolic body list head");
        let SigMatch::BinOp(_, lhs, rhs) = match_sig(&prepared.arena, body) else {
            panic!("prepared recursive body should stay intact");
        };
        let SigMatch::Proj(0, feedback_group) = match_sig(&prepared.arena, lhs) else {
            panic!("feedback edge should stay as proj(0, symref(var))");
        };
        assert_eq!(match_sym_ref(&prepared.arena, feedback_group), Some(var));
        assert_eq!(match_sig(&prepared.arena, rhs), SigMatch::Input(0));
        assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Real));
    }

    #[test]
    fn prepare_signals_for_fir_records_reduced_numeric_types() {
        let mut arena = tlib::TreeArena::new();
        let outputs = {
            let mut b = SigBuilder::new(&mut arena);
            let v0 = b.int(1);
            let v1 = b.int(2);
            let v2 = b.int(3);
            let waveform = b.waveform(&[v0, v1, v2]);
            let input = b.input(0);
            let read = b.rdtbl(waveform, input);
            let selector = b.int(1);
            let zero = b.real(0.0);
            let mix = b.select2(selector, read, zero);
            vec![waveform, read, mix]
        };

        let prepared = prepare_signals_for_fir(&arena, &outputs, &ui::UiProgram::empty())
            .expect("simple numeric typing should work");

        assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Int));
        assert_eq!(prepared.ty(prepared.outputs[1]), Some(SimpleSigType::Int));
        assert_eq!(prepared.ty(prepared.outputs[2]), Some(SimpleSigType::Real));
    }

    #[test]
    fn prepare_signals_for_fir_closes_unresolved_recursive_types_to_real() {
        let mut arena = tlib::TreeArena::new();
        let self_ref = de_bruijn_ref(&mut arena, 1);
        let body = {
            let mut b = SigBuilder::new(&mut arena);
            b.proj(0, self_ref)
        };
        let body_list = arena.cons(body, arena.nil());
        let group = de_bruijn_rec(&mut arena, body_list);
        let output = {
            let mut b = SigBuilder::new(&mut arena);
            b.proj(0, group)
        };

        let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
            .expect("recursive typing should converge");

        assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Real));
    }

    #[test]
    fn prepare_signals_for_fir_keeps_integer_recursive_min_feedback_int() {
        let mut arena = tlib::TreeArena::new();
        let self_ref = de_bruijn_ref(&mut arena, 1);
        let body = {
            let mut b = SigBuilder::new(&mut arena);
            let feedback = b.proj(0, self_ref);
            let prev = b.delay1(feedback);
            let inc = b.int(1);
            let sum = b.add(prev, inc);
            let cap = b.int(3);
            b.min(sum, cap)
        };
        let body_list = arena.cons(body, arena.nil());
        let group = de_bruijn_rec(&mut arena, body_list);
        let output = {
            let mut b = SigBuilder::new(&mut arena);
            b.proj(0, group)
        };

        let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
            .expect("recursive int min should prepare");

        assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Int));
        let SigMatch::Proj(_, prepared_group) = match_sig(&prepared.arena, prepared.outputs[0])
        else {
            panic!("prepared output should stay a projection");
        };
        let (_, prepared_body_list) =
            match_sym_rec(&prepared.arena, prepared_group).expect("symbolic recursion expected");
        let prepared_body = prepared
            .arena
            .hd(prepared_body_list)
            .expect("prepared recursion body head");
        let SigMatch::Min(sum, cap) = match_sig(&prepared.arena, prepared_body) else {
            panic!("prepared body should stay SIGMIN");
        };
        assert_eq!(match_sig(&prepared.arena, cap), SigMatch::Int(3));
        let SigMatch::BinOp(_, prev, inc) = match_sig(&prepared.arena, sum) else {
            panic!("prepared min lhs should stay integer addition");
        };
        assert!(
            !matches!(match_sig(&prepared.arena, prev), SigMatch::FloatCast(_)),
            "integer recursive feedback should not be promoted to float before SIGMIN"
        );
        assert_eq!(match_sig(&prepared.arena, inc), SigMatch::Int(1));
    }

    #[test]
    fn prepare_signals_for_fir_keeps_integer_recursive_abs_feedback_int() {
        let mut arena = tlib::TreeArena::new();
        let self_ref = de_bruijn_ref(&mut arena, 1);
        let body = {
            let mut b = SigBuilder::new(&mut arena);
            let feedback = b.proj(0, self_ref);
            let prev = b.delay1(feedback);
            let inc = b.int(1);
            let sum = b.add(prev, inc);
            b.abs(sum)
        };
        let body_list = arena.cons(body, arena.nil());
        let group = de_bruijn_rec(&mut arena, body_list);
        let output = {
            let mut b = SigBuilder::new(&mut arena);
            b.proj(0, group)
        };

        let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
            .expect("recursive int abs should prepare");

        assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Int));
        let SigMatch::Proj(_, prepared_group) = match_sig(&prepared.arena, prepared.outputs[0])
        else {
            panic!("prepared output should stay a projection");
        };
        let (_, prepared_body_list) =
            match_sym_rec(&prepared.arena, prepared_group).expect("symbolic recursion expected");
        let prepared_body = prepared
            .arena
            .hd(prepared_body_list)
            .expect("prepared recursion body head");
        let SigMatch::Abs(sum) = match_sig(&prepared.arena, prepared_body) else {
            panic!("prepared body should stay SIGABS");
        };
        let SigMatch::BinOp(_, prev, inc) = match_sig(&prepared.arena, sum) else {
            panic!("prepared abs operand should stay integer addition");
        };
        assert!(
            !matches!(match_sig(&prepared.arena, prev), SigMatch::FloatCast(_)),
            "integer recursive feedback should not be promoted to float before SIGABS"
        );
        assert_eq!(match_sig(&prepared.arena, inc), SigMatch::Int(1));
    }

    #[test]
    fn prepare_signals_for_fir_promotes_delay_amounts_to_int() {
        let mut arena = tlib::TreeArena::new();
        let output = {
            let mut b = SigBuilder::new(&mut arena);
            let input = b.input(0);
            let amount = b.real(1.5);
            b.delay(input, amount)
        };

        let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
            .expect("delay promotion should succeed");

        let SigMatch::Delay(_, amount) = match_sig(&prepared.arena, prepared.outputs[0]) else {
            panic!("promoted output should stay SIGDELAY");
        };
        let SigMatch::IntCast(inner) = match_sig(&prepared.arena, amount) else {
            panic!("delay amount should be promoted to SIGINTCAST");
        };
        assert_eq!(match_sig(&prepared.arena, inner), SigMatch::Real(1.5));
    }

    #[test]
    fn prepare_signals_for_fir_promotes_select2_selector_and_mixed_branches() {
        let mut arena = tlib::TreeArena::new();
        let output = {
            let mut b = SigBuilder::new(&mut arena);
            let selector = b.input(0);
            let then_value = b.int(1);
            let else_value = b.input(1);
            b.select2(selector, then_value, else_value)
        };

        let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
            .expect("select2 promotion should succeed");

        let SigMatch::Select2(selector, then_value, else_value) =
            match_sig(&prepared.arena, prepared.outputs[0])
        else {
            panic!("promoted output should stay SIGSELECT2");
        };
        let SigMatch::IntCast(selector_inner) = match_sig(&prepared.arena, selector) else {
            panic!("select2 selector should be promoted to SIGINTCAST");
        };
        assert_eq!(
            match_sig(&prepared.arena, selector_inner),
            SigMatch::Input(0)
        );
        assert_eq!(
            match_sig(&prepared.arena, then_value),
            SigMatch::Real(1.0),
            "mixed-typed branch should be promoted to real"
        );
        assert_eq!(match_sig(&prepared.arena, else_value), SigMatch::Input(1));
        assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Real));
    }

    #[test]
    fn prepare_signals_for_fir_recovers_shared_select2_selector_from_float_context() {
        let mut arena = tlib::TreeArena::new();
        let (arith_out, select_out) = {
            let mut b = SigBuilder::new(&mut arena);
            let input0 = b.input(0);
            let half = b.real(0.5);
            let cmp = b.lt(input0, half);
            let input1 = b.input(1);
            let arith = b.add(cmp, input1);
            let one = b.int(1);
            let zero = b.int(0);
            let sel = b.select2(cmp, one, zero);
            (arith, sel)
        };

        let prepared =
            prepare_signals_for_fir(&arena, &[arith_out, select_out], &ui::UiProgram::empty())
                .expect("shared comparison should promote in both arithmetic and select2 contexts");

        let SigMatch::Select2(selector, _, _) = match_sig(&prepared.arena, prepared.outputs[1])
        else {
            panic!("second output should stay SIGSELECT2");
        };
        assert!(
            matches!(match_sig(&prepared.arena, selector), SigMatch::IntCast(_))
                || matches!(
                    match_sig(&prepared.arena, selector),
                    SigMatch::BinOp(
                        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne,
                        _,
                        _
                    )
                ),
            "shared select2 selector should stay in an integer comparison domain"
        );
    }

    #[test]
    fn prepare_signals_for_fir_recovers_shared_delay_amount_from_float_context() {
        let mut arena = tlib::TreeArena::new();
        let (arith_out, delay_out) = {
            let mut b = SigBuilder::new(&mut arena);
            let input0 = b.input(0);
            let half = b.real(0.5);
            let cmp = b.lt(input0, half);
            let input1 = b.input(1);
            let arith = b.add(cmp, input1);
            let input2 = b.input(2);
            let delay = b.delay(input2, cmp);
            (arith, delay)
        };

        let prepared =
            prepare_signals_for_fir(&arena, &[arith_out, delay_out], &ui::UiProgram::empty())
                .expect("shared comparison should promote in both arithmetic and delay contexts");

        let SigMatch::Delay(_, amount) = match_sig(&prepared.arena, prepared.outputs[1]) else {
            panic!("second output should stay SIGDELAY");
        };
        assert!(
            matches!(match_sig(&prepared.arena, amount), SigMatch::IntCast(_))
                || matches!(
                    match_sig(&prepared.arena, amount),
                    SigMatch::BinOp(
                        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne,
                        _,
                        _
                    )
                ),
            "shared delay amount should stay in an integer comparison domain"
        );
    }

    #[test]
    fn prepare_signals_for_fir_recovers_shared_rdtbl_index_from_float_context() {
        let mut arena = tlib::TreeArena::new();
        let (arith_out, table_out) = {
            let mut b = SigBuilder::new(&mut arena);
            let input0 = b.input(0);
            let half = b.real(0.5);
            let cmp = b.lt(input0, half);
            let input1 = b.input(1);
            let arith = b.add(cmp, input1);
            let v0 = b.real(0.0);
            let v1 = b.real(1.0);
            let waveform = b.waveform(&[v0, v1]);
            let table = b.rdtbl(waveform, cmp);
            (arith, table)
        };

        let prepared =
            prepare_signals_for_fir(&arena, &[arith_out, table_out], &ui::UiProgram::empty())
                .expect("shared comparison should promote in both arithmetic and rdtbl contexts");

        let SigMatch::RdTbl(_, index) = match_sig(&prepared.arena, prepared.outputs[1]) else {
            panic!("second output should stay SIGRDTBL");
        };
        assert!(
            matches!(match_sig(&prepared.arena, index), SigMatch::IntCast(_))
                || matches!(
                    match_sig(&prepared.arena, index),
                    SigMatch::BinOp(
                        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne,
                        _,
                        _
                    )
                ),
            "shared table-read index should stay in an integer comparison domain"
        );
    }

    #[test]
    fn prepare_signals_for_fir_recovers_shared_wrtbl_write_signal_from_float_context() {
        let mut arena = tlib::TreeArena::new();
        let (arith_out, table_out) = {
            let mut b = SigBuilder::new(&mut arena);
            let input0 = b.input(0);
            let half = b.real(0.5);
            let cmp = b.lt(input0, half);
            let input1 = b.input(1);
            let arith = b.add(cmp, input1);
            let size = b.int(8);
            let generator = b.int(0);
            let write_index = b.int(1);
            let table = b.wrtbl(size, generator, write_index, cmp);
            (arith, table)
        };

        let prepared =
            prepare_signals_for_fir(&arena, &[arith_out, table_out], &ui::UiProgram::empty())
                .expect("shared comparison should promote in both arithmetic and wrtbl contexts");

        let SigMatch::WrTbl(_, _, _, write_signal) =
            match_sig(&prepared.arena, prepared.outputs[1])
        else {
            panic!("second output should stay SIGWRTBL");
        };
        assert!(
            matches!(
                match_sig(&prepared.arena, write_signal),
                SigMatch::IntCast(_)
            ) || matches!(
                match_sig(&prepared.arena, write_signal),
                SigMatch::BinOp(
                    BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne,
                    _,
                    _
                )
            ),
            "shared wrtbl write signal should stay in an integer comparison domain"
        );
    }

    #[test]
    fn prepare_signals_for_fir_recovers_shared_zero_pad_amount_from_float_context() {
        let mut arena = tlib::TreeArena::new();
        let (arith_out, padded_out) = {
            let mut b = SigBuilder::new(&mut arena);
            let input0 = b.input(0);
            let half = b.real(0.5);
            let cmp = b.lt(input0, half);
            let input1 = b.input(1);
            let arith = b.add(cmp, input1);
            let input2 = b.input(2);
            let padded = b.zero_pad(input2, cmp);
            (arith, padded)
        };

        let prepared =
            prepare_signals_for_fir(&arena, &[arith_out, padded_out], &ui::UiProgram::empty())
                .expect(
                    "shared comparison should promote in both arithmetic and zero_pad contexts",
                );

        let SigMatch::ZeroPad(_, amount) = match_sig(&prepared.arena, prepared.outputs[1]) else {
            panic!("second output should stay SIGZEROPAD");
        };
        assert!(
            matches!(match_sig(&prepared.arena, amount), SigMatch::IntCast(_))
                || matches!(
                    match_sig(&prepared.arena, amount),
                    SigMatch::BinOp(
                        BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge | BinOp::Eq | BinOp::Ne,
                        _,
                        _
                    )
                ),
            "shared zero-pad amount should stay in an integer comparison domain"
        );
    }

    #[test]
    fn prepare_signals_for_fir_promotes_table_read_index_to_int() {
        let mut arena = tlib::TreeArena::new();
        let output = {
            let mut b = SigBuilder::new(&mut arena);
            let v0 = b.real(0.0);
            let v1 = b.real(1.0);
            let waveform = b.waveform(&[v0, v1]);
            let index = b.input(0);
            b.rdtbl(waveform, index)
        };

        let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
            .expect("table promotion should succeed");

        let SigMatch::RdTbl(_, index) = match_sig(&prepared.arena, prepared.outputs[0]) else {
            panic!("promoted output should stay SIGRDTBL");
        };
        let SigMatch::IntCast(inner) = match_sig(&prepared.arena, index) else {
            panic!("table read index should be promoted to SIGINTCAST");
        };
        assert_eq!(match_sig(&prepared.arena, inner), SigMatch::Input(0));
    }

    #[test]
    fn prepare_signals_for_fir_promotes_real_mul_operands_before_binop() {
        let mut arena = tlib::TreeArena::new();
        let output = {
            let mut b = SigBuilder::new(&mut arena);
            let gate_init = b.int(0);
            let gate_next = b.int(1);
            let gate = b.prefix(gate_init, gate_next);
            let carrier_init = b.real(0.0);
            let carrier_next = b.real(0.5);
            let carrier = b.prefix(carrier_init, carrier_next);
            let inner = b.binop(BinOp::Mul, carrier, gate);
            b.binop(BinOp::Mul, inner, gate)
        };

        let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
            .expect("mixed real/int multiplication should prepare");

        let SigMatch::BinOp(BinOp::Mul, left, right) =
            match_sig(&prepared.arena, prepared.outputs[0])
        else {
            panic!("prepared output should stay SIGBINOP(Mul, ...)");
        };
        assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Real));
        assert!(
            matches!(match_sig(&prepared.arena, right), SigMatch::FloatCast(_)),
            "outer real multiplication must cast the integer operand before the BinOp"
        );

        let SigMatch::BinOp(BinOp::Mul, _, inner_right) = match_sig(&prepared.arena, left) else {
            panic!("inner multiplication should stay SIGBINOP(Mul, ...)");
        };
        assert!(
            matches!(
                match_sig(&prepared.arena, inner_right),
                SigMatch::FloatCast(_)
            ),
            "inner real multiplication must cast the integer operand before the BinOp"
        );
    }

    #[test]
    fn recursive_fixpoint_recomputes_body_types_after_real_widening() {
        let mut arena = tlib::TreeArena::new();
        let self_ref = de_bruijn_ref(&mut arena, 1);
        let counter_body = {
            let mut b = SigBuilder::new(&mut arena);
            let counter_proj = b.proj(0, self_ref);
            let counter_prev = b.delay1(counter_proj);
            let one = b.int(1);
            b.binop(BinOp::Add, counter_prev, one)
        };
        let amp_body = {
            let mut b = SigBuilder::new(&mut arena);
            let amp_proj = b.proj(1, self_ref);
            let amp_prev = b.delay1(amp_proj);
            let half = b.real(0.5);
            b.binop(BinOp::Add, amp_prev, half)
        };
        let gated_body = {
            let mut b = SigBuilder::new(&mut arena);
            let amp_proj = b.proj(1, self_ref);
            let counter_proj = b.proj(0, self_ref);
            let amp_prev = b.delay1(amp_proj);
            let counter_prev = b.delay1(counter_proj);
            let period = b.int(128);
            let zero = b.int(0);
            let one = b.int(1);
            let rem = b.binop(BinOp::Rem, counter_prev, period);
            let eq = b.binop(BinOp::Eq, rem, zero);
            let gate = b.binop(BinOp::Sub, one, eq);
            b.binop(BinOp::Mul, amp_prev, gate)
        };
        let nil = arena.nil();
        let tail2 = arena.cons(gated_body, nil);
        let tail1 = arena.cons(amp_body, tail2);
        let body_list = arena.cons(counter_body, tail1);
        let group = de_bruijn_rec(&mut arena, body_list);
        let output = {
            let mut b = SigBuilder::new(&mut arena);
            b.proj(2, group)
        };

        let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
            .expect("recursive real widening should converge");

        assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Real));
    }

    #[test]
    fn prepare_signals_for_fir_uses_foreign_function_return_type() {
        let mut arena = tlib::TreeArena::new();
        let output = {
            let ty_int = arena.int(0);
            let ty_real = arena.int(1);
            let incfile = arena.symbol("<math.h>");
            let libfile = arena.symbol("\"\"");
            let name_f32 = arena.symbol("isnanf");
            let name_f64 = arena.symbol("isnan");
            let name_f80 = arena.symbol("isnanl");
            let name_fx = arena.symbol("isnanfx");
            let nil = arena.nil();
            let names = {
                let tail0 = arena.cons(name_fx, nil);
                let tail1 = arena.cons(name_f80, tail0);
                let tail2 = arena.cons(name_f64, tail1);
                arena.cons(name_f32, tail2)
            };
            let arg_types = arena.cons(ty_real, nil);
            let payload = arena.cons(names, arg_types);
            let signature = arena.cons(ty_int, payload);
            let ff_tag = arena.intern_tag("FFUN");
            let ff = arena.intern(tlib::NodeKind::Tag(ff_tag), &[signature, incfile, libfile]);
            let input0 = {
                let mut b = SigBuilder::new(&mut arena);
                b.input(0)
            };
            let args = arena.cons(input0, nil);
            let mut b = SigBuilder::new(&mut arena);
            b.ffun(ff, args)
        };

        let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
            .expect("foreign function result type should prepare");

        assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Int));
    }

    #[test]
    fn prepare_signals_for_fir_canonicalizes_unary_recursive_projection_indices() {
        let mut arena = tlib::TreeArena::new();
        let self_ref = de_bruijn_ref(&mut arena, 1);
        let body = {
            let mut b = SigBuilder::new(&mut arena);
            let feedback = b.proj(7, self_ref);
            b.delay1(feedback)
        };
        let body_list = arena.cons(body, arena.nil());
        let group = de_bruijn_rec(&mut arena, body_list);
        let output = {
            let mut b = SigBuilder::new(&mut arena);
            b.proj(7, group)
        };

        let prepared = prepare_signals_for_fir(&arena, &[output], &ui::UiProgram::empty())
            .expect("degenerate recursive projection should prepare");

        let SigMatch::Proj(0, prepared_group) = match_sig(&prepared.arena, prepared.outputs[0])
        else {
            panic!("prepared output should canonicalize to proj(0, ...)");
        };
        let (_, prepared_body_list) =
            match_sym_rec(&prepared.arena, prepared_group).expect("symbolic recursion expected");
        let prepared_body = prepared
            .arena
            .hd(prepared_body_list)
            .expect("prepared recursion body head");
        let SigMatch::Delay1(feedback) = match_sig(&prepared.arena, prepared_body) else {
            panic!("prepared body should stay SIGDELAY1");
        };
        let SigMatch::Proj(0, feedback_group) = match_sig(&prepared.arena, feedback) else {
            panic!("feedback edge should canonicalize to proj(0, symref(var))");
        };
        let (var, _) =
            match_sym_rec(&prepared.arena, prepared_group).expect("symbolic recursion expected");
        assert_eq!(match_sym_ref(&prepared.arena, feedback_group), Some(var));
    }

    #[test]
    fn prepare_signals_for_fir_handles_shared_unary_recursion_dag_linearly() {
        let mut arena = tlib::TreeArena::new();
        let self_ref = de_bruijn_ref(&mut arena, 1);
        let body = {
            let mut b = SigBuilder::new(&mut arena);
            let feedback = b.proj(7, self_ref);
            b.delay1(feedback)
        };
        let body_list = arena.cons(body, arena.nil());
        let group = de_bruijn_rec(&mut arena, body_list);
        let leaf = {
            let mut b = SigBuilder::new(&mut arena);
            b.proj(7, group)
        };
        let mut shared = leaf;
        for _ in 0..24 {
            let mut b = SigBuilder::new(&mut arena);
            shared = b.add(shared, shared);
        }

        let prepared = prepare_signals_for_fir(&arena, &[shared], &ui::UiProgram::empty())
            .expect("shared unary recursion dag should prepare");

        assert!(
            prepared.outputs[0].as_u32() != 0,
            "preparation should produce a staged output"
        );
    }
}
