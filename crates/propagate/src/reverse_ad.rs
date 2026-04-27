//! Reverse-mode automatic differentiation for `rad(expr, seeds)`.
//!
//! # Source provenance
//! Original Rust design — there is no upstream C++ reverse-mode AD path
//! to mirror at this surface. The plan is documented in
//! `porting/reverse-ad-rad-implementation-plan-2026-04-27-en.md`.
//!
//! # Output layout
//!
//! ```text
//! rad(expr, (s0, s1, …, s{N-1}))
//!   = [ expr_0, expr_1, …, expr_{M-1},
//!       d sum(expr_i) / d s_0,
//!       d sum(expr_i) / d s_1,
//!       …,
//!       d sum(expr_i) / d s_{N-1} ]
//! ```
//!
//! The implicit cotangent on every primal output is `1.0`. A future VJP-style
//! API can expose a custom output cotangent vector.
//!
//! # Algorithm
//! Three explicit passes on a single `ReverseADTransform` instance:
//!
//! 1. **Active subgraph collection.** Postorder DFS from each primal output
//!    through differentiable children, stopping at any `SigId` that appears
//!    in the seed list. DAG sharing is preserved by a `visited` set so each
//!    node is visited once.
//! 2. **Adjoint accumulation.** Initialize `adjoints[primal] = 1.0` for every
//!    primal, then walk the postorder in reverse and emit local transpose
//!    contributions `child_bar += y_bar * d y / d child` for each visited
//!    `y`. Existing entries are summed via [`add_adjoint`].
//! 3. **Seed extraction.** Re-emit `primals` followed by, for each seed
//!    lane, the accumulated `adjoints[seed]` (or `0.0` when the seed is not
//!    reached from any primal output, e.g. `rad(sin(x), y)`).
//!
//! # Phase B scope
//!
//! Phase B targets the feed-forward subset that overlaps the existing
//! forward-AD differentiable set:
//!
//! - constants, audio inputs, UI controls (zero contribution unless seed),
//! - arithmetic `BinOp` (Add/Sub/Mul/Div/Rem and discrete bitwise/comparison),
//! - unary trig/exp/log/sqrt/abs and inverse-trig (Acos/Asin/Atan),
//! - binary math `Pow`, `Atan2`, `Fmod`, `Remainder`,
//! - `Min`/`Max` via primal comparison,
//! - `Select2` routes adjoint to the chosen branch,
//! - `IntCast` (zero) and `FloatCast` (forward),
//! - bargraphs (zero contribution).
//!
//! Out of scope for phase B (raise [`PropagateError::RadUnsupportedNode`]):
//!
//! - delay, prefix, recursion, projection,
//! - read/write tables and waveform tables,
//! - foreign functions,
//! - soundfile accessors and other mutable / opaque families.
//!
//! Phase C extends the supported set; phase D refines the strict
//! diagnostics.

use ahash::{AHashMap, AHashSet};
use signals::{BinOp, SigBuilder, SigId, SigMatch, match_sig};
use smallvec::SmallVec;
use tlib::TreeArena;

use crate::PropagateError;

/// Collects the active subgraph and accumulates adjoints for one
/// `rad(expr, seeds)` call.
struct ReverseADTransform<'a> {
    arena: &'a mut TreeArena,
    seed_set: AHashSet<SigId>,
    /// Postorder of the active subgraph (children before parents).
    /// Seed nodes appear as leaves.
    postorder: Vec<SigId>,
    /// DAG sharing: every signal is visited at most once.
    visited: AHashSet<SigId>,
    /// Accumulated adjoint per visited node.
    adjoints: AHashMap<SigId, SigId>,
}

impl<'a> ReverseADTransform<'a> {
    fn new(arena: &'a mut TreeArena, seeds: &[SigId]) -> Self {
        let mut seed_set = AHashSet::with_capacity(seeds.len());
        for &s in seeds {
            seed_set.insert(s);
        }
        Self {
            arena,
            seed_set,
            postorder: Vec::new(),
            visited: AHashSet::new(),
            adjoints: AHashMap::new(),
        }
    }

