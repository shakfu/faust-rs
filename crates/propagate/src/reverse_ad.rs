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
//! Phase C extension (this revision):
//!
//! - read-only `RdTbl(T, idx)` propagates adjoint through the read index
//!   only, using the same symmetric finite-difference slope as the FAD pass
//!   `(rdtbl(T, idx + 1) - rdtbl(T, idx - 1)) / 2`.
//! - unary foreign functions (`tanhf`/`tanh`/`tanhl`,
//!   `sinhf`/`sinh`/`sinhl`, `coshf`/`cosh`/`coshl` and the inverse-trig
//!   counterparts) propagate adjoint through the same chain-rule formulas
//!   used by FAD.
//! - pass-through wrappers (`Attach`, `Enable`, `Control`) and `Output`
//!   forward adjoint through the signal-carrying operand only, matching
//!   FAD's transparency contract.
//!
//! Out of scope for phase C (raise [`PropagateError::RadUnsupportedNode`]):
//!
//! - delay, prefix, recursion, projection,
//! - mutable tables (`WrTbl` with non-nil write ports) and waveform
//!   sources used outside `rdtbl`,
//! - non-unary or unrecognized foreign functions,
//! - soundfile accessors,
//! - representation casts and other opaque families.
//!
//! Phase D refines the strict diagnostics around temporal nodes.
//!
//! # Why temporal nodes refuse adjoint
//!
//! Forward-mode AD applies a *causal* rule for delays:
//!
//! ```text
//! d/dp delay1(x) = delay1(x')   // tangent at frame n only depends on frame n-1
//! ```
//!
//! Reverse-mode AD requires the transpose, which is *anti-causal*:
//!
//! ```text
//! adj_x[n] += adj_y[n + 1]      // adjoint at frame n depends on a future frame
//! ```
//!
//! A correct reverse pass therefore needs either
//!
//! - a finite block tape that buffers primal intermediates and a backward
//!   scan over that block (BPTT — out of scope for phase 1), or
//! - a causal approximation that is explicitly not exact reverse mode.
//!
//! Phase 1 RAD takes the strict route: any signal family whose transpose
//! would be non-causal raises [`PropagateError::RadUnsupportedNode`] with
//! a tailored diagnostic. The plan reserves `rad(expr, seeds, horizon)`
//! and `-rad-horizon N` for a future BPTT mode (plan §10.3); phase 1 must
//! never silently emit a misleading gradient.

