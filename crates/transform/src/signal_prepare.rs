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

use signals::{BinOp, SigBuilder, SigId, SigMatch, dump_sig_readable, match_sig};
use sigtype::{SigType, TypeAnnotator};
use tlib::{
    RecursionError, TreeArena, list_to_vec, match_sym_rec, match_sym_ref, sym_rec, sym_ref,
    tree_to_int, vec_to_list,
};
use ui::UiProgram;

/// Reduced signal type used by the pre-FIR preparation stage.
///
/// This is intentionally smaller than the C++ `sigtyperules` lattice. It keeps
/// only the distinctions required by the reduced `SignalPromotion` subset and
/// by FIR type selection in the fast-lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Reduced signal type domain used by the FIR-preparation pass.
pub enum SimpleSigType {
    /// Integer-valued signal.
    Int,
    /// Real-valued signal.
    Real,
    /// Soundfile handle payload.
    Sound,
}

/// Prepared signal package consumed by the fast-lane FIR lowerer.
///
/// The package owns a private staging arena so preparation passes can rewrite
/// the signal forest without mutating the original parse/eval arena.
#[derive(Debug)]
/// Result of preparing a propagated signal list for FIR lowering.
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

/// Errors returned while preparing signals for FIR lowering.
#[derive(Debug)]
/// Typed failures returned by the signal-preparation pass.
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
    let typed_before_promotion = infer_simple_types(&arena, &outputs)?;
    let outputs = promote_signals_for_fir(&mut arena, &outputs, &typed_before_promotion)?;
    let types = infer_simple_types(&arena, &outputs)?;
    // Full type annotation with interval bounds via the sigtype system.
    let sig_types = infer_full_types(&arena, &outputs, ui)?;
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

/// Runs the reduced simple typer on the prepared output forest.
///
/// This pass is executed twice by [`prepare_signals_for_fir`]:
/// - before promotion, to drive cast insertion,
/// - after promotion, to expose final prepared types to the FIR lowerer.
fn infer_simple_types(
    arena: &TreeArena,
    outputs: &[SigId],
) -> Result<HashMap<SigId, SimpleSigType>, SignalPrepareError> {
    let mut typer = SimpleTyper::new(arena);
    for output in outputs {
        typer.infer_sig(*output)?;
    }
    typer.resolve_unknowns(outputs)?;
    Ok(typer.finish())
}

/// Applies the reduced promotion pass to all prepared outputs.
///
/// Promotion preserves graph sharing through memoization inside
/// [`SignalPromoter`], so repeated subtrees stay interned once in the staging
/// arena.
fn promote_signals_for_fir(
    arena: &mut TreeArena,
    outputs: &[SigId],
    types: &HashMap<SigId, SimpleSigType>,
) -> Result<Vec<SigId>, SignalPrepareError> {
    let mut promoter = SignalPromoter::new(arena, types);
    outputs
        .iter()
        .map(|output| promoter.promote(*output))
        .collect()
}

/// Internal fixpoint slot used while inferring reduced signal types.
///
/// `Unknown` only exists during recursive-group convergence. Final exported
/// types always map unresolved slots to [`SimpleSigType::Real`], matching the
/// current fast-lane fallback policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// One slot in the recursive-group fixpoint lattice.
enum TypeSlot {
    Unknown,
    Int,
    Real,
    Sound,
}

impl TypeSlot {
    /// Converts one internal slot to the public reduced type domain.
    fn finalize(self) -> SimpleSigType {
        match self {
            Self::Unknown => SimpleSigType::Real,
            Self::Int => SimpleSigType::Int,
            Self::Real => SimpleSigType::Real,
            Self::Sound => SimpleSigType::Sound,
        }
    }
}

/// Reduced signal typer run after forest-wide symbolic recursion conversion.
///
/// This is intentionally smaller than the C++ `sigtyperules` engine. It only
/// tracks the distinctions currently required by:
/// - delay and prefix state typing,
/// - recursion-group convergence,
/// - table element/index typing,
/// - the reduced promotion rules applied before FIR lowering.
///
/// The typer memoizes:
/// - one slot per visited node,
/// - one vector of slots per symbolic recursion group,
/// - one temporary active-group state while computing recursive fixpoints.
struct SimpleTyper<'a> {
    arena: &'a TreeArena,
    node_types: HashMap<SigId, TypeSlot>,
    group_types: HashMap<SigId, Vec<TypeSlot>>,
    active_groups: HashMap<SigId, Vec<TypeSlot>>,
}

impl<'a> SimpleTyper<'a> {
    /// Creates one reduced typer over the prepared staging arena.
    fn new(arena: &'a TreeArena) -> Self {
        Self {
            arena,
            node_types: HashMap::new(),
            group_types: HashMap::new(),
            active_groups: HashMap::new(),
        }
    }

    /// Finalizes the memoized slot map into exported reduced types.
    fn finish(self) -> HashMap<SigId, SimpleSigType> {
        self.node_types
            .into_iter()
            .map(|(id, ty)| (id, ty.finalize()))
            .collect()
    }