    /// Returns the cached children that contribute adjoint signal flow for
    /// the dispatch of node `sig`. Pure read on the arena; the same matcher
    /// drives the adjoint emission below.
    fn active_children(&self, sig: SigId) -> Result<SmallVec<[SigId; 4]>, PropagateError> {
        let mut out: SmallVec<[SigId; 4]> = SmallVec::new();
        match match_sig(self.arena, sig) {
            // Leaves — no descent.
            SigMatch::Int(_)
            | SigMatch::Real(_)
            | SigMatch::Input(_)
            | SigMatch::HSlider(_)
            | SigMatch::VSlider(_)
            | SigMatch::NumEntry(_)
            | SigMatch::Button(_)
            | SigMatch::Checkbox(_) => {}
            // Arithmetic / math.
            SigMatch::BinOp(_, x, y)
            | SigMatch::Pow(x, y)
            | SigMatch::Min(x, y)
            | SigMatch::Max(x, y)
            | SigMatch::Atan2(x, y)
            | SigMatch::Fmod(x, y)
            | SigMatch::Remainder(x, y) => {
                out.push(x);
                out.push(y);
            }
            SigMatch::Sin(x)
            | SigMatch::Cos(x)
            | SigMatch::Tan(x)
            | SigMatch::Exp(x)
            | SigMatch::Log(x)
            | SigMatch::Log10(x)
            | SigMatch::Sqrt(x)
            | SigMatch::Abs(x)
            | SigMatch::Acos(x)
            | SigMatch::Asin(x)
            | SigMatch::Atan(x)
            | SigMatch::IntCast(x)
            | SigMatch::FloatCast(x) => {
                out.push(x);
            }
            SigMatch::Select2(cond, x, y) => {
                // The condition is a discrete branch selector; phase B does
                // not propagate adjoint through it.
                let _ = cond;
                out.push(x);
                out.push(y);
            }
            SigMatch::VBargraph(_, inner) | SigMatch::HBargraph(_, inner) => {
                // Metering sinks: no adjoint contribution but still walked
                // so a seed reachable only through a bargraph is correctly
                // recorded as unreachable (gradient zero).
                let _ = inner;
            }
            // Phase-B unsupported families: temporal, recursive, table,
            // foreign function, soundfile, opaque.
            SigMatch::Delay1(_)
            | SigMatch::Delay(_, _)
            | SigMatch::Prefix(_, _) => {
                return Err(PropagateError::RadUnsupportedNode {
                    node: sig,
                    kind: "delay-or-prefix",
                });
            }
            SigMatch::Proj(_, _) | SigMatch::Rec(_) => {
                return Err(PropagateError::RadUnsupportedNode {
                    node: sig,
                    kind: "recursive-projection",
                });
            }
            SigMatch::RdTbl(_, _) | SigMatch::WrTbl(_, _, _, _) | SigMatch::Waveform(_) => {
                return Err(PropagateError::RadUnsupportedNode {
                    node: sig,
                    kind: "table-or-waveform",
                });
            }
            SigMatch::FFun(_, _) => {
                return Err(PropagateError::RadUnsupportedNode {
                    node: sig,
                    kind: "ffun",
                });
            }
            SigMatch::Soundfile(_)
            | SigMatch::SoundfileLength(_, _)
            | SigMatch::SoundfileRate(_, _)
            | SigMatch::SoundfileBuffer(_, _, _, _) => {
                return Err(PropagateError::RadUnsupportedNode {
                    node: sig,
                    kind: "soundfile",
                });
            }
            SigMatch::Attach(_, _) | SigMatch::Enable(_, _) | SigMatch::Control(_, _) => {
                return Err(PropagateError::RadUnsupportedNode {
                    node: sig,
                    kind: "pass-through",
                });
            }
            // Catch-all: every other signal family is opaque to RAD in
            // phase B. This includes representation-level casts, integer
            // rounding, foreign constants/variables, generators, and
            // signal-pipeline glue (Seq/ZeroPad/OnDemand/Upsampling/
            // Downsampling). Reject loudly rather than silently dropping
            // a gradient.
            _ => {
                return Err(PropagateError::RadUnsupportedNode {
                    node: sig,
                    kind: "other",
                });
            }
        }
        Ok(out)
    }

    /// DFS from `root` producing a deterministic postorder, stopping descent
    /// at any seed.
    fn collect_dfs(&mut self, root: SigId) -> Result<(), PropagateError> {
        if !self.visited.insert(root) {
            return Ok(());
        }
        if self.seed_set.contains(&root) {
            self.postorder.push(root);
            return Ok(());
        }
        let children = self.active_children(root)?;
        for child in children {
            self.collect_dfs(child)?;
        }
        self.postorder.push(root);
        Ok(())
    }

