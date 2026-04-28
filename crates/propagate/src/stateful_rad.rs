//! Read-only feasibility analysis for stateful reverse-mode AD.
//!
//! # Source provenance
//! Original Rust design for RAD phase E0, documented in
//! `porting/reverse-ad-rad-implementation-plan-2026-04-27-en.md` section
//! "19. Feasibility analysis for stateful RAD".
//!
//! # Scope
//! This module does not enable reverse-mode differentiation through
//! recursive or delayed signals. It classifies one recursive signal group so
//! future RAD phases can decide whether a causal transpose is structurally
//! plausible:
//!
//! - [`RadRecLinearity::LinearLti`] means every current-recursion back-edge
//!   appears linearly and only under constant coefficients. Independent input
//!   or UI driving terms are allowed because they do not change the linearity
//!   of the state transition.
//! - [`RadRecLinearity::LinearTimeVarying`] means the current back-edge is
//!   still linear, but at least one coefficient depends on a non-constant
//!   signal such as an input or UI control.
//! - [`RadRecLinearity::Nonlinear`] means a current back-edge flows through a
//!   nonlinear primitive, a branch, a discrete cast, or another expression
//!   that is not affine in the recursive variables.
//!
//! The analysis is intentionally conservative and read-only. Unsupported or
//! opaque signal families are treated as time-varying coefficients unless
//! they contain the current recursive reference in a recognized nonlinear
//! position.

use ahash::AHashMap;
use signals::{BinOp, SigId, SigMatch, match_sig};
use tlib::{TreeArena, TreeId, list_to_vec, match_de_bruijn_rec, match_de_bruijn_ref};

/// Linearity class for a recursive group with respect to its own back-edges.
///
/// This is a structural predicate for future stateful RAD phases, not a
/// promise that `rad(...)` currently accepts the group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadRecLinearity {
    /// Linear time-invariant state transition: recursive variables only appear
    /// linearly and under literal coefficients.
    LinearLti,
    /// Linear but time-varying state transition: recursive variables only
    /// appear linearly, but at least one coefficient is signal-dependent.
    LinearTimeVarying,
    /// Nonlinear dependence on a recursive variable.
    Nonlinear,
}

/// Strategy gate for a future RAD pass over one recursive signal group.
///
/// The enum names mirror plan §19.6:
///
/// - [`RecRadMode::LinearTranspose`] is the E1 target. The group is eligible
///   for an exact linear transpose, but `rad(...)` still needs a block/tape
///   evaluation convention before it can emit code.
/// - [`RecRadMode::BlockLinearTimeVarying`] is the E2 target. The group stays
///   linear in recursive state, but coefficients must be read from the primal
///   block at the corresponding sample.
/// - [`RecRadMode::BpttRequired`] is the phase-F target for nonlinear
///   feedback; it requires finite-horizon unrolling and a backward sweep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecRadMode {
    /// Phase E1 candidate: linear time-invariant recursive state transition.
    LinearTranspose,
    /// Phase E2 candidate: linear time-varying transition requiring block
    /// coefficient replay.
    BlockLinearTimeVarying,
    /// Phase F candidate: nonlinear recurrence requiring BPTT.
    BpttRequired,
}

impl From<RadRecLinearity> for RecRadMode {
    fn from(value: RadRecLinearity) -> Self {
        match value {
            RadRecLinearity::LinearLti => Self::LinearTranspose,
            RadRecLinearity::LinearTimeVarying => Self::BlockLinearTimeVarying,
            RadRecLinearity::Nonlinear => Self::BpttRequired,
        }
    }
}

impl RadRecLinearity {
    fn max(self, other: Self) -> Self {
        use RadRecLinearity::{LinearLti, LinearTimeVarying, Nonlinear};
        match (self, other) {
            (Nonlinear, _) | (_, Nonlinear) => Nonlinear,
            (LinearTimeVarying, _) | (_, LinearTimeVarying) => LinearTimeVarying,
            (LinearLti, LinearLti) => LinearLti,
        }
    }

