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
//! # Current architecture
//! [`ForwardADTransform`] carries one tangent lane per selected seed inside
//! one internal dual-number carrier, and [`generate_fad_signals_multi`]
//! drives one transformer for the whole seed set. This keeps the
//! differentiation rules, recursion bookkeeping, and seed lifting expressed
//! once in the transformer and lets recursive groups share one interleaved
//! `DEBRUIJNREC` instead of rebuilding one private primal shadow per seed.
//!
//! Internally, one differentiated signal is represented as:
//!
//! ```text
//! Dual {
//!   primal,
//!   tangents = [d/ds0, d/ds1, …, d/ds{N-1}]
//! }
//! ```
//!
//! where the seed order is exactly the order of the lowered seed outputs passed
//! by `fad(expr, seed_box)`.
//!
//! # Dual-number algebra
//! Forward-mode AD (FAD) propagates derivatives alongside values by carrying a
//! *tangent* component next to each signal. A **dual signal** is written
//! `u + ε·u'` where `ε² = 0`. A **seed** is any signal `s` chosen by the
//! caller; `ds/ds = 1` and every other independent input has seed `0`.
//!
//! A seed is not restricted to a UI control: it can be any `SigId` in the
//! signal DAG (a slider, a lambda-bound recursive input, an expression).
//! Seed recognition is pure `SigId` equality — the arena hash-conses every
//! node, so every external reference to the seed inside the primal body
//! shares the same `TreeId` as the seed argument and the equality check
//! fires at each occurrence. The transform short-circuits at that leaf and
//! never descends into the seed's own recursive body.
//!
//! Repeated seeds are preserved by lane index, not deduplicated semantically:
//! if the seed box lowers to `[s, s]`, the differentiated output bundle still
//! exposes two tangent lanes in that same order.
//!
//! When the seed expression carries free `DEBRUIJNREF` nodes, crossing a
//! `DEBRUIJNREC` scope shifts those references by one level. The transform
//! therefore lifts every active seed on entry into each REC body (and
//! restores the previous seed vector on exit) so `SigId` equality keeps
//! matching under the new scope.
//!
//! The transform maintains two internal seed views:
//!
//! - `diff_seeds: Vec<SigId>` preserves deterministic lane order,
//! - `diff_seed_index: AHashMap<SigId, SmallVec<[usize; 2]>>` provides the
//!   reverse lookup from one `SigId` to one or more tangent lanes.
//!
//! This keeps seed recognition explicit while avoiding a linear scan over the
//! seed list at every visited node.
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
//! `SigId` equals a selected seed returns `1.0` on the matching tangent lane
//! and `0.0` on every other lane.
//!
//! | Node | Tangent |
//! |------|---------|
//! | any signal `s` such that `s == seed_j` | lane `j` = `1.0`, every other lane = `0.0` |
//! | `hslider` / `vslider` / `numentry` (not a seed) | `0.0` |
//! | `button`, `checkbox` | `0.0` (discrete, not differentiable) |
//!
//! Under the explicit-seed model, `[autodiff:false]` metadata is ignored:
//! whether a signal is differentiated is decided by the caller's seed list,
//! not by per-control annotations. The metadata still parses without error
//! so legacy sources remain accepted.
//!
//! The zero-tangent treatment for controls is intentional and not just a
//! parser limitation:
//!
//! - `hslider` / `vslider` / `numentry` are ordinary continuous signals when
//!   they are selected as explicit seeds; otherwise they are treated as
//!   independent of the active differentiation variables;
//! - `button` / `checkbox` are discrete state/control sources, so the current
//!   FAD pass models them as piecewise-constant and returns zero tangents;
//! - `vbargraph` / `hbargraph` are metering sinks, not differentiated signal
//!   paths.
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
//! Note: `abs` is not differentiable at `x = 0`; the expression `x/|x|`
//! has undefined behaviour there and produces `NaN` or `±inf` at runtime.
//!
//! ## Binary math (`pow`, `min`, `max`)
//!
//! ### `pow(x, y)`
//! ```text
//! d/dp x^y = x^y · (y' · ln(x) + y · x' / x)
//! ```
//! Both `x` and `y` may depend on the active seeds. The primal term `x^y` and
//! the common scalar factors are hoisted once, then one tangent lane is emitted
//! per seed.
//!
//! ### `min(x, y)` / `max(x, y)`
//! ```text
//! d/dp min(x,y) = select2(x < y, x', y')
//! d/dp max(x,y) = select2(x > y, x', y')
//! ```
//! The selector comes from the primal comparison. The tangent is piecewise
//! constant and uses the chosen branch's tangent lane.
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
//!
//! ### `remainder(x, y)`
//! ```text
//! d/dp remainder(x, y) = x' − y'·round(x/y)
//! ```
//!
//! ## Foreign functions (`FFun`)
//!
//! Foreign functions are matched by name against a known-differentiable set.
//! The FFUN descriptor carries one name per precision variant; all slots are
//! checked so the match is precision-agnostic (`tanhf` / `tanh` / `tanhl`
//! all identify the same mathematical operation).
//!
//! Recognized unary rules:
//!
//! | Function | Names (`f32` \| `f64` \| `ldbl`) | Tangent rule | Notes |
//! |----------|----------------------------------|--------------|-------|
//! | `tanh(x)` | `tanhf` / `tanh` / `tanhl` | `x' · (1 − tanh²(x))` | computed from primal |
//! | `sinh(x)` | `sinhf` / `sinh` / `sinhl` | `x' · cosh(x)` | `cosh(x)` rebuilt as `sqrt(1 + sinh²(x))` |
//! | `cosh(x)` | `coshf` / `cosh` / `coshl` | `x' · sinh(x)` | `sinh(x)` rebuilt from exponentials |
//! | `atanh(x)` | `atanhf` / `atanh` / `atanhl` | `x' / (1 − x²)` | |
//! | `asinh(x)` | `asinhf` / `asinh` / `asinhl` | `x' / sqrt(1 + x²)` | |
//! | `acosh(x)` | `acoshf` / `acosh` / `acoshl` | `x' / sqrt(x² − 1)` | |
//!
//! Unrecognized FFUN calls, non-unary FFUNs, table reads/writes, soundfile
//! accessors, waveforms, on-demand up/downsampling, `Gen`, `PermVar`,
//! `TempVar`, and every other unmatched signal variant fall through to a zero
//! tangent with the primal unchanged.
//!
//! ## Cast operators
//!
//! | Node | Tangent |
//! |------|---------|
//! | `float_cast(x)` | `float_cast(x')` |
//! | `int_cast(x)` | `0` (piecewise-constant step function) |
//! | `bit_cast(x)` | `0` (reinterpret-cast, semantically opaque) |
//!
//! These choices are deliberate approximation boundaries:
//!
//! - `float_cast` preserves the underlying numeric value and therefore forwards
//!   the tangent through the cast;
//! - `int_cast` introduces discontinuous truncation/rounding semantics, so the
//!   derivative is treated as zero rather than trying to model impulses at
//!   integer boundaries;
//! - `bit_cast` changes representation rather than mathematical value and is
//!   therefore outside the differentiable subset.
//!
//! ## Delay operators
//!
//! ### Unit delay `delay1(x)`
//! ```text
//! d/dp delay1(x) = delay1(x')
//! ```
//!
//! ### Variable delay `delay(x, d)`
//! ```text
//! d/dp x[n − d(n)] = x'[n − d(n)]
//!                  − d'(n) · ∇x[n − d(n)]
//! ```
//! where `∇x[n−d]` is approximated as `delay(x − delay1(x), d)`.
//!
//! ## Control-flow nodes
//!
//! | Node | Tangent |
//! |------|---------|
//! | `select2(cond, x, y)` | `select2(cond, x', y')` |
//! | `prefix(init, x)` | `prefix(init', x')` |
//!
//! ## Projection and recursive groups (de Bruijn form)
//!
//! `propagate` always emits recursive groups in de Bruijn form
//! (`DEBRUIJNREC` / `DEBRUIJNREF` tag nodes). The transform never sees
//! symbolic form (`SYMREC` / `SYMREF`); `de_bruijn_to_sym` runs once in
//! `signal_prepare` *after* FAD, over all process outputs together.
//!
//! FAD's index bookkeeping rests on three distinct notions:
//!
//! - **Level** (payload on `DEBRUIJNREF`): a static integer `k` meaning
//!   "the `k`-th enclosing `DEBRUIJNREC`, innermost = 1".
//! - **`debruijn_depth`** (traversal counter on [`ForwardADTransform`]):
//!   incremented only when FAD enters a `DEBRUIJNREC` body that it rewrites.
//! - **Slot index** (integer on a `Proj` node): selects one element from
//!   a REC body.
//!
//! For a transformer carrying `N` seed lanes, every differentiated recursion
//! body is interleaved as:
//!
//! ```text
//! [p0, t0_s0, t0_s1, …, p1, t1_s0, t1_s1, …]
//! ```
//!
//! so the original slot `i` maps to:
//!
//! - primal slot `i * (1 + N)`
//! - tangent slot `i * (1 + N) + 1 + seed_index`
//!
//! The projection arm classifies the group into three semantic cases:
//!
//! | Group | Meaning | Primal projection | Tangent projection |
//! |-------|---------|-------------------|--------------------|
//! | `DEBRUIJNREC` directly | the current transform just rebuilt this recursion | `proj(i * (1 + N), …)` | `proj(i * (1 + N) + 1 + lane, …)` |
//! | `DEBRUIJNREF(level <= debruijn_depth)` | points into a recursion already rewritten on this path | same as above | same as above |
//! | `DEBRUIJNREF(level > debruijn_depth)` | points to an enclosing recursion that this transform never entered | `proj(i, …)` | `0` on every lane |
//! | Other | defensive fallback outside the expected de Bruijn-only flow | `proj(i, …)` | `proj(i, …)` lane-wise |
//!
//! The `level > debruijn_depth` case is the subtle one. It means the current
//! transform is differentiating inside an inner recursion, but the projection
//! points to an outer recursion group whose body has not been rewritten by this
//! transformer instance on the current descent path. In that situation the
//! outer group keeps its original slot numbering, so the primal projection is
//! forwarded unchanged and the tangent lanes are forced to zero.
//!
//! Conceptually:
//!
//! ```text
//! outer = rec([...])
//! inner = rec([ ..., proj(slot_i, ref(level = 2)), ... ])
//! ```
//!
//! While differentiating `inner`, `proj(slot_i, ref(level = 2))` still means
//! "read the already-existing primal slot `slot_i` from `outer`". No
//! differentiated `(1 + N)` expansion is available there unless the transform
//! has also entered and rebuilt `outer` on this path.
//!
//! A placeholder is inserted into the cache before descending into the REC body
//! so back-edges can resolve while the final interleaved node is still being
//! rebuilt. The placeholder is strictly internal to that recursive descent.
//! It is never an externally visible semantic result:
//!
//! - it breaks the cycle while recursive slots are being interned,
//! - it is replaced by the finished interleaved `DEBRUIJNREC` before the
//!   recursive descent returns,
//! - every public output projects from the rebuilt recursion, not the
//!   placeholder shape.
//!
//! The `de_bruijn_to_sym` conversion is deferred to `signal_prepare`, where it
//! runs once over all process outputs through one `Converter` instance. This
//! guarantees that the same `DEBRUIJNREC` physical node maps to the same
//! `SYMREC` name in every primal and tangent lane.
//!
//! ## Pass-through helper nodes (`attach`, `enable`, `control`, `Output`)
//!
//! These carry a left (signal) and right (side-effect/control) operand. Only
//! the left operand's tangent is forwarded; the right operand is structurally
//! preserved with its tangent dropped. `Output` is similarly transparent and
//! differentiates only the wrapped signal.
//!
//! ## Bargraph outputs (`vbargraph`, `hbargraph`)
//!
//! Zero tangent for both. Bargraphs are metering outputs, not DSP signal paths.
//!
//! # Zero-tangent fallback boundary
//! Several signal families are intentionally outside the current
//! differentiable subset and therefore preserve the primal while emitting zero
//! tangents on every lane.
//!
//! | Family | Current treatment | Rationale |
//! |--------|-------------------|-----------|
//! | integer comparisons / bitwise / shifts | zero tangent | discrete integer semantics |
//! | `button`, `checkbox` | zero tangent | event-like / piecewise-constant control |
//! | `int_cast`, `bit_cast` | zero tangent | discontinuous or representation-level operation |
//! | unknown `FFun` / non-unary `FFun` | zero tangent | no trusted derivative rule in this pass |
//! | tables, soundfiles, waveforms, `Gen`, `PermVar`, `TempVar` | zero tangent | not yet modeled as differentiable primitives |
//! | unmatched / defensive fallback variants | zero tangent | preserve compilation robustness while keeping the primal |
//!
//! This is a support boundary, not a claim that the mathematical derivative is
//! literally zero in all cases. The invariant of the pass is narrower: when a
//! signal family has no explicit forward rule, compilation stays defined and
//! the primal is preserved instead of fabricating an unverified tangent.
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
//!
//! # Recursive code generation note
//! The unified multi-seed transform reuses the primal slot already present in
//! the differentiated recursive group. For recursive programs this removes the
//! previous "one AD-local primal shadow recursion per seed" pattern and makes
//! primal/tangent outputs project into the same interleaved `DEBRUIJNREC`.
//!
//! For example, the single-seed program
//!
//! ```text
//! process = fad((2 : + ~ *(p)), p);
//! ```
//!
//! now lowers conceptually to one recursive group:
//!
//! ```text
//! [ y[n] = p * y[n-1] + 2,
//!   dy/dp[n] = y[n-1] + p * dy/dp[n-1] ]
//! ```
//!
//! instead of:
//!
//! ```text
//! [ original primal recursion ] + [ duplicated AD-local primal recursion + tangent ]
//! ```
//!
//! The result is the same DSP semantics with less duplicated recursive state in
//! the emitted code.

