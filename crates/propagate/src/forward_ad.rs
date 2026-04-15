//! Forward-mode automatic differentiation helpers for the `propagate` phase.
//!
//! # Source provenance (C++)
//! - `compiler/propagate/propagate.cpp`
//! - `compiler/transform/sigtyperules.hh`
//! - `compiler/signals/signals.cpp`
//!
//! # Scope
//! - Collect differentiable UI controls reachable from one propagated signal.
//! - Expand each primal output into `primal + one tangent per enabled control`.
//! - Preserve shared signal DAG structure through memoized transformation.
//! - Keep reverse-mode (`rad`) explicitly out of scope for this phase.
//!
//! # Dual-number algebra
//! Forward-mode AD (FAD) propagates derivatives alongside values by carrying a
//! *tangent* component next to each signal.  A **dual signal** is written
//! `u + ε·u'` where `ε² = 0`.  The differentiation variable is one selected
//! UI control `p`; its seed is `dp/dp = 1`, and every other independent
//! variable has seed `0`.
//!
//! One [`ForwardADTransform`] instance differentiates the entire signal DAG
//! with respect to a single control; [`generate_fad_signals`] drives one
//! transformer per reachable control and assembles the output bundle.
//!
//! # Differentiation rules by node kind
//!
//! ## Constants and audio inputs
//!
//! | Node | Tangent |
//! |------|---------|
//! | `int(c)` | `0` |
//! | `real(c)` | `0.0` |
//! | `sigInput(_)` | `0.0` |
//!
//! Audio inputs are treated as independent of all UI controls.
//!
//! ## UI controls
//!
//! | Node | Tangent |
//! |------|---------|
//! | `hslider(p)` / `vslider(p)` / `numentry(p)` — selected control | `1.0` |
//! | any other continuous control | `0.0` |
//! | `button`, `checkbox` | `0.0` (discrete, not differentiable) |
//!
//! Controls annotated with `[autodiff:false]` metadata are excluded from the
//! reachable set and therefore never become the selected control.
//!
//! ## Arithmetic binary operators (`BinOp`)
//!
//! | Operator | Tangent rule |
//! |----------|-------------|
//! | `x + y` | `x' + y'` |
//! | `x - y` | `x' - y'` |
//! | `x * y` | `x'·y + x·y'` (product rule) |
//! | `x / y` | `(x'·y − x·y') / y²` (quotient rule) |
//! | `x % y` | `x' − y'·⌊x/y⌋` |
//! | shifts, bitwise ops, comparisons | `0` (non-differentiable integer ops) |
//!
//! ## Unary transcendentals (chain rule: `d f(x)/dp = f'(x) · x'`)
//!
//! | Function | Derivative `f'(x)` | Tangent emitted |
//! |----------|--------------------|-----------------|
//! | `sin(x)` | `cos(x)` | `cos(x) * x'` |
//! | `cos(x)` | `−sin(x)` | `(0 - sin(x)) * x'` |
//! | `tan(x)` | `1 / cos²(x)` | `(1 / cos(x)²) * x'` |
//! | `exp(x)` | `exp(x)` | `exp(x) * x'` |
//! | `log(x)` | `1 / x` | `(1 / x) * x'` |
//! | `log10(x)` | `1 / (x · ln 10)` | `(1 / (x * log(10))) * x'` |
//! | `sqrt(x)` | `1 / (2 · √x)` | `(1 / (2 * sqrt(x))) * x'` |
//! | `abs(x)` | `x / |x|` (sign) | `(x / abs(x)) * x'` |
//! | `acos(x)` | `−1 / √(1 − x²)` | `(-1 / sqrt(1 - x²)) * x'` |
//! | `asin(x)` | `1 / √(1 − x²)` | `(1 / sqrt(1 - x²)) * x'` |
//! | `atan(x)` | `1 / (1 + x²)` | `(1 / (1 + x²)) * x'` |
//!
//! Note: `abs` is not differentiable at `x = 0`; the expression `x/|x|` has
//! undefined behaviour there and produces `NaN` or `±inf` at runtime.
//!
//! ## Binary math (`pow`, `min`, `max`)
//!
//! ### `pow(x, y)`
//! General power rule:
//! ```text
//! d/dp x^y = x^y · (y' · ln(x) + y · x' / x)
//! ```
//! Both `x` and `y` may depend on the control; both tangents are combined.
//!
//! ### `min(x, y)` / `max(x, y)`
//! ```text
//! d/dp min(x,y) = select2(x < y, x', y')
//! d/dp max(x,y) = select2(x > y, x', y')
//! ```
//! The selector is the primal comparison; the tangent is piecewise-constant
//! with a sub-gradient of `0` on the boundary.
//!
//! ### `atan2`, `fmod`, `remainder`
//! Currently fall through to zero tangent (unimplemented).
//!
//! ## Cast operators
//!
//! | Node | Tangent |
//! |------|---------|
//! | `float_cast(x)` | `float_cast(x')` |
//! | `int_cast(x)` | `0` (piecewise-constant step function) |
//! | `bit_cast(x)` | `0` (reinterpret-cast, semantically opaque) |
//!
//! ## Delay operators
//!
//! ### Unit delay `delay1(x)`
//! ```text
//! d/dp delay1(x) = delay1(x')
//! ```
//! A fixed one-sample shift is linear, so the derivative is simply delayed.
//!
//! ### Variable delay `delay(x, d)`
//! The discrete-time derivative decomposes into a *content* term and a *time*
//! term via the Leibniz rule:
//! ```text
//! d/dp x[n − d(n)] = x'[n − d(n)]              (content derivative)
//!                  − d'(n) · ∇x[n − d(n)]       (time derivative)
//! ```
//! where `∇x[n−d]` is the backward finite difference `x[n−d] − x[n−d−1]`,
//! approximated as `delay(x, d) − delay(delay1(x), d)`.
//!
//! Emitted as:
//! ```text
//! tangent = delay(x', d) − d' · delay(x − delay1(x), d)
//! ```
//!
//! ## Control-flow nodes
//!
//! | Node | Tangent |
//! |------|---------|
//! | `select2(cond, x, y)` | `select2(cond, x', y')` — cond is not differentiated |
//! | `prefix(init, x)` | `prefix(init', x')` |
//!
//! ## Projection and recursive groups (`sigRec` / `sigProj`)
//!
//! Recursive signal groups are differentiated in parallel: for each recursive
//! variable `x`, a tangent variable `FAD_x` is introduced.
//!
//! ```text
//! d/dp sigRec(x, body) = sigRec(FAD_x, d(body)/dp)
//! d/dp sigRef(x)       = sigRef(FAD_x)
//! d/dp sigProj(i, g)   = sigProj(i, d(g)/dp)
//! ```
//!
//! De Bruijn indices are converted to symbolic form via `de_bruijn_to_sym`
//! before differentiation so that variable names are explicit and `FAD_`
//! pairing is unambiguous.  The tangent group is seeded with a placeholder
//! `sigRec(FAD_x, nil)` before the body is traversed, which breaks cycles.
//!
//! ## Pass-through helper nodes (`attach`, `enable`, `control`)
//!
//! These carry a left (signal) and right (side-effect/control) operand.  Only
//! the left operand's tangent is forwarded; the right operand is structurally
//! preserved with its tangent dropped.
//!
//! ## Bargraph outputs (`vbargraph`, `hbargraph`)
//!
//! Zero tangent.  Bargraphs are metering outputs, not DSP signal paths.
//!
//! ## Unhandled / non-differentiable nodes
//!
//! Table reads/writes (`rdtbl`, `wrtbl`), FFun calls, soundfile accessors,
//! waveforms, on-demand/up/downsampling, `Gen`, `PermVar`, `TempVar`, and all
//! other unmatched variants fall through to zero tangent with the primal
//! unchanged.
//!
//! # Integration contract
//! This module is intentionally internal to `propagate`:
//! - `box_arity_typed(...)` reports expanded output arity for `fad(expr)`,
//! - output expansion happens only after the wrapped box has already lowered to
//!   signal IR,
//! - controls are sourced from the already-built [`ui::UiProgram`] registry so
//!   metadata filtering such as `[autodiff:false]` is stable and centralized.
//!
//! # Ordering invariant
//! Tangent outputs are emitted deterministically:
//! 1. preserve primal output order,
//! 2. for each primal, collect reachable controls once,
//! 3. sort controls by canonical UI label, then by `ControlId`.