    fn with_coeff(self, variation: IndependentVariation) -> Self {
        match variation {
            IndependentVariation::Constant => self,
            IndependentVariation::TimeVarying => self.max(Self::LinearTimeVarying),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IndependentVariation {
    Constant,
    TimeVarying,
}

impl IndependentVariation {
    fn max(self, other: Self) -> Self {
        match (self, other) {
            (Self::TimeVarying, _) | (_, Self::TimeVarying) => Self::TimeVarying,
            (Self::Constant, Self::Constant) => Self::Constant,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExprClass {
    depends_on_current_rec: bool,
    rec_linearity: RadRecLinearity,
    independent_variation: IndependentVariation,
}

impl ExprClass {
    fn constant() -> Self {
        Self {
            depends_on_current_rec: false,
            rec_linearity: RadRecLinearity::LinearLti,
            independent_variation: IndependentVariation::Constant,
        }
    }

    fn time_varying() -> Self {
        Self {
            depends_on_current_rec: false,
            rec_linearity: RadRecLinearity::LinearLti,
            independent_variation: IndependentVariation::TimeVarying,
        }
    }

    fn current_rec() -> Self {
        Self {
            depends_on_current_rec: true,
            rec_linearity: RadRecLinearity::LinearLti,
            independent_variation: IndependentVariation::Constant,
        }
    }

    fn nonlinear() -> Self {
        Self {
            depends_on_current_rec: true,
            rec_linearity: RadRecLinearity::Nonlinear,
            independent_variation: IndependentVariation::Constant,
        }
    }

    fn additive(self, other: Self) -> Self {
        match (self.depends_on_current_rec, other.depends_on_current_rec) {
            (false, false) => Self {
                depends_on_current_rec: false,
                rec_linearity: RadRecLinearity::LinearLti,
                independent_variation: self.independent_variation.max(other.independent_variation),
            },
            (true, false) => self,
            (false, true) => other,
            (true, true) => Self {
                depends_on_current_rec: true,
                rec_linearity: self.rec_linearity.max(other.rec_linearity),
                independent_variation: IndependentVariation::Constant,
            },
        }
    }

    fn multiplicative(self, other: Self) -> Self {
        match (self.depends_on_current_rec, other.depends_on_current_rec) {
            (true, true) => Self::nonlinear(),
            (true, false) => Self {
                depends_on_current_rec: true,
                rec_linearity: self.rec_linearity.with_coeff(other.independent_variation),
                independent_variation: IndependentVariation::Constant,
            },
            (false, true) => Self {
                depends_on_current_rec: true,
                rec_linearity: other.rec_linearity.with_coeff(self.independent_variation),
                independent_variation: IndependentVariation::Constant,
            },
            (false, false) => Self {
                depends_on_current_rec: false,
                rec_linearity: RadRecLinearity::LinearLti,
                independent_variation: self.independent_variation.max(other.independent_variation),
            },
        }
    }

    fn denominator(self, denom: Self) -> Self {
        if denom.depends_on_current_rec {
            return Self::nonlinear();
        }
        if self.depends_on_current_rec {
            Self {
                depends_on_current_rec: true,
                rec_linearity: self.rec_linearity.with_coeff(denom.independent_variation),
                independent_variation: IndependentVariation::Constant,
            }
        } else {
            Self {
                depends_on_current_rec: false,
                rec_linearity: RadRecLinearity::LinearLti,
                independent_variation: self.independent_variation.max(denom.independent_variation),
            }
        }
    }

    fn pure_unary(self) -> Self {
        if self.depends_on_current_rec {
            Self::nonlinear()
        } else {
            self
        }
    }

    fn discrete_unary(self) -> Self {
        if self.depends_on_current_rec {
            Self::nonlinear()
        } else {
            Self::time_varying()
        }
    }

    fn temporal_shift(self) -> Self {
        if self.depends_on_current_rec {
            self
        } else {
            Self {
                depends_on_current_rec: false,
                rec_linearity: RadRecLinearity::LinearLti,
                independent_variation: self.independent_variation,
            }
        }
    }

    fn branch(selector: Self, when_zero: Self, when_nonzero: Self) -> Self {
        if selector.depends_on_current_rec
            || when_zero.depends_on_current_rec
            || when_nonzero.depends_on_current_rec
        {
            Self::nonlinear()
        } else {
            Self {
                depends_on_current_rec: false,
                rec_linearity: RadRecLinearity::LinearLti,
                independent_variation: selector
                    .independent_variation
                    .max(when_zero.independent_variation)
                    .max(when_nonzero.independent_variation),
            }
        }
    }
}

/// Classifies one `DEBRUIJNREC(body)` group for future stateful RAD modes.
///
/// Returns `None` when `group` is not a De Bruijn recursive group. A
/// successful result is purely structural: current `rad(...)` lowering still
/// rejects recursion and delay nodes until a later phase adds a concrete
/// transpose or BPTT implementation.
#[must_use]
pub fn classify_de_bruijn_rec_group(arena: &TreeArena, group: SigId) -> Option<RadRecLinearity> {
    let body = match_de_bruijn_rec(arena, group)?;
    let branches = list_to_vec(arena, body)?;
    let mut analyzer = LinearityAnalyzer::new();
    Some(
        branches
            .into_iter()
            .map(|branch| analyzer.classify(arena, branch, 1).rec_linearity)
            .fold(RadRecLinearity::LinearLti, RadRecLinearity::max),
    )
}

/// Classifies one `DEBRUIJNREC(body)` group into the RAD strategy it would
/// need in a future stateful reverse-mode implementation.
///
/// Returns `None` for non-`DEBRUIJNREC` inputs. A returned mode is a gate, not
/// an implementation hook: phase-1 `rad(...)` still rejects recursive
/// projections until E1/E2/F add the corresponding runtime semantics.
#[must_use]
pub fn classify_de_bruijn_rec_rad_mode(arena: &TreeArena, group: SigId) -> Option<RecRadMode> {
    classify_de_bruijn_rec_group(arena, group).map(RecRadMode::from)
}

/// Classifies a recursive projection such as `Proj(i, DEBRUIJNREC(...))`.
///
/// This is the shape reached by propagated recursive boxes and by RAD's
/// current strict rejection path. Returning `None` means the signal is not a
/// direct projection over a De Bruijn recursive group.
#[must_use]
pub fn classify_recursive_projection_rad_mode(arena: &TreeArena, sig: SigId) -> Option<RecRadMode> {
    let SigMatch::Proj(_, group) = match_sig(arena, sig) else {
        return None;
    };
    classify_de_bruijn_rec_rad_mode(arena, group)
}

struct LinearityAnalyzer {
    memo: AHashMap<(TreeId, i64), ExprClass>,
}

impl LinearityAnalyzer {
    fn new() -> Self {
        Self {
            memo: AHashMap::new(),
        }
    }

    fn classify(&mut self, arena: &TreeArena, sig: SigId, current_level: i64) -> ExprClass {
        if let Some(cached) = self.memo.get(&(sig, current_level)).copied() {
            return cached;
        }

        let class = self.classify_uncached(arena, sig, current_level);
        self.memo.insert((sig, current_level), class);
        class
    }

    fn classify_uncached(
        &mut self,
        arena: &TreeArena,
        sig: SigId,
        current_level: i64,
    ) -> ExprClass {
        if match_de_bruijn_ref(arena, sig) == Some(current_level) {
            return ExprClass::current_rec();
        }
        if match_de_bruijn_ref(arena, sig).is_some() {
            return ExprClass::time_varying();
        }
        if let Some(body) = match_de_bruijn_rec(arena, sig) {
            let Some(branches) = list_to_vec(arena, body) else {
                return ExprClass::time_varying();
            };
            return branches
                .into_iter()
                .map(|branch| self.classify(arena, branch, current_level + 1))
                .fold(ExprClass::time_varying(), ExprClass::additive);
        }

        match match_sig(arena, sig) {
            SigMatch::Int(_) | SigMatch::Real(_) => ExprClass::constant(),
            SigMatch::Input(_)
            | SigMatch::Button(_)
            | SigMatch::Checkbox(_)
            | SigMatch::VSlider(_)
            | SigMatch::HSlider(_)
            | SigMatch::NumEntry(_)
            | SigMatch::FConst(_, _, _)
            | SigMatch::FVar(_, _, _)
            | SigMatch::Soundfile(_) => ExprClass::time_varying(),
            SigMatch::Output(_, x)
            | SigMatch::FloatCast(x)
            | SigMatch::VBargraph(_, x)
            | SigMatch::HBargraph(_, x)
            | SigMatch::Lowest(x)
            | SigMatch::Highest(x)
            | SigMatch::Clocked(_, x) => self.classify(arena, x, current_level),
            SigMatch::IntCast(x)
            | SigMatch::BitCast(x)
            | SigMatch::Gen(x)
            | SigMatch::Floor(x)
            | SigMatch::Ceil(x)
            | SigMatch::Rint(x)
            | SigMatch::Round(x) => self.classify(arena, x, current_level).discrete_unary(),
            SigMatch::Delay1(x) => self.classify(arena, x, current_level).temporal_shift(),
            SigMatch::Delay(x, amount) => {
                let x_class = self.classify(arena, x, current_level);
                let amount_class = self.classify(arena, amount, current_level);
                if amount_class.depends_on_current_rec {
                    ExprClass::nonlinear()
                } else if amount_class.independent_variation == IndependentVariation::TimeVarying {
                    ExprClass {
                        depends_on_current_rec: x_class.depends_on_current_rec,
                        rec_linearity: x_class
                            .rec_linearity
                            .max(RadRecLinearity::LinearTimeVarying),
                        independent_variation: x_class.independent_variation,
                    }
                } else {
                    x_class.temporal_shift()
                }
            }
            SigMatch::Prefix(init, x) => {
                let init_class = self.classify(arena, init, current_level);
                let x_class = self.classify(arena, x, current_level);
                if init_class.depends_on_current_rec {
                    ExprClass::nonlinear()
                } else {
                    x_class.temporal_shift()
                }
            }
            SigMatch::BinOp(op, x, y) => {
                let lhs = self.classify(arena, x, current_level);
                let rhs = self.classify(arena, y, current_level);
                match op {
                    BinOp::Add | BinOp::Sub => lhs.additive(rhs),
                    BinOp::Mul => lhs.multiplicative(rhs),
                    BinOp::Div => lhs.denominator(rhs),
                    BinOp::Rem
                    | BinOp::Lsh
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
                    | BinOp::Xor => {
                        if lhs.depends_on_current_rec || rhs.depends_on_current_rec {
                            ExprClass::nonlinear()
                        } else {
                            ExprClass::time_varying()
                        }
                    }
                }
            }
            SigMatch::Pow(x, y)
            | SigMatch::Min(x, y)
            | SigMatch::Max(x, y)
            | SigMatch::Atan2(x, y)
            | SigMatch::Fmod(x, y)
            | SigMatch::Remainder(x, y)
            | SigMatch::ZeroPad(x, y)
            | SigMatch::Seq(x, y) => {
                let lhs = self.classify(arena, x, current_level);
                let rhs = self.classify(arena, y, current_level);
                if lhs.depends_on_current_rec || rhs.depends_on_current_rec {
                    ExprClass::nonlinear()
                } else {
                    ExprClass::time_varying()
                }
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
            | SigMatch::Atan(x) => self.classify(arena, x, current_level).pure_unary(),
            SigMatch::Select2(selector, when_zero, when_nonzero) => ExprClass::branch(
                self.classify(arena, selector, current_level),
                self.classify(arena, when_zero, current_level),
                self.classify(arena, when_nonzero, current_level),
            ),
            SigMatch::AssertBounds(x, lo, hi) => {
                let x_class = self.classify(arena, x, current_level);
                let lo_class = self.classify(arena, lo, current_level);
                let hi_class = self.classify(arena, hi, current_level);
                if lo_class.depends_on_current_rec || hi_class.depends_on_current_rec {
                    ExprClass::nonlinear()
                } else {
                    x_class
                }
            }
            SigMatch::RdTbl(table, ridx) => {
                let table_class = self.classify(arena, table, current_level);
                let idx_class = self.classify(arena, ridx, current_level);
                if table_class.depends_on_current_rec || idx_class.depends_on_current_rec {
                    ExprClass::nonlinear()
                } else {
                    ExprClass::time_varying()
                }
            }
            SigMatch::WrTbl(size, generator, wi, ws)
            | SigMatch::SoundfileBuffer(size, generator, wi, ws) => {
                let combined = self
                    .classify(arena, size, current_level)
                    .additive(self.classify(arena, generator, current_level))
                    .additive(self.classify(arena, wi, current_level))
                    .additive(self.classify(arena, ws, current_level));
                if combined.depends_on_current_rec {
                    ExprClass::nonlinear()
                } else {
                    ExprClass::time_varying()
                }
            }
            SigMatch::SoundfileLength(sf, part) | SigMatch::SoundfileRate(sf, part) => {
                let combined = self
                    .classify(arena, sf, current_level)
                    .additive(self.classify(arena, part, current_level));
                if combined.depends_on_current_rec {
                    ExprClass::nonlinear()
                } else {
                    ExprClass::time_varying()
                }
            }
            SigMatch::FFun(_, largs) => {
                let Some(args) = list_to_vec(arena, largs) else {
                    return ExprClass::time_varying();
                };
                let combined = args
                    .into_iter()
                    .map(|arg| self.classify(arena, arg, current_level))
                    .fold(ExprClass::constant(), ExprClass::additive);
                if combined.depends_on_current_rec {
                    ExprClass::nonlinear()
                } else {
                    ExprClass::time_varying()
                }
            }
            SigMatch::Proj(_, group) => match match_de_bruijn_ref(arena, group) {
                Some(level) if level == current_level => ExprClass::current_rec(),
                Some(_) => ExprClass::time_varying(),
                None => {
                    let group_class = self.classify(arena, group, current_level);
                    if group_class.depends_on_current_rec {
                        group_class
                    } else {
                        ExprClass::time_varying()
                    }
                }
            },
            SigMatch::Rec(body) => self.classify(arena, body, current_level),
            SigMatch::Attach(x, y) | SigMatch::Enable(x, y) | SigMatch::Control(x, y) => {
                let x_class = self.classify(arena, x, current_level);
                let y_class = self.classify(arena, y, current_level);
                if y_class.depends_on_current_rec {
                    ExprClass::nonlinear()
                } else {
                    x_class
                }
            }
            SigMatch::Waveform(values)
            | SigMatch::OnDemand(values)
            | SigMatch::Upsampling(values)
            | SigMatch::Downsampling(values) => {
                let combined = values
                    .iter()
                    .copied()
                    .map(|x| self.classify(arena, x, current_level))
                    .fold(ExprClass::constant(), ExprClass::additive);
                if combined.depends_on_current_rec {
                    ExprClass::nonlinear()
                } else {
                    ExprClass::time_varying()
                }
            }
            SigMatch::TempVar(x) | SigMatch::PermVar(x) => self.classify(arena, x, current_level),
            SigMatch::Unknown => ExprClass::time_varying(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        RadRecLinearity, RecRadMode, classify_de_bruijn_rec_group, classify_de_bruijn_rec_rad_mode,
        classify_recursive_projection_rad_mode,
    };
    use signals::{SigBuilder, SigId};
    use tlib::{TreeArena, de_bruijn_rec, de_bruijn_ref, vec_to_list};

    fn one_branch_rec(arena: &mut TreeArena, branch: SigId) -> SigId {
        let body = vec_to_list(arena, &[branch]);
        de_bruijn_rec(arena, body)
    }

    #[test]
    fn classifier_accepts_constant_coefficient_linear_recursion_as_lti() {
        let mut arena = TreeArena::new();
        let ref1 = de_bruijn_ref(&mut arena, 1);
        let branch = {
            let mut b = SigBuilder::new(&mut arena);
            let prev = b.proj(0, ref1);
            let half = b.real(0.5);
            let scaled_prev = b.mul(half, prev);
            let input = b.input(0);
            b.add(input, scaled_prev)
        };
        let rec = one_branch_rec(&mut arena, branch);

        assert_eq!(
            classify_de_bruijn_rec_group(&arena, rec),
            Some(RadRecLinearity::LinearLti)
        );
    }

    #[test]
    fn classifier_marks_signal_dependent_coefficient_as_time_varying() {
        let mut arena = TreeArena::new();
        let ref1 = de_bruijn_ref(&mut arena, 1);
        let branch = {
            let mut b = SigBuilder::new(&mut arena);
            let prev = b.proj(0, ref1);
            let coeff = b.input(0);
            b.mul(coeff, prev)
        };
        let rec = one_branch_rec(&mut arena, branch);

        assert_eq!(
            classify_de_bruijn_rec_group(&arena, rec),
            Some(RadRecLinearity::LinearTimeVarying)
        );
    }

    #[test]
    fn classifier_marks_transcendental_of_recursive_state_as_nonlinear() {
        let mut arena = TreeArena::new();
        let ref1 = de_bruijn_ref(&mut arena, 1);
        let branch = {
            let mut b = SigBuilder::new(&mut arena);
            let prev = b.proj(0, ref1);
            b.sin(prev)
        };
        let rec = one_branch_rec(&mut arena, branch);

        assert_eq!(
            classify_de_bruijn_rec_group(&arena, rec),
            Some(RadRecLinearity::Nonlinear)
        );
    }

    #[test]
    fn classifier_treats_delay1_of_recursive_state_as_lti_shift() {
        let mut arena = TreeArena::new();
        let ref1 = de_bruijn_ref(&mut arena, 1);
        let branch = {
            let mut b = SigBuilder::new(&mut arena);
            let prev = b.proj(0, ref1);
            b.delay1(prev)
        };
        let rec = one_branch_rec(&mut arena, branch);

        assert_eq!(
            classify_de_bruijn_rec_group(&arena, rec),
            Some(RadRecLinearity::LinearLti)
        );
    }

    #[test]
    fn classifier_accepts_multi_output_cross_coupled_lti_recursion() {
        let mut arena = TreeArena::new();
        let ref1 = de_bruijn_ref(&mut arena, 1);
        let (branch0, branch1) = {
            let mut b = SigBuilder::new(&mut arena);
            let prev0 = b.proj(0, ref1);
            let prev1 = b.proj(1, ref1);
            let half = b.real(0.5);
            let scaled_prev1 = b.mul(half, prev1);
            let input = b.input(0);
            let branch0 = b.add(input, scaled_prev1);
            let branch1 = b.sub(prev0, input);
            (branch0, branch1)
        };
        let body = vec_to_list(&mut arena, &[branch0, branch1]);
        let rec = de_bruijn_rec(&mut arena, body);

        assert_eq!(
            classify_de_bruijn_rec_group(&arena, rec),
            Some(RadRecLinearity::LinearLti)
        );
    }

    #[test]
    fn rec_rad_mode_maps_linearity_classes_to_phase_targets() {
        assert_eq!(
            RecRadMode::from(RadRecLinearity::LinearLti),
            RecRadMode::LinearTranspose
        );
        assert_eq!(
            RecRadMode::from(RadRecLinearity::LinearTimeVarying),
            RecRadMode::BlockLinearTimeVarying
        );
        assert_eq!(
            RecRadMode::from(RadRecLinearity::Nonlinear),
            RecRadMode::BpttRequired
        );
    }

    #[test]
    fn classifier_reports_rad_mode_for_de_bruijn_group_and_projection() {
        let mut arena = TreeArena::new();
        let ref1 = de_bruijn_ref(&mut arena, 1);
        let branch = {
            let mut b = SigBuilder::new(&mut arena);
            let prev = b.proj(0, ref1);
            let coeff = b.input(0);
            b.mul(coeff, prev)
        };
        let rec = one_branch_rec(&mut arena, branch);
        let proj = SigBuilder::new(&mut arena).proj(0, rec);

        assert_eq!(
            classify_de_bruijn_rec_rad_mode(&arena, rec),
            Some(RecRadMode::BlockLinearTimeVarying)
        );
        assert_eq!(
            classify_recursive_projection_rad_mode(&arena, proj),
            Some(RecRadMode::BlockLinearTimeVarying)
        );
    }
}