    /// Infers one reduced type slot for one signal node.
    ///
    /// Most cases mirror the type intent of the C++ signal rules, but in the
    /// reduced domain `Int / Real / Sound`. Unsupported or malformed nodes are
    /// reported as typed preparation errors so the fast-lane fails explicitly.
    fn infer_sig(&mut self, sig: SigId) -> Result<TypeSlot, SignalPrepareError> {
        if let Some(ty) = self.node_types.get(&sig)
            && *ty != TypeSlot::Unknown
        {
            return Ok(*ty);
        }
        let ty = match match_sig(self.arena, sig) {
            SigMatch::Unknown => {
                return Err(SignalPrepareError::Typing(format!(
                    "unsupported signal node {} during simple typing",
                    sig.as_u32()
                )));
            }
            SigMatch::Int(_) => TypeSlot::Int,
            SigMatch::Real(_) => TypeSlot::Real,
            SigMatch::Input(_) => TypeSlot::Real,
            SigMatch::Output(_, inner) => self.infer_sig(inner)?,
            SigMatch::Delay1(value) => {
                let value_ty = self.infer_sig(value)?;
                self.sample_type(value_ty, "SIGDELAY1")?
            }
            SigMatch::Delay(value, amount) => {
                let value_ty = self.infer_sig(value)?;
                let _ = self.infer_sig(amount)?;
                self.sample_type(value_ty, "SIGDELAY")?
            }
            SigMatch::Prefix(init, value) => {
                let init_ty = self.infer_sig(init)?;
                let value_ty = self.infer_sig(value)?;
                self.unify_slots(
                    self.sample_type(init_ty, "SIGPREFIX")?,
                    self.sample_type(value_ty, "SIGPREFIX")?,
                    "SIGPREFIX",
                )?
            }
            SigMatch::IntCast(inner) | SigMatch::BitCast(inner) => {
                let _ = self.infer_sig(inner)?;
                TypeSlot::Int
            }
            SigMatch::FloatCast(inner) => {
                let _ = self.infer_sig(inner)?;
                TypeSlot::Real
            }
            SigMatch::Gen(inner) => self.infer_sig(inner)?,
            SigMatch::RdTbl(table, index) => {
                let table_ty = self.infer_sig(table)?;
                let _ = self.infer_sig(index)?;
                self.numeric_type(table_ty, "SIGRDTBL table")?
            }
            SigMatch::WrTbl(size, generator, write_index, write_signal) => {
                let _ = self.infer_sig(size)?;
                let generator_ty = self.infer_sig(generator)?;
                let gen_ty = self.numeric_type(generator_ty, "SIGWRTBL generator")?;
                if self.arena.is_nil(write_index) && self.arena.is_nil(write_signal) {
                    gen_ty
                } else {
                    let _ = self.infer_sig(write_index)?;
                    let write_signal_ty = self.infer_sig(write_signal)?;
                    let write_ty = self.numeric_type(write_signal_ty, "SIGWRTBL write signal")?;
                    self.unify_slots(gen_ty, write_ty, "SIGWRTBL")?
                }
            }
            SigMatch::Select2(selector, then_value, else_value) => {
                let _ = self.infer_sig(selector)?;
                let then_ty = self.infer_sig(then_value)?;
                let else_ty = self.infer_sig(else_value)?;
                self.unify_slots(
                    self.numeric_type(then_ty, "SIGSELECT2 then branch")?,
                    self.numeric_type(else_ty, "SIGSELECT2 else branch")?,
                    "SIGSELECT2",
                )?
            }
            SigMatch::AssertBounds(_, _, current) => self.infer_sig(current)?,
            SigMatch::Lowest(_) | SigMatch::Highest(_) => TypeSlot::Real,
            SigMatch::BinOp(op, left, right) => {
                let left_ty = self.infer_sig(left)?;
                let right_ty = self.infer_sig(right)?;
                let left_ty = self.numeric_type(left_ty, "SIGBINOP left operand")?;
                let right_ty = self.numeric_type(right_ty, "SIGBINOP right operand")?;
                match op {
                    BinOp::Div => TypeSlot::Real,
                    BinOp::Gt | BinOp::Lt | BinOp::Ge | BinOp::Le | BinOp::Eq | BinOp::Ne => {
                        TypeSlot::Int
                    }
                    BinOp::Lsh
                    | BinOp::ARsh
                    | BinOp::LRsh
                    | BinOp::And
                    | BinOp::Or
                    | BinOp::Xor => TypeSlot::Int,
                    BinOp::Rem => self.unify_slots(left_ty, right_ty, "SIGBINOP remainder")?,
                    BinOp::Add | BinOp::Sub | BinOp::Mul => {
                        self.unify_slots(left_ty, right_ty, "SIGBINOP arithmetic")?
                    }
                }
            }
            SigMatch::Pow(_, _)
            | SigMatch::Acos(_)
            | SigMatch::Asin(_)
            | SigMatch::Atan(_)
            | SigMatch::Atan2(_, _)
            | SigMatch::Cos(_)
            | SigMatch::Sin(_)
            | SigMatch::Tan(_)
            | SigMatch::Exp(_)
            | SigMatch::Log(_)
            | SigMatch::Log10(_)
            | SigMatch::Sqrt(_)
            | SigMatch::Fmod(_, _)
            | SigMatch::Remainder(_, _)
            | SigMatch::Floor(_)
            | SigMatch::Ceil(_)
            | SigMatch::Rint(_)
            | SigMatch::Round(_) => {
                self.visit_children(sig)?;
                TypeSlot::Real
            }
            SigMatch::Abs(inner) => {
                let inner_ty = self.infer_sig(inner)?;
                self.numeric_type(inner_ty, "SIGABS operand")?
            }
            SigMatch::Min(left, right) | SigMatch::Max(left, right) => {
                let left_ty = self.infer_sig(left)?;
                let right_ty = self.infer_sig(right)?;
                self.unify_slots(
                    self.numeric_type(left_ty, "SIGMINMAX left operand")?,
                    self.numeric_type(right_ty, "SIGMINMAX right operand")?,
                    "SIGMINMAX",
                )?
            }
            SigMatch::FFun(_, largs) => {
                self.visit_list_like_children(largs)?;
                TypeSlot::Real
            }
            SigMatch::FConst(ty, _, _) | SigMatch::FVar(ty, _, _) => self.foreign_type(ty),
            SigMatch::Proj(index, group) => self.infer_proj(index, group)?,
            SigMatch::Rec(body) => self.infer_sig(body)?,
            SigMatch::Button(_) | SigMatch::Checkbox(_) => TypeSlot::Real,
            SigMatch::VSlider(_) | SigMatch::HSlider(_) | SigMatch::NumEntry(_) => TypeSlot::Real,
            SigMatch::VBargraph(_, value) | SigMatch::HBargraph(_, value) => {
                self.infer_sig(value)?
            }
            SigMatch::Attach(left, right)
            | SigMatch::Enable(left, right)
            | SigMatch::Control(left, right) => {
                let left_ty = self.infer_sig(left)?;
                let _ = self.infer_sig(right)?;
                left_ty
            }
            SigMatch::Waveform(values) => self.infer_waveform(values)?,
            SigMatch::Soundfile(_) => TypeSlot::Sound,
            SigMatch::SoundfileLength(soundfile, part)
            | SigMatch::SoundfileRate(soundfile, part) => {
                let _ = self.infer_sig(soundfile)?;
                let _ = self.infer_sig(part)?;
                TypeSlot::Int
            }
            SigMatch::SoundfileBuffer(soundfile, chan, part, index) => {
                let _ = self.infer_sig(soundfile)?;
                let _ = self.infer_sig(chan)?;
                let _ = self.infer_sig(part)?;
                let _ = self.infer_sig(index)?;
                TypeSlot::Real
            }
            SigMatch::TempVar(value) => self.infer_sig(value)?,
            SigMatch::PermVar(value) => {
                let value_ty = self.infer_sig(value)?;
                self.sample_type(value_ty, "SIGPERMVAR")?
            }
            SigMatch::Seq(left, right) => {
                let _ = self.infer_sig(left)?;
                self.infer_sig(right)?
            }
            SigMatch::ZeroPad(value, amount) => {
                let value_ty = self.infer_sig(value)?;
                let _ = self.infer_sig(amount)?;
                self.sample_type(value_ty, "SIGZEROPAD")?
            }
            SigMatch::OnDemand(items)
            | SigMatch::Upsampling(items)
            | SigMatch::Downsampling(items) => {
                for item in items {
                    let _ = self.infer_sig(*item)?;
                }
                TypeSlot::Real
            }
            SigMatch::Clocked(clock, value) => {
                let _ = self.infer_sig(clock)?;
                self.infer_sig(value)?
            }
        };
        self.node_types.insert(sig, ty);
        Ok(ty)
    }