use ahash::{AHashMap, AHashSet};
use signals::{BinOp, SigBuilder, SigId, SigMatch, match_sig};
use smallvec::SmallVec;
use tlib::{NodeKind, TreeArena, list_to_vec, tree_to_str};

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
            SigMatch::RdTbl(table, ridx) => {
                // Phase C: read-only tables are differentiable through the
                // read index. Mutable tables fall through to the strict
                // failure path below.
                if !is_readonly_table_source(self.arena, table) {
                    return Err(PropagateError::RadUnsupportedNode {
                        node: sig,
                        kind: "writable-table",
                    });
                }
                out.push(ridx);
            }
            SigMatch::FFun(ff, largs) => {
                // Phase C: only the unary chain-rule rules from FAD are
                // recognized. Non-unary or unknown FFuns refuse adjoint.
                let args = list_to_vec(self.arena, largs).unwrap_or_default();
                if args.len() != 1 || !ffun_unary_supported(self.arena, ff) {
                    return Err(PropagateError::RadUnsupportedNode {
                        node: sig,
                        kind: "ffun",
                    });
                }
                out.push(args[0]);
            }
            SigMatch::Attach(x, _) | SigMatch::Enable(x, _) | SigMatch::Control(x, _) => {
                // Pass-through nodes: only the signal-carrying operand
                // contributes to the adjoint flow; the side-effect /
                // control operand is ignored, mirroring FAD.
                out.push(x);
            }
            SigMatch::Output(_, inner) => {
                // `Output` is transparent to differentiation; forward the
                // adjoint to the wrapped signal.
                out.push(inner);
            }
            // Phase-B/C unsupported families: temporal, recursive,
            // mutable table, soundfile, opaque.
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
            SigMatch::WrTbl(_, _, _, _) | SigMatch::Waveform(_) => {
                return Err(PropagateError::RadUnsupportedNode {
                    node: sig,
                    kind: "writable-table-or-waveform-direct",
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
            SigMatch::RdTbl(table, ridx) => {
                // Phase C: read-only table read.
                //   y = rdtbl(T, i)
                //   slope(i) ≈ (rdtbl(T, i+1) - rdtbl(T, i-1)) / 2
                //   i_bar += y_bar * slope(i)
                let mut b = SigBuilder::new(self.arena);
                let one = b.int(1);
                let two = b.real(2.0);
                let idx_plus = b.add(ridx, one);
                let idx_minus = b.sub(ridx, one);
                let plus = b.rdtbl(table, idx_plus);
                let minus = b.rdtbl(table, idx_minus);
                let diff = b.sub(plus, minus);
                let slope = b.div(diff, two);
                let contrib = b.mul(y_bar, slope);
                self.add_adjoint(ridx, contrib);
            }
            SigMatch::FFun(ff, largs) => {
                // Resolve the FFUN family name first so the ffun_is /
                // arena-immutable lookups don't fight the SigBuilder's
                // exclusive borrow during contribution construction.
                let args = list_to_vec(self.arena, largs).unwrap_or_default();
                let arg = args[0];
                let kind = ffun_unary_kind(self.arena, ff);
                let mut b = SigBuilder::new(self.arena);
                let primal = b.ffun(ff, largs);
                let contrib = match kind {
                    Some(FFunUnaryKind::Tanh) => {
                        let tanh_sq = b.mul(primal, primal);
                        let one = b.real(1.0);
                        let sech_sq = b.sub(one, tanh_sq);
                        b.mul(y_bar, sech_sq)
                    }
                    Some(FFunUnaryKind::Sinh) => {
                        let sinh_sq = b.mul(primal, primal);
                        let one = b.real(1.0);
                        let one_plus_sq = b.add(one, sinh_sq);
                        let cosh_x = b.sqrt(one_plus_sq);
                        b.mul(y_bar, cosh_x)
                    }
                    Some(FFunUnaryKind::Cosh) => {
                        let exp_x = b.exp(arg);
                        let minus_one = b.real(-1.0);
                        let neg_x = b.mul(minus_one, arg);
                        let exp_neg_x = b.exp(neg_x);
                        let diff = b.sub(exp_x, exp_neg_x);
                        let half = b.real(0.5);
                        let sinh_x = b.mul(half, diff);
                        b.mul(y_bar, sinh_x)
                    }
                    Some(FFunUnaryKind::Atanh) => {
                        let x_sq = b.mul(arg, arg);
                        let one = b.real(1.0);
                        let denom = b.sub(one, x_sq);
                        b.div(y_bar, denom)
                    }
                    Some(FFunUnaryKind::Asinh) => {
                        let x_sq = b.mul(arg, arg);
                        let one = b.real(1.0);
                        let sum = b.add(one, x_sq);
                        let denom = b.sqrt(sum);
                        b.div(y_bar, denom)
                    }
                    Some(FFunUnaryKind::Acosh) => {
                        let x_sq = b.mul(arg, arg);
                        let one = b.real(1.0);
                        let diff = b.sub(x_sq, one);
                        let denom = b.sqrt(diff);
                        b.div(y_bar, denom)
                    }
                    None => {
                        // Defensive: should be unreachable because
                        // `active_children` already gated on the
                        // recognized names.
                        return Err(PropagateError::RadUnsupportedNode {
                            node: y,
                            kind: "ffun-unrecognized",
                        });
                    }
                };
                self.add_adjoint(arg, contrib);
            }
            SigMatch::Attach(x, _) | SigMatch::Enable(x, _) | SigMatch::Control(x, _) => {
                // Pass-through wrappers: forward the full adjoint to the
                // signal-carrying operand only.
                self.add_adjoint(x, y_bar);
            }
            SigMatch::Output(_, inner) => {
                self.add_adjoint(inner, y_bar);
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

/// One of the recognized unary FFUN families; used by the FFUN arm of
/// `propagate_adjoint` to pick the correct chain rule without re-running
/// the name-matching logic per branch.
enum FFunUnaryKind {
    Tanh,
    Sinh,
    Cosh,
    Atanh,
    Asinh,
    Acosh,
}

fn ffun_unary_kind(arena: &TreeArena, ff: SigId) -> Option<FFunUnaryKind> {
    if ffun_is(arena, ff, &["tanhf", "tanh", "tanhl"]) {
        Some(FFunUnaryKind::Tanh)
    } else if ffun_is(arena, ff, &["sinhf", "sinh", "sinhl"]) {
        Some(FFunUnaryKind::Sinh)
    } else if ffun_is(arena, ff, &["coshf", "cosh", "coshl"]) {
        Some(FFunUnaryKind::Cosh)
    } else if ffun_is(arena, ff, &["atanhf", "atanh", "atanhl"]) {
        Some(FFunUnaryKind::Atanh)
    } else if ffun_is(arena, ff, &["asinhf", "asinh", "asinhl"]) {
        Some(FFunUnaryKind::Asinh)
    } else if ffun_is(arena, ff, &["acoshf", "acosh", "acoshl"]) {
        Some(FFunUnaryKind::Acosh)
    } else {
        None
    }
}

/// Returns `true` when the FFUN descriptor's name (in any precision
/// variant) matches one of the provided targets. Mirrors
/// `forward_ad::ForwardADTransform::ffun_is`.
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

/// Set of unary FFUN names whose chain-rule rule is implemented by RAD.
/// Kept aligned with the FAD pass.
const RAD_FFUN_UNARY_NAMES: &[&str] = &[
    "tanhf", "tanh", "tanhl", "sinhf", "sinh", "sinhl", "coshf", "cosh", "coshl", "atanhf",
    "atanh", "atanhl", "asinhf", "asinh", "asinhl", "acoshf", "acosh", "acoshl",
];

fn ffun_unary_supported(arena: &TreeArena, ff: SigId) -> bool {
    ffun_is(arena, ff, RAD_FFUN_UNARY_NAMES)
}

/// Same read-only-table classifier as the FAD pass: a `Waveform` is
/// always read-only, and a `WrTbl(_, _, nil, nil)` is a write-once table
/// with no live writer port and therefore safe to read-differentiate.
fn is_readonly_table_source(arena: &TreeArena, sig: SigId) -> bool {
    match match_sig(arena, sig) {
        SigMatch::Waveform(_) => true,
        SigMatch::WrTbl(_, _, widx, wsig) => arena.is_nil(widx) && arena.is_nil(wsig),
        _ => false,
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
