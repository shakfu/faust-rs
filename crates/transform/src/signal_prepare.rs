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
//!
//! Reduced typing deliberately stops short of the full C++ type lattice. The
//! goal is only to support the upcoming promotion pass and to feed `signal_fir`
//! with enough information for delay/recursion/table lowering.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;

use signals::{BinOp, SigId, SigMatch, match_sig};
use tlib::{RecursionError, TreeArena, match_sym_rec, match_sym_ref, tree_to_int};

/// Reduced signal type used by the pre-FIR preparation stage.
///
/// This is intentionally smaller than the C++ `sigtyperules` lattice. It keeps
/// only the distinctions required by the reduced `SignalPromotion` subset and
/// by FIR type selection in the fast-lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
pub struct PreparedSignals {
    /// Private staging arena containing the prepared signal forest.
    pub arena: TreeArena,
    /// Prepared output roots interned in [`Self::arena`].
    pub outputs: Vec<SigId>,
    /// Reduced type annotation for prepared signal nodes.
    pub types: HashMap<SigId, SimpleSigType>,
}

impl PreparedSignals {
    /// Returns the reduced prepared type for one signal node, when available.
    #[must_use]
    pub fn ty(&self, sig: SigId) -> Option<SimpleSigType> {
        self.types.get(&sig).copied()
    }
}

/// Errors returned while preparing signals for FIR lowering.
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
/// reduced type per prepared signal node.
///
/// C++ parity note: both `deBruijn2Sym(...)` and the later type/promotion flow
/// conceptually operate on the whole output list, not independently per root.
/// This function mirrors that contract by cloning all outputs through one memo
/// table, converting one list root, and then typing the prepared forest.
pub fn prepare_signals_for_fir(
    src_arena: &TreeArena,
    outputs: &[SigId],
) -> Result<PreparedSignals, SignalPrepareError> {
    let mut arena = TreeArena::new();
    let cloned_outputs = arena.clone_forest_from(src_arena, outputs);
    let cloned_list = vec_to_list(&mut arena, &cloned_outputs);
    let symbolic_list = tlib::de_bruijn_to_sym(&mut arena, cloned_list)?;
    let outputs = list_to_vec(&arena, symbolic_list)
        .expect("prepare_signals_for_fir rebuilds a proper cons list");
    let types = infer_simple_types(&arena, &outputs)?;
    Ok(PreparedSignals {
        arena,
        outputs,
        types,
    })
}

fn infer_simple_types(
    arena: &TreeArena,
    outputs: &[SigId],
) -> Result<HashMap<SigId, SimpleSigType>, SignalPrepareError> {
    let mut typer = SimpleTyper::new(arena);
    for output in outputs {
        typer.infer_sig(*output)?;
    }
    Ok(typer.finish())
}

fn vec_to_list(arena: &mut TreeArena, values: &[SigId]) -> SigId {
    let mut out = arena.nil();
    for value in values.iter().rev() {
        out = arena.cons(*value, out);
    }
    out
}

fn list_to_vec(arena: &TreeArena, mut list: SigId) -> Option<Vec<SigId>> {
    let mut out = Vec::new();
    while !arena.is_nil(list) {
        out.push(arena.hd(list)?);
        list = arena.tl(list)?;
    }
    Some(out)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TypeSlot {
    Unknown,
    Int,
    Real,
    Sound,
}

impl TypeSlot {
    fn finalize(self) -> SimpleSigType {
        match self {
            Self::Unknown => SimpleSigType::Real,
            Self::Int => SimpleSigType::Int,
            Self::Real => SimpleSigType::Real,
            Self::Sound => SimpleSigType::Sound,
        }
    }
}

struct SimpleTyper<'a> {
    arena: &'a TreeArena,
    node_types: HashMap<SigId, TypeSlot>,
    group_types: HashMap<SigId, Vec<TypeSlot>>,
    active_groups: HashMap<SigId, Vec<TypeSlot>>,
    active_vars: HashMap<SigId, SigId>,
}

impl<'a> SimpleTyper<'a> {
    fn new(arena: &'a TreeArena) -> Self {
        Self {
            arena,
            node_types: HashMap::new(),
            group_types: HashMap::new(),
            active_groups: HashMap::new(),
            active_vars: HashMap::new(),
        }
    }

    fn finish(self) -> HashMap<SigId, SimpleSigType> {
        self.node_types
            .into_iter()
            .map(|(id, ty)| (id, ty.finalize()))
            .collect()
    }