    /// Re-runs inference for nodes left as `Unknown` after recursion-group
    /// fixpoint convergence so exported reduced types reflect the finalized
    /// group slots rather than the intermediate approximation.
    fn resolve_unknowns(&mut self, outputs: &[SigId]) -> Result<(), SignalPrepareError> {
        loop {
            let unknown_ids: Vec<SigId> = self
                .node_types
                .iter()
                .filter_map(|(id, ty)| (*ty == TypeSlot::Unknown).then_some(*id))
                .collect();
            if unknown_ids.is_empty() {
                return Ok(());
            }
            for id in &unknown_ids {
                self.node_types.remove(id);
            }
            for id in &unknown_ids {
                self.infer_sig(*id)?;
            }
            for output in outputs {
                self.infer_sig(*output)?;
            }
            let remaining_unknowns = self
                .node_types
                .values()
                .filter(|ty| **ty == TypeSlot::Unknown)
                .count();
            if remaining_unknowns == 0 {
                return Ok(());
            }
            if remaining_unknowns >= unknown_ids.len() {
                return Err(SignalPrepareError::Typing(
                    "recursive reduced typing did not converge after unknown-slot refresh"
                        .to_owned(),
                ));
            }
        }
    }

    /// Visits all children of a generic signal node.
    ///
    /// Tree children that are parser-style lists are traversed structurally
    /// rather than treated as standalone signal nodes.
    fn visit_children(&mut self, sig: SigId) -> Result<(), SignalPrepareError> {
        let node = self.arena.node(sig).ok_or_else(|| {
            SignalPrepareError::Typing(format!(
                "missing node {} during simple typing",
                sig.as_u32()
            ))
        })?;
        for child in node.children.as_slice() {
            if self.arena.is_list(*child) {
                self.visit_list_like_children(*child)?;
            } else {
                let _ = self.infer_sig(*child)?;
            }
        }
        Ok(())
    }

    /// Visits a `cons`/`nil`-encoded child list used by some signal payloads.
    fn visit_list_like_children(&mut self, mut list: SigId) -> Result<(), SignalPrepareError> {
        while !self.arena.is_nil(list) {
            let head = self.arena.hd(list).ok_or_else(|| {
                SignalPrepareError::Typing("malformed list payload during simple typing".to_owned())
            })?;
            let _ = self.infer_sig(head)?;
            list = self.arena.tl(list).ok_or_else(|| {
                SignalPrepareError::Typing("malformed list payload during simple typing".to_owned())
            })?;
        }
        Ok(())
    }

    /// Infers the type of one projection out of a symbolic recursion group.
    ///
    /// This method is the key post-`de_bruijn_to_sym` recursion hook used by
    /// the fast-lane. It accepts:
    /// - `proj(i, sym_ref(var))` during active group inference,
    /// - `proj(i, sym_rec(...))` when visiting a fully materialized group,
    /// - `SIGREC` as a compatibility fallback for remaining pre-symbolic users.
    fn infer_proj(&mut self, index: i32, group: SigId) -> Result<TypeSlot, SignalPrepareError> {
        let requested_index = usize::try_from(index).map_err(|_| {
            SignalPrepareError::Typing(format!("negative projection index {index}"))
        })?;
        if let Some(var) = match_sym_ref(self.arena, group) {
            let active_group = self
                .active_groups
                .keys()
                .copied()
                .find(|group_id| {
                    match_sym_rec(self.arena, *group_id)
                        .map(|(group_var, _)| group_var == var)
                        .unwrap_or(false)
                })
                .or_else(|| {
                    self.group_types.keys().copied().find(|group_id| {
                        match_sym_rec(self.arena, *group_id)
                            .map(|(group_var, _)| group_var == var)
                            .unwrap_or(false)
                    })
                })
                .ok_or_else(|| {
                    SignalPrepareError::Typing(format!(
                        "unbound symbolic recursion variable {} during simple typing",
                        var.as_u32()
                    ))
                })?;
            let slots = if let Some(slots) = self.active_groups.get(&active_group) {
                slots
            } else {
                self.group_types.get(&active_group).ok_or_else(|| {
                    SignalPrepareError::Typing(format!(
                        "missing recursion state for group {}",
                        active_group.as_u32()
                    ))
                })?
            };
            let index = if slots.len() == 1 { 0 } else { requested_index };
            return slots.get(index).copied().ok_or_else(|| {
                SignalPrepareError::Typing(format!(
                    "projection index {index} out of bounds for symbolic recursion group {} with {} slots",
                    active_group.as_u32(),
                    slots.len()
                ))
            });
        }

        if match_sym_rec(self.arena, group).is_some() {
            let slots = self.infer_group(group)?;
            let index = if slots.len() == 1 { 0 } else { requested_index };
            return slots.get(index).copied().ok_or_else(|| {
                SignalPrepareError::Typing(format!(
                    "projection index {index} out of bounds for symbolic recursion group {} with {} slots",
                    group.as_u32(),
                    slots.len()
                ))
            });
        }

        if let SigMatch::Rec(body) = match_sig(self.arena, group) {
            if index != 0 {
                return Err(SignalPrepareError::Typing(format!(
                    "projection index {index} unsupported for SIGREC fallback"
                )));
            }
            return self.infer_sig(body);
        }

        Err(SignalPrepareError::Typing(format!(
            "SIGPROJ target {} is neither symbolic recursion nor SIGREC",
            group.as_u32()
        )))
    }