    /// `target_bar = target_bar + contribution`, with cheap zero-folding so
    /// `0 + x` becomes `x` instead of materializing a redundant Add.
    fn add_adjoint(&mut self, target: SigId, contribution: SigId) {
        match self.adjoints.get(&target).copied() {
            None => {
                self.adjoints.insert(target, contribution);
            }
            Some(existing) => {
                let mut b = SigBuilder::new(self.arena);
                let summed = b.add(existing, contribution);
                self.adjoints.insert(target, summed);
            }
        }
    }

    /// Propagates `y_bar` into the adjoints of `y`'s active children using
    /// the rule table from the module docstring.
    fn propagate_adjoint(&mut self, y: SigId, y_bar: SigId) -> Result<(), PropagateError> {
        // Re-decode `y` for each call — the matcher is a cheap lookup and
        // keeps the rule arms fully local.
        match match_sig(self.arena, y) {
            SigMatch::Int(_)
            | SigMatch::Real(_)
            | SigMatch::Input(_)
            | SigMatch::HSlider(_)
            | SigMatch::VSlider(_)
            | SigMatch::NumEntry(_)
            | SigMatch::Button(_)
            | SigMatch::Checkbox(_) => {
                // Leaves; nothing downstream.
            }
            SigMatch::BinOp(op, x, z) => self.propagate_binop(op, x, z, y_bar),
            SigMatch::Pow(x, z) => {
                // d/dx x^z = x^z * z / x ; d/dz x^z = x^z * log(x)
                let mut b = SigBuilder::new(self.arena);
                let scaled_y_bar_over_x = b.div(y_bar, x);
                let dx_factor = b.mul(scaled_y_bar_over_x, z);
                let pow = b.pow(x, z);
                let x_contrib = b.mul(dx_factor, pow);
                let log_x = b.log(x);
                let pow2 = b.pow(x, z);
                let z_contrib_inner = b.mul(y_bar, log_x);
                let z_contrib = b.mul(z_contrib_inner, pow2);
                self.add_adjoint(x, x_contrib);
                self.add_adjoint(z, z_contrib);
            }
            SigMatch::Min(x, z) => {
                let mut b = SigBuilder::new(self.arena);
                let cond = b.lt(x, z);
                let zero = b.real(0.0);
                let x_contrib = b.select2(cond, zero, y_bar);
                let z_contrib = b.select2(cond, y_bar, zero);
                self.add_adjoint(x, x_contrib);
                self.add_adjoint(z, z_contrib);
            }
            SigMatch::Max(x, z) => {
                let mut b = SigBuilder::new(self.arena);
                let cond = b.gt(x, z);
                let zero = b.real(0.0);
                let x_contrib = b.select2(cond, zero, y_bar);
                let z_contrib = b.select2(cond, y_bar, zero);
                self.add_adjoint(x, x_contrib);
                self.add_adjoint(z, z_contrib);
            }
            SigMatch::Atan2(num, den) => {
                // d/d num atan2(num,den) = den / (num² + den²)
                // d/d den atan2(num,den) = -num / (num² + den²)
                let mut b = SigBuilder::new(self.arena);
                let num_sq = b.mul(num, num);
                let den_sq = b.mul(den, den);
                let denom = b.add(num_sq, den_sq);
                let num_factor = b.div(den, denom);
                let zero = b.real(0.0);
                let neg_num = b.sub(zero, num);
                let den_factor = b.div(neg_num, denom);
                let num_contrib = b.mul(y_bar, num_factor);
                let den_contrib = b.mul(y_bar, den_factor);
                self.add_adjoint(num, num_contrib);
                self.add_adjoint(den, den_contrib);
            }
            SigMatch::Fmod(x, z) => {
                // d/dx fmod(x,z) = 1 ; d/dz fmod(x,z) = -floor(x/z)
                let mut b = SigBuilder::new(self.arena);
                let q = b.div(x, z);
                let floor_q = b.floor(q);
                let zero = b.real(0.0);
                let neg_floor = b.sub(zero, floor_q);
                let z_contrib = b.mul(y_bar, neg_floor);
                self.add_adjoint(x, y_bar);
                self.add_adjoint(z, z_contrib);
            }
            SigMatch::Remainder(x, z) => {
                let mut b = SigBuilder::new(self.arena);
                let q = b.div(x, z);
                let round_q = b.round(q);
                let zero = b.real(0.0);
                let neg_round = b.sub(zero, round_q);
                let z_contrib = b.mul(y_bar, neg_round);
                self.add_adjoint(x, y_bar);
                self.add_adjoint(z, z_contrib);
            }
            SigMatch::Sin(x) => {
                let mut b = SigBuilder::new(self.arena);
                let cos_x = b.cos(x);
                let contrib = b.mul(y_bar, cos_x);
                self.add_adjoint(x, contrib);
            }
            SigMatch::Cos(x) => {
                let mut b = SigBuilder::new(self.arena);
                let sin_x = b.sin(x);
                let zero = b.real(0.0);
                let neg_sin = b.sub(zero, sin_x);
                let contrib = b.mul(y_bar, neg_sin);
                self.add_adjoint(x, contrib);
            }
            SigMatch::Tan(x) => {
                let mut b = SigBuilder::new(self.arena);
                let cos_x = b.cos(x);
                let cos_sq = b.mul(cos_x, cos_x);
                let one = b.real(1.0);
                let inv = b.div(one, cos_sq);
                let contrib = b.mul(y_bar, inv);
                self.add_adjoint(x, contrib);
            }
            SigMatch::Exp(x) => {
                let mut b = SigBuilder::new(self.arena);
                let exp_x = b.exp(x);
                let contrib = b.mul(y_bar, exp_x);
                self.add_adjoint(x, contrib);
            }
            SigMatch::Log(x) => {
                let mut b = SigBuilder::new(self.arena);
                let one = b.real(1.0);
                let inv = b.div(one, x);
                let contrib = b.mul(y_bar, inv);
                self.add_adjoint(x, contrib);
            }
            SigMatch::Log10(x) => {
                let mut b = SigBuilder::new(self.arena);
                let ten = b.real(10.0);
                let log_ten = b.log(ten);
                let denom = b.mul(x, log_ten);
                let one = b.real(1.0);
                let inv = b.div(one, denom);
                let contrib = b.mul(y_bar, inv);
                self.add_adjoint(x, contrib);
            }
            SigMatch::Sqrt(x) => {
                let mut b = SigBuilder::new(self.arena);
                let two = b.real(2.0);
                let root = b.sqrt(x);
                let denom = b.mul(two, root);
                let one = b.real(1.0);
                let inv = b.div(one, denom);
                let contrib = b.mul(y_bar, inv);
                self.add_adjoint(x, contrib);
            }
            SigMatch::Abs(x) => {
                let mut b = SigBuilder::new(self.arena);
                let denom = b.abs(x);
                let sign = b.div(x, denom);
                let contrib = b.mul(y_bar, sign);
                self.add_adjoint(x, contrib);
            }
            SigMatch::Acos(x) => {
                let mut b = SigBuilder::new(self.arena);
                let one = b.real(1.0);
                let x_sq = b.mul(x, x);
                let inside = b.sub(one, x_sq);
                let root = b.sqrt(inside);
                let minus_one = b.real(-1.0);
                let inv = b.div(minus_one, root);
                let contrib = b.mul(y_bar, inv);
                self.add_adjoint(x, contrib);
            }
            SigMatch::Asin(x) => {
                let mut b = SigBuilder::new(self.arena);
                let one = b.real(1.0);
                let x_sq = b.mul(x, x);
                let inside = b.sub(one, x_sq);
                let root = b.sqrt(inside);
                let inv = b.div(one, root);
                let contrib = b.mul(y_bar, inv);
                self.add_adjoint(x, contrib);
            }
            SigMatch::Atan(x) => {
                let mut b = SigBuilder::new(self.arena);
                let one = b.real(1.0);
                let x_sq = b.mul(x, x);
                let denom = b.add(one, x_sq);
                let inv = b.div(one, denom);
                let contrib = b.mul(y_bar, inv);
                self.add_adjoint(x, contrib);
            }
            SigMatch::FloatCast(x) => {
                let mut b = SigBuilder::new(self.arena);
                let contrib = b.float_cast(y_bar);
                self.add_adjoint(x, contrib);
            }
            SigMatch::IntCast(_) => {
                // d/dx int_cast(x) is treated as zero (truncation jumps).
            }
            SigMatch::Select2(cond, x, z) => {
                // y = select2(cond, x, z): the chosen branch receives
                // `y_bar`, the other receives `0`. SigBuilder::select2(a, b)
                // returns `b` when `cond != 0` and `a` when `cond == 0`,
                // matching the convention used by the FAD pass.
                let mut b = SigBuilder::new(self.arena);
                let zero = b.real(0.0);
                // x branch: cond == 0 -> select2(cond, y_bar, 0) = y_bar.
                let x_contrib = b.select2(cond, y_bar, zero);
                // z branch: cond != 0 -> select2(cond, 0, y_bar) = y_bar.
                let z_contrib = b.select2(cond, zero, y_bar);
                self.add_adjoint(x, x_contrib);
                self.add_adjoint(z, z_contrib);
            }
            SigMatch::VBargraph(_, _) | SigMatch::HBargraph(_, _) => {
                // Sinks: no adjoint passes through.
            }
            // The unsupported families were rejected in `active_children`
            // before they reached postorder. Reaching them here means a
            // direct primal output of an unsupported family — we still
            // refuse rather than silently dropping the gradient.
            _ => {
                return Err(PropagateError::RadUnsupportedNode {
                    node: y,
                    kind: "other-direct-primal",
                });
            }
        }
        Ok(())
    }