use ahash::{AHashMap, AHashSet};
use signals::{BinOp, SigBuilder, SigId, SigMatch, match_sig};
use tlib::{
    NodeKind, TreeArena, TreeId, de_bruijn_to_sym, list_to_vec, match_sym_rec, match_sym_ref,
    sym_rec, sym_ref, vec_to_list,
};
use ui::{ControlId, ControlSpec};

use crate::{PropagateContext, PropagateError};

/// Internal dual-number carrier used while differentiating one signal graph.
///
/// `primal` is the original signal expression. `tangent` is the derivative of
/// that expression with respect to a single selected control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Dual {
    primal: SigId,
    tangent: SigId,
}

/// Walks one propagated signal DAG and records the differentiable UI controls
/// it depends on.
///
/// The collector relies on the grouped-UI control registry rather than raw
/// slider labels inside the signal graph. This keeps metadata ownership
/// centralized and makes `[autodiff:false]` filtering match the canonical UI
/// extraction pass.
struct ADControlCollector<'a> {
    arena: &'a TreeArena,
    controls: Vec<ControlId>,
    visited: AHashSet<SigId>,
    seen_controls: AHashSet<ControlId>,
    ui_controls: &'a [ControlSpec],
}

impl<'a> ADControlCollector<'a> {
    /// Creates one collector bound to the propagated signal arena and the
    /// canonical UI control registry for the current program.
    fn new(arena: &'a TreeArena, ui_controls: &'a [ControlSpec]) -> Self {
        Self {
            arena,
            controls: Vec::new(),
            visited: AHashSet::new(),
            seen_controls: AHashSet::new(),
            ui_controls,
        }
    }