    /// Solves one symbolic recursion group to a reduced vector of slot types.
    ///
    /// The algorithm uses a monotone fixpoint on [`TypeSlot`] vectors:
    /// - start with all outputs as `Unknown`,
    /// - re-infer all bodies under the current approximation,
    /// - merge the old and new vectors,
    /// - stop when the vector stabilizes.
    ///
    /// Remaining `Unknown` slots collapse to `Real`, which is the current
    /// parity-preserving fallback for unconstrained recursive numeric signals.
    fn infer_group(&mut self, group: SigId) -> Result<Vec<TypeSlot>, SignalPrepareError> {
        if let Some(types) = self.group_types.get(&group) {
            return Ok(types.clone());
        }
        let (_var, body_list) = match_sym_rec(self.arena, group).ok_or_else(|| {
            SignalPrepareError::Typing(format!(
                "symbolic recursion group {} expected during simple typing",
                group.as_u32()
            ))
        })?;
        let bodies = list_to_vec(self.arena, body_list).ok_or_else(|| {
            SignalPrepareError::Typing(format!(
                "malformed symbolic recursion body list for group {}",
                group.as_u32()
            ))
        })?;
        let mut current = vec![TypeSlot::Unknown; bodies.len()];

        loop {
            self.active_groups.insert(group, current.clone());
            let mut next = Vec::with_capacity(bodies.len());
            for body in &bodies {
                next.push(self.infer_sig(*body)?);
            }
            let merged = merge_group_slots(&current, &next)?;
            if merged == current {
                let finalized: Vec<TypeSlot> = merged
                    .iter()
                    .copied()
                    .map(|slot| match slot {
                        TypeSlot::Unknown => TypeSlot::Real,
                        other => other,
                    })
                    .collect();
                self.active_groups.remove(&group);
                self.group_types.insert(group, finalized.clone());
                return Ok(finalized);
            }
            current = merged;
        }
    }

    /// Infers the aggregate type of one waveform literal payload.
    fn infer_waveform(&mut self, values: &[SigId]) -> Result<TypeSlot, SignalPrepareError> {
        let mut out = TypeSlot::Int;
        for value in values {
            let value_ty = self.infer_sig(*value)?;
            out = self.unify_slots(
                out,
                self.numeric_type(value_ty, "SIGWAVEFORM element")?,
                "SIGWAVEFORM",
            )?;
        }
        Ok(out)
    }

    /// Decodes the reduced type tag used by foreign constants and variables.
    fn foreign_type(&self, ty: SigId) -> TypeSlot {
        match tree_to_int(self.arena, ty) {
            Some(0) => TypeSlot::Int,
            Some(_) => TypeSlot::Real,
            None => TypeSlot::Real,
        }
    }

    /// Rejects non-numeric payloads in contexts that require arithmetic values.
    fn numeric_type(&self, slot: TypeSlot, context: &str) -> Result<TypeSlot, SignalPrepareError> {
        match slot {
            TypeSlot::Sound => Err(SignalPrepareError::Typing(format!(
                "{context} cannot use a soundfile handle as a numeric signal"
            ))),
            other => Ok(other),
        }
    }

    /// Alias used by stateful sample operators such as delays and prefixes.
    fn sample_type(&self, slot: TypeSlot, context: &str) -> Result<TypeSlot, SignalPrepareError> {
        self.numeric_type(slot, context)
    }

    /// Merges two reduced slots according to the current fast-lane promotion rules.
    ///
    /// Integer/real combinations widen to `Real`. Soundfile handles only unify
    /// with themselves.
    fn unify_slots(
        &self,
        left: TypeSlot,
        right: TypeSlot,
        context: &str,
    ) -> Result<TypeSlot, SignalPrepareError> {
        match (left, right) {
            (TypeSlot::Unknown, other) | (other, TypeSlot::Unknown) => Ok(other),
            (TypeSlot::Int, TypeSlot::Int) => Ok(TypeSlot::Int),
            (TypeSlot::Real, TypeSlot::Real)
            | (TypeSlot::Real, TypeSlot::Int)
            | (TypeSlot::Int, TypeSlot::Real) => Ok(TypeSlot::Real),
            (TypeSlot::Sound, TypeSlot::Sound) => Ok(TypeSlot::Sound),
            _ => Err(SignalPrepareError::Typing(format!(
                "{context} cannot unify {:?} and {:?}",
                left.finalize(),
                right.finalize()
            ))),
        }
    }
}

/// Merges two recursion-group slot vectors during fixpoint iteration.
fn merge_group_slots(
    current: &[TypeSlot],
    next: &[TypeSlot],
) -> Result<Vec<TypeSlot>, SignalPrepareError> {
    if current.len() != next.len() {
        return Err(SignalPrepareError::Typing(
            "recursive group type vector arity mismatch".to_owned(),
        ));
    }
    current
        .iter()
        .copied()
        .zip(next.iter().copied())
        .map(|(left, right)| match (left, right) {
            (TypeSlot::Unknown, other) | (other, TypeSlot::Unknown) => Ok(other),
            (TypeSlot::Int, TypeSlot::Int) => Ok(TypeSlot::Int),
            (TypeSlot::Real, TypeSlot::Real)
            | (TypeSlot::Real, TypeSlot::Int)
            | (TypeSlot::Int, TypeSlot::Real) => Ok(TypeSlot::Real),
            (TypeSlot::Sound, TypeSlot::Sound) => Ok(TypeSlot::Sound),
            _ => Err(SignalPrepareError::Typing(
                "recursive group mixes incompatible simple types".to_owned(),
            )),
        })
        .collect()
}