    fn infer_sig(&mut self, sig: SigId) -> Result<TypeSlot, SignalPrepareError> {
        if let Some(ty) = self.node_types.get(&sig) {
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
            | SigMatch::Min(_, _)
            | SigMatch::Max(_, _)
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
            | SigMatch::Abs(_)
            | SigMatch::Fmod(_, _)
            | SigMatch::Remainder(_, _)
            | SigMatch::Floor(_)
            | SigMatch::Ceil(_)
            | SigMatch::Rint(_)
            | SigMatch::Round(_) => {
                self.visit_children(sig)?;
                TypeSlot::Real
            }
            SigMatch::FFun(_, largs) => {
                self.visit_list_like_children(largs)?;
                TypeSlot::Real
            }
            SigMatch::FConst(ty, _, _) | SigMatch::FVar(ty, _, _) => self.foreign_type(ty),
            SigMatch::Proj(index, group) => self.infer_proj(index, group)?,
            SigMatch::Rec(body) => self.infer_sig(body)?,
            SigMatch::Button(_) | SigMatch::Checkbox(_) => TypeSlot::Real,
            SigMatch::VSlider(_, init, min, max, step)
            | SigMatch::HSlider(_, init, min, max, step)
            | SigMatch::NumEntry(_, init, min, max, step) => {
                let init_ty = self.infer_sig(init)?;
                let min_ty = self.infer_sig(min)?;
                let max_ty = self.infer_sig(max)?;
                let step_ty = self.infer_sig(step)?;
                let init_ty = self.numeric_type(init_ty, "slider init")?;
                let min_ty = self.numeric_type(min_ty, "slider min")?;
                let max_ty = self.numeric_type(max_ty, "slider max")?;
                let step_ty = self.numeric_type(step_ty, "slider step")?;
                let _ = self.unify_slots(init_ty, min_ty, "slider range")?;
                let _ = self.unify_slots(max_ty, step_ty, "slider range")?;
                TypeSlot::Real
            }
            SigMatch::VBargraph(_, min, max, value) | SigMatch::HBargraph(_, min, max, value) => {
                let _ = self.infer_sig(min)?;
                let _ = self.infer_sig(max)?;
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

    fn infer_proj(&mut self, index: i32, group: SigId) -> Result<TypeSlot, SignalPrepareError> {
        let index = usize::try_from(index).map_err(|_| {
            SignalPrepareError::Typing(format!("negative projection index {index}"))
        })?;
        if let Some(var) = match_sym_ref(self.arena, group) {
            let active_group = self.active_vars.get(&var).copied().ok_or_else(|| {
                SignalPrepareError::Typing(format!(
                    "unbound symbolic recursion variable {} during simple typing",
                    var.as_u32()
                ))
            })?;
            let slots = self.active_groups.get(&active_group).ok_or_else(|| {
                SignalPrepareError::Typing(format!(
                    "missing active recursion state for group {}",
                    active_group.as_u32()
                ))
            })?;
            return slots.get(index).copied().ok_or_else(|| {
                SignalPrepareError::Typing(format!(
                    "projection index {index} out of bounds for symbolic recursion group"
                ))
            });
        }

        if match_sym_rec(self.arena, group).is_some() {
            let slots = self.infer_group(group)?;
            return slots.get(index).copied().ok_or_else(|| {
                SignalPrepareError::Typing(format!(
                    "projection index {index} out of bounds for symbolic recursion group"
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

    fn infer_group(&mut self, group: SigId) -> Result<Vec<TypeSlot>, SignalPrepareError> {
        if let Some(types) = self.group_types.get(&group) {
            return Ok(types.clone());
        }
        let (var, body_list) = match_sym_rec(self.arena, group).ok_or_else(|| {
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
        self.active_vars.insert(var, group);

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
                self.active_vars.remove(&var);
                self.group_types.insert(group, finalized.clone());
                return Ok(finalized);
            }
            current = merged;
        }
    }

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

    fn foreign_type(&self, ty: SigId) -> TypeSlot {
        match tree_to_int(self.arena, ty) {
            Some(0) => TypeSlot::Int,
            Some(_) => TypeSlot::Real,
            None => TypeSlot::Real,
        }
    }

    fn numeric_type(&self, slot: TypeSlot, context: &str) -> Result<TypeSlot, SignalPrepareError> {
        match slot {
            TypeSlot::Sound => Err(SignalPrepareError::Typing(format!(
                "{context} cannot use a soundfile handle as a numeric signal"
            ))),
            other => Ok(other),
        }
    }

    fn sample_type(&self, slot: TypeSlot, context: &str) -> Result<TypeSlot, SignalPrepareError> {
        self.numeric_type(slot, context)
    }

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

        let prepared =
            prepare_signals_for_fir(&arena, &[proj0, proj1]).expect("closed recursion group");

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

        let prepared =
            prepare_signals_for_fir(&arena, &outputs).expect("simple numeric typing should work");

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

        let prepared =
            prepare_signals_for_fir(&arena, &[output]).expect("recursive typing should converge");

        assert_eq!(prepared.ty(prepared.outputs[0]), Some(SimpleSigType::Real));
    }
}