use ahash::AHashMap;
use signals::{BinOp, SigBuilder, SigId, SigMatch, match_sig};
use smallvec::SmallVec;
use tlib::{
    NodeKind, TreeArena, TreeId, de_bruijn_rec, lift_de_bruijn, list_to_vec, match_de_bruijn_rec,
    match_de_bruijn_ref, tree_to_str, vec_to_list,
};

use crate::PropagateError;

/// Internal dual-number carrier used while differentiating one signal graph.
///
/// `primal` is the original signal expression. `tangents[j]` is the derivative
/// of that expression with respect to the `j`-th selected seed.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Dual {
    primal: SigId,
    tangents: SmallVec<[SigId; 2]>,
}

/// Memoized forward-mode transformer for one selected differentiation seed set.
///
/// One transformer instance computes all tangent lanes for one shared signal
/// DAG. The cache prevents exponential blow-up on reused subgraphs and also
/// breaks recursion cycles while rebuilding `sigRec/sigProj`-style groups.
struct ForwardADTransform<'a> {
    arena: &'a mut TreeArena,
    diff_seeds: Vec<SigId>,
    diff_seed_index: AHashMap<SigId, SmallVec<[usize; 2]>>,
    cache: AHashMap<SigId, Dual>,
    debruijn_depth: i64,
}