/// Reduced `SignalPromotion` pass run after simple typing and before FIR lowering.
///
/// This pass mirrors the subset of C++ `sigPromotion.cpp` currently required by
/// the fast-lane:
/// - delay amounts, table indices, and clock selectors are forced to integer,
/// - mixed arithmetic operands are widened to real when needed,
/// - branch/table/write operands are cast to compatible element types,
/// - clocked wrappers preserve their clock/value split when casts are inserted.
///
/// The pass is memoized so shared prepared subtrees remain shared in the staged
/// arena.
struct SignalPromoter<'a> {
    arena: &'a mut TreeArena,
    types: &'a HashMap<SigId, SimpleSigType>,
    memo: HashMap<SigId, SigId>,
}

impl<'a> SignalPromoter<'a> {
    /// Creates one promotion pass over the prepared staging arena.
    fn new(arena: &'a mut TreeArena, types: &'a HashMap<SigId, SimpleSigType>) -> Self {
        Self {
            arena,
            types,
            memo: HashMap::new(),
        }
    }

    /// Promotes one signal tree or list node, preserving structural sharing.
    fn promote(&mut self, sig: SigId) -> Result<SigId, SignalPrepareError> {
        if let Some(promoted) = self.memo.get(&sig) {
            return Ok(*promoted);
        }

        let promoted = if self.arena.is_nil(sig) {
            sig
        } else if self.arena.is_list(sig) {
            let head = self.arena.hd(sig).ok_or_else(|| {
                SignalPrepareError::Typing("malformed list during signal promotion".to_owned())
            })?;
            let tail = self.arena.tl(sig).ok_or_else(|| {
                SignalPrepareError::Typing("malformed list during signal promotion".to_owned())
            })?;
            let promoted_head = self.promote(head)?;
            let promoted_tail = self.promote(tail)?;
            self.arena.cons(promoted_head, promoted_tail)
        } else if let Some((var, body_list)) = match_sym_rec(self.arena, sig) {
            let promoted_body = self.promote(body_list)?;
            sym_rec(self.arena, var, promoted_body)
        } else if let Some(var) = match_sym_ref(self.arena, sig) {
            sym_ref(self.arena, var)
        } else {
            self.promote_signal(sig)?
        };

        self.memo.insert(sig, promoted);
        Ok(promoted)
    }