    /// Recursively visits one signal and records each reachable differentiable
    /// control at most once.
    ///
    /// Traversal is memoized on `SigId` to preserve DAG complexity when the
    /// same shared subtree appears through several parents.
    fn collect(&mut self, sig: SigId) {
        if !self.visited.insert(sig) {
            return;
        }

        match match_sig(self.arena, sig) {
            SigMatch::HSlider(control)
            | SigMatch::VSlider(control)
            | SigMatch::NumEntry(control) => {
                if self.is_autodiff_enabled(control) && self.seen_controls.insert(control) {
                    self.controls.push(control);
                }
            }
            SigMatch::Output(_, sig)
            | SigMatch::Delay1(sig)
            | SigMatch::IntCast(sig)
            | SigMatch::BitCast(sig)
            | SigMatch::FloatCast(sig)
            | SigMatch::Gen(sig)
            | SigMatch::Lowest(sig)
            | SigMatch::Highest(sig)
            | SigMatch::Acos(sig)
            | SigMatch::Asin(sig)
            | SigMatch::Atan(sig)
            | SigMatch::Cos(sig)
            | SigMatch::Sin(sig)
            | SigMatch::Tan(sig)
            | SigMatch::Exp(sig)
            | SigMatch::Log(sig)
            | SigMatch::Log10(sig)
            | SigMatch::Sqrt(sig)
            | SigMatch::Abs(sig)
            | SigMatch::Floor(sig)
            | SigMatch::Ceil(sig)
            | SigMatch::Rint(sig)
            | SigMatch::Round(sig)
            | SigMatch::TempVar(sig)
            | SigMatch::PermVar(sig) => self.collect(sig),
            SigMatch::Delay(a, b)
            | SigMatch::Prefix(a, b)
            | SigMatch::RdTbl(a, b)
            | SigMatch::BinOp(_, a, b)
            | SigMatch::Pow(a, b)
            | SigMatch::Min(a, b)
            | SigMatch::Max(a, b)
            | SigMatch::Atan2(a, b)
            | SigMatch::Fmod(a, b)
            | SigMatch::Remainder(a, b)
            | SigMatch::Attach(a, b)
            | SigMatch::Enable(a, b)
            | SigMatch::Control(a, b)
            | SigMatch::Seq(a, b)
            | SigMatch::ZeroPad(a, b)
            | SigMatch::Clocked(a, b)
            | SigMatch::SoundfileLength(a, b)
            | SigMatch::SoundfileRate(a, b) => {
                self.collect(a);
                self.collect(b);
            }
            SigMatch::AssertBounds(a, b, c) | SigMatch::Select2(a, b, c) => {
                self.collect(a);
                self.collect(b);
                self.collect(c);
            }
            SigMatch::WrTbl(a, b, c, d) | SigMatch::SoundfileBuffer(a, b, c, d) => {
                self.collect(a);
                self.collect(b);
                self.collect(c);
                self.collect(d);
            }
            SigMatch::FFun(fun, args) => {
                self.collect(fun);
                self.collect_list(args);
            }
            SigMatch::Proj(_, group) => self.collect(group),
            SigMatch::Rec(body) => self.collect_list(body),
            SigMatch::Waveform(values)
            | SigMatch::OnDemand(values)
            | SigMatch::Upsampling(values)
            | SigMatch::Downsampling(values) => {
                for child in values {
                    self.collect(*child);
                }
            }
            _ => {
                if let Some((_var, body)) = match_sym_rec(self.arena, sig) {
                    self.collect_list(body);
                }
            }
        }
    }

    /// Traverses one cons-list of signal children when present.
    fn collect_list(&mut self, list: TreeId) {
        if let Some(values) = list_to_vec(self.arena, list) {
            for value in values {
                self.collect(value);
            }
        }
    }

    /// Returns whether a control is differentiable for this phase.
    ///
    /// Current rule:
    /// - default to enabled,
    /// - disable only when the canonical UI metadata contains
    ///   `autodiff = "false"` (case-insensitive on the value).
    fn is_autodiff_enabled(&self, control: ControlId) -> bool {
        self.ui_controls
            .get(usize::try_from(control).ok().unwrap_or(usize::MAX))
            .is_none_or(|spec| {
                !spec
                    .metadata
                    .iter()
                    .any(|(key, value)| key == "autodiff" && value.eq_ignore_ascii_case("false"))
            })
    }

