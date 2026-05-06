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
//! The module now also exposes an explicit verification boundary:
//! - [`PreparedSignals::verify`] checks those postconditions on an already built
//!   staging forest,
//! - [`prepare_signals_for_fir_verified`] returns a wrapper that certifies the
//!   prepared forest passed the boundary checks before reaching FIR lowering.
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

use normalize::normalform::{NormalFormError, promote_signals_fastlane, simplify_signals_fastlane};
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

    /// Verifies the documented postconditions of the prepared staging forest.
    ///
    /// This is intentionally a structural boundary verifier:
    /// - it checks only properties already guaranteed or already assumed by the
    ///   fast-lane,
    /// - it does not change the forest,
    /// - it fails close to the stage boundary when an invariant regresses.
    pub fn verify(&self, ui: &UiProgram) -> Result<(), SignalPrepareError> {
        let derived_types = derive_simple_types(&self.arena, &self.sig_types);
        let mut visited = HashSet::new();
        let mut reachable_typed_nodes = Vec::new();
        let mut sym_group_arities = HashMap::new();

        for &out in &self.outputs {
            verify_prepared_signal(
                &self.arena,
                ui,
                out,
                &mut visited,
                &mut sym_group_arities,
                &mut reachable_typed_nodes,
            )?;
        }

        for sig in reachable_typed_nodes {
            let Some(actual_reduced) = self.types.get(&sig).copied() else {
                return Err(SignalPrepareError::Validation(format!(
                    "prepared signal {} is reachable but missing reduced type annotation",
                    sig.as_u32()
                )));
            };
            let Some(actual_full) = self.sig_types.get(&sig) else {
                return Err(SignalPrepareError::Validation(format!(
                    "prepared signal {} is reachable but missing full SigType annotation",
                    sig.as_u32()
                )));
            };
            let Some(expected_reduced) = derived_types.get(&sig).copied() else {
                return Err(SignalPrepareError::Validation(format!(
                    "prepared signal {} is reachable but has no derived reduced type",
                    sig.as_u32()
                )));
            };
            if actual_reduced != expected_reduced {
                return Err(SignalPrepareError::Validation(format!(
                    "prepared signal {} has inconsistent reduced type: stored={actual_reduced:?}, derived={expected_reduced:?}, full={actual_full:?}",
                    sig.as_u32()
                )));
            }
        }

        Ok(())
    }

    /// Consumes this prepared forest and returns a verified wrapper when the
    /// boundary checks succeed.
    pub fn into_verified(
        self,
        ui: &UiProgram,
    ) -> Result<VerifiedPreparedSignals, SignalPrepareError> {
        self.verify(ui)?;
        Ok(VerifiedPreparedSignals { inner: self })
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
    verify_prepared_output_arity(outputs.len(), prepared.outputs.len())?;
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
    verify_prepared_output_arity(outputs.len(), prepared.outputs.len())?;
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
    let symbolic_list = canonicalize_unary_rec_projections(&mut arena, symbolic_list)?;
    let outputs = list_to_vec(&arena, symbolic_list)
        .expect("prepare_signals_for_fir rebuilds a proper cons list");
    let sig_types_before = infer_full_types(&arena, &outputs, ui)?;
    let outputs = promote_signals_fastlane(&mut arena, &sig_types_before, &outputs)
        .map_err(SignalPrepareError::Promotion)?;
    let sig_types_after_promotion = infer_full_types(&arena, &outputs, ui)?;
    let outputs = simplify_signals_fastlane(&mut arena, &sig_types_after_promotion, &outputs);

    let outputs = normalize::merge_isomorphic_symrec_groups(&mut arena, &outputs);
    let sig_types_after_merge = infer_full_types(&arena, &outputs, ui)?;
    let outputs = simplify_signals_fastlane(&mut arena, &sig_types_after_merge, &outputs);
    let outputs = canonicalize_one_sample_delays(&mut arena, &outputs)?;
    let sig_types_after_canonicalize = infer_full_types(&arena, &outputs, ui)?;
    let outputs = promote_signals_fastlane(&mut arena, &sig_types_after_canonicalize, &outputs)
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

fn verify_prepared_output_arity(expected: usize, actual: usize) -> Result<(), SignalPrepareError> {
    if expected == actual {
        return Ok(());
    }
    Err(SignalPrepareError::Validation(format!(
        "prepared output arity changed across staging: expected {expected}, got {actual}"
    )))
}

fn verify_prepared_signal(
    arena: &TreeArena,
    ui: &UiProgram,
    sig: SigId,
    visited: &mut HashSet<SigId>,
    sym_group_arities: &mut HashMap<SigId, usize>,
    reachable_typed_nodes: &mut Vec<SigId>,
) -> Result<(), SignalPrepareError> {
    if !visited.insert(sig) {
        return Ok(());
    }
    if arena.is_nil(sig) || arena.is_list(sig) {
        return Err(SignalPrepareError::Validation(format!(
            "prepared signal traversal reached unexpected list/nil node {}",
            sig.as_u32()
        )));
    }
    if tlib::match_de_bruijn_rec(arena, sig).is_some()
        || tlib::match_de_bruijn_ref(arena, sig).is_some()
    {
        return Err(SignalPrepareError::Validation(format!(
            "prepared signal {} still contains de Bruijn recursion form",
            sig.as_u32()
        )));
    }
    if match_sym_ref(arena, sig).is_some() {
        return Err(SignalPrepareError::Validation(format!(
            "prepared signal {} unexpectedly exposes a bare symbolic recursion reference",
            sig.as_u32()
        )));
    }
    if let Some((var, body_list)) = match_sym_rec(arena, sig) {
        reachable_typed_nodes.push(sig);
        let bodies = list_to_vec(arena, body_list).ok_or_else(|| {
            SignalPrepareError::Validation(
                "malformed symbolic recursion body list in prepared signals".to_owned(),
            )
        })?;
        if bodies.is_empty() {
            return Err(SignalPrepareError::Validation(format!(
                "symbolic recursion group {} has empty body list",
                sig.as_u32()
            )));
        }
        match sym_group_arities.insert(var, bodies.len()) {
            Some(previous) if previous != bodies.len() => {
                return Err(SignalPrepareError::Validation(format!(
                    "symbolic recursion variable {} was observed with inconsistent arities: {previous} vs {}",
                    var.as_u32(),
                    bodies.len()
                )));
            }
            _ => {}
        }
        for body in bodies {
            verify_prepared_signal(
                arena,
                ui,
                body,
                visited,
                sym_group_arities,
                reachable_typed_nodes,
            )?;
        }
        return Ok(());
    }

    reachable_typed_nodes.push(sig);
    match match_sig(arena, sig) {
        SigMatch::Unknown => {
            return Err(SignalPrepareError::Validation(format!(
                "prepared signal {} could not be decoded by match_sig",
                sig.as_u32()
            )));
        }
        SigMatch::Int(_)
        | SigMatch::Real(_)
        | SigMatch::Input(_)
        | SigMatch::Button(_)
        | SigMatch::Checkbox(_)
        | SigMatch::VSlider(_)
        | SigMatch::HSlider(_)
        | SigMatch::NumEntry(_)
        | SigMatch::Soundfile(_)
        | SigMatch::FConst(_, _, _)
        | SigMatch::FVar(_, _, _) => {}
        SigMatch::Output(_, inner)
        | SigMatch::Delay1(inner)
        | SigMatch::IntCast(inner)
        | SigMatch::BitCast(inner)
        | SigMatch::FloatCast(inner)
        | SigMatch::Gen(inner)
        | SigMatch::Lowest(inner)
        | SigMatch::Highest(inner)
        | SigMatch::Acos(inner)
        | SigMatch::Asin(inner)
        | SigMatch::Atan(inner)
        | SigMatch::Cos(inner)
        | SigMatch::Sin(inner)
        | SigMatch::Tan(inner)
        | SigMatch::Exp(inner)
        | SigMatch::Log(inner)
        | SigMatch::Log10(inner)
        | SigMatch::Sqrt(inner)
        | SigMatch::Abs(inner)
        | SigMatch::Floor(inner)
        | SigMatch::Ceil(inner)
        | SigMatch::Rint(inner)
        | SigMatch::Round(inner)
        | SigMatch::TempVar(inner)
        | SigMatch::PermVar(inner) => verify_prepared_signal(
            arena,
            ui,
            inner,
            visited,
            sym_group_arities,
            reachable_typed_nodes,
        )?,
        SigMatch::Delay(x, y)
        | SigMatch::Prefix(x, y)
        | SigMatch::RdTbl(x, y)
        | SigMatch::Pow(x, y)
        | SigMatch::Min(x, y)
        | SigMatch::Max(x, y)
        | SigMatch::Atan2(x, y)
        | SigMatch::Fmod(x, y)
        | SigMatch::Remainder(x, y)
        | SigMatch::Attach(x, y)
        | SigMatch::Enable(x, y)
        | SigMatch::Control(x, y)
        | SigMatch::Seq(x, y)
        | SigMatch::ZeroPad(x, y)
        | SigMatch::Clocked(x, y) => {
            verify_prepared_signal(
                arena,
                ui,
                x,
                visited,
                sym_group_arities,
                reachable_typed_nodes,
            )?;
            verify_prepared_signal(
                arena,
                ui,
                y,
                visited,
                sym_group_arities,
                reachable_typed_nodes,
            )?;
        }
        SigMatch::Fir(coefs) | SigMatch::Iir(coefs) => {
            if coefs.is_empty() {
                return Err(SignalPrepareError::Validation(format!(
                    "prepared filter carrier {} has an empty coefficient vector",
                    sig.as_u32()
                )));
            }
            for &child in coefs {
                verify_prepared_signal(
                    arena,
                    ui,
                    child,
                    visited,
                    sym_group_arities,
                    reachable_typed_nodes,
                )?;
            }
        }
        SigMatch::BinOp(_, x, y) => {
            verify_prepared_signal(
                arena,
                ui,
                x,
                visited,
                sym_group_arities,
                reachable_typed_nodes,
            )?;
            verify_prepared_signal(
                arena,
                ui,
                y,
                visited,
                sym_group_arities,
                reachable_typed_nodes,
            )?;
        }
        SigMatch::Select2(selector, then_value, else_value)
        | SigMatch::AssertBounds(selector, then_value, else_value) => {
            verify_prepared_signal(
                arena,
                ui,
                selector,
                visited,
                sym_group_arities,
                reachable_typed_nodes,
            )?;
            verify_prepared_signal(
                arena,
                ui,
                then_value,
                visited,
                sym_group_arities,
                reachable_typed_nodes,
            )?;
            verify_prepared_signal(
                arena,
                ui,
                else_value,
                visited,
                sym_group_arities,
                reachable_typed_nodes,
            )?;
        }
        SigMatch::WrTbl(size, generator, write_index, write_signal) => {
            for child in [size, generator] {
                verify_prepared_signal(
                    arena,
                    ui,
                    child,
                    visited,
                    sym_group_arities,
                    reachable_typed_nodes,
                )?;
            }
            let readonly = arena.is_nil(write_index) && arena.is_nil(write_signal);
            let malformed_write_pair = arena.is_nil(write_index) ^ arena.is_nil(write_signal);
            if malformed_write_pair {
                return Err(SignalPrepareError::Validation(format!(
                    "write table {} uses inconsistent readonly/write placeholders",
                    sig.as_u32()
                )));
            }
            if !readonly {
                for child in [write_index, write_signal] {
                    verify_prepared_signal(
                        arena,
                        ui,
                        child,
                        visited,
                        sym_group_arities,
                        reachable_typed_nodes,
                    )?;
                }
            }
        }
        SigMatch::FFun(_, arg_list) => {
            let args = list_to_vec(arena, arg_list).ok_or_else(|| {
                SignalPrepareError::Validation(
                    "malformed foreign-function argument list in prepared signals".to_owned(),
                )
            })?;
            for arg in args {
                verify_prepared_signal(
                    arena,
                    ui,
                    arg,
                    visited,
                    sym_group_arities,
                    reachable_typed_nodes,
                )?;
            }
        }
        SigMatch::Proj(index, group) => {
            if index < 0 {
                return Err(SignalPrepareError::Validation(format!(
                    "projection {} uses negative index {index}",
                    sig.as_u32()
                )));
            }
            let reverse_group_body = match match_sig(arena, group) {
                SigMatch::ReverseTimeRec(body) => Some(body),
                _ => None,
            };
            let projection_group = reverse_group_body.unwrap_or(group);
            let arity = if let Some((var, _)) = match_sym_rec(arena, projection_group) {
                verify_prepared_signal(
                    arena,
                    ui,
                    group,
                    visited,
                    sym_group_arities,
                    reachable_typed_nodes,
                )?;
                sym_group_arities.get(&var).copied().ok_or_else(|| {
                    SignalPrepareError::Validation(format!(
                        "projection {} targets recursion group {} without registered arity",
                        sig.as_u32(),
                        group.as_u32()
                    ))
                })?
            } else if let Some(var) = match_sym_ref(arena, projection_group) {
                sym_group_arities.get(&var).copied().ok_or_else(|| {
                    SignalPrepareError::Validation(format!(
                        "projection {} targets symbolic recursion ref {} before its group arity is known",
                        sig.as_u32(),
                        var.as_u32()
                    ))
                })?
            } else {
                return Err(SignalPrepareError::Validation(format!(
                    "projection {} does not target symbolic recursion",
                    sig.as_u32()
                )));
            };
            let index = usize::try_from(index).expect("negative indices rejected above");
            if index >= arity {
                return Err(SignalPrepareError::Validation(format!(
                    "projection {} index {index} is out of range for recursion arity {arity}",
                    sig.as_u32()
                )));
            }
            if arity == 1 && index != 0 {
                return Err(SignalPrepareError::Validation(format!(
                    "projection {} targets unary recursion with non-canonical index {index}",
                    sig.as_u32()
                )));
            }
        }
        SigMatch::Rec(_) => {
            return Err(SignalPrepareError::Validation(format!(
                "prepared signal {} still contains legacy SIGREC form",
                sig.as_u32()
            )));
        }
        SigMatch::ReverseTimeRec(body) => {
            verify_prepared_signal(
                arena,
                ui,
                body,
                visited,
                sym_group_arities,
                reachable_typed_nodes,
            )?;
        }
        SigMatch::VBargraph(control, inner) | SigMatch::HBargraph(control, inner) => {
            verify_control_exists(ui, control, sig)?;
            verify_prepared_signal(
                arena,
                ui,
                inner,
                visited,
                sym_group_arities,
                reachable_typed_nodes,
            )?;
        }
        SigMatch::Waveform(values)
        | SigMatch::OnDemand(values)
        | SigMatch::Upsampling(values)
        | SigMatch::Downsampling(values) => {
            for &child in values {
                verify_prepared_signal(
                    arena,
                    ui,
                    child,
                    visited,
                    sym_group_arities,
                    reachable_typed_nodes,
                )?;
            }
        }
        SigMatch::SoundfileLength(soundfile, part) | SigMatch::SoundfileRate(soundfile, part) => {
            verify_prepared_signal(
                arena,
                ui,
                soundfile,
                visited,
                sym_group_arities,
                reachable_typed_nodes,
            )?;
            verify_prepared_signal(
                arena,
                ui,
                part,
                visited,
                sym_group_arities,
                reachable_typed_nodes,
            )?;
        }
        SigMatch::SoundfileBuffer(soundfile, chan, part, ridx) => {
            for child in [soundfile, chan, part, ridx] {
                verify_prepared_signal(
                    arena,
                    ui,
                    child,
                    visited,
                    sym_group_arities,
                    reachable_typed_nodes,
                )?;
            }
        }
    }

    match match_sig(arena, sig) {
        SigMatch::Button(control)
        | SigMatch::Checkbox(control)
        | SigMatch::VSlider(control)
        | SigMatch::HSlider(control)
        | SigMatch::NumEntry(control)
        | SigMatch::Soundfile(control) => verify_control_exists(ui, control, sig),
        _ => Ok(()),
    }
}

fn verify_control_exists(
    ui: &UiProgram,
    control: ui::ControlId,
    sig: SigId,
) -> Result<(), SignalPrepareError> {
    if ui.control(control).is_some() {
        return Ok(());
    }
    Err(SignalPrepareError::Validation(format!(
        "prepared signal {} references missing UI control id {}",
        sig.as_u32(),
        control
    )))
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

/// Rebuilds the staged forest so literal one-sample delays use `Delay1`.
///
/// `normalize` may legally expose unary feedback as `Delay(x, 1)`. The
/// fast-lane keeps a narrower canonical form for one-sample delays so all
/// downstream consumers (notably SIGGEN interpretation and recursion lowering)
/// can reason on a single representation.
fn canonicalize_one_sample_delays(
    arena: &mut TreeArena,
    outputs: &[SigId],
) -> Result<Vec<SigId>, SignalPrepareError> {
    let mut memo = HashMap::new();
    outputs
        .iter()
        .map(|&sig| rewrite_one_sample_delays(arena, sig, &mut memo))
        .collect()
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

fn rewrite_one_sample_delays(
    arena: &mut TreeArena,
    sig: SigId,
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
                "malformed list during one-sample delay canonicalization".to_owned(),
            )
        })?;
        let tail = arena.tl(sig).ok_or_else(|| {
            SignalPrepareError::Typing(
                "malformed list during one-sample delay canonicalization".to_owned(),
            )
        })?;
        let head = rewrite_one_sample_delays(arena, head, memo)?;
        let tail = rewrite_one_sample_delays(arena, tail, memo)?;
        arena.cons(head, tail)
    } else if let Some((var, body_list)) = match_sym_rec(arena, sig) {
        let body_list = rewrite_one_sample_delays(arena, body_list, memo)?;
        sym_rec(arena, var, body_list)
    } else if let SigMatch::Delay(value, amount) = match_sig(arena, sig) {
        let value = rewrite_one_sample_delays(arena, value, memo)?;
        let amount = rewrite_one_sample_delays(arena, amount, memo)?;
        if matches!(match_sig(arena, amount), SigMatch::Int(1)) {
            let mut b = SigBuilder::new(arena);
            b.delay1(value)
        } else {
            let mut b = SigBuilder::new(arena);
            b.delay(value, amount)
        }
    } else {
        let node = arena.node(sig).cloned().ok_or_else(|| {
            SignalPrepareError::Typing(format!(
                "missing node {} during one-sample delay canonicalization",
                sig.as_u32()
            ))
        })?;
        let mut children = Vec::with_capacity(node.children.len());
        for child in node.children.as_slice() {
            children.push(rewrite_one_sample_delays(arena, *child, memo)?);
        }
        arena.intern(node.kind, &children)
    };

    memo.insert(sig, rewritten);
    Ok(rewritten)
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