    fn propagate_binop(&mut self, op: BinOp, x: SigId, z: SigId, y_bar: SigId) {
        let mut b = SigBuilder::new(self.arena);
        match op {
            BinOp::Add => {
                self.add_adjoint(x, y_bar);
                self.add_adjoint(z, y_bar);
            }
            BinOp::Sub => {
                let zero = b.real(0.0);
                let neg = b.sub(zero, y_bar);
                self.add_adjoint(x, y_bar);
                self.add_adjoint(z, neg);
            }
            BinOp::Mul => {
                let xc = b.mul(y_bar, z);
                let zc = b.mul(y_bar, x);
                self.add_adjoint(x, xc);
                self.add_adjoint(z, zc);
            }
            BinOp::Div => {
                let xc = b.div(y_bar, z);
                let zsq = b.mul(z, z);
                let zero = b.real(0.0);
                let neg_x = b.sub(zero, x);
                let scaled = b.div(neg_x, zsq);
                let zc = b.mul(y_bar, scaled);
                self.add_adjoint(x, xc);
                self.add_adjoint(z, zc);
            }
            BinOp::Rem => {
                let q = b.div(x, z);
                let floor_q = b.floor(q);
                let zero = b.real(0.0);
                let neg_floor = b.sub(zero, floor_q);
                let zc = b.mul(y_bar, neg_floor);
                self.add_adjoint(x, y_bar);
                self.add_adjoint(z, zc);
            }
            // Discrete: comparisons, shifts, bitwise → zero contribution.
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
            | BinOp::Xor => {}
        }
    }

