//! Forward-mode automatic differentiation helpers for the `propagate` phase.
//!
//! # Source provenance (C++)
//! - `compiler/propagate/propagate.cpp`
//! - `compiler/transform/sigtyperules.hh`
//! - `compiler/signals/signals.cpp`
//!
//! # Scope
//! - Differentiate a list of primal outputs with respect to a list of seed
//!   signals supplied by the caller.
//! - Expand each primal output into `primal + one tangent per seed`.
//! - Preserve shared signal DAG structure through memoized transformation.
//! - Keep reverse-mode (`rad`) explicitly out of scope for this phase.
//!
//! # Dual-number algebra
//! Forward-mode AD (FAD) propagates derivatives alongside values by carrying a
//! *tangent* component next to each signal.  A **dual signal** is written
//! `u + ε·u'` where `ε² = 0`.  A **seed** is any signal `s` chosen by the
//! caller; `ds/ds = 1` and every other independent input has seed `0`.
//!
//! A seed is not restricted to a UI control: it can be any `SigId` in the
//! signal DAG (a slider, a lambda-bound recursive input, an expression).
//! Seed recognition is pure `SigId` equality — the arena hash-conses every
//! node, so every external reference to the seed inside the primal body
//! shares the same `TreeId` as the seed argument and the equality check
//! fires at each occurrence.  The transform short-circuits at that leaf and
//! never descends into the seed's own recursive body.
//!
//! When the seed expression carries free `DEBRUIJNREF` nodes, crossing a
//! `DEBRUIJNREC` scope shifts those references by one level. The transform
//! therefore lifts the seed on entry into each REC body (and restores it on
//! exit) so `SigId` equality keeps matching under the new scope — without
//! this shift, occurrences of the seed inside a recursive body would be
//! silently missed and their tangent would collapse to zero. See
//! [`ForwardADTransform::transform_uncached`] for the mechanics.
//!
//! One [`ForwardADTransform`] instance differentiates the entire signal DAG
//! with respect to a single seed; [`generate_fad_signals_multi`] drives one
//! transformer per seed and assembles the output bundle.
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
//! ## Seeds and UI controls
//!
//! Seed equality is checked *before* node-kind dispatch, so any node whose
//! `SigId` equals the seed returns tangent `1.0` regardless of its kind.
//!
//! | Node | Tangent |
//! |------|---------|
//! | any signal `s` such that `s == seed` | `1.0` |
//! | `hslider` / `vslider` / `numentry` (not the seed) | `0.0` |
//! | `button`, `checkbox` | `0.0` (discrete, not differentiable) |
//!
//! Under the explicit-seed model, `[autodiff:false]` metadata is ignored:
//! whether a signal is differentiated is decided by the caller's seed list,
//! not by per-control annotations. The metadata still parses without error
//! so legacy sources remain accepted.
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
//! ### `atan2(y, x)`
//! ```text
//! d/dp atan2(y, x) = (x·y' − y·x') / (x² + y²)
//! ```
//!
//! ### `fmod(x, y)`
//! ```text
//! d/dp fmod(x, y) = x' − y'·floor(x/y)
//! ```
//! Derivation: `fmod(x,y) = x − y·floor(x/y)`, differentiate both sides.
//!
//! ### `remainder(x, y)`
//! ```text
//! d/dp remainder(x, y) = x' − y'·round(x/y)
//! ```
//! Derivation: `remainder(x,y) = x − y·round(x/y)`, differentiate both sides.
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
//! ## Projection and recursive groups (de Bruijn form)
//!
//! `propagate` always emits recursive groups in de Bruijn form
//! (`DEBRUIJNREC` / `DEBRUIJNREF` tag nodes). The transform never sees
//! symbolic form (`SYMREC` / `SYMREF`); `de_bruijn_to_sym` runs once in
//! `signal_prepare` *after* FAD, over all process outputs together.
//!
//! ### Three interacting quantities
//!
//! FAD's index bookkeeping rests on three distinct notions:
//!
//! - **Level** (payload on `DEBRUIJNREF`): a static integer `k` meaning
//!   "the `k`-th enclosing `DEBRUIJNREC`, innermost = 1". FAD never
//!   rewrites this value — it is a property of the node.
//! - **`debruijn_depth`** (traversal counter on [`ForwardADTransform`]):
//!   incremented only when FAD *enters* a `DEBRUIJNREC` body and
//!   interleaves it (see [`ForwardADTransform::transform_uncached`]).
//!   It counts the RECs that FAD itself has rewritten on the current path
//!   from the top-level output signal being differentiated — not all
//!   RECs in the program.
//! - **Slot index** (integer on a `Proj` node): selects one element from
//!   a REC body. After interleaving `[p0, t0, p1, t1, …]`, the original
//!   index `i` maps to primal slot `2i` and tangent slot `2i+1`.
//!
//! ### `DEBRUIJNREC(body)`
//!
//! The body list is differentiated and the primal/tangent pairs are
//! **interleaved**: `[p0, t0, p1, t1, …]`.  One new `DEBRUIJNREC` node
//! thus carries both the primal and tangent recurrence:
//! ```text
//! d/dp DEBRUIJNREC([e0, e1, …]) =
//!     DEBRUIJNREC([primal(e0), tangent(e0), primal(e1), tangent(e1), …])
//! ```
//! A placeholder is inserted into the cache before recursing so that
//! back-edges inside the body resolve without loop.
//!
//! ### `DEBRUIJNREF`
//! ```text
//! d/dp DEBRUIJNREF = DEBRUIJNREF
//! ```
//! The node itself is unchanged; the index adjustment (if any) is
//! resolved at the **projection site** based on whether the referenced
//! REC was interleaved by FAD or not.
//!
//! ### `Proj(i, group)` — index classification
//!
//! The projection arm inspects `group` and applies one of four rules:
//!
//! | Group | Meaning | Primal index | Tangent |
//! |-------|---------|--------------|---------|
//! | `DEBRUIJNREC` (directly) | FAD just rewrote this REC; body is interleaved | `2i` | `proj(2i+1, …)` |
//! | `DEBRUIJNREF` with `level ≤ debruijn_depth` | Points into a REC that FAD entered and interleaved on this path | `2i` | `proj(2i+1, …)` |
//! | `DEBRUIJNREF` with `level > debruijn_depth` | Points to a REC *enclosing* FAD's entry point (e.g. an outer FxLMS-bank loop): that REC was never interleaved | `i` (unchanged) | `0` |
//! | Other | Unreachable in the de-Bruijn-only pipeline; defensive identity | `i` | `proj(i, …)` |
//!
//! The `level > debruijn_depth` case can only arise when `fad(...)` is
//! applied to an inner sub-expression that contains a `DEBRUIJNREF`
//! whose target REC encloses the call site. FAD never entered that
//! outer REC, so its body was not doubled; the reference keeps its
//! original slot number and its tangent is zero (the outer loop is not
//! being differentiated).
//!
//! The `de_bruijn_to_sym` conversion is deferred to `signal_prepare`, where
//! it runs once over all process outputs through one `Converter` instance.
//! This guarantees that the same `DEBRUIJNREC` physical node maps to the
//! same `SYMREC` name in every primal and tangent lane.
//!
//! ## Pass-through helper nodes (`attach`, `enable`, `control`)
//!
//! These carry a left (signal) and right (side-effect/control) operand.  Only
//! the left operand's tangent is forwarded; the right operand is structurally
//! preserved with its tangent dropped.
//!
//! ## Bargraph outputs (`vbargraph`, `hbargraph`)
//!
//! Zero tangent for both.  Bargraphs are metering outputs, not DSP signal paths.
//!
//! ## Foreign functions (`FFun`)
//!
//! Foreign functions are matched by name against a known-differentiable set.
//! The FFUN descriptor carries one name per precision variant; all slots are
//! checked so the match is precision-agnostic (`tanhf` / `tanh` / `tanhl`
//! all identify the same mathematical operation).
//!
//! | Function | Names (`f32` \| `f64` \| `ldbl`) | Tangent rule | Notes |
//! |----------|----------------------------------|--------------|-------|
//! | `tanh(x)` | `tanhf` / `tanh` / `tanhl` | `x' · (1 − tanh²(x))` | = `x' · sech²(x)`; from primal |
//! | `sinh(x)` | `sinhf` / `sinh` / `sinhl` | `x' · cosh(x)` | `cosh` via `sqrt(1 + sinh²(x))` |
//! | `cosh(x)` | `coshf` / `cosh` / `coshl` | `x' · sinh(x)` | `sinh` via `(exp(x)−exp(−x))/2` |
//! | `atanh(x)` | `atanhf` / `atanh` / `atanhl` | `x' / (1 − x²)` | |
//! | `asinh(x)` | `asinhf` / `asinh` / `asinhl` | `x' / sqrt(1 + x²)` | |
//! | `acosh(x)` | `acoshf` / `acosh` / `acoshl` | `x' / sqrt(x² − 1)` | |
//!
//! `sinh` and `cosh` are mutual dependencies; each derivative uses the other
//! function.  Since both are external `ffunction` calls with no built-in
//! `SigBuilder` equivalent, the dependency is broken algebraically:
//! - `sinh` arm: `cosh(x) = sqrt(1 + sinh²(x))` — exact since `cosh ≥ 0`.
//! - `cosh` arm: `sinh(x) = (exp(x) − exp(−x)) / 2` — exact, sign-preserving.
//!
//! Unrecognized FFun calls (any foreign function not in the table above),
//! table reads/writes (`rdtbl`, `wrtbl`), soundfile accessors, waveforms,
//! on-demand/up/downsampling, `Gen`, `PermVar`, `TempVar`, and all other
//! unmatched variants fall through to zero tangent with the primal unchanged.
//!
//! # Integration contract
//! This module is intentionally internal to `propagate`:
//! - `box_arity_typed(...)` reports expanded output arity for `fad(expr, seeds…)`,
//! - output expansion happens only after the wrapped box has already lowered to
//!   signal IR,
//! - seeds are user-supplied through `fad(expr, seed)`; the seed
//!   sub-expression is lowered to signals like any other box and its outputs
//!   become the seed list. `[autodiff:false]` metadata is not consulted.
//!
//! # Ordering invariant
//! Tangent outputs are emitted deterministically:
//! 1. preserve primal output order,
//! 2. for each primal, emit one tangent per seed in the seed list's order.