    /// Promotes one non-list, non-recursion signal node.
    fn promote_signal(&mut self, sig: SigId) -> Result<SigId, SignalPrepareError> {
        let promoted = match match_sig(self.arena, sig) {
            SigMatch::Unknown => self.clone_generic(sig)?,
            SigMatch::Int(_)
            | SigMatch::Real(_)
            | SigMatch::Input(_)
            | SigMatch::Button(_)
            | SigMatch::Checkbox(_) => sig,
            SigMatch::Output(index, inner) => {
                let inner = self.promote(inner)?;
                let mut b = SigBuilder::new(self.arena);
                b.output(index, inner)
            }
            SigMatch::Delay1(value) => {
                let value = self.promote(value)?;
                let mut b = SigBuilder::new(self.arena);
                b.delay1(value)
            }
            SigMatch::Delay(value, amount) => {
                let value = self.promote(value)?;
                let amount_promoted = self.promote(amount)?;
                let amount_promoted = self.smart_int_cast(amount, amount_promoted)?;
                let mut b = SigBuilder::new(self.arena);
                b.delay(value, amount_promoted)
            }
            SigMatch::Prefix(init, value) => {
                let init_promoted = self.promote(init)?;
                let value_promoted = self.promote(value)?;
                let (init_promoted, value_promoted) = if self.same_type(init, value) {
                    (init_promoted, value_promoted)
                } else {
                    (
                        self.smart_float_cast(init, init_promoted)?,
                        self.smart_float_cast(value, value_promoted)?,
                    )
                };
                let mut b = SigBuilder::new(self.arena);
                b.prefix(init_promoted, value_promoted)
            }
            SigMatch::IntCast(inner) => {
                let inner_promoted = self.promote(inner)?;
                self.smart_int_cast(inner, inner_promoted)?
            }
            SigMatch::BitCast(inner) => {
                let inner = self.promote(inner)?;
                let mut b = SigBuilder::new(self.arena);
                b.bit_cast(inner)
            }
            SigMatch::FloatCast(inner) => {
                let inner_promoted = self.promote(inner)?;
                self.smart_float_cast(inner, inner_promoted)?
            }
            SigMatch::Gen(inner) => {
                let inner = self.promote(inner)?;
                let mut b = SigBuilder::new(self.arena);
                b.generate(inner)
            }
            SigMatch::RdTbl(table, index) => {
                let table = self.promote(table)?;
                let index_promoted = self.promote(index)?;
                let index_promoted = self.smart_int_cast(index, index_promoted)?;
                let mut b = SigBuilder::new(self.arena);
                b.rdtbl(table, index_promoted)
            }
            SigMatch::WrTbl(size, generator, write_index, write_signal) => {
                let size = self.promote(size)?;
                let generator_promoted = self.promote(generator)?;
                if self.arena.is_nil(write_index) && self.arena.is_nil(write_signal) {
                    let mut b = SigBuilder::new(self.arena);
                    b.wrtbl_readonly(size, generator_promoted)
                } else {
                    let write_index_promoted = self.promote(write_index)?;
                    let write_index_promoted =
                        self.smart_int_cast(write_index, write_index_promoted)?;
                    let write_signal_promoted = self.promote(write_signal)?;
                    let write_signal_promoted =
                        self.smart_cast(generator, write_signal, write_signal_promoted)?;
                    let mut b = SigBuilder::new(self.arena);
                    b.wrtbl(
                        size,
                        generator_promoted,
                        write_index_promoted,
                        write_signal_promoted,
                    )
                }
            }
            SigMatch::Select2(selector, then_value, else_value) => {
                let selector_promoted = self.promote(selector)?;
                let selector_promoted = self.smart_int_cast(selector, selector_promoted)?;
                let then_promoted = self.promote(then_value)?;
                let else_promoted = self.promote(else_value)?;
                let (then_promoted, else_promoted) = if self.same_type(then_value, else_value) {
                    (then_promoted, else_promoted)
                } else {
                    (
                        self.smart_float_cast(then_value, then_promoted)?,
                        self.smart_float_cast(else_value, else_promoted)?,
                    )
                };
                let mut b = SigBuilder::new(self.arena);
                b.select2(selector_promoted, then_promoted, else_promoted)
            }
            SigMatch::AssertBounds(min, max, current) => {
                let min = self.promote(min)?;
                let max = self.promote(max)?;
                let current = self.promote(current)?;
                let mut b = SigBuilder::new(self.arena);
                b.assert_bounds(min, max, current)
            }
            SigMatch::Lowest(inner) => {
                let inner = self.promote(inner)?;
                let mut b = SigBuilder::new(self.arena);
                b.lowest(inner)
            }
            SigMatch::Highest(inner) => {
                let inner = self.promote(inner)?;
                let mut b = SigBuilder::new(self.arena);
                b.highest(inner)
            }
            SigMatch::BinOp(op, left, right) => self.promote_binop(sig, op, left, right)?,
            SigMatch::Pow(left, right) => {
                self.promote_real_binary(|b, l, r| b.pow(l, r), left, right)?
            }
            SigMatch::Min(left, right) => {
                self.promote_minmax(|b, l, r| b.min(l, r), left, right)?
            }
            SigMatch::Max(left, right) => {
                self.promote_minmax(|b, l, r| b.max(l, r), left, right)?
            }
            SigMatch::Acos(inner) => self.promote_real_unary(|b, x| b.acos(x), inner)?,
            SigMatch::Asin(inner) => self.promote_real_unary(|b, x| b.asin(x), inner)?,
            SigMatch::Atan(inner) => self.promote_real_unary(|b, x| b.atan(x), inner)?,
            SigMatch::Atan2(left, right) => {
                self.promote_real_binary(|b, l, r| b.atan2(l, r), left, right)?
            }
            SigMatch::Cos(inner) => self.promote_real_unary(|b, x| b.cos(x), inner)?,
            SigMatch::Sin(inner) => self.promote_real_unary(|b, x| b.sin(x), inner)?,
            SigMatch::Tan(inner) => self.promote_real_unary(|b, x| b.tan(x), inner)?,
            SigMatch::Exp(inner) => self.promote_real_unary(|b, x| b.exp(x), inner)?,
            SigMatch::Log(inner) => self.promote_real_unary(|b, x| b.log(x), inner)?,
            SigMatch::Log10(inner) => self.promote_real_unary(|b, x| b.log10(x), inner)?,
            SigMatch::Sqrt(inner) => self.promote_real_unary(|b, x| b.sqrt(x), inner)?,
            SigMatch::Abs(inner) => self.promote_abs(inner)?,
            SigMatch::Fmod(left, right) => {
                self.promote_real_binary(|b, l, r| b.fmod(l, r), left, right)?
            }
            SigMatch::Remainder(left, right) => {
                self.promote_real_binary(|b, l, r| b.remainder(l, r), left, right)?
            }
            SigMatch::Floor(inner) => self.promote_real_unary(|b, x| b.floor(x), inner)?,
            SigMatch::Ceil(inner) => self.promote_real_unary(|b, x| b.ceil(x), inner)?,
            SigMatch::Rint(inner) => self.promote_real_unary(|b, x| b.rint(x), inner)?,
            SigMatch::Round(inner) => self.promote_real_unary(|b, x| b.round(x), inner)?,
            SigMatch::FFun(ff, largs) => {
                let largs = self.promote(largs)?;
                let mut b = SigBuilder::new(self.arena);
                b.ffun(ff, largs)
            }
            SigMatch::FConst(ty, name, file) => {
                let mut b = SigBuilder::new(self.arena);
                b.fconst(ty, name, file)
            }
            SigMatch::FVar(ty, name, file) => {
                let mut b = SigBuilder::new(self.arena);
                b.fvar(ty, name, file)
            }
            SigMatch::Proj(index, group) => {
                let group = self.promote(group)?;
                let mut b = SigBuilder::new(self.arena);
                b.proj(index, group)
            }
            SigMatch::Rec(body) => {
                let body = self.promote(body)?;
                let mut b = SigBuilder::new(self.arena);
                b.rec(body)
            }
            SigMatch::VSlider(control) => {
                let mut b = SigBuilder::new(self.arena);
                b.vslider(control)
            }
            SigMatch::HSlider(control) => {
                let mut b = SigBuilder::new(self.arena);
                b.hslider(control)
            }
            SigMatch::NumEntry(control) => {
                let mut b = SigBuilder::new(self.arena);
                b.numentry(control)
            }
            SigMatch::VBargraph(control, value) => {
                let value_promoted = self.promote(value)?;
                let value_promoted = self.smart_float_cast(value, value_promoted)?;
                let mut b = SigBuilder::new(self.arena);
                b.vbargraph(control, value_promoted)
            }
            SigMatch::HBargraph(control, value) => {
                let value_promoted = self.promote(value)?;
                let value_promoted = self.smart_float_cast(value, value_promoted)?;
                let mut b = SigBuilder::new(self.arena);
                b.hbargraph(control, value_promoted)
            }
            SigMatch::Attach(left, right) => {
                let left = self.promote(left)?;
                let right = self.promote(right)?;
                let mut b = SigBuilder::new(self.arena);
                b.attach(left, right)
            }
            SigMatch::Enable(left, right) => {
                let left = self.promote(left)?;
                let right = self.promote(right)?;
                let mut b = SigBuilder::new(self.arena);
                b.enable(left, right)
            }
            SigMatch::Control(left, right) => {
                let left = self.promote(left)?;
                let right = self.promote(right)?;
                let mut b = SigBuilder::new(self.arena);
                b.control(left, right)
            }
            SigMatch::Waveform(values) => {
                let values = values.to_vec();
                self.promote_waveform(&values)?
            }
            SigMatch::Soundfile(control) => {
                let mut b = SigBuilder::new(self.arena);
                b.soundfile(control)
            }
            SigMatch::SoundfileLength(soundfile, part) => {
                let soundfile = self.promote(soundfile)?;
                let part_promoted = self.promote(part)?;
                let part_promoted = self.smart_int_cast(part, part_promoted)?;
                let mut b = SigBuilder::new(self.arena);
                b.soundfile_length(soundfile, part_promoted)
            }
            SigMatch::SoundfileRate(soundfile, part) => {
                let soundfile = self.promote(soundfile)?;
                let part_promoted = self.promote(part)?;
                let part_promoted = self.smart_int_cast(part, part_promoted)?;
                let mut b = SigBuilder::new(self.arena);
                b.soundfile_rate(soundfile, part_promoted)
            }
            SigMatch::SoundfileBuffer(soundfile, chan, part, index) => {
                let soundfile = self.promote(soundfile)?;
                let chan = self.promote(chan)?;
                let part_promoted = self.promote(part)?;
                let part_promoted = self.smart_int_cast(part, part_promoted)?;
                let index_promoted = self.promote(index)?;
                let index_promoted = self.smart_int_cast(index, index_promoted)?;
                let mut b = SigBuilder::new(self.arena);
                b.soundfile_buffer(soundfile, chan, part_promoted, index_promoted)
            }
            SigMatch::TempVar(value) => {
                let value = self.promote(value)?;
                let mut b = SigBuilder::new(self.arena);
                b.temp_var(value)
            }
            SigMatch::PermVar(value) => {
                let value = self.promote(value)?;
                let mut b = SigBuilder::new(self.arena);
                b.perm_var(value)
            }
            SigMatch::Seq(left, right) => {
                let left = self.promote(left)?;
                let right = self.promote(right)?;
                let mut b = SigBuilder::new(self.arena);
                b.seq(left, right)
            }
            SigMatch::ZeroPad(value, amount) => {
                let value = self.promote(value)?;
                let amount_promoted = self.promote(amount)?;
                let amount_promoted = self.smart_int_cast(amount, amount_promoted)?;
                let mut b = SigBuilder::new(self.arena);
                b.zero_pad(value, amount_promoted)
            }
            SigMatch::OnDemand(items) => {
                let items = items.to_vec();
                self.promote_clocked_family(&items, |b, items| b.on_demand(items))?
            }
            SigMatch::Upsampling(items) => {
                let items = items.to_vec();
                self.promote_clocked_family(&items, |b, items| b.upsampling(items))?
            }
            SigMatch::Downsampling(items) => {
                let items = items.to_vec();
                self.promote_clocked_family(&items, |b, items| b.downsampling(items))?
            }
            SigMatch::Clocked(clock_env, value) => {
                let clock_env = self.promote(clock_env)?;
                let value = self.promote(value)?;
                let mut b = SigBuilder::new(self.arena);
                b.clocked(clock_env, value)
            }
        };
        Ok(promoted)
    }