    /// Drives the three passes and returns the bundled
    /// `[primals…, adjoint(seeds)…]` output list.
    fn run(&mut self, primals: &[SigId], seeds: &[SigId]) -> Result<Vec<SigId>, PropagateError> {
        for &p in primals {
            self.collect_dfs(p)?;
        }

        // Initialize: each primal contributes 1 to the cotangent sum.
        let one = SigBuilder::new(self.arena).real(1.0);
        for &p in primals {
            self.add_adjoint(p, one);
        }

        for i in (0..self.postorder.len()).rev() {
            let y = self.postorder[i];
            if self.seed_set.contains(&y) {
                continue;
            }
            let Some(y_bar) = self.adjoints.get(&y).copied() else {
                // Unreachable in a well-formed graph: every postorder node
                // is initialized with at least one parent contribution
                // before we visit it. Skip defensively.
                continue;
            };
            self.propagate_adjoint(y, y_bar)?;
        }

        let zero = SigBuilder::new(self.arena).real(0.0);
        let mut out = Vec::with_capacity(primals.len() + seeds.len());
        out.extend_from_slice(primals);
        for &s in seeds {
            out.push(self.adjoints.get(&s).copied().unwrap_or(zero));
        }
        Ok(out)
    }
}

/// Public entry point for `rad(expr, seeds)` propagation.
pub(super) fn generate_rad_signals(
    arena: &mut TreeArena,
    primals: &[SigId],
    seeds: &[SigId],
) -> Result<Vec<SigId>, PropagateError> {
    if seeds.is_empty() {
        // Mirrors the FAD identity short-circuit: with no seeds the bundle
        // collapses to the primal outputs.
        return Ok(primals.to_vec());
    }
    let mut transform = ReverseADTransform::new(arena, seeds);
    transform.run(primals, seeds)
}