impl<'a> ForwardADTransform<'a> {
    fn new(arena: &'a mut TreeArena, diff_seeds: &[SigId]) -> Self {
        Self {
            arena,
            diff_seed_index: Self::build_seed_index(diff_seeds),
            diff_seeds: diff_seeds.to_vec(),
            cache: AHashMap::new(),
            debruijn_depth: 0,
        }
    }

    fn build_seed_index(seeds: &[SigId]) -> AHashMap<SigId, SmallVec<[usize; 2]>> {
        let mut index = AHashMap::new();
        for (slot, &seed) in seeds.iter().enumerate() {
            index.entry(seed).or_insert_with(SmallVec::new).push(slot);
        }
        index
    }

    fn seed_count(&self) -> usize {
        self.diff_seeds.len()
    }

    fn bundle_lane_count(&self) -> i32 {
        1 + self.seed_count() as i32
    }

    fn repeat_lane_value(value: SigId, count: usize) -> SmallVec<[SigId; 2]> {
        let mut tangents = SmallVec::with_capacity(count);
        tangents.resize(count, value);
        tangents
    }

    fn repeated_lane_sig(&self, sig: SigId) -> SmallVec<[SigId; 2]> {
        Self::repeat_lane_value(sig, self.seed_count())
    }

    fn zero_tangent_lanes_real(&mut self) -> SmallVec<[SigId; 2]> {
        let zero = SigBuilder::new(self.arena).real(0.0);
        Self::repeat_lane_value(zero, self.seed_count())
    }