    /// Sorts the collected controls into deterministic emission order.
    ///
    /// The primary key is the canonical UI label because that is the most
    /// user-visible ordering contract carried by the current plan. `ControlId`
    /// remains a stable tie-breaker.
    fn sort_controls_by_label(&mut self) {
        self.controls.sort_by(|left, right| {
            let left_label = self
                .ui_controls
                .get(usize::try_from(*left).ok().unwrap_or(usize::MAX))
                .map(|spec| spec.label.as_str())
                .unwrap_or("");
            let right_label = self
                .ui_controls
                .get(usize::try_from(*right).ok().unwrap_or(usize::MAX))
                .map(|spec| spec.label.as_str())
                .unwrap_or("");
            left_label.cmp(right_label).then(left.cmp(right))
        });
    }
}

/// Memoized forward-mode transformer for one selected differentiation control.
///
/// One transformer instance computes `d(signal) / d(control)` across a shared
/// signal DAG. The cache prevents exponential blow-up on reused subgraphs and
/// also breaks recursion cycles while rebuilding `sigRec/sigProj`-style groups.
struct ForwardADTransform<'a> {
    arena: &'a mut TreeArena,
    diff_control: ControlId,
    cache: AHashMap<SigId, Dual>,
}

impl<'a> ForwardADTransform<'a> {
    /// Creates one transformer for the selected differentiable control.
    fn new(arena: &'a mut TreeArena, diff_control: ControlId) -> Self {
        Self {
            arena,
            diff_control,
            cache: AHashMap::new(),
        }
    }

    /// Returns the primal/tangent pair for one signal, using memoized results
    /// whenever the signal was already differentiated.
    fn transform(&mut self, sig: SigId) -> Dual {
        if let Some(dual) = self.cache.get(&sig).copied() {
            return dual;
        }
        let dual = self.transform_uncached(sig);
        self.cache.insert(sig, dual);
        dual
    }

    /// Differentiates one list of signals and preserves list order.
    fn transform_list(&mut self, list: TreeId) -> Vec<Dual> {
        list_to_vec(self.arena, list)
            .unwrap_or_default()
            .into_iter()
            .map(|sig| self.transform(sig))
            .collect()
    }