    /// Applies the reduced promotion policy for binary operators.
    fn promote_binop(
        &mut self,
        node: SigId,
        op: BinOp,
        left: SigId,
        right: SigId,
    ) -> Result<SigId, SignalPrepareError> {
        let left_promoted = self.promote(left)?;
        let right_promoted = self.promote(right)?;
        let out = match op {
            BinOp::Add
            | BinOp::Sub
            | BinOp::Mul
            | BinOp::Gt
            | BinOp::Lt
            | BinOp::Ge
            | BinOp::Le
            | BinOp::Eq
            | BinOp::Ne => {
                if self.same_type(left, right) {
                    let mut b = SigBuilder::new(self.arena);
                    b.binop(op, left_promoted, right_promoted)
                } else {
                    let left_promoted = self.smart_float_cast(left, left_promoted)?;
                    let right_promoted = self.smart_float_cast(right, right_promoted)?;
                    let mut b = SigBuilder::new(self.arena);
                    b.binop(op, left_promoted, right_promoted)
                }
            }
            BinOp::Rem => {
                if self.same_type(left, right)
                    && self.ty(left)? == SimpleSigType::Int
                    && self.ty(right)? == SimpleSigType::Int
                {
                    let mut b = SigBuilder::new(self.arena);
                    b.binop(op, left_promoted, right_promoted)
                } else {
                    let left_promoted = self.smart_float_cast(left, left_promoted)?;
                    let right_promoted = self.smart_float_cast(right, right_promoted)?;
                    let mut b = SigBuilder::new(self.arena);
                    b.fmod(left_promoted, right_promoted)
                }
            }
            BinOp::Div => {
                let left_promoted = self.smart_float_cast(left, left_promoted)?;
                let right_promoted = self.smart_float_cast(right, right_promoted)?;
                let mut b = SigBuilder::new(self.arena);
                b.binop(op, left_promoted, right_promoted)
            }
            BinOp::And | BinOp::Or | BinOp::Xor | BinOp::Lsh | BinOp::ARsh | BinOp::LRsh => {
                let left_promoted = self.smart_int_cast(left, left_promoted)?;
                let right_promoted = self.smart_int_cast(right, right_promoted)?;
                let mut b = SigBuilder::new(self.arena);
                b.binop(op, left_promoted, right_promoted)
            }
        };
        self.memo.insert(node, out);
        Ok(out)
    }

