//! Signal type inference — port of `compiler/signals/sigtyperules.cpp`.
//!
//! # Algorithm
//! Bottom-up structural recursion over the signal forest with two extensions:
//!
//! 1. **Memoisation**: already-inferred types are stored in `env` to handle
//!    DAG sharing without re-computing subtrees.
//!
//! 2. **Recursive fixed-point** for `SymRec`/`Proj` groups: types are
//!    initialised to `make_maximal()` (top of the lattice), then refined
//!    through the body until stabilisation.  Convergence is guaranteed because
//!    the lattice is finite and `join` is monotone.
//!
//! # C++ source
//! `compiler/signals/sigtyperules.cpp` — `inferSigType`, `typeAnnotation`,
//! `updateRecTypes`.

use std::collections::{HashMap, HashSet};

use interval::Interval;
use signals::{BinOp, SigId, SigMatch, match_sig};
use tlib::{TreeArena, match_sym_rec, match_sym_ref};
use ui::{ControlId, ControlKind, UiProgram};

use crate::enums::{Boolean, Computability, Nature, Variability, Vectorability};
use crate::factory::{make_maximal, make_simple, make_table_type, make_tuplet};
use crate::ops::{check_delay_interval, float_cast, int_cast, samp_cast, union_types};
use crate::types::SigType;

/// Typed failures returned by the type inference pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeError(pub String);

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "type error: {}", self.0)
    }
}

impl std::error::Error for TypeError {}

/// Bottom-up signal type annotator.
///
/// Carries memoised results in `env`; recursive group heads are seeded with
/// `make_maximal()` to break cycles before the fixed-point loop refines them.
pub struct TypeAnnotator<'a> {
    arena: &'a TreeArena,
    ui_program: &'a UiProgram,
    /// Memoised type results.
    env: HashMap<SigId, SigType>,
    /// Nodes whose type is currently being computed (cycle guard).
    in_progress: HashSet<SigId>,
}

impl<'a> TypeAnnotator<'a> {
    /// Create a new annotator for the given signal forest.
    #[must_use]
    pub fn new(arena: &'a TreeArena, ui_program: &'a UiProgram) -> Self {
        Self {
            arena,
            ui_program,
            env: HashMap::new(),
            in_progress: HashSet::new(),
        }
    }