    /// Computes the dual form for one signal node.
    ///
    /// Handles the two symbolic node shapes first (before the `SigMatch`
    /// dispatch) because they are represented differently in the arena:
    ///
    /// - `sigRef(x)` → tangent is `sigRef(FAD_x)`.
    /// - `sigRec(x, body)` → tangent is `sigRec(FAD_x, d(body)/dp)`.
    ///   A placeholder entry is inserted into the cache before recursing into
    ///   `body` so that self-referential cycles resolve correctly.
    ///
    /// All other nodes are dispatched through the `SigMatch` arm; unsupported
    /// or intentionally non-differentiable nodes fall back to a zero tangent
    /// while preserving the original primal signal.
    fn transform_uncached(&mut self, sig: SigId) -> Dual {
        // Rule: d/dp sigRef(x) = sigRef(FAD_x)
        if let Some(var) = match_sym_ref(self.arena, sig) {
            let fad_var = self.fad_var(var);
            return Dual {
                primal: sig,
                tangent: sym_ref(self.arena, fad_var),
            };
        }
        // Rule: d/dp sigRec(x, body) = sigRec(FAD_x, d(body)/dp)
        // Seed the cache with a placeholder first to handle back-edges.
        if let Some((var, body)) = match_sym_rec(self.arena, sig) {
            let fad_var = self.fad_var(var);
            let tangent_placeholder = sym_rec(self.arena, fad_var, self.arena.nil());
            let placeholder = Dual {
                primal: sig,
                tangent: tangent_placeholder,
            };
            self.cache.insert(sig, placeholder);
            let duals = self.transform_list(body);
            let primals: Vec<_> = duals.iter().map(|dual| dual.primal).collect();
            let tangents: Vec<_> = duals.iter().map(|dual| dual.tangent).collect();
            let primal_body = vec_to_list(self.arena, &primals);
            let tangent_body = vec_to_list(self.arena, &tangents);
            return Dual {
                primal: sym_rec(self.arena, var, primal_body),
                tangent: sym_rec(self.arena, fad_var, tangent_body),
            };
        }

        match match_sig(self.arena, sig) {
            // Constants: d/dp c = 0
            SigMatch::Int(_) => Dual {
                primal: sig,
                tangent: SigBuilder::new(self.arena).int(0),
            },
            SigMatch::Real(_) => Dual {
                primal: sig,
                tangent: SigBuilder::new(self.arena).real(0.0),
            },
            // Audio inputs are independent of all UI controls: d/dp input = 0
            SigMatch::Input(_) => Dual {
                primal: sig,
                tangent: SigBuilder::new(self.arena).real(0.0),
            },
            // Continuous UI controls: seed = 1 for the selected control, 0 otherwise.
            SigMatch::HSlider(control)
            | SigMatch::VSlider(control)
            | SigMatch::NumEntry(control) => {
                let tangent = if control == self.diff_control {
                    SigBuilder::new(self.arena).real(1.0)
                } else {
                    SigBuilder::new(self.arena).real(0.0)
                };
                Dual {
                    primal: sig,
                    tangent,
                }
            }
            // Discrete UI controls are not differentiable: d/dp button = 0
            SigMatch::Button(_) | SigMatch::Checkbox(_) => Dual {
                primal: sig,
                tangent: SigBuilder::new(self.arena).real(0.0),
            },
            SigMatch::BinOp(op, x, y) => self.transform_binop(op, x, y),
            SigMatch::Sin(x) => self.unary_chain(
                x,
                |b, primal_x| b.sin(primal_x),
                |b, primal_x, tangent_x| {
                    let cos_x = b.cos(primal_x);
                    b.mul(cos_x, tangent_x)
                },
            ),
            SigMatch::Cos(x) => self.unary_chain(
                x,
                |b, primal_x| b.cos(primal_x),
                |b, primal_x, tangent_x| {
                    let sin_x = b.sin(primal_x);
                    let zero = b.real(0.0);
                    let neg_sin_x = b.sub(zero, sin_x);
                    b.mul(neg_sin_x, tangent_x)
                },
            ),
            SigMatch::Tan(x) => self.unary_chain(
                x,
                |b, primal_x| b.tan(primal_x),
                |b, primal_x, tangent_x| {
                    let cos_x = b.cos(primal_x);
                    let cos_sq = b.mul(cos_x, cos_x);
                    let one = b.real(1.0);
                    let inv = b.div(one, cos_sq);
                    b.mul(inv, tangent_x)
                },
            ),
            // Rule: d/dp exp(x) = exp(x) · x'
            SigMatch::Exp(x) => self.unary_chain(
                x,
                |b, primal_x| b.exp(primal_x),
                |b, primal_x, tangent_x| {
                    let exp_x = b.exp(primal_x);
                    b.mul(exp_x, tangent_x)
                },
            ),
            // Rule: d/dp log(x) = (1/x) · x'
            SigMatch::Log(x) => self.unary_chain(
                x,
                |b, primal_x| b.log(primal_x),
                |b, primal_x, tangent_x| {
                    let one = b.real(1.0);
                    let inv = b.div(one, primal_x);
                    b.mul(inv, tangent_x)
                },
            ),
            // Rule: d/dp log10(x) = (1 / (x · ln 10)) · x'
            SigMatch::Log10(x) => self.unary_chain(
                x,
                |b, primal_x| b.log10(primal_x),
                |b, primal_x, tangent_x| {
                    let ten = b.real(10.0);
                    let log_ten = b.log(ten);
                    let denom = b.mul(primal_x, log_ten);
                    let one = b.real(1.0);
                    let inv = b.div(one, denom);
                    b.mul(inv, tangent_x)
                },
            ),
            // Rule: d/dp sqrt(x) = (1 / (2·√x)) · x'
            SigMatch::Sqrt(x) => self.unary_chain(
                x,
                |b, primal_x| b.sqrt(primal_x),
                |b, primal_x, tangent_x| {
                    let two = b.real(2.0);
                    let root = b.sqrt(primal_x);
                    let denom = b.mul(two, root);
                    let one = b.real(1.0);
                    let inv = b.div(one, denom);
                    b.mul(inv, tangent_x)
                },
            ),
            // Rule: d/dp |x| = (x/|x|) · x'  — undefined at x=0 (NaN/±inf at runtime)
            SigMatch::Abs(x) => self.unary_chain(
                x,
                |b, primal_x| b.abs(primal_x),
                |b, primal_x, tangent_x| {
                    let denom = b.abs(primal_x);
                    let sign = b.div(primal_x, denom);
                    b.mul(sign, tangent_x)
                },
            ),
            // Rule: d/dp acos(x) = (-1 / √(1-x²)) · x'
            SigMatch::Acos(x) => self.unary_chain(
                x,
                |b, primal_x| b.acos(primal_x),
                |b, primal_x, tangent_x| {
                    let one = b.real(1.0);
                    let x_sq = b.mul(primal_x, primal_x);
                    let inside = b.sub(one, x_sq);
                    let root = b.sqrt(inside);
                    let minus_one = b.real(-1.0);
                    let inv = b.div(minus_one, root);
                    b.mul(inv, tangent_x)
                },
            ),
            // Rule: d/dp asin(x) = (1 / √(1-x²)) · x'
            SigMatch::Asin(x) => self.unary_chain(
                x,
                |b, primal_x| b.asin(primal_x),
                |b, primal_x, tangent_x| {
                    let one = b.real(1.0);
                    let x_sq = b.mul(primal_x, primal_x);
                    let inside = b.sub(one, x_sq);
                    let root = b.sqrt(inside);
                    let inv = b.div(one, root);
                    b.mul(inv, tangent_x)
                },
            ),
            // Rule: d/dp atan(x) = (1 / (1+x²)) · x'
            SigMatch::Atan(x) => self.unary_chain(
                x,
                |b, primal_x| b.atan(primal_x),
                |b, primal_x, tangent_x| {
                    let one = b.real(1.0);
                    let x_sq = b.mul(primal_x, primal_x);
                    let denom = b.add(one, x_sq);
                    let inv = b.div(one, denom);
                    b.mul(inv, tangent_x)
                },
            ),
            // General power rule: d/dp x^y = x^y · (y'·ln(x) + y·x'/x)
            // Handles the case where both base and exponent depend on the control.
            SigMatch::Pow(x, y) => {
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.pow(dual_x.primal, dual_y.primal);
                let log_x = b.log(dual_x.primal);
                let term1 = b.mul(dual_y.tangent, log_x); // y'·ln(x)
                let scaled_dx = b.mul(dual_y.primal, dual_x.tangent);
                let term2 = b.div(scaled_dx, dual_x.primal); // y·x'/x
                let sum = b.add(term1, term2);
                let tangent = b.mul(primal, sum);
                Dual { primal, tangent }
            }
            // Rule: d/dp min(x,y) = select2(x < y, x', y')
            SigMatch::Min(x, y) => {
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.min(dual_x.primal, dual_y.primal);
                let cond = b.lt(dual_x.primal, dual_y.primal);
                let tangent = b.select2(cond, dual_x.tangent, dual_y.tangent);
                Dual { primal, tangent }
            }
            // Rule: d/dp max(x,y) = select2(x > y, x', y')
            SigMatch::Max(x, y) => {
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.max(dual_x.primal, dual_y.primal);
                let cond = b.gt(dual_x.primal, dual_y.primal);
                let tangent = b.select2(cond, dual_x.tangent, dual_y.tangent);
                Dual { primal, tangent }
            }
            // Rule: d/dp delay1(x) = delay1(x')  — unit delay is linear
            SigMatch::Delay1(x) => {
                let dual_x = self.transform(x);
                let mut b = SigBuilder::new(self.arena);
                Dual {
                    primal: b.delay1(dual_x.primal),
                    tangent: b.delay1(dual_x.tangent),
                }
            }
            // Variable-delay Leibniz rule (discrete time):
            //   d/dp x[n−d(n)] = x'[n−d]               (content derivative)
            //                  − d'(n) · ∇x[n−d]        (time derivative)
            // where ∇x[n−d] ≈ x[n−d] − x[n−d−1]
            //               = delay(x, d) − delay(delay1(x), d)
            //               = delay(x − delay1(x), d)
            SigMatch::Delay(x, d) => {
                let dual_x = self.transform(x);
                let dual_d = self.transform(d);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.delay(dual_x.primal, dual_d.primal);
                let term1 = b.delay(dual_x.tangent, dual_d.primal); // x'[n-d]
                let delayed_primal = b.delay1(dual_x.primal);
                let time_gradient = b.sub(dual_x.primal, delayed_primal); // x - delay1(x)
                let delayed_time_gradient = b.delay(time_gradient, dual_d.primal); // ∇x[n-d]
                let scaled_delay = b.mul(dual_d.tangent, delayed_time_gradient); // d' · ∇x[n-d]
                Dual {
                    primal,
                    tangent: b.sub(term1, scaled_delay),
                }
            }
            // Rule: d/dp select2(cond, x, y) = select2(cond, x', y')
            // The condition is not differentiated (treated as a discrete selector).
            SigMatch::Select2(cond, x, y) => {
                let dual_cond = self.transform(cond);
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                Dual {
                    primal: b.select2(dual_cond.primal, dual_x.primal, dual_y.primal),
                    tangent: b.select2(dual_cond.primal, dual_x.tangent, dual_y.tangent),
                }
            }
            // Rule: d/dp prefix(init, x) = prefix(init', x')
            // prefix(a, b)[n] = a when n=0, b[n-1] otherwise — linear in both args.
            SigMatch::Prefix(x, y) => {
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                Dual {
                    primal: b.prefix(dual_x.primal, dual_y.primal),
                    tangent: b.prefix(dual_x.tangent, dual_y.tangent),
                }
            }
            // Rule: d/dp float_cast(x) = float_cast(x')  — linear promotion
            SigMatch::FloatCast(x) => {
                let dual_x = self.transform(x);
                let mut b = SigBuilder::new(self.arena);
                Dual {
                    primal: b.float_cast(dual_x.primal),
                    tangent: b.float_cast(dual_x.tangent),
                }
            }
            // Rule: d/dp int_cast(x) = 0  — floor/truncate is piecewise-constant
            SigMatch::IntCast(x) => {
                let dual_x = self.transform(x);
                let mut b = SigBuilder::new(self.arena);
                Dual {
                    primal: b.int_cast(dual_x.primal),
                    tangent: b.int(0),
                }
            }
            // Rule: d/dp proj(i, g) = proj(i, d(g)/dp)  — projection is linear
            SigMatch::Proj(index, group) => {
                let dual_group = self.transform(group);
                let mut b = SigBuilder::new(self.arena);
                Dual {
                    primal: b.proj(index, dual_group.primal),
                    tangent: b.proj(index, dual_group.tangent),
                }
            }
            // Output wrappers are transparent: differentiate the wrapped signal.
            SigMatch::Output(_, inner) => self.transform(inner),
            // Helper nodes: forward left-operand tangent, drop right-operand tangent.
            // attach(x, y), enable(x, y), control(x, y) are structurally preserved
            // but only x's derivative is emitted.
            SigMatch::Attach(x, y) => self.pass_through_binary(x, y, |b, px, py| b.attach(px, py)),
            SigMatch::Enable(x, y) => self.pass_through_binary(x, y, |b, px, py| b.enable(px, py)),
            SigMatch::Control(x, y) => {
                self.pass_through_binary(x, y, |b, px, py| b.control(px, py))
            }
            // Bargraphs are metering outputs only; their tangent is always zero.
            SigMatch::VBargraph(_, inner) => {
                let _ = self.transform(inner);
                let mut b = SigBuilder::new(self.arena);
                Dual {
                    primal: sig,
                    tangent: b.real(0.0),
                }
            }
            // Fallback: all unhandled nodes (table ops, FFun, soundfile, waveform,
            // Gen, PermVar, TempVar, …) are treated as non-differentiable constants.
            _ => Dual {
                primal: sig,
                tangent: SigBuilder::new(self.arena).real(0.0),
            },
        }
    }