use ahash::AHashMap;
use signals::{BinOp, SigBuilder, SigId, SigMatch, match_sig};
use tlib::{
    NodeKind, TreeArena, TreeId, de_bruijn_rec, lift_de_bruijn, list_to_vec, match_de_bruijn_rec,
    match_de_bruijn_ref, tree_to_str, vec_to_list,
};

use crate::PropagateError;

/// Internal dual-number carrier used while differentiating one signal graph.
///
/// `primal` is the original signal expression. `tangent` is the derivative of
/// that expression with respect to a single selected control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Dual {
    primal: SigId,
    tangent: SigId,
}

/// Memoized forward-mode transformer for one selected differentiation seed signal.
///
/// One transformer instance computes `d(signal) / d(seed)` across a shared
/// signal DAG. The cache prevents exponential blow-up on reused subgraphs and
/// also breaks recursion cycles while rebuilding `sigRec/sigProj`-style groups.
struct ForwardADTransform<'a> {
    arena: &'a mut TreeArena,
    diff_seed: SigId,
    cache: AHashMap<SigId, Dual>,
    debruijn_depth: i64,
}

impl<'a> ForwardADTransform<'a> {
    /// Creates one transformer for the selected differentiation seed signal.
    fn new(arena: &'a mut TreeArena, diff_seed: SigId) -> Self {
        Self {
            arena,
            diff_seed,
            cache: AHashMap::new(),
            debruijn_depth: 0,
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
    /// Handles de Bruijn tag nodes before the `SigMatch` dispatch:
    ///
    /// - `DEBRUIJNREF` → tangent is the same node (index shift at projection site).
    /// - `DEBRUIJNREC(body)` → tangent is a new `DEBRUIJNREC` with interleaved
    ///   `[primal, tangent]` pairs; a placeholder is inserted into the cache
    ///   before recursing to handle back-edges correctly.
    ///
    /// All other nodes are dispatched through `SigMatch`; unsupported or
    /// non-differentiable nodes fall back to a zero tangent.
    fn transform_uncached(&mut self, sig: SigId) -> Dual {
        // Seed signal: d(seed)/d(seed) = 1
        if sig == self.diff_seed {
            return Dual {
                primal: sig,
                tangent: SigBuilder::new(self.arena).real(1.0),
            };
        }

        // Rule: d/dp DEBRUIJNREF = DEBRUIJNREF (index shift happens at projection site)
        if match_de_bruijn_ref(self.arena, sig).is_some() {
            return Dual {
                primal: sig,
                tangent: sig,
            };
        }
        // Rule: d/dp DEBRUIJNREC(body) — interleave primals and tangents so that
        //       proj(i, rec) picks primal from slot 2i and tangent from slot 2i+1.
        if let Some(body) = match_de_bruijn_rec(self.arena, sig) {
            // Seed cache before recursing to handle back-edges.
            self.cache.insert(
                sig,
                Dual {
                    primal: sig,
                    tangent: sig,
                },
            );

            self.debruijn_depth += 1;

            // Seed lifting across DEBRUIJNREC scopes.
            //
            // Seed recognition is by `SigId` equality. When the seed expression
            // itself contains free `DEBRUIJNREF(k)` nodes pointing at some
            // enclosing REC, the *same* logical reference, viewed from inside
            // the body we're about to enter, has level `k+1` — one more REC
            // stands between the reference and its binder. Hash-consing means
            // those two de Bruijn references are distinct `SigId`s, so without
            // adjustment the seed-equality check inside the body would miss
            // every occurrence and the tangent would be silently zero.
            //
            // `lift_de_bruijn` shifts every free de Bruijn reference in the
            // seed up by one level. After the recursion into the body we
            // restore the previous seed so siblings and enclosing scopes keep
            // matching against the unshifted form.
            let old_seed = self.diff_seed;
            self.diff_seed = lift_de_bruijn(self.arena, self.diff_seed);

            let duals = self.transform_list(body);

            self.diff_seed = old_seed;
            self.debruijn_depth -= 1;

            let mut expanded_body = Vec::with_capacity(duals.len() * 2);
            for dual in duals {
                expanded_body.push(dual.primal);
                expanded_body.push(dual.tangent);
            }
            let list_node = vec_to_list(self.arena, &expanded_body);
            let fad_rec = de_bruijn_rec(self.arena, list_node);
            return Dual {
                primal: fad_rec,
                tangent: fad_rec,
            };
        }

        match match_sig(self.arena, sig) {
            // Constants: d/dp c = 0
            SigMatch::Int(_) => Dual {
                primal: sig,
                tangent: SigBuilder::new(self.arena).int(0),
            },
            SigMatch::Real(_) => self.zero_tangent(sig),
            // Audio inputs are independent of all UI controls: d/dp input = 0
            SigMatch::Input(_) => self.zero_tangent(sig),
            // Continuous UI controls: tangent = 0 (seed equality handled above).
            SigMatch::HSlider(_) | SigMatch::VSlider(_) | SigMatch::NumEntry(_) => {
                self.zero_tangent(sig)
            }
            // Discrete UI controls are not differentiable: d/dp button = 0
            SigMatch::Button(_) | SigMatch::Checkbox(_) => self.zero_tangent(sig),
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
                let tangent = b.select2(cond, dual_y.tangent, dual_x.tangent);
                Dual { primal, tangent }
            }
            // Rule: d/dp max(x,y) = select2(x > y, x', y')
            SigMatch::Max(x, y) => {
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.max(dual_x.primal, dual_y.primal);
                let cond = b.gt(dual_x.primal, dual_y.primal);
                let tangent = b.select2(cond, dual_y.tangent, dual_x.tangent);
                Dual { primal, tangent }
            }
            // Rule: d/dp atan2(y, x) = (x·y' - y·x') / (x² + y²)
            SigMatch::Atan2(y, x) => {
                let dual_y = self.transform(y);
                let dual_x = self.transform(x);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.atan2(dual_y.primal, dual_x.primal);
                let x_dy = b.mul(dual_x.primal, dual_y.tangent);
                let y_dx = b.mul(dual_y.primal, dual_x.tangent);
                let num = b.sub(x_dy, y_dx);
                let x2 = b.mul(dual_x.primal, dual_x.primal);
                let y2 = b.mul(dual_y.primal, dual_y.primal);
                let denom = b.add(x2, y2);
                Dual {
                    primal,
                    tangent: b.div(num, denom),
                }
            }
            // Rule: d/dp fmod(x, y) = x' - y'·floor(x/y)
            // Derivation: fmod(x,y) = x - y·floor(x/y), differentiate both sides.
            SigMatch::Fmod(x, y) => {
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.fmod(dual_x.primal, dual_y.primal);
                let raw_q = b.div(dual_x.primal, dual_y.primal);
                let quotient = b.floor(raw_q);
                let scaled = b.mul(dual_y.tangent, quotient);
                Dual {
                    primal,
                    tangent: b.sub(dual_x.tangent, scaled),
                }
            }
            // Rule: d/dp remainder(x, y) = x' - y'·round(x/y)
            // Derivation: remainder(x,y) = x - y·round(x/y), differentiate both sides.
            SigMatch::Remainder(x, y) => {
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.remainder(dual_x.primal, dual_y.primal);
                let raw_q = b.div(dual_x.primal, dual_y.primal);
                let quotient = b.round(raw_q);
                let scaled = b.mul(dual_y.tangent, quotient);
                Dual {
                    primal,
                    tangent: b.sub(dual_x.tangent, scaled),
                }
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
            // Rule: d/dp proj(i, g) = proj(i, d(g)/dp)  — projection is linear,
            // but the slot index must be adjusted to account for FAD's interleaving
            // of DEBRUIJNREC bodies into [p0, t0, p1, t1, ...].
            //
            // See the "Projection and recursive groups" section of the module
            // docstring for the full scheme (level vs debruijn_depth vs slot index).
            SigMatch::Proj(index, group) => {
                let dual_group = self.transform(group);
                // Classify the group node to pick the index/tangent rule.
                //
                // `BoundRec`   — group is a DEBRUIJNREC whose body was interleaved
                //                by the DEBRUIJNREC arm above, or a DEBRUIJNREF
                //                whose level ≤ debruijn_depth (i.e. it resolves to
                //                a REC that FAD entered and doubled on this path).
                //                Primal slot = 2i, tangent slot = 2i+1.
                //
                // `UnboundRef` — group is a DEBRUIJNREF with level > debruijn_depth:
                //                it refers to a REC enclosing FAD's entry point
                //                (e.g. the 64-tap outer loop in an FxLMS bank when
                //                fad() is applied to the inner expression). That
                //                REC was never interleaved, so the slot index stays
                //                unchanged and the tangent is zero — the outer
                //                loop is not being differentiated.
                //
                // `Other`      — defensive fallback: any group that is neither a
                //                DEBRUIJNREC nor a DEBRUIJNREF tag node. The
                //                propagate→FAD pipeline only emits de Bruijn form
                //                (SYMREC/SYMREF appear only after de_bruijn_to_sym,
                //                which runs *after* FAD), so this branch is
                //                unreachable on well-formed input. Kept as an
                //                identity projection to keep the transform total.
                enum GroupKind {
                    BoundRec,
                    UnboundRef,
                    Other,
                }
                let kind = if match_de_bruijn_rec(self.arena, group).is_some() {
                    GroupKind::BoundRec
                } else if let Some(level) = match_de_bruijn_ref(self.arena, group) {
                    if level <= self.debruijn_depth {
                        GroupKind::BoundRec
                    } else {
                        GroupKind::UnboundRef
                    }
                } else {
                    GroupKind::Other
                };
                let mut b = SigBuilder::new(self.arena);
                match kind {
                    GroupKind::BoundRec => Dual {
                        primal: b.proj(index * 2, dual_group.primal),
                        tangent: b.proj(index * 2 + 1, dual_group.tangent),
                    },
                    GroupKind::UnboundRef => Dual {
                        primal: b.proj(index, dual_group.primal),
                        tangent: b.real(0.0),
                    },
                    GroupKind::Other => Dual {
                        primal: b.proj(index, dual_group.primal),
                        tangent: b.proj(index, dual_group.tangent),
                    },
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
            // We still transform `inner` so the cache is populated for any
            // subgraphs it shares with differentiated signals.
            SigMatch::VBargraph(_, inner) | SigMatch::HBargraph(_, inner) => {
                let _ = self.transform(inner);
                self.zero_tangent(sig)
            }
            // Foreign functions: dispatch on name for known differentiable functions.
            // The FFUN descriptor carries a names list [f32_name, f64_name, …]; we
            // check every slot against the target set so the match is precision-agnostic.
            SigMatch::FFun(ff, largs) => self.transform_ffun(sig, ff, largs),
            // Fallback: all unhandled nodes (table ops, unrecognized FFun, soundfile,
            // waveform, Gen, PermVar, TempVar, …) are non-differentiable constants.
            _ => self.zero_tangent(sig),
        }
    }

    /// Returns `true` if the FFUN descriptor `ff` carries any name from `targets`.
    ///
    /// The FFUN descriptor node has tag `"FFUN"` and children
    /// `[signature, incfile, libfile]`.  The signature cons-list has layout
    /// `[ret_type, names_list, arg0_type, …]`, where `names_list` holds one
    /// symbol per precision variant (f32 index 0, f64 index 1, long-double index 2).
    /// Checking all slots makes the match precision-agnostic.
    fn ffun_is(arena: &TreeArena, ff: SigId, targets: &[&str]) -> bool {
        let Some(node) = arena.node(ff) else {
            return false;
        };
        let NodeKind::Tag(tag_id) = node.kind else {
            return false;
        };
        if arena.tag_name(tag_id) != Some("FFUN") {
            return false;
        }
        let [signature, _, _] = node.children.as_slice() else {
            return false;
        };
        let Some(sig_items) = list_to_vec(arena, *signature) else {
            return false;
        };
        let Some(names_node) = sig_items.get(1) else {
            return false;
        };
        let Some(name_ids) = list_to_vec(arena, *names_node) else {
            return false;
        };
        name_ids
            .iter()
            .any(|id| tree_to_str(arena, *id).is_some_and(|n| targets.contains(&n)))
    }

    /// Returns a `Dual` that carries `sig` as primal with a zero tangent.
    ///
    /// Used for every node kind whose derivative is identically zero
    /// (constants, audio inputs, non-seed UI controls, bargraphs, unmatched
    /// foreign functions, and the generic fallback).
    fn zero_tangent(&mut self, sig: SigId) -> Dual {
        Dual {
            primal: sig,
            tangent: SigBuilder::new(self.arena).real(0.0),
        }
    }

    /// Chain rule for a unary foreign function `f(x)` of known derivative.
    ///
    /// Rebuilds `primal = ff(x_primal)` with a fresh args list and delegates
    /// the tangent computation to `tangent_fn`, which receives `(builder,
    /// primal, x_primal, x_tangent)`. Passing `primal` lets rules like
    /// `tanh` reuse `tanh(x)` itself (`x' · (1 − tanh²(x))`) without a second
    /// `ffun` call.
    fn ffun_unary_chain<FTangent>(&mut self, ff: SigId, arg: SigId, tangent_fn: FTangent) -> Dual
    where
        FTangent: FnOnce(&mut SigBuilder<'_>, SigId, SigId, SigId) -> SigId,
    {
        let dual_x = self.transform(arg);
        let largs = vec_to_list(self.arena, &[dual_x.primal]);
        let mut b = SigBuilder::new(self.arena);
        let primal = b.ffun(ff, largs);
        let tangent = tangent_fn(&mut b, primal, dual_x.primal, dual_x.tangent);
        Dual { primal, tangent }
    }

    /// Dispatches a foreign-function node on its name for the known
    /// differentiable set. Unary FFUNs with an unrecognized name and
    /// every FFUN of different arity fall through to a zero tangent.
    ///
    /// The FFUN descriptor carries a names list `[f32_name, f64_name, …]`;
    /// [`Self::ffun_is`] checks every slot so the match is precision-agnostic.
    fn transform_ffun(&mut self, sig: SigId, ff: SigId, largs: SigId) -> Dual {
        let args = list_to_vec(self.arena, largs).unwrap_or_default();
        if args.len() != 1 {
            return self.zero_tangent(sig);
        }
        let arg = args[0];
        if Self::ffun_is(self.arena, ff, &["tanhf", "tanh", "tanhl"]) {
            // d/dp tanh(x) = x' · (1 − tanh²(x))   [= x' · sech²(x)]
            self.ffun_unary_chain(ff, arg, |b, primal, _x, tx| {
                let tanh_sq = b.mul(primal, primal);
                let one = b.real(1.0);
                let sech_sq = b.sub(one, tanh_sq);
                b.mul(sech_sq, tx)
            })
        } else if Self::ffun_is(self.arena, ff, &["sinhf", "sinh", "sinhl"]) {
            // d/dp sinh(x) = x' · cosh(x).  cosh is also an external ffunction,
            // so we derive it from cosh(x) = sqrt(1 + sinh²(x)), reusing
            // primal = sinh(x).  Exact since cosh ≥ 0.
            self.ffun_unary_chain(ff, arg, |b, primal, _x, tx| {
                let sinh_sq = b.mul(primal, primal);
                let one = b.real(1.0);
                let one_plus_sq = b.add(one, sinh_sq);
                let cosh_x = b.sqrt(one_plus_sq);
                b.mul(cosh_x, tx)
            })
        } else if Self::ffun_is(self.arena, ff, &["coshf", "cosh", "coshl"]) {
            // d/dp cosh(x) = x' · sinh(x).  sinh is also an external ffunction;
            // compute via the exp identity sinh(x) = (exp(x) − exp(−x)) / 2.
            self.ffun_unary_chain(ff, arg, |b, _primal, x, tx| {
                let exp_x = b.exp(x);
                let minus_one = b.real(-1.0);
                let neg_x = b.mul(minus_one, x);
                let exp_neg_x = b.exp(neg_x);
                let diff = b.sub(exp_x, exp_neg_x);
                let half = b.real(0.5);
                let sinh_x = b.mul(half, diff);
                b.mul(sinh_x, tx)
            })
        } else if Self::ffun_is(self.arena, ff, &["atanhf", "atanh", "atanhl"]) {
            // d/dp atanh(x) = x' / (1 − x²)
            self.ffun_unary_chain(ff, arg, |b, _primal, x, tx| {
                let x_sq = b.mul(x, x);
                let one = b.real(1.0);
                let denom = b.sub(one, x_sq);
                b.div(tx, denom)
            })
        } else if Self::ffun_is(self.arena, ff, &["asinhf", "asinh", "asinhl"]) {
            // d/dp asinh(x) = x' / sqrt(1 + x²)
            self.ffun_unary_chain(ff, arg, |b, _primal, x, tx| {
                let x_sq = b.mul(x, x);
                let one = b.real(1.0);
                let sum = b.add(one, x_sq);
                let denom = b.sqrt(sum);
                b.div(tx, denom)
            })
        } else if Self::ffun_is(self.arena, ff, &["acoshf", "acosh", "acoshl"]) {
            // d/dp acosh(x) = x' / sqrt(x² − 1)
            self.ffun_unary_chain(ff, arg, |b, _primal, x, tx| {
                let x_sq = b.mul(x, x);
                let one = b.real(1.0);
                let diff = b.sub(x_sq, one);
                let denom = b.sqrt(diff);
                b.div(tx, denom)
            })
        } else {
            self.zero_tangent(sig)
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
}

/// Expands propagated primal outputs into the forward-mode AD output bundle.
///
/// Output layout: `[p1, t1_s1, t1_s2, …, p2, t2_s1, t2_s2, …]` where
/// `sK` ranges over `seeds` in order. One tangent per seed per primal.
/// A single-output seed degenerates to the canonical `[primal, tangent]`
/// pair; multi-output seeds bundle several independent differentiation
/// variables through a single `fad` node.
///
/// No pre-conversion is applied. Seed recognition is by `SigId` equality:
/// every external reference to the seed in the body shares one `TreeId`
/// because the arena hash-conses every node. The transform short-circuits at
/// the seed leaf and never enters the seed's own recursive body.
///
/// The `de_bruijn_to_sym` conversion is deferred to `signal_prepare`, where
/// it runs once over all process outputs through one `Converter` instance.
/// This guarantees that the same `DEBRUIJNREC` sub-term maps to the same
/// `SYMREC` name in every primal and tangent lane, preventing the
/// fresh-name drift that triggered the nested-FAD bug.
pub(super) fn generate_fad_signals_multi(
    arena: &mut TreeArena,
    outputs: &[SigId],
    seeds: &[SigId],
) -> Result<Vec<SigId>, PropagateError> {
    if seeds.is_empty() {
        return Ok(outputs.to_vec());
    }
    // Compute one tangent row per seed directly on the de Bruijn signals.
    // tangent_rows[j][i] = tangent of output i with respect to seed j.
    let mut tangent_rows: Vec<Vec<SigId>> = Vec::with_capacity(seeds.len());
    for &seed in seeds {
        let mut fad = ForwardADTransform::new(arena, seed);
        let row: Vec<SigId> = outputs.iter().map(|&s| fad.transform(s).tangent).collect();
        tangent_rows.push(row);
    }
    let mut result = Vec::with_capacity(outputs.len() * (1 + seeds.len()));
    for (i, &p) in outputs.iter().enumerate() {
        result.push(p);
        for row in &tangent_rows {
            result.push(row[i]);
        }
    }
    Ok(result)
}