    /// Annotate all output roots and return the complete `SigId → SigType` map.
    ///
    /// # C++ source
    /// `typeAnnotation(Tree sig, bool causality)` entry point.
    pub fn annotate(&mut self, outputs: &[SigId]) -> Result<HashMap<SigId, SigType>, TypeError> {
        for &out in outputs {
            self.infer(out)?;
        }
        Ok(self.env.clone())
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Core dispatch
    // ─────────────────────────────────────────────────────────────────────────

    /// Infer the type of one signal node (memoised).
    ///
    /// # C++ source
    /// `Type T(Tree term, Tree env)` / `inferSigType(Tree sig, Tree env)`
    fn infer(&mut self, sig: SigId) -> Result<SigType, TypeError> {
        // Return memoised result if available.
        if let Some(t) = self.env.get(&sig) {
            return Ok(t.clone());
        }
        // If in progress (recursive reference), return the current approximation
        // seeded by the fixed-point loop, or conservative maximal if outside loop.
        if self.in_progress.contains(&sig) {
            return Ok(self.env.get(&sig).cloned().unwrap_or_else(make_maximal));
        }

        self.in_progress.insert(sig);
        let ty = self.infer_inner(sig)?;
        self.in_progress.remove(&sig);
        self.env.insert(sig, ty.clone());
        Ok(ty)
    }

    fn infer_inner(&mut self, sig: SigId) -> Result<SigType, TypeError> {
        match match_sig(self.arena, sig) {
            // ── Literals ────────────────────────────────────────────────────
            SigMatch::Int(n) => Ok(make_simple(
                Nature::Int,
                Variability::Konst,
                Computability::Comp,
                Vectorability::Vect,
                Boolean::Num,
                interval::singleton(f64::from(n)),
            )),

            SigMatch::Real(r) => Ok(make_simple(
                Nature::Real,
                Variability::Konst,
                Computability::Comp,
                Vectorability::Vect,
                Boolean::Num,
                interval::singleton(r),
            )),

            // ── Inputs / Outputs ────────────────────────────────────────────
            // C++: TINPUT = makeSimpleType(kReal, kSamp, kExec, kVect, kNum, interval(-1, 1))
            // Audio inputs carry the normalised audio range [-1, 1], not an
            // uninformative placeholder.  The lsb=-24 matches 24-bit precision.
            SigMatch::Input(_) => Ok(make_simple(
                Nature::Real,
                Variability::Samp,
                Computability::Exec,
                Vectorability::Vect,
                Boolean::Num,
                interval::Interval::new(-1.0, 1.0, -24),
            )),

            SigMatch::Output(_, s) => {
                let st = self.infer(s)?;
                Ok(samp_cast(st))
            }

            // ── Delays ──────────────────────────────────────────────────────
            // C++: castInterval(sampCast(t), itv::reunion(t->getInterval(), interval(0)))
            // The first sample of a delay1 is 0, so the interval must include 0.
            SigMatch::Delay1(x) => {
                let tx = self.infer(x)?;
                let itv = interval::reunion(tx.interval(), interval::singleton(0.0));
                Ok(samp_cast(tx).promote_interval(itv))
            }

            SigMatch::Delay(x, n) => {
                let tn = self.infer(n)?;
                // Validate that the delay amount has a bounded non-negative interval.
                check_delay_interval(&tn).map_err(|e| TypeError(e.0))?;
                let tx = self.infer(x)?;
                // C++: castInterval(sampCast(t1), itv::reunion(t1->getInterval(), interval(0)))
                let itv = interval::reunion(tx.interval(), interval::singleton(0.0));
                Ok(samp_cast(tx).promote_interval(itv))
            }

            SigMatch::Prefix(init, s) => {
                let ti = self.infer(init)?;
                let ts = self.infer(s)?;
                // C++: castInterval(sampCast(t1|t2), reunion(t1, t2))
                // union_types already computes reunion(a.interval(), b.interval()), so this
                // matches the C++ behaviour.
                Ok(samp_cast(union_types(ti, ts)))
            }

            // ── Casts ───────────────────────────────────────────────────────
            SigMatch::IntCast(x) => {
                let tx = self.infer(x)?;
                Ok(int_cast(tx))
            }

            SigMatch::BitCast(x) => {
                let tx = self.infer(x)?;
                Ok(crate::ops::bit_cast(tx))
            }

            SigMatch::FloatCast(x) => {
                let tx = self.infer(x)?;
                Ok(float_cast(tx))
            }

            // ── Binary operators ─────────────────────────────────────────────
            SigMatch::BinOp(op, lhs, rhs) => {
                let tl = self.infer(lhs)?;
                let tr = self.infer(rhs)?;
                Ok(self.infer_binop(op, tl, tr))
            }

            // ── Pow / Min / Max ──────────────────────────────────────────────
            SigMatch::Pow(x, y) => {
                let tx = self.infer(x)?;
                let ty = self.infer(y)?;
                let itv = interval::ops::math::pow(tx.interval(), ty.interval());
                let t = union_types(tx, ty);
                Ok(float_cast(t).promote_interval(itv))
            }

            SigMatch::Min(x, y) => {
                let tx = self.infer(x)?;
                let ty = self.infer(y)?;
                let itv = interval::ops::logic::min(tx.interval(), ty.interval());
                Ok(union_types(tx, ty).promote_interval(itv))
            }

            SigMatch::Max(x, y) => {
                let tx = self.infer(x)?;
                let ty = self.infer(y)?;
                let itv = interval::ops::logic::max(tx.interval(), ty.interval());
                Ok(union_types(tx, ty).promote_interval(itv))
            }

            // ── Unary math (always Real) ─────────────────────────────────────
            SigMatch::Abs(x) => self.infer_unary_math(x, interval::ops::arithmetic::abs),
            SigMatch::Sqrt(x) => self.infer_unary_math(x, interval::ops::math::sqrt),
            SigMatch::Exp(x) => self.infer_unary_math(x, interval::ops::math::exp),
            SigMatch::Log(x) => self.infer_unary_math(x, interval::ops::math::log),
            SigMatch::Log10(x) => self.infer_unary_math(x, interval::ops::math::log10),
            SigMatch::Floor(x) => self.infer_unary_math(x, interval::ops::math::floor),
            SigMatch::Ceil(x) => self.infer_unary_math(x, interval::ops::math::ceil),
            SigMatch::Rint(x) => self.infer_unary_math(x, interval::ops::math::rint),
            SigMatch::Round(x) => self.infer_unary_math(x, interval::ops::math::round),
            SigMatch::Sin(x) => self.infer_unary_math(x, interval::ops::trig::sin),
            SigMatch::Cos(x) => self.infer_unary_math(x, interval::ops::trig::cos),
            SigMatch::Tan(x) => self.infer_unary_math(x, interval::ops::trig::tan),
            SigMatch::Asin(x) => self.infer_unary_math(x, interval::ops::trig::asin),
            SigMatch::Acos(x) => self.infer_unary_math(x, interval::ops::trig::acos),
            SigMatch::Atan(x) => self.infer_unary_math(x, interval::ops::trig::atan),

            SigMatch::Atan2(y, x) => {
                let ty = self.infer(y)?;
                let tx = self.infer(x)?;
                let itv = interval::ops::trig::atan2(ty.interval(), tx.interval());
                let t = samp_cast(union_types(ty, tx));
                Ok(float_cast(t).promote_interval(itv))
            }

            SigMatch::Fmod(x, y) => {
                let tx = self.infer(x)?;
                let ty = self.infer(y)?;
                let itv = interval::ops::arithmetic::mod_interval(tx.interval(), ty.interval());
                let t = samp_cast(union_types(tx, ty));
                Ok(float_cast(t).promote_interval(itv))
            }

            SigMatch::Remainder(x, y) => {
                let tx = self.infer(x)?;
                let ty = self.infer(y)?;
                let t = samp_cast(union_types(tx, ty));
                Ok(float_cast(t))
            }

            // ── Select ───────────────────────────────────────────────────────
            // C++: makeSimpleType(st1|st2 nature, st1|st2|stsel variability,
            //                     st1|st2|stsel computability, ..., reunion(st1, st2) interval)
            // Note: NO samp_cast — if all inputs are Konst, the result can be Konst.
            SigMatch::Select2(cond, s1, s2) => {
                let tc = self.infer(cond)?;
                let t1 = self.infer(s1)?;
                let t2 = self.infer(s2)?;
                let itv = interval::reunion(t1.interval(), t2.interval());
                let t = union_types(t1, t2)
                    .promote_variability(tc.variability())
                    .promote_computability(tc.computability())
                    .promote_vectorability(tc.vectorability());
                Ok(t.promote_interval(itv))
            }

            // ── Tables ───────────────────────────────────────────────────────
            SigMatch::RdTbl(tbl, idx) => {
                let ttbl = self.infer(tbl)?;
                let tidx = self.infer(idx)?;
                infer_read_table(ttbl, tidx)
            }

            SigMatch::WrTbl(size, generator, wi, ws) => {
                let ttbl = self.infer_write_table(size, generator)?;
                let twi = self.infer(wi)?;
                let tws = self.infer(ws)?;
                infer_write_table_type(ttbl, twi, tws)
            }

            SigMatch::Gen(x) => self.infer(x),

            // ── Waveform ─────────────────────────────────────────────────────
            SigMatch::Waveform(elems) => {
                if elems.is_empty() {
                    return Ok(make_simple(
                        Nature::Real,
                        Variability::Konst,
                        Computability::Comp,
                        Vectorability::Vect,
                        Boolean::Num,
                        interval::empty(),
                    ));
                }
                let mut t = self.infer(elems[0])?;
                let mut itv = t.interval();
                for &e in &elems[1..] {
                    let te = self.infer(e)?;
                    itv = interval::reunion(itv, te.interval());
                    t = union_types(t, te);
                }
                Ok(make_table_type(t.promote_interval(itv)))
            }

            // ── UI controls ──────────────────────────────────────────────────
            SigMatch::Button(id) => Ok(self.infer_button(id)),
            SigMatch::Checkbox(id) => Ok(self.infer_checkbox(id)),
            SigMatch::VSlider(id) => Ok(self.infer_slider(id)),
            SigMatch::HSlider(id) => Ok(self.infer_slider(id)),
            SigMatch::NumEntry(id) => Ok(self.infer_slider(id)),
            SigMatch::VBargraph(id, x) => Ok(self.infer_bargraph(id, x)?),
            SigMatch::HBargraph(id, x) => Ok(self.infer_bargraph(id, x)?),

            // ── Soundfile ────────────────────────────────────────────────────
            // C++: makeSimpleType(kInt, kBlock, kInit, kVect, kNum, interval(0, INT32_MAX))
            SigMatch::Soundfile(_) => Ok(make_simple(
                Nature::Int,
                Variability::Block,
                Computability::Init,
                Vectorability::Vect,
                Boolean::Num,
                interval::Interval::new(0.0, i32::MAX as f64, 0),
            )),
            // C++: makeSimpleType(kInt, max(kBlock, t2->variability()), kInit, kVect, kNum, interval(0, INT32_MAX))
            SigMatch::SoundfileLength(sf, part) => {
                self.infer(sf)?;
                let t_part = self.infer(part)?;
                let var = Variability::Block.join(t_part.variability());
                Ok(make_simple(
                    Nature::Int,
                    var,
                    Computability::Init,
                    Vectorability::Vect,
                    Boolean::Num,
                    interval::Interval::new(0.0, i32::MAX as f64, 0),
                ))
            }
            SigMatch::SoundfileRate(sf, part) => {
                self.infer(sf)?;
                let t_part = self.infer(part)?;
                let var = Variability::Block.join(t_part.variability());
                Ok(make_simple(
                    Nature::Int,
                    var,
                    Computability::Init,
                    Vectorability::Vect,
                    Boolean::Num,
                    interval::Interval::new(0.0, i32::MAX as f64, 0),
                ))
            }
            // C++: makeSimpleType(kReal, kSamp, kInit, kVect, kNum, interval(-1, 1))
            SigMatch::SoundfileBuffer(sf, x, part, z) => {
                self.infer(sf)?;
                self.infer(x)?;
                let t_part = self.infer(part)?;
                self.infer(z)?;
                let _ = t_part;
                Ok(make_simple(
                    Nature::Real,
                    Variability::Samp,
                    Computability::Init,
                    Vectorability::Vect,
                    Boolean::Num,
                    interval::Interval::new(-1.0, 1.0, -24),
                ))
            }

            // ── Attach / Enable / Control ────────────────────────────────────
            SigMatch::Attach(x, _y) => self.infer(x),
            SigMatch::Enable(x, _y) => self.infer(x),
            SigMatch::Control(x, _y) => self.infer(x),

            // ── Recursion ────────────────────────────────────────────────────
            // Symbolic recursion (after de_bruijn_to_sym).
            _ if match_sym_rec(self.arena, sig).is_some() => self.infer_sym_rec(sig),
            _ if match_sym_ref(self.arena, sig).is_some() => {
                // Reference to the recursive variable — return current approx.
                Ok(self.env.get(&sig).cloned().unwrap_or_else(make_maximal))
            }

            // `Rec` (de Bruijn form, should be converted before reaching here).
            SigMatch::Rec(body) => {
                // Conservative fallback: treat as maximal.
                self.infer(body).map(|_| make_maximal())
            }

            SigMatch::Proj(idx, group) => self.infer_proj(idx, group),

            // ── Foreign ─────────────────────────────────────────────────────
            SigMatch::FFun(_, _) => Ok(make_maximal()),
            // C++: makeSimpleType(tree2int(type), kKonst, kInit, kVect, kNum, interval())
            SigMatch::FConst(kind, _, _) => self.infer_foreign_const_type(kind),
            // C++: makeSimpleType(tree2int(type), kBlock, kExec, kVect, kNum, interval())
            SigMatch::FVar(kind, _, _) => self.infer_foreign_var_type(kind),

            // ── Misc / conservative ──────────────────────────────────────────
            // C++: isSigAssertBounds(sig, min, max, cur)
            // Returns cur's type with interval clamped to [max(cur.lo, min), min(cur.hi, max)].
            SigMatch::AssertBounds(min_sig, max_sig, cur_sig) => {
                let t_min = self.infer(min_sig)?;
                let t_max = self.infer(max_sig)?;
                let t_cur = self.infer(cur_sig)?;
                let i_cur = t_cur.interval();
                let lo_bound = t_min.interval().lo();
                let hi_bound = t_max.interval().hi();
                let iend = if !i_cur.is_empty() {
                    interval::Interval::new(lo_bound.max(i_cur.lo()), hi_bound.min(i_cur.hi()), -24)
                } else {
                    interval::Interval::new(lo_bound, hi_bound, -24)
                };
                Ok(t_cur.promote_interval(iend))
            }

            // C++: makeSimpleType(kReal, kKonst, kComp, kVect, kNum, interval(i1.lo()))
            SigMatch::Lowest(x) => {
                let tx = self.infer(x)?;
                Ok(make_simple(
                    Nature::Real,
                    Variability::Konst,
                    Computability::Comp,
                    Vectorability::Vect,
                    Boolean::Num,
                    interval::singleton(tx.interval().lo()),
                ))
            }
            // C++: makeSimpleType(kReal, kKonst, kComp, kVect, kNum, interval(i1.hi()))
            SigMatch::Highest(x) => {
                let tx = self.infer(x)?;
                Ok(make_simple(
                    Nature::Real,
                    Variability::Konst,
                    Computability::Comp,
                    Vectorability::Vect,
                    Boolean::Num,
                    interval::singleton(tx.interval().hi()),
                ))
            }

            SigMatch::TempVar(x) => self.infer(x),
            // C++: castInterval(sampCast(t1), reunion(t1.interval, interval(0)))
            SigMatch::PermVar(x) => {
                let tx = self.infer(x)?;
                let itv = interval::reunion(tx.interval(), interval::singleton(0.0));
                Ok(samp_cast(tx).promote_interval(itv))
            }
            // C++: T(x, env); return T(y, env)  — return type is y's type only
            SigMatch::Seq(x, y) => {
                self.infer(x)?;
                self.infer(y)
            }
            // C++: castInterval(sampCast(t1), reunion(t1.interval, interval(0)))
            SigMatch::ZeroPad(x, _) => {
                let tx = self.infer(x)?;
                let itv = interval::reunion(tx.interval(), interval::singleton(0.0));
                Ok(samp_cast(tx).promote_interval(itv))
            }
            SigMatch::Clocked(_, x) => self.infer(x),

            // C++: type each sub for side effects, then return
            //   makeSimpleType(kReal, kSamp, kExec, kScal, kNum, interval(-1, 1))
            // The block lacks a proper type but must not be kKonst (avoids constant propagation).
            SigMatch::OnDemand(subs)
            | SigMatch::Upsampling(subs)
            | SigMatch::Downsampling(subs) => {
                for &s in subs {
                    self.infer(s)?;
                }
                Ok(make_simple(
                    Nature::Real,
                    Variability::Samp,
                    Computability::Exec,
                    Vectorability::Scal,
                    Boolean::Num,
                    interval::Interval::new(-1.0, 1.0, -24),
                ))
            }

            // Unknown / unhandled — check for cons lists first (recursion bodies).
            //
            // C++ `T(sig, env)` handles list nodes by iterating with hd/tl and
            // building a `TupletType` from the element types.  The signal arena
            // represents recursion bodies as `cons(signal, … cons(signal, nil))`.
            SigMatch::Unknown if self.arena.is_list(sig) => {
                let mut components = Vec::new();
                let mut cur = sig;
                while !self.arena.is_nil(cur) {
                    let head = self.arena.hd(cur).ok_or_else(|| {
                        TypeError("malformed cons list during type inference".into())
                    })?;
                    components.push(self.infer(head)?);
                    cur = self.arena.tl(cur).ok_or_else(|| {
                        TypeError("malformed cons list during type inference".into())
                    })?;
                }
                Ok(make_tuplet(components))
            }

            // Unknown / unhandled — conservative.
            SigMatch::Unknown => Ok(make_maximal()),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Helpers
    // ─────────────────────────────────────────────────────────────────────────

    fn infer_unary_math(
        &mut self,
        x: SigId,
        f: fn(Interval) -> Interval,
    ) -> Result<SigType, TypeError> {
        let tx = self.infer(x)?;
        let itv = f(tx.interval());
        let t = samp_cast(tx);
        Ok(float_cast(t).promote_interval(itv))
    }

    fn infer_binop(&self, op: BinOp, tl: SigType, tr: SigType) -> SigType {
        use interval::ops::{arithmetic as ar, logic as lg};

        let il = tl.interval();
        let ir = tr.interval();
        let (itv, nature) = match op {
            BinOp::Add => (ar::add(il, ir), tl.nature().join(tr.nature())),
            BinOp::Sub => (ar::sub(il, ir), tl.nature().join(tr.nature())),
            BinOp::Mul => (ar::mul(il, ir), tl.nature().join(tr.nature())),
            BinOp::Div => (ar::div(il, ir), Nature::Real),
            BinOp::Rem => (ar::mod_interval(il, ir), tl.nature().join(tr.nature())),
            BinOp::Lsh => (lg::lsh(il, ir), Nature::Int),
            BinOp::ARsh | BinOp::LRsh => (lg::rsh(il, ir), Nature::Int),
            BinOp::Gt => (lg::gt(il, ir), Nature::Int),
            BinOp::Lt => (lg::lt(il, ir), Nature::Int),
            BinOp::Ge => (lg::ge(il, ir), Nature::Int),
            BinOp::Le => (lg::le(il, ir), Nature::Int),
            BinOp::Eq => (lg::eq(il, ir), Nature::Int),
            BinOp::Ne => (lg::ne(il, ir), Nature::Int),
            BinOp::And => (lg::and(il, ir), Nature::Int),
            BinOp::Or => (lg::or(il, ir), Nature::Int),
            BinOp::Xor => (lg::xor(il, ir), Nature::Int),
        };
        let variability = tl.variability().join(tr.variability());
        let computability = tl.computability().join(tr.computability());
        let vectorability = tl.vectorability().join(tr.vectorability());
        let boolean = match op {
            BinOp::Gt | BinOp::Lt | BinOp::Ge | BinOp::Le | BinOp::Eq | BinOp::Ne => Boolean::Bool,
            _ => Boolean::Num,
        };
        make_simple(
            nature,
            variability,
            computability,
            vectorability,
            boolean,
            itv,
        )
    }

    // ── UI controls ──────────────────────────────────────────────────────────

    fn infer_button(&self, _id: ControlId) -> SigType {
        // C++: castInterval(TGUI, gAlgebra.Button(interval(0)))
        // TGUI = makeSimpleType(kReal, kBlock, kExec, kVect, kNum, interval(0,1))
        // castInterval keeps kReal/kVect/kNum — it does NOT set kBool or kScal.
        make_simple(
            Nature::Real,
            Variability::Block,
            Computability::Exec,
            Vectorability::Vect,
            Boolean::Num,
            interval::Interval::new(0.0, 1.0, 0),
        )
    }

    fn infer_checkbox(&self, _id: ControlId) -> SigType {
        // C++: same as Button — castInterval(TGUI, gAlgebra.Checkbox(interval(0)))
        make_simple(
            Nature::Real,
            Variability::Block,
            Computability::Exec,
            Vectorability::Vect,
            Boolean::Num,
            interval::Interval::new(0.0, 1.0, 0),
        )
    }

    fn infer_slider(&self, id: ControlId) -> SigType {
        let itv = self
            .ui_program
            .control(id)
            .and_then(|spec| {
                // Only slider-family controls have ranges.
                if !matches!(
                    spec.kind,
                    ControlKind::VSlider | ControlKind::HSlider | ControlKind::NumEntry
                ) {
                    return None;
                }
                spec.range
            })
            .map(|r| {
                // vslider(_name, _init, lo, hi, step)
                interval::ops::ui::vslider(
                    interval::empty(), // _name (unused)
                    interval::singleton(r.init),
                    interval::singleton(r.min),
                    interval::singleton(r.max),
                    interval::singleton(r.step),
                )
            })
            .unwrap_or_else(interval::Interval::new_default);

        // C++: castInterval(TGUI, vslider_interval(...))
        // TGUI = makeSimpleType(kReal, kBlock, kExec, kVect, kNum, interval())
        // castInterval replaces only the interval → Computability is kExec, not kInit.
        make_simple(
            Nature::Real,
            Variability::Block,
            Computability::Exec,
            Vectorability::Vect,
            Boolean::Num,
            itv,
        )
    }

    fn infer_bargraph(&mut self, _id: ControlId, x: SigId) -> Result<SigType, TypeError> {
        // C++: T(s1, env)->promoteVariability(kBlock)
        // The lo/hi bound signals are computed for side effects in C++ but their
        // values are NOT reflected in the returned type.  The returned type is the
        // input signal's type with variability promoted to at least kBlock.
        let tx = self.infer(x)?;
        Ok(tx.promote_variability(Variability::Block))
    }

    // ── Tables ───────────────────────────────────────────────────────────────

    fn infer_write_table(&mut self, size: SigId, generator: SigId) -> Result<SigType, TypeError> {
        let _tsize = self.infer(size)?;
        let tgen = self.infer(generator)?;
        Ok(make_table_type(tgen))
    }

    // ── Foreign type annotation ───────────────────────────────────────────────

    fn foreign_nature(&self, kind: SigId) -> Nature {
        // The kind node encodes the C type via a tree integer.
        // 0 = int (kInt), 1 = float/double (kReal), default = Real.
        match tlib::tree_to_int(self.arena, kind) {
            Some(0) => Nature::Int,
            _ => Nature::Real,
        }
    }

    /// `inferFConstType`: constant, evaluated at init, no static range known.
    ///
    /// # C++ source
    /// `makeSimpleType(tree2int(type), kKonst, kInit, kVect, kNum, interval())`
    ///
    /// `interval()` in C++ uses the default member initializers:
    /// `fLo = std::numeric_limits<double>::lowest()`, `fHi = std::numeric_limits<double>::max()`.
    /// This is the fully-open interval `[f64::MIN, f64::MAX]` — not NaN/empty.
    /// Rust equivalent: `Interval::new_default()`.
    fn infer_foreign_const_type(&self, kind: SigId) -> Result<SigType, TypeError> {
        Ok(make_simple(
            self.foreign_nature(kind),
            Variability::Konst,
            Computability::Init,
            Vectorability::Vect,
            Boolean::Num,
            interval::Interval::new_default(),
        ))
    }

    /// `inferFVarType`: varies by block like a UI element, executed each block.
    ///
    /// # C++ source
    /// `makeSimpleType(tree2int(type), kBlock, kExec, kVect, kNum, interval())`
    ///
    /// Same interval semantics as `inferFConstType` — see that method's doc.
    fn infer_foreign_var_type(&self, kind: SigId) -> Result<SigType, TypeError> {
        Ok(make_simple(
            self.foreign_nature(kind),
            Variability::Block,
            Computability::Exec,
            Vectorability::Vect,
            Boolean::Num,
            interval::Interval::new_default(),
        ))
    }

    // ── Recursive groups ─────────────────────────────────────────────────────

    /// Infer the type of a symbolic recursion head `(var, body)`.
    ///
    /// # C++ source
    /// `inferRecType` / `updateRecTypes` fixed-point loop in
    /// `sigtyperules.cpp`.
    fn infer_sym_rec(&mut self, sig: SigId) -> Result<SigType, TypeError> {
        let Some((var, body)) = match_sym_rec(self.arena, sig) else {
            return Ok(make_maximal());
        };

        // Seed with maximal type (top of lattice).
        self.env.insert(var, make_maximal());

        // Fixed-point iteration: refine until stable.
        loop {
            let prev = self.env.get(&var).cloned().unwrap_or_else(make_maximal);
            let new = self.infer(body)?;
            self.env.insert(var, new.clone());
            if new == prev {
                break;
            }
        }

        Ok(self.env.get(&var).cloned().unwrap_or_else(make_maximal))
    }

    /// Infer the type of projection from a tuplet recursion group.
    ///
    /// # C++ source
    /// `inferProjType(Type t, int i, int vec)` — called with `vec = kScal`.
    /// Each component is promoted by the group's variability/computability, and
    /// vectorability is promoted to kScal (the more conservative of the two).
    fn infer_proj(&mut self, idx: i32, group: SigId) -> Result<SigType, TypeError> {
        let tg = self.infer(group)?;
        let (gv, gc) = (tg.variability(), tg.computability());
        let comp = match &tg {
            SigType::Tuplet(tt) => {
                let i = usize::try_from(idx).unwrap_or(0);
                tt.components.get(i).cloned().unwrap_or_else(make_maximal)
            }
            other => other.clone(),
        };
        Ok(comp
            .promote_variability(gv)
            .promote_computability(gc)
            .promote_vectorability(Vectorability::Scal))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Table type helpers (free functions)
// ─────────────────────────────────────────────────────────────────────────────

/// Type of a table read: element type of `tbl`, with index variability joined.
///
/// # C++ source
/// `inferReadTableType(Type tbl, Type ri)`
fn infer_read_table(tbl: SigType, idx: SigType) -> Result<SigType, TypeError> {
    let content = match &tbl {
        SigType::Table(tt) => *tt.content.clone(),
        other => other.clone(),
    };
    // Raise variability/computability by the index type.
    let t = content
        .promote_variability(idx.variability())
        .promote_computability(idx.computability());
    Ok(t)
}

/// Type of a table write: preserve table type, assert write-type ≤ table nature.
///
/// # C++ source
/// `inferWriteTableType(Type tbl, Type wi, Type ws)`
fn infer_write_table_type(tbl: SigType, _wi: SigType, _ws: SigType) -> Result<SigType, TypeError> {
    // Return the table type unchanged — writes don't change the table's type.
    Ok(tbl)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use signals::SigBuilder;
    use tlib::TreeArena;
    use ui::UiProgram;

    use super::*;
    use crate::enums::*;

    fn empty_ui() -> UiProgram {
        UiProgram::empty()
    }

    fn annotate(arena: &TreeArena, outputs: &[SigId]) -> HashMap<SigId, SigType> {
        let ui = empty_ui();
        let mut ann = TypeAnnotator::new(arena, &ui);
        ann.annotate(outputs).expect("annotation failed")
    }

    #[test]
    fn int_literal_is_int_konst() {
        let mut arena = TreeArena::new();
        let mut b = SigBuilder::new(&mut arena);
        let s = b.int(42);
        let types = annotate(&arena, &[s]);
        let ty = &types[&s];
        assert_eq!(ty.nature(), Nature::Int);
        assert_eq!(ty.variability(), Variability::Konst);
        assert_eq!(ty.computability(), Computability::Comp);
        assert_eq!(ty.interval(), interval::singleton(42.0));
    }

    #[test]
    #[allow(clippy::approx_constant)] // 3.14 is the test value, not an approximation of PI
    fn real_literal_is_real_konst() {
        let mut arena = TreeArena::new();
        let mut b = SigBuilder::new(&mut arena);
        let s = b.real(3.14);
        let types = annotate(&arena, &[s]);
        let ty = &types[&s];
        assert_eq!(ty.nature(), Nature::Real);
        assert_eq!(ty.variability(), Variability::Konst);
        assert_eq!(ty.interval(), interval::singleton(3.14));
    }

    #[test]
    fn input_is_real_samp() {
        let mut arena = TreeArena::new();
        let mut b = SigBuilder::new(&mut arena);
        let s = b.input(0);
        let types = annotate(&arena, &[s]);
        let ty = &types[&s];
        // C++: TINPUT = makeSimpleType(kReal, kSamp, kExec, kVect, kNum, interval(-1,1))
        assert_eq!(ty.nature(), Nature::Real);
        assert_eq!(ty.variability(), Variability::Samp);
        assert_eq!(ty.computability(), Computability::Exec);
        assert_eq!(ty.vectorability(), Vectorability::Vect);
        assert_eq!(ty.boolean(), Boolean::Num);
        // Interval must be the audio range [-1, 1], not the uninformative placeholder.
        let itv = ty.interval();
        assert_eq!(itv.lo(), -1.0);
        assert_eq!(itv.hi(), 1.0);
    }

    #[test]
    fn button_is_real_block_vect_num() {
        let mut arena = TreeArena::new();
        let mut b = SigBuilder::new(&mut arena);
        let s = b.button(0);
        let types = annotate(&arena, &[s]);
        let ty = &types[&s];
        // C++: castInterval(TGUI, ...) → kReal, kBlock, kExec, kVect, kNum, [0,1]
        assert_eq!(ty.nature(), Nature::Real);
        assert_eq!(ty.variability(), Variability::Block);
        assert_eq!(ty.computability(), Computability::Exec);
        assert_eq!(ty.vectorability(), Vectorability::Vect);
        assert_eq!(ty.boolean(), Boolean::Num);
        assert_eq!(ty.interval().lo(), 0.0);
        assert_eq!(ty.interval().hi(), 1.0);
    }

    #[test]
    fn select2_includes_cond_variability() {
        // If the cond is Samp, the result must be at least Samp even if branches are Konst.
        let mut arena = TreeArena::new();
        let mut b = SigBuilder::new(&mut arena);
        let cond = b.input(0); // Samp
        let s1 = b.int(0); // Konst
        let s2 = b.int(1); // Konst
        let s = b.select2(cond, s1, s2);
        let types = annotate(&arena, &[s]);
        // C++: variability = st1|st2|stsel = Konst|Konst|Samp = Samp
        assert_eq!(types[&s].variability(), Variability::Samp);
    }

    #[test]
    fn select2_all_konst_stays_konst() {
        // If all three (cond and both branches) are Konst, result should be Konst, NOT Samp.
        let mut arena = TreeArena::new();
        let mut b = SigBuilder::new(&mut arena);
        let cond = b.int(1); // Konst
        let s1 = b.int(0); // Konst
        let s2 = b.real(3.0); // Konst
        let s = b.select2(cond, s1, s2);
        let types = annotate(&arena, &[s]);
        // Without the samp_cast bug, a fully-constant select2 stays Konst.
        assert_eq!(types[&s].variability(), Variability::Konst);
    }

    #[test]
    fn slider_computability_is_exec() {
        // C++: TGUI = kExec → castInterval(TGUI, ...) → kExec
        let mut arena = TreeArena::new();
        let mut b = SigBuilder::new(&mut arena);
        let s = b.hslider(0);
        let ui = UiProgram::empty();
        let mut ann = TypeAnnotator::new(&arena, &ui);
        let types = ann.annotate(&[s]).expect("annotation failed");
        assert_eq!(types[&s].computability(), Computability::Exec);
    }

    #[test]
    fn seq_returns_type_of_second_signal() {
        // C++: T(x, env); return T(y, env)
        let mut arena = TreeArena::new();
        let mut b = SigBuilder::new(&mut arena);
        let x = b.int(42); // Konst Int
        let y = b.input(0); // Samp Real
        let s = b.seq(x, y);
        let types = annotate(&arena, &[s]);
        // Must be the type of y, not the union of x and y.
        assert_eq!(types[&s].nature(), Nature::Real);
        assert_eq!(types[&s].variability(), Variability::Samp);
    }

    #[test]
    fn checkbox_is_real_block_vect_num() {
        let mut arena = TreeArena::new();
        let mut b = SigBuilder::new(&mut arena);
        let s = b.checkbox(0);
        let types = annotate(&arena, &[s]);
        let ty = &types[&s];
        // C++: same contract as Button
        assert_eq!(ty.nature(), Nature::Real);
        assert_eq!(ty.variability(), Variability::Block);
        assert_eq!(ty.computability(), Computability::Exec);
        assert_eq!(ty.vectorability(), Vectorability::Vect);
        assert_eq!(ty.boolean(), Boolean::Num);
        assert_eq!(ty.interval().lo(), 0.0);
        assert_eq!(ty.interval().hi(), 1.0);
    }

    #[test]
    fn add_ints_stays_int() {
        let mut arena = TreeArena::new();
        let mut b = SigBuilder::new(&mut arena);
        let a = b.int(1);
        let bv = b.int(2);
        let s = b.binop(signals::BinOp::Add, a, bv);
        let types = annotate(&arena, &[s]);
        let ty = &types[&s];
        assert_eq!(ty.nature(), Nature::Int);
        assert_eq!(ty.interval(), interval::Interval::new(3.0, 3.0, 0));
    }

    #[test]
    fn add_int_real_promotes_to_real() {
        let mut arena = TreeArena::new();
        let mut b = SigBuilder::new(&mut arena);
        let a = b.int(1);
        let bv = b.real(0.5);
        let s = b.binop(signals::BinOp::Add, a, bv);
        let types = annotate(&arena, &[s]);
        assert_eq!(types[&s].nature(), Nature::Real);
    }

    #[test]
    fn delay1_is_samp_and_interval_includes_zero() {
        let mut arena = TreeArena::new();
        let mut b = SigBuilder::new(&mut arena);
        // delay1 of a real constant 0.5: the result must include 0 (initial sample).
        let c = b.real(0.5);
        let s = b.delay1(c);
        let types = annotate(&arena, &[s]);
        let ty = &types[&s];
        assert_eq!(ty.variability(), Variability::Samp);
        // C++: castInterval(sampCast(t), reunion(t.interval, interval(0)))
        // singleton(0.5) ∪ singleton(0) = [0, 0.5]
        assert!(ty.interval().lo() <= 0.0, "interval lo must be ≤ 0");
        assert!(ty.interval().hi() >= 0.5, "interval hi must be ≥ 0.5");
    }

    #[test]
    fn delay1_of_negative_constant_interval_includes_zero() {
        let mut arena = TreeArena::new();
        let mut b = SigBuilder::new(&mut arena);
        let c = b.real(-1.0);
        let s = b.delay1(c);
        let types = annotate(&arena, &[s]);
        let ty = &types[&s];
        // singleton(-1) ∪ singleton(0) = [-1, 0]
        assert!(ty.interval().lo() <= -1.0);
        assert!(ty.interval().hi() >= 0.0);
    }

    #[test]
    fn int_cast_changes_nature() {
        let mut arena = TreeArena::new();
        let mut b = SigBuilder::new(&mut arena);
        let r = b.real(1.7);
        let s = b.int_cast(r);
        let types = annotate(&arena, &[s]);
        assert_eq!(types[&s].nature(), Nature::Int);
    }
}