    fn zero_tangent_lanes_int(&mut self) -> SmallVec<[SigId; 2]> {
        let zero = SigBuilder::new(self.arena).int(0);
        Self::repeat_lane_value(zero, self.seed_count())
    }

    fn transform(&mut self, sig: SigId) -> Dual {
        if let Some(dual) = self.cache.get(&sig).cloned() {
            return dual;
        }
        let dual = self.transform_uncached(sig);
        self.cache.insert(sig, dual.clone());
        dual
    }

    fn transform_list(&mut self, list: TreeId) -> Vec<Dual> {
        list_to_vec(self.arena, list)
            .unwrap_or_default()
            .into_iter()
            .map(|sig| self.transform(sig))
            .collect()
    }

    fn transform_uncached(&mut self, sig: SigId) -> Dual {
        if let Some(seed_slots) = self.diff_seed_index.get(&sig).cloned() {
            let one = SigBuilder::new(self.arena).real(1.0);
            let mut tangents = self.zero_tangent_lanes_real();
            for slot in seed_slots {
                tangents[slot] = one;
            }
            return Dual {
                primal: sig,
                tangents,
            };
        }

        if match_de_bruijn_ref(self.arena, sig).is_some() {
            return Dual {
                primal: sig,
                tangents: self.repeated_lane_sig(sig),
            };
        }

        if let Some(body) = match_de_bruijn_rec(self.arena, sig) {
            self.cache.insert(
                sig,
                Dual {
                    primal: sig,
                    tangents: self.repeated_lane_sig(sig),
                },
            );

            self.debruijn_depth += 1;

            let old_seeds = std::mem::take(&mut self.diff_seeds);
            let old_seed_index = std::mem::replace(&mut self.diff_seed_index, AHashMap::new());
            self.diff_seeds = old_seeds
                .iter()
                .map(|&seed| lift_de_bruijn(self.arena, seed))
                .collect();
            self.diff_seed_index = Self::build_seed_index(&self.diff_seeds);

            let duals = self.transform_list(body);

            self.diff_seeds = old_seeds;
            self.diff_seed_index = old_seed_index;
            self.debruijn_depth -= 1;

            let mut expanded_body =
                Vec::with_capacity(duals.len() * self.bundle_lane_count() as usize);
            for dual in duals {
                expanded_body.push(dual.primal);
                expanded_body.extend(dual.tangents);
            }
            let list_node = vec_to_list(self.arena, &expanded_body);
            let fad_rec = de_bruijn_rec(self.arena, list_node);
            return Dual {
                primal: fad_rec,
                tangents: self.repeated_lane_sig(fad_rec),
            };
        }

        match match_sig(self.arena, sig) {
            SigMatch::Int(_) => Dual {
                primal: sig,
                tangents: self.zero_tangent_lanes_int(),
            },
            SigMatch::Real(_) => self.zero_tangent(sig),
            SigMatch::Input(_) => self.zero_tangent(sig),
            SigMatch::HSlider(_) | SigMatch::VSlider(_) | SigMatch::NumEntry(_) => {
                self.zero_tangent(sig)
            }
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
            SigMatch::Exp(x) => self.unary_chain(
                x,
                |b, primal_x| b.exp(primal_x),
                |b, primal_x, tangent_x| {
                    let exp_x = b.exp(primal_x);
                    b.mul(exp_x, tangent_x)
                },
            ),
            SigMatch::Log(x) => self.unary_chain(
                x,
                |b, primal_x| b.log(primal_x),
                |b, primal_x, tangent_x| {
                    let one = b.real(1.0);
                    let inv = b.div(one, primal_x);
                    b.mul(inv, tangent_x)
                },
            ),
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
            SigMatch::Abs(x) => self.unary_chain(
                x,
                |b, primal_x| b.abs(primal_x),
                |b, primal_x, tangent_x| {
                    let denom = b.abs(primal_x);
                    let sign = b.div(primal_x, denom);
                    b.mul(sign, tangent_x)
                },
            ),
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
            SigMatch::Pow(x, y) => {
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.pow(dual_x.primal, dual_y.primal);
                let log_x = b.log(dual_x.primal);
                let tangents = dual_x
                    .tangents
                    .iter()
                    .copied()
                    .zip(dual_y.tangents.iter().copied())
                    .map(|(tx, ty)| {
                        let term1 = b.mul(ty, log_x);
                        let scaled_dx = b.mul(dual_y.primal, tx);
                        let term2 = b.div(scaled_dx, dual_x.primal);
                        let sum = b.add(term1, term2);
                        b.mul(primal, sum)
                    })
                    .collect::<SmallVec<[SigId; 2]>>();
                Dual { primal, tangents }
            }
            SigMatch::Min(x, y) => {
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.min(dual_x.primal, dual_y.primal);
                let cond = b.lt(dual_x.primal, dual_y.primal);
                let tangents = dual_x
                    .tangents
                    .iter()
                    .copied()
                    .zip(dual_y.tangents.iter().copied())
                    .map(|(tx, ty)| b.select2(cond, ty, tx))
                    .collect::<SmallVec<[SigId; 2]>>();
                Dual { primal, tangents }
            }
            SigMatch::Max(x, y) => {
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.max(dual_x.primal, dual_y.primal);
                let cond = b.gt(dual_x.primal, dual_y.primal);
                let tangents = dual_x
                    .tangents
                    .iter()
                    .copied()
                    .zip(dual_y.tangents.iter().copied())
                    .map(|(tx, ty)| b.select2(cond, ty, tx))
                    .collect::<SmallVec<[SigId; 2]>>();
                Dual { primal, tangents }
            }
            SigMatch::Atan2(y, x) => {
                let dual_y = self.transform(y);
                let dual_x = self.transform(x);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.atan2(dual_y.primal, dual_x.primal);
                let x2 = b.mul(dual_x.primal, dual_x.primal);
                let y2 = b.mul(dual_y.primal, dual_y.primal);
                let denom = b.add(x2, y2);
                let tangents = dual_x
                    .tangents
                    .iter()
                    .copied()
                    .zip(dual_y.tangents.iter().copied())
                    .map(|(tx, ty)| {
                        let x_dy = b.mul(dual_x.primal, ty);
                        let y_dx = b.mul(dual_y.primal, tx);
                        let num = b.sub(x_dy, y_dx);
                        b.div(num, denom)
                    })
                    .collect::<SmallVec<[SigId; 2]>>();
                Dual { primal, tangents }
            }
            SigMatch::Fmod(x, y) => {
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.fmod(dual_x.primal, dual_y.primal);
                let raw_q = b.div(dual_x.primal, dual_y.primal);
                let quotient = b.floor(raw_q);
                let tangents = dual_x
                    .tangents
                    .iter()
                    .copied()
                    .zip(dual_y.tangents.iter().copied())
                    .map(|(tx, ty)| {
                        let scaled = b.mul(ty, quotient);
                        b.sub(tx, scaled)
                    })
                    .collect::<SmallVec<[SigId; 2]>>();
                Dual { primal, tangents }
            }
            SigMatch::Remainder(x, y) => {
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.remainder(dual_x.primal, dual_y.primal);
                let raw_q = b.div(dual_x.primal, dual_y.primal);
                let quotient = b.round(raw_q);
                let tangents = dual_x
                    .tangents
                    .iter()
                    .copied()
                    .zip(dual_y.tangents.iter().copied())
                    .map(|(tx, ty)| {
                        let scaled = b.mul(ty, quotient);
                        b.sub(tx, scaled)
                    })
                    .collect::<SmallVec<[SigId; 2]>>();
                Dual { primal, tangents }
            }
            SigMatch::Delay1(x) => {
                let dual_x = self.transform(x);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.delay1(dual_x.primal);
                let tangents = dual_x
                    .tangents
                    .into_iter()
                    .map(|tx| b.delay1(tx))
                    .collect::<SmallVec<[SigId; 2]>>();
                Dual { primal, tangents }
            }
            SigMatch::Delay(x, d) => {
                let dual_x = self.transform(x);
                let dual_d = self.transform(d);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.delay(dual_x.primal, dual_d.primal);
                let delayed_primal = b.delay1(dual_x.primal);
                let time_gradient = b.sub(dual_x.primal, delayed_primal);
                let delayed_time_gradient = b.delay(time_gradient, dual_d.primal);
                let tangents = dual_x
                    .tangents
                    .iter()
                    .copied()
                    .zip(dual_d.tangents.iter().copied())
                    .map(|(tx, td)| {
                        let term1 = b.delay(tx, dual_d.primal);
                        let scaled_delay = b.mul(td, delayed_time_gradient);
                        b.sub(term1, scaled_delay)
                    })
                    .collect::<SmallVec<[SigId; 2]>>();
                Dual { primal, tangents }
            }
            SigMatch::RdTbl(table, ridx) => self.transform_rdtbl(table, ridx),
            SigMatch::Select2(cond, x, y) => {
                let dual_cond = self.transform(cond);
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.select2(dual_cond.primal, dual_x.primal, dual_y.primal);
                let tangents = dual_x
                    .tangents
                    .iter()
                    .copied()
                    .zip(dual_y.tangents.iter().copied())
                    .map(|(tx, ty)| b.select2(dual_cond.primal, tx, ty))
                    .collect::<SmallVec<[SigId; 2]>>();
                Dual { primal, tangents }
            }
            SigMatch::Prefix(x, y) => {
                let dual_x = self.transform(x);
                let dual_y = self.transform(y);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.prefix(dual_x.primal, dual_y.primal);
                let tangents = dual_x
                    .tangents
                    .iter()
                    .copied()
                    .zip(dual_y.tangents.iter().copied())
                    .map(|(tx, ty)| b.prefix(tx, ty))
                    .collect::<SmallVec<[SigId; 2]>>();
                Dual { primal, tangents }
            }
            SigMatch::FloatCast(x) => {
                let dual_x = self.transform(x);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.float_cast(dual_x.primal);
                let tangents = dual_x
                    .tangents
                    .into_iter()
                    .map(|tx| b.float_cast(tx))
                    .collect::<SmallVec<[SigId; 2]>>();
                Dual { primal, tangents }
            }
            SigMatch::IntCast(x) => {
                let dual_x = self.transform(x);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.int_cast(dual_x.primal);
                let tangents = Self::repeat_lane_value(b.int(0), self.seed_count());
                Dual { primal, tangents }
            }
            SigMatch::Proj(index, group) => {
                let dual_group = self.transform(group);
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

                let lane_count = self.bundle_lane_count();
                let mut b = SigBuilder::new(self.arena);
                match kind {
                    GroupKind::BoundRec => {
                        let primal = b.proj(index * lane_count, dual_group.primal);
                        let tangents = dual_group
                            .tangents
                            .iter()
                            .enumerate()
                            .map(|(slot, &group_lane)| {
                                b.proj(index * lane_count + 1 + slot as i32, group_lane)
                            })
                            .collect::<SmallVec<[SigId; 2]>>();
                        Dual { primal, tangents }
                    }
                    GroupKind::UnboundRef => {
                        let primal = b.proj(index, dual_group.primal);
                        let tangents = Self::repeat_lane_value(b.real(0.0), self.seed_count());
                        Dual { primal, tangents }
                    }
                    GroupKind::Other => {
                        let primal = b.proj(index, dual_group.primal);
                        let tangents = dual_group
                            .tangents
                            .into_iter()
                            .map(|group_lane| b.proj(index, group_lane))
                            .collect::<SmallVec<[SigId; 2]>>();
                        Dual { primal, tangents }
                    }
                }
            }
            SigMatch::Output(_, inner) => self.transform(inner),
            SigMatch::Attach(x, y) => self.pass_through_binary(x, y, |b, px, py| b.attach(px, py)),
            SigMatch::Enable(x, y) => self.pass_through_binary(x, y, |b, px, py| b.enable(px, py)),
            SigMatch::Control(x, y) => {
                self.pass_through_binary(x, y, |b, px, py| b.control(px, py))
            }
            SigMatch::VBargraph(_, inner) | SigMatch::HBargraph(_, inner) => {
                let _ = self.transform(inner);
                self.zero_tangent(sig)
            }
            SigMatch::FFun(ff, largs) => self.transform_ffun(sig, ff, largs),
            _ => self.zero_tangent(sig),
        }
    }

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