    /// Promotes one unary operator that always expects a real operand.
    fn promote_real_unary(
        &mut self,
        build: impl FnOnce(&mut SigBuilder<'_>, SigId) -> SigId,
        inner: SigId,
    ) -> Result<SigId, SignalPrepareError> {
        let inner_promoted = self.promote(inner)?;
        let inner_promoted = self.smart_float_cast(inner, inner_promoted)?;
        let mut b = SigBuilder::new(self.arena);
        Ok(build(&mut b, inner_promoted))
    }

    /// Promotes one binary operator that always expects real operands.
    fn promote_real_binary(
        &mut self,
        build: impl FnOnce(&mut SigBuilder<'_>, SigId, SigId) -> SigId,
        left: SigId,
        right: SigId,
    ) -> Result<SigId, SignalPrepareError> {
        let left_promoted = self.promote(left)?;
        let right_promoted = self.promote(right)?;
        let left_promoted = self.smart_float_cast(left, left_promoted)?;
        let right_promoted = self.smart_float_cast(right, right_promoted)?;
        let mut b = SigBuilder::new(self.arena);
        Ok(build(&mut b, left_promoted, right_promoted))
    }

    /// Promotes `min`/`max`, preserving all-int operands when possible.
    fn promote_minmax(
        &mut self,
        build: impl FnOnce(&mut SigBuilder<'_>, SigId, SigId) -> SigId,
        left: SigId,
        right: SigId,
    ) -> Result<SigId, SignalPrepareError> {
        let left_promoted = self.promote(left)?;
        let right_promoted = self.promote(right)?;
        let (left_promoted, right_promoted) = if self.same_type(left, right) {
            (left_promoted, right_promoted)
        } else {
            (
                self.smart_float_cast(left, left_promoted)?,
                self.smart_float_cast(right, right_promoted)?,
            )
        };
        let mut b = SigBuilder::new(self.arena);
        Ok(build(&mut b, left_promoted, right_promoted))
    }

    /// Promotes `abs`, preserving integer operands when possible.
    fn promote_abs(&mut self, inner: SigId) -> Result<SigId, SignalPrepareError> {
        let inner_promoted = self.promote(inner)?;
        let inner_promoted = if self.ty(inner)? == SimpleSigType::Int {
            inner_promoted
        } else {
            self.smart_float_cast(inner, inner_promoted)?
        };
        let mut b = SigBuilder::new(self.arena);
        Ok(b.abs(inner_promoted))
    }

    /// Promotes one waveform payload, preserving all-int literals when possible.
    fn promote_waveform(&mut self, values: &[SigId]) -> Result<SigId, SignalPrepareError> {
        let all_int = values
            .iter()
            .all(|value| self.ty(*value).is_ok_and(|ty| ty == SimpleSigType::Int));
        let mut promoted = Vec::with_capacity(values.len());
        for value in values {
            let promoted_value = self.promote(*value)?;
            let promoted_value = if all_int {
                promoted_value
            } else {
                self.smart_float_cast_preserving_clocked(*value, promoted_value)?
            };
            promoted.push(promoted_value);
        }
        let mut b = SigBuilder::new(self.arena);
        Ok(b.waveform(&promoted))
    }

    /// Promotes `ondemand` / `upsampling` / `downsampling` payload lists.
    ///
    /// The first item is the clock expression and may therefore require the
    /// dedicated clock cast logic.
    fn promote_clocked_family(
        &mut self,
        items: &[SigId],
        build: impl FnOnce(&mut SigBuilder<'_>, &[SigId]) -> SigId,
    ) -> Result<SigId, SignalPrepareError> {
        let mut promoted = Vec::with_capacity(items.len());
        for (index, item) in items.iter().copied().enumerate() {
            let promoted_item = self.promote(item)?;
            let promoted_item = if index == 0 {
                self.smart_clock_cast(item, promoted_item)?
            } else {
                promoted_item
            };
            promoted.push(promoted_item);
        }
        let mut b = SigBuilder::new(self.arena);
        Ok(build(&mut b, &promoted))
    }

    /// Forces one clock expression to integer while preserving clock wrappers.
    fn smart_clock_cast(
        &mut self,
        original: SigId,
        promoted: SigId,
    ) -> Result<SigId, SignalPrepareError> {
        if self.ty(original)? == SimpleSigType::Int {
            return Ok(promoted);
        }
        if let SigMatch::Clocked(clock_env, clock) = match_sig(self.arena, promoted) {
            let mut b = SigBuilder::new(self.arena);
            let clock = b.int_cast(clock);
            return Ok(b.clocked(clock_env, clock));
        }
        let mut b = SigBuilder::new(self.arena);
        Ok(b.int_cast(promoted))
    }

    /// Inserts an integer cast only when the original reduced type is real.
    fn smart_int_cast(
        &mut self,
        original: SigId,
        promoted: SigId,
    ) -> Result<SigId, SignalPrepareError> {
        if self.ty(original)? == SimpleSigType::Real {
            let mut b = SigBuilder::new(self.arena);
            Ok(b.int_cast(promoted))
        } else {
            Ok(promoted)
        }
    }

    /// Inserts a float cast while preserving clocked wrapper structure.
    fn smart_float_cast(
        &mut self,
        original: SigId,
        promoted: SigId,
    ) -> Result<SigId, SignalPrepareError> {
        self.smart_float_cast_preserving_clocked(original, promoted)
    }

    /// Inserts a float cast, but keeps `clocked(env, value)` as a clocked node.
    fn smart_float_cast_preserving_clocked(
        &mut self,
        original: SigId,
        promoted: SigId,
    ) -> Result<SigId, SignalPrepareError> {
        if self.ty(original)? != SimpleSigType::Int {
            return Ok(promoted);
        }
        if let SigMatch::Clocked(clock_env, value) = match_sig(self.arena, promoted) {
            let mut b = SigBuilder::new(self.arena);
            let value = b.float_cast(value);
            return Ok(b.clocked(clock_env, value));
        }
        let mut b = SigBuilder::new(self.arena);
        Ok(b.float_cast(promoted))
    }

    /// Casts one promoted source signal to the reduced type expected by a target.
    fn smart_cast(
        &mut self,
        target: SigId,
        source: SigId,
        promoted_source: SigId,
    ) -> Result<SigId, SignalPrepareError> {
        let target_ty = self.ty(target)?;
        let source_ty = self.ty(source)?;
        if target_ty == source_ty {
            Ok(promoted_source)
        } else if target_ty == SimpleSigType::Real && source_ty == SimpleSigType::Int {
            self.smart_float_cast(source, promoted_source)
        } else if target_ty == SimpleSigType::Int && source_ty == SimpleSigType::Real {
            self.smart_int_cast(source, promoted_source)
        } else {
            Ok(promoted_source)
        }
    }

    /// Returns whether two source signals share the same reduced prepared type.
    fn same_type(&self, left: SigId, right: SigId) -> bool {
        self.ty(left).ok() == self.ty(right).ok()
    }

    /// Looks up the reduced prepared type of one source signal.
    fn ty(&self, sig: SigId) -> Result<SimpleSigType, SignalPrepareError> {
        self.types.get(&sig).copied().ok_or_else(|| {
            SignalPrepareError::Typing(format!(
                "missing reduced type for signal {} during promotion: {}",
                sig.as_u32(),
                dump_sig_readable(self.arena, sig)
            ))
        })
    }

    /// Generic structural clone fallback for signal nodes without dedicated promotion logic.
    fn clone_generic(&mut self, sig: SigId) -> Result<SigId, SignalPrepareError> {
        let node = self.arena.node(sig).cloned().ok_or_else(|| {
            SignalPrepareError::Typing(format!("missing node {} during promotion", sig.as_u32()))
        })?;
        let mut promoted_children = Vec::with_capacity(node.children.len());
        for child in node.children.as_slice() {
            promoted_children.push(self.promote(*child)?);
        }
        Ok(self.arena.intern(node.kind, &promoted_children))
    }
}

#[cfg(test)]
mod tests {
    use signals::{SigBuilder, SigMatch, match_sig};
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