    /// Applies the chain rule for a unary signal node `f(x)`.
    ///
    /// Calls `primal_fn` to build `f(x)` and `tangent_fn` to build `f'(x) · x'`,
    /// both receiving the primal value of `x` and (for `tangent_fn`) its tangent.
    fn unary_chain<FPrimal, FTangent>(
        &mut self,
        x: SigId,
        primal_fn: FPrimal,
        tangent_fn: FTangent,
    ) -> Dual
    where
        FPrimal: FnOnce(&mut SigBuilder<'_>, SigId) -> SigId,
        FTangent: FnOnce(&mut SigBuilder<'_>, SigId, SigId) -> SigId,
    {
        let dual_x = self.transform(x);
        let mut b = SigBuilder::new(self.arena);
        let primal = primal_fn(&mut b, dual_x.primal);
        let tangent = tangent_fn(&mut b, dual_x.primal, dual_x.tangent);
        Dual { primal, tangent }
    }

    /// Rebuilds binary nodes that conceptually forward differentiation through
    /// their left operand while preserving the original binary primal node.
    ///
    /// This matches the current `propagate` plan for helper nodes such as
    /// `attach`, `enable`, and `control`.
    fn pass_through_binary<F>(&mut self, x: SigId, y: SigId, primal_fn: F) -> Dual
    where
        F: FnOnce(&mut SigBuilder<'_>, SigId, SigId) -> SigId,
    {
        let dual_x = self.transform(x);
        let dual_y = self.transform(y);
        let mut b = SigBuilder::new(self.arena);
        Dual {
            primal: primal_fn(&mut b, dual_x.primal, dual_y.primal),
            tangent: dual_x.tangent,
        }
    }

    /// Differentiates one binary arithmetic/logical node.
    ///
    /// | Operator | Tangent rule |
    /// |----------|-------------|
    /// | `Add` | `x' + y'` |
    /// | `Sub` | `x' − y'` |
    /// | `Mul` | `x'·y + x·y'` (product rule) |
    /// | `Div` | `(x'·y − x·y') / y²` (quotient rule) |
    /// | `Rem` | `x' − y'·⌊x/y⌋` |
    /// | shifts, bitwise, comparisons | `0` (non-differentiable integer ops) |
    fn transform_binop(&mut self, op: BinOp, x: SigId, y: SigId) -> Dual {
        let dual_x = self.transform(x);
        let dual_y = self.transform(y);
        let mut b = SigBuilder::new(self.arena);
        let primal = match op {
            BinOp::Add => b.add(dual_x.primal, dual_y.primal),
            BinOp::Sub => b.sub(dual_x.primal, dual_y.primal),
            BinOp::Mul => b.mul(dual_x.primal, dual_y.primal),
            BinOp::Div => b.div(dual_x.primal, dual_y.primal),
            BinOp::Rem => b.rem(dual_x.primal, dual_y.primal),
            BinOp::Lsh => b.lsh(dual_x.primal, dual_y.primal),
            BinOp::ARsh => b.arsh(dual_x.primal, dual_y.primal),
            BinOp::LRsh => b.lrsh(dual_x.primal, dual_y.primal),
            BinOp::Gt => b.gt(dual_x.primal, dual_y.primal),
            BinOp::Lt => b.lt(dual_x.primal, dual_y.primal),
            BinOp::Ge => b.ge(dual_x.primal, dual_y.primal),
            BinOp::Le => b.le(dual_x.primal, dual_y.primal),
            BinOp::Eq => b.eq(dual_x.primal, dual_y.primal),
            BinOp::Ne => b.ne(dual_x.primal, dual_y.primal),
            BinOp::And => b.and(dual_x.primal, dual_y.primal),
            BinOp::Or => b.or(dual_x.primal, dual_y.primal),
            BinOp::Xor => b.xor(dual_x.primal, dual_y.primal),
        };
        let tangent = match op {
            // x' + y'
            BinOp::Add => b.add(dual_x.tangent, dual_y.tangent),
            // x' − y'
            BinOp::Sub => b.sub(dual_x.tangent, dual_y.tangent),
            // x'·y + x·y'
            BinOp::Mul => {
                let t1 = b.mul(dual_x.tangent, dual_y.primal);
                let t2 = b.mul(dual_x.primal, dual_y.tangent);
                b.add(t1, t2)
            }
            // (x'·y − x·y') / y²
            BinOp::Div => {
                let t1 = b.mul(dual_x.tangent, dual_y.primal);
                let t2 = b.mul(dual_x.primal, dual_y.tangent);
                let num = b.sub(t1, t2);
                let den = b.mul(dual_y.primal, dual_y.primal);
                b.div(num, den)
            }
            // x' − y'·⌊x/y⌋   (derivative of floating-point remainder)
            BinOp::Rem => {
                let x_div_y = b.div(dual_x.primal, dual_y.primal);
                let floor = b.floor(x_div_y);
                let term2 = b.mul(dual_y.tangent, floor);
                b.sub(dual_x.tangent, term2)
            }
            // Shifts, bitwise ops, comparisons: integer / discrete — tangent is 0.
            BinOp::Lsh
            | BinOp::ARsh
            | BinOp::LRsh
            | BinOp::Gt
            | BinOp::Lt
            | BinOp::Ge
            | BinOp::Le
            | BinOp::Eq
            | BinOp::Ne
            | BinOp::And
            | BinOp::Or
            | BinOp::Xor => b.int(0),
        };
        Dual { primal, tangent }
    }

    /// Returns the symbolic recursion variable used for tangent groups.
    ///
    /// The generated name follows the current C++-style `FAD_*` convention.
    /// When the source recursion identifier is not a symbol/string literal, a
    /// deterministic fallback name based on the node id is used instead.
    fn fad_var(&mut self, var: TreeId) -> TreeId {
        let name = match self.arena.kind(var) {
            Some(NodeKind::Symbol(text)) | Some(NodeKind::StringLiteral(text)) => {
                format!("FAD_{}", text.as_ref())
            }
            _ => format!("FAD_node_{}", var.as_u32()),
        };
        self.arena.symbol(&name)
    }
}

/// Expands propagated primal outputs into the forward-mode AD output bundle.
///
/// For each input primal output:
/// - first emit the primal itself,
/// - then emit one tangent per reachable differentiable control, in
///   deterministic control order.
///
/// Before differentiation, each output passes through `de_bruijn_to_sym(...)`
/// so the transform works on the canonical symbolic recursion representation
/// expected by the current porting model.
pub(super) fn generate_fad_signals(
    arena: &mut TreeArena,
    outputs: &[SigId],
    ctx: &PropagateContext<'_>,
) -> Result<Vec<SigId>, PropagateError> {
    let converted_outputs: Vec<SigId> = outputs
        .iter()
        .copied()
        .map(|sig| de_bruijn_to_sym(arena, sig).unwrap_or(sig))
        .collect();

    let mut result = Vec::new();
    for out_sig in converted_outputs {
        result.push(out_sig);
        let mut collector = ADControlCollector::new(arena, ctx.ui_controls);
        collector.collect(out_sig);
        collector.sort_controls_by_label();
        for control in collector.controls {
            let mut fad = ForwardADTransform::new(arena, control);
            let dual = fad.transform(out_sig);
            result.push(dual.tangent);
        }
    }
    Ok(result)
}