    fn zero_tangent(&mut self, sig: SigId) -> Dual {
        Dual {
            primal: sig,
            tangents: self.zero_tangent_lanes_real(),
        }
    }

    fn is_readonly_table_source(&self, sig: SigId) -> bool {
        match match_sig(self.arena, sig) {
            SigMatch::Waveform(_) => true,
            SigMatch::WrTbl(_, _, widx, wsig) => self.arena.is_nil(widx) && self.arena.is_nil(wsig),
            _ => false,
        }
    }

    fn transform_rdtbl(&mut self, table: SigId, ridx: SigId) -> Dual {
        let dual_index = self.transform(ridx);
        let readonly = self.is_readonly_table_source(table);
        let mut b = SigBuilder::new(self.arena);
        let primal = b.rdtbl(table, dual_index.primal);
        if !readonly {
            return Dual {
                primal,
                tangents: self.zero_tangent_lanes_real(),
            };
        }

        let one = b.int(1);
        let two = b.real(2.0);
        let idx_plus = b.add(dual_index.primal, one);
        let idx_minus = b.sub(dual_index.primal, one);
        let plus = b.rdtbl(table, idx_plus);
        let minus = b.rdtbl(table, idx_minus);
        let diff = b.sub(plus, minus);
        let slope = b.div(diff, two);
        let tangents = dual_index
            .tangents
            .into_iter()
            .map(|tidx| b.mul(slope, tidx))
            .collect::<SmallVec<[SigId; 2]>>();
        Dual { primal, tangents }
    }

    fn ffun_unary_chain<FTangent>(
        &mut self,
        ff: SigId,
        arg: SigId,
        mut tangent_fn: FTangent,
    ) -> Dual
    where
        FTangent: FnMut(&mut SigBuilder<'_>, SigId, SigId, SigId) -> SigId,
    {
        let dual_x = self.transform(arg);
        let largs = vec_to_list(self.arena, &[dual_x.primal]);
        let mut b = SigBuilder::new(self.arena);
        let primal = b.ffun(ff, largs);
        let tangents = dual_x
            .tangents
            .into_iter()
            .map(|tx| tangent_fn(&mut b, primal, dual_x.primal, tx))
            .collect::<SmallVec<[SigId; 2]>>();
        Dual { primal, tangents }
    }

    fn transform_ffun(&mut self, sig: SigId, ff: SigId, largs: SigId) -> Dual {
        let args = list_to_vec(self.arena, largs).unwrap_or_default();
        if args.len() != 1 {
            return self.zero_tangent(sig);
        }
        let arg = args[0];
        if Self::ffun_is(self.arena, ff, &["tanhf", "tanh", "tanhl"]) {
            self.ffun_unary_chain(ff, arg, |b, primal, _x, tx| {
                let tanh_sq = b.mul(primal, primal);
                let one = b.real(1.0);
                let sech_sq = b.sub(one, tanh_sq);
                b.mul(sech_sq, tx)
            })
        } else if Self::ffun_is(self.arena, ff, &["sinhf", "sinh", "sinhl"]) {
            self.ffun_unary_chain(ff, arg, |b, primal, _x, tx| {
                let sinh_sq = b.mul(primal, primal);
                let one = b.real(1.0);
                let one_plus_sq = b.add(one, sinh_sq);
                let cosh_x = b.sqrt(one_plus_sq);
                b.mul(cosh_x, tx)
            })
        } else if Self::ffun_is(self.arena, ff, &["coshf", "cosh", "coshl"]) {
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
            self.ffun_unary_chain(ff, arg, |b, _primal, x, tx| {
                let x_sq = b.mul(x, x);
                let one = b.real(1.0);
                let denom = b.sub(one, x_sq);
                b.div(tx, denom)
            })
        } else if Self::ffun_is(self.arena, ff, &["asinhf", "asinh", "asinhl"]) {
            self.ffun_unary_chain(ff, arg, |b, _primal, x, tx| {
                let x_sq = b.mul(x, x);
                let one = b.real(1.0);
                let sum = b.add(one, x_sq);
                let denom = b.sqrt(sum);
                b.div(tx, denom)
            })
        } else if Self::ffun_is(self.arena, ff, &["acoshf", "acosh", "acoshl"]) {
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

    fn unary_chain<FPrimal, FTangent>(
        &mut self,
        x: SigId,
        primal_fn: FPrimal,
        mut tangent_fn: FTangent,
    ) -> Dual
    where
        FPrimal: FnOnce(&mut SigBuilder<'_>, SigId) -> SigId,
        FTangent: FnMut(&mut SigBuilder<'_>, SigId, SigId) -> SigId,
    {
        let dual_x = self.transform(x);
        let mut b = SigBuilder::new(self.arena);
        let primal = primal_fn(&mut b, dual_x.primal);
        let tangents = dual_x
            .tangents
            .into_iter()
            .map(|tx| tangent_fn(&mut b, dual_x.primal, tx))
            .collect::<SmallVec<[SigId; 2]>>();
        Dual { primal, tangents }
    }

    fn pass_through_binary<F>(&mut self, x: SigId, y: SigId, primal_fn: F) -> Dual
    where
        F: FnOnce(&mut SigBuilder<'_>, SigId, SigId) -> SigId,
    {
        let dual_x = self.transform(x);
        let dual_y = self.transform(y);
        let mut b = SigBuilder::new(self.arena);
        Dual {
            primal: primal_fn(&mut b, dual_x.primal, dual_y.primal),
            tangents: dual_x.tangents,
        }
    }

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

        let tangents = match op {
            BinOp::Add => dual_x
                .tangents
                .iter()
                .copied()
                .zip(dual_y.tangents.iter().copied())
                .map(|(tx, ty)| b.add(tx, ty))
                .collect::<SmallVec<[SigId; 2]>>(),
            BinOp::Sub => dual_x
                .tangents
                .iter()
                .copied()
                .zip(dual_y.tangents.iter().copied())
                .map(|(tx, ty)| b.sub(tx, ty))
                .collect::<SmallVec<[SigId; 2]>>(),
            BinOp::Mul => dual_x
                .tangents
                .iter()
                .copied()
                .zip(dual_y.tangents.iter().copied())
                .map(|(tx, ty)| {
                    let t1 = b.mul(tx, dual_y.primal);
                    let t2 = b.mul(dual_x.primal, ty);
                    b.add(t1, t2)
                })
                .collect::<SmallVec<[SigId; 2]>>(),
            BinOp::Div => dual_x
                .tangents
                .iter()
                .copied()
                .zip(dual_y.tangents.iter().copied())
                .map(|(tx, ty)| {
                    let t1 = b.mul(tx, dual_y.primal);
                    let t2 = b.mul(dual_x.primal, ty);
                    let num = b.sub(t1, t2);
                    let den = b.mul(dual_y.primal, dual_y.primal);
                    b.div(num, den)
                })
                .collect::<SmallVec<[SigId; 2]>>(),
            BinOp::Rem => {
                let x_div_y = b.div(dual_x.primal, dual_y.primal);
                let floor = b.floor(x_div_y);
                dual_x
                    .tangents
                    .iter()
                    .copied()
                    .zip(dual_y.tangents.iter().copied())
                    .map(|(tx, ty)| {
                        let term2 = b.mul(ty, floor);
                        b.sub(tx, term2)
                    })
                    .collect::<SmallVec<[SigId; 2]>>()
            }
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
            | BinOp::Xor => Self::repeat_lane_value(b.int(0), self.seed_count()),
        };
        Dual { primal, tangents }
    }
}

/// Expands propagated primal outputs into the forward-mode AD output bundle.
///
/// Output layout: `[p1, t1_s1, t1_s2, …, p2, t2_s1, t2_s2, …]` where
/// `sK` ranges over `seeds` in order.
pub(super) fn generate_fad_signals_multi(
    arena: &mut TreeArena,
    outputs: &[SigId],
    seeds: &[SigId],
) -> Result<Vec<SigId>, PropagateError> {
    if seeds.is_empty() {
        return Ok(outputs.to_vec());
    }

    let mut fad = ForwardADTransform::new(arena, seeds);
    let duals = outputs
        .iter()
        .map(|&sig| fad.transform(sig))
        .collect::<Vec<_>>();

    let mut result = Vec::with_capacity(outputs.len() * (1 + seeds.len()));
    for dual in duals {
        result.push(dual.primal);
        result.extend(dual.tangents);
    }
    Ok(result)
}
