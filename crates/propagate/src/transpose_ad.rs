//! Structural scaffolding for phase-E1 recursive RAD transposition.
//!
//! # Source provenance
//! Original Rust design for RAD phase E1, documented in
//! `porting/reverse-ad-rad-implementation-plan-2026-04-27-en.md` section
//! "19. Feasibility analysis for stateful RAD".
//!
//! # Scope
//! This module is deliberately not wired into `rad(...)` lowering yet.  It
//! builds the small, testable core needed by the future E1 implementation:
//! given one affine LTI `DEBRUIJNREC` group, extract the linear state
//! transition matrix and build the transposed recursive group.
//!
//! The exported entry point is [`transpose_lti_de_bruijn_rec_scaffold`].
//! It accepts only groups that [`crate::stateful_rad::classify_de_bruijn_rec_group`]
//! has already proven [`RadRecLinearity::LinearLti`]. The second pass in this
//! file is intentionally narrower than that classifier: the classifier answers
//! "is a future exact RAD mode possible?", while this scaffold answers "can
//! the current E1 extractor build the concrete transposed state graph without
//! the block/tape convention yet?".
//!
//! # Input representation
//! The input is the same propagated De Bruijn recursion shape used by
//! [`crate::stateful_rad`]:
//!
//! ```text
//! DEBRUIJNREC([
//!     branch_0(Proj(0, DEBRUIJNREF(1)), Proj(1, DEBRUIJNREF(1)), ...),
//!     branch_1(...),
//!     ...
//! ])
//! ```
//!
//! Each branch is one row of the primal state update. If the group has `N`
//! branches, it has `N` state lanes. A term such as
//! `0.5 * Proj(1, DEBRUIJNREF(1))` in `branch_0` is recorded as a matrix
//! contribution from source output row `0` to primal state slot `1` with
//! coefficient `0.5`.
//!
//! Independent driving terms such as `input(0)`, UI controls, or literals not
//! multiplying a recursive state lane are ignored by the extractor because the
//! state-to-state transpose does not depend on them. Those terms still matter
//! for the primal program and for input-parameter gradients; this module only
//! builds the recursive-state adjoint skeleton.
//!
//! # Output representation
//! The returned value is another `DEBRUIJNREC` group with the same arity. Its
//! `target_slot` branch has the shape:
//!
//! ```text
//! input(target_slot)
//!   + sum_for_each_original_term_targeting_slot(
//!       coeff * Proj(source_output, DEBRUIJNREF(1))
//!     )
//! ```
//!
//! In matrix notation, if the primal recurrence is
//!
//! ```text
//! y[n] = A * y[n-1] + d[n]
//! ```
//!
//! the scaffold emits the recurrence for the block-local adjoint state:
//!
//! ```text
//! y_bar[n] = cotangent[n] + A^T * y_bar[n+1]
//! ```
//!
//! The emitted graph still uses ordinary `Proj(_, DEBRUIJNREF(1))` edges.
//! Its interpretation as `y_bar[n+1]` rather than `y_bar[n-1]` belongs to the
//! future reverse-block evaluator, not to this structural pass.
//!
//! The emitted group uses `input(i)` as the incoming cotangent for primal
//! output lane `i`, plus the transposed feedback from the previous adjoint
//! state. That is only an internal structural representation; a later phase
//! must still define the block/tape convention that supplies those inputs in
//! time-reversed order before this can become user-visible `rad(...)`
//! behavior.
//!
//! # Current conservative limits
//! - accepted recursive-state terms: `Proj(slot, DEBRUIJNREF(1))`, sums,
//!   differences, and multiplication/division by state-independent constant
//!   expressions;
//! - independent driving terms are ignored because they do not contribute to
//!   the state-to-state transpose;
//! - temporal operators over recursive state are rejected until the block
//!   evaluation convention fixes their adjoint placement.
//!
//! The first bullet is narrower than the E0 classifier. A group containing
//! `delay1(Proj(...))` is still LTI structurally, but this scaffold returns
//! [`TransposeAdError::TemporalTermNeedsBlockConvention`] because placing the
//! corresponding adjoint delay requires the reverse-time block semantics.
//!
//! # Failure policy
//! This module returns structured [`TransposeAdError`] values instead of
//! silently dropping unsupported recursive-state terms. That is a correctness
//! guard: a missing state term would emit an incomplete transpose and produce
//! a wrong gradient once the scaffold is wired into user-visible RAD.
//!
//! # Relationship to phase-1 RAD
//! Current phase-1 `rad(...)` still rejects recursive and temporal signal
//! families in `reverse_ad`. This module is a preparatory implementation and
//! a test target for the future phase-E1 path described in the porting plan.

use std::fmt::{Display, Formatter};

use crate::stateful_rad::{RadRecLinearity, classify_de_bruijn_rec_group};
use signals::{BinOp, SigBuilder, SigId, SigMatch, match_sig};
use tlib::{TreeArena, de_bruijn_rec, de_bruijn_ref, list_to_vec, match_de_bruijn_rec};

/// Error returned by the phase-E1 transposition scaffold.
///
/// These errors describe why the current structural extractor cannot build an
/// exact transposed recursive group. They are intentionally more precise than
/// a boolean "not supported" result so future RAD diagnostics can distinguish
/// malformed IR, non-LTI feedback, unsupported-but-linear syntax, temporal
/// placement gaps, and arity/index problems.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransposeAdError {
    /// The input was not a `DEBRUIJNREC(body)` group.
    ///
    /// The scaffold is defined only at the recursive-group boundary. Callers
    /// that start from a projection should pass the projected group, not the
    /// `Proj` node itself.
    NotRecursiveGroup,
    /// The recursive body list was malformed.
    ///
    /// Propagated recursion groups should carry a proper list of branch
    /// signals. A malformed list means the input is outside the expected
    /// propagation contract and no transpose can be trusted.
    MalformedBody,
    /// The classifier did not prove the group is LTI.
    ///
    /// This covers both [`RadRecLinearity::LinearTimeVarying`] and
    /// [`RadRecLinearity::Nonlinear`] results. Time-varying coefficients need
    /// phase E2 coefficient replay; nonlinear feedback needs a later BPTT or
    /// hybrid path.
    NotLinearLti,
    /// The affine extractor found a recursive-state term outside the current
    /// narrow E1 scaffold.
    ///
    /// Examples include a recursive state flowing through a smooth nonlinear
    /// primitive, a comparison, a branch, a table read, or a term where both
    /// operands of a multiplication depend on the current recursive state.
    UnsupportedLinearTerm,
    /// A temporal operator over recursive state needs the future block/tape
    /// convention before it can be transposed safely.
    ///
    /// The E0 classifier can mark `delay1(state)` as LTI, but the exact adjoint
    /// placement is anti-causal in stream time. The scaffold refuses it until
    /// reverse-block evaluation fixes the meaning of the shifted edge.
    TemporalTermNeedsBlockConvention,
    /// A projected recursive slot did not fit the group arity.
    ///
    /// Slot indices come from `Proj(slot, DEBRUIJNREF(1))`. Negative indices,
    /// indices that cannot convert to `usize`, and indices outside the branch
    /// count all indicate malformed recursive state references for this group.
    SlotOutOfRange,
}

impl Display for TransposeAdError {
    /// Formats the error as a compact diagnostic fragment.
    ///
    /// Higher-level RAD diagnostics are expected to add source context and the
    /// affected signal node. This implementation keeps the scaffold error
    /// suitable for `std::error::Error` and unit-test comparisons.
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRecursiveGroup => f.write_str("not a DEBRUIJNREC group"),
            Self::MalformedBody => f.write_str("malformed DEBRUIJNREC body list"),
            Self::NotLinearLti => f.write_str("recursive group is not classified as LinearLti"),
            Self::UnsupportedLinearTerm => {
                f.write_str("unsupported affine LTI term in recursive body")
            }
            Self::TemporalTermNeedsBlockConvention => {
                f.write_str("temporal recursive term needs a block/tape convention")
            }
            Self::SlotOutOfRange => f.write_str("recursive projection slot is out of range"),
        }
    }
}

impl std::error::Error for TransposeAdError {}

/// One coefficient entry of the primal state-transition matrix.
///
/// A term in primal branch `source_output` of the form
/// `coeff * Proj(state_slot, DEBRUIJNREF(1))` means:
///
/// ```text
/// A[source_output, state_slot] += coeff
/// ```
///
/// The emitted transposed group uses the same value as:
///
/// ```text
/// adjoint_branch[state_slot] += coeff * prev_adjoint[source_output]
/// ```
///
/// Coefficients are kept as `SigId` values rather than folded scalars so the
/// scaffold can preserve any constant signal expression that the classifier
/// accepted as state-independent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LinearTerm {
    /// Row in the primal update matrix, i.e. the branch where the term was
    /// found and the adjoint lane that feeds the transposed term.
    source_output: usize,
    /// Column in the primal update matrix, i.e. the recursive state lane read
    /// by the primal term and the branch that receives the transposed term.
    state_slot: usize,
    /// State-independent coefficient accumulated along the affine path from
    /// the branch root to the recursive projection.
    coeff: SigId,
}

/// Builds the transposed affine LTI recursive group for future RAD phase E1.
///
/// Mapping status: `adapted`, preparatory only. This mirrors the system
/// transposition rules in the RAD plan but intentionally does not change
/// current `rad(...)` behavior. Callers must not treat the returned group as
/// executable reverse-mode code until the surrounding block/tape evaluation
/// convention exists.
///
/// # Accepted input
/// `group` must be a well-formed `DEBRUIJNREC(body)` whose body list contains
/// one branch per state lane. The E0 classifier must classify it as
/// [`RadRecLinearity::LinearLti`].
///
/// The extractor currently accepts recursive-state occurrences built from:
///
/// - direct projections: `Proj(slot, DEBRUIJNREF(1))`;
/// - addition and subtraction;
/// - multiplication by state-independent coefficients;
/// - division by state-independent denominators;
/// - transparent wrappers: `FloatCast`, `Output`, `Lowest`, and `Highest`.
///
/// State-independent subgraphs are ignored unless they multiply or divide a
/// recursive-state path, because they do not contribute to the recursive
/// state-transition transpose.
///
/// # Returned graph
/// On success, the result is a new `DEBRUIJNREC` group with the same arity as
/// the input. Branch `i` starts with `input(i)`, representing the incoming
/// cotangent for primal lane `i`, then adds one transposed feedback term for
/// every extracted matrix entry targeting state slot `i`.
///
/// # Errors
/// The function returns [`TransposeAdError::NotLinearLti`] before extraction
/// when the classifier cannot prove an LTI transition. Extraction can still
/// return [`TransposeAdError::UnsupportedLinearTerm`] or
/// [`TransposeAdError::TemporalTermNeedsBlockConvention`] for LTI expressions
/// that are outside this narrow scaffold's current syntax.
pub fn transpose_lti_de_bruijn_rec_scaffold(
    arena: &mut TreeArena,
    group: SigId,
) -> Result<SigId, TransposeAdError> {
    let body = match_de_bruijn_rec(arena, group).ok_or(TransposeAdError::NotRecursiveGroup)?;
    if classify_de_bruijn_rec_group(arena, group) != Some(RadRecLinearity::LinearLti) {
        return Err(TransposeAdError::NotLinearLti);
    }
    let branches = list_to_vec(arena, body).ok_or(TransposeAdError::MalformedBody)?;
    let arity = branches.len();
    let mut terms = Vec::new();
    let one = SigBuilder::new(arena).real(1.0);

    for (source_output, branch) in branches.into_iter().enumerate() {
        extract_affine_state_terms(arena, branch, 1, one, source_output, arity, &mut terms)?;
    }

    let rec_ref = de_bruijn_ref(arena, 1);
    let mut transposed_branches = Vec::with_capacity(arity);
    for target_slot in 0..arity {
        let target_i32 =
            i32::try_from(target_slot).map_err(|_| TransposeAdError::SlotOutOfRange)?;
        let mut expr = SigBuilder::new(arena).input(target_i32);
        for term in terms.iter().filter(|term| term.state_slot == target_slot) {
            let source_i32 =
                i32::try_from(term.source_output).map_err(|_| TransposeAdError::SlotOutOfRange)?;
            let prev_adj = SigBuilder::new(arena).proj(source_i32, rec_ref);
            let scaled = SigBuilder::new(arena).mul(term.coeff, prev_adj);
            expr = SigBuilder::new(arena).add(expr, scaled);
        }
        transposed_branches.push(expr);
    }

    let body = tlib::vec_to_list(arena, &transposed_branches);
    Ok(de_bruijn_rec(arena, body))
}

/// Extracts affine recursive-state terms from one primal branch.
///
/// `coeff` is the accumulated state-independent multiplier on the path from
/// the branch root to `sig`. `source_output` identifies the primal branch
/// being scanned. Each direct `Proj(slot, DEBRUIJNREF(current_level))` appends
/// one [`LinearTerm`] with that accumulated coefficient.
///
/// The extractor is deliberately syntax-directed. It does not normalize
/// algebraically equivalent expressions, distribute multiplication over sums,
/// or fold constants. The E0 classifier has already guaranteed LTI structure;
/// this pass only records the subset that can be emitted by the current E1
/// scaffold.
///
/// State-independent subgraphs return `Ok(())` because they are driving terms.
/// They affect the primal recurrence but not the state-transition matrix.
fn extract_affine_state_terms(
    arena: &mut TreeArena,
    sig: SigId,
    current_level: i64,
    coeff: SigId,
    source_output: usize,
    arity: usize,
    out: &mut Vec<LinearTerm>,
) -> Result<(), TransposeAdError> {
    match match_sig(arena, sig) {
        SigMatch::Proj(slot, group)
            if tlib::match_de_bruijn_ref(arena, group) == Some(current_level) =>
        {
            let state_slot = usize::try_from(slot).map_err(|_| TransposeAdError::SlotOutOfRange)?;
            if state_slot >= arity {
                return Err(TransposeAdError::SlotOutOfRange);
            }
            out.push(LinearTerm {
                source_output,
                state_slot,
                coeff,
            });
            Ok(())
        }
        SigMatch::BinOp(BinOp::Add, x, y) => {
            extract_affine_state_terms(arena, x, current_level, coeff, source_output, arity, out)?;
            extract_affine_state_terms(arena, y, current_level, coeff, source_output, arity, out)
        }
        SigMatch::BinOp(BinOp::Sub, x, y) => {
            extract_affine_state_terms(arena, x, current_level, coeff, source_output, arity, out)?;
            let neg_one = SigBuilder::new(arena).real(-1.0);
            let neg_coeff = SigBuilder::new(arena).mul(coeff, neg_one);
            extract_affine_state_terms(
                arena,
                y,
                current_level,
                neg_coeff,
                source_output,
                arity,
                out,
            )
        }
        SigMatch::BinOp(BinOp::Mul, x, y) => {
            let x_depends = contains_current_rec_ref(arena, x, current_level);
            let y_depends = contains_current_rec_ref(arena, y, current_level);
            match (x_depends, y_depends) {
                (true, false) => {
                    let scaled = SigBuilder::new(arena).mul(coeff, y);
                    extract_affine_state_terms(
                        arena,
                        x,
                        current_level,
                        scaled,
                        source_output,
                        arity,
                        out,
                    )
                }
                (false, true) => {
                    let scaled = SigBuilder::new(arena).mul(coeff, x);
                    extract_affine_state_terms(
                        arena,
                        y,
                        current_level,
                        scaled,
                        source_output,
                        arity,
                        out,
                    )
                }
                (false, false) => Ok(()),
                (true, true) => Err(TransposeAdError::UnsupportedLinearTerm),
            }
        }
        SigMatch::BinOp(BinOp::Div, x, y) => {
            if contains_current_rec_ref(arena, y, current_level) {
                return Err(TransposeAdError::UnsupportedLinearTerm);
            }
            if contains_current_rec_ref(arena, x, current_level) {
                let scaled = SigBuilder::new(arena).div(coeff, y);
                extract_affine_state_terms(
                    arena,
                    x,
                    current_level,
                    scaled,
                    source_output,
                    arity,
                    out,
                )
            } else {
                Ok(())
            }
        }
        SigMatch::Delay1(x) | SigMatch::Delay(x, _) | SigMatch::Prefix(_, x)
            if contains_current_rec_ref(arena, x, current_level) =>
        {
            Err(TransposeAdError::TemporalTermNeedsBlockConvention)
        }
        SigMatch::FloatCast(x)
        | SigMatch::Output(_, x)
        | SigMatch::Lowest(x)
        | SigMatch::Highest(x) => {
            extract_affine_state_terms(arena, x, current_level, coeff, source_output, arity, out)
        }
        _ if contains_current_rec_ref(arena, sig, current_level) => {
            Err(TransposeAdError::UnsupportedLinearTerm)
        }
        _ => Ok(()),
    }
}

/// Returns whether `sig` contains a reference to the current recursive group.
///
/// The scan is structural and conservative. It recognizes a direct
/// `DEBRUIJNREF(current_level)` immediately, then recursively walks child
/// nodes. Nested `DEBRUIJNREC` scopes increment `current_level` so references
/// inside the nested body are interpreted relative to that nested recursion
/// rather than the enclosing group.
///
/// This helper is used by the extractor to decide whether an operand is a
/// coefficient/driving expression or part of the recursive state path.
fn contains_current_rec_ref(arena: &TreeArena, sig: SigId, current_level: i64) -> bool {
    if tlib::match_de_bruijn_ref(arena, sig) == Some(current_level) {
        return true;
    }
    if let Some(body) = match_de_bruijn_rec(arena, sig) {
        return contains_current_rec_ref(arena, body, current_level + 1);
    }
    let Some(node) = arena.node(sig) else {
        return false;
    };
    node.children
        .as_slice()
        .iter()
        .copied()
        .any(|child| contains_current_rec_ref(arena, child, current_level))
}

#[cfg(test)]
mod tests {
    use super::{TransposeAdError, transpose_lti_de_bruijn_rec_scaffold};
    use signals::{BinOp, SigBuilder, SigId, SigMatch, match_sig};
    use tlib::{
        TreeArena, de_bruijn_rec, de_bruijn_ref, list_to_vec, match_de_bruijn_rec, vec_to_list,
    };

    fn rec_group(arena: &mut TreeArena, branches: &[SigId]) -> SigId {
        let body = vec_to_list(arena, branches);
        de_bruijn_rec(arena, body)
    }

    fn projected_slot(arena: &TreeArena, sig: SigId) -> Option<usize> {
        let SigMatch::Proj(slot, group) = match_sig(arena, sig) else {
            return None;
        };
        if tlib::match_de_bruijn_ref(arena, group) == Some(1) {
            usize::try_from(slot).ok()
        } else {
            None
        }
    }

    fn branch_contains_prev_slot(arena: &TreeArena, sig: SigId, slot: usize) -> bool {
        if projected_slot(arena, sig) == Some(slot) {
            return true;
        }
        match match_sig(arena, sig) {
            SigMatch::BinOp(BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div, x, y) => {
                branch_contains_prev_slot(arena, x, slot)
                    || branch_contains_prev_slot(arena, y, slot)
            }
            _ => false,
        }
    }

    #[test]
    fn scaffold_transposes_cross_coupled_affine_lti_group() {
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
        let group = rec_group(&mut arena, &[branch0, branch1]);

        let transposed =
            transpose_lti_de_bruijn_rec_scaffold(&mut arena, group).expect("LTI group");
        let body = match_de_bruijn_rec(&arena, transposed).expect("transposed group");
        let branches = list_to_vec(&arena, body).expect("body list");

        assert_eq!(branches.len(), 2);
        assert!(
            branch_contains_prev_slot(&arena, branches[0], 1),
            "target adjoint slot 0 must receive original row 1"
        );
        assert!(
            branch_contains_prev_slot(&arena, branches[1], 0),
            "target adjoint slot 1 must receive original row 0"
        );
    }

    #[test]
    fn scaffold_rejects_time_varying_and_nonlinear_groups() {
        let mut arena = TreeArena::new();
        let ref1 = de_bruijn_ref(&mut arena, 1);
        let ltv_group = {
            let mut b = SigBuilder::new(&mut arena);
            let prev = b.proj(0, ref1);
            let coeff = b.input(0);
            let branch = b.mul(coeff, prev);
            rec_group(&mut arena, &[branch])
        };
        assert_eq!(
            transpose_lti_de_bruijn_rec_scaffold(&mut arena, ltv_group),
            Err(TransposeAdError::NotLinearLti)
        );

        let nonlinear_group = {
            let mut b = SigBuilder::new(&mut arena);
            let prev = b.proj(0, ref1);
            let branch = b.sin(prev);
            rec_group(&mut arena, &[branch])
        };
        assert_eq!(
            transpose_lti_de_bruijn_rec_scaffold(&mut arena, nonlinear_group),
            Err(TransposeAdError::NotLinearLti)
        );
    }

    #[test]
    fn scaffold_keeps_temporal_lti_terms_blocked() {
        let mut arena = TreeArena::new();
        let ref1 = de_bruijn_ref(&mut arena, 1);
        let group = {
            let mut b = SigBuilder::new(&mut arena);
            let prev = b.proj(0, ref1);
            let branch = b.delay1(prev);
            rec_group(&mut arena, &[branch])
        };

        assert_eq!(
            transpose_lti_de_bruijn_rec_scaffold(&mut arena, group),
            Err(TransposeAdError::TemporalTermNeedsBlockConvention)
        );
    }

    /// Tiny interpreter over the small subset of `SigMatch` nodes that the E1
    /// scaffold currently emits / accepts. Used by the numeric oracles below
    /// to evaluate one recurrence frame given the previous-frame state and
    /// per-frame `input(_)` lanes. Panics on any node outside the expected
    /// LTI subset; that panic is itself part of the test contract.
    fn eval_branch(
        arena: &TreeArena,
        sig: SigId,
        inputs: &[f32],
        prev_state: &[f32],
    ) -> f32 {
        match match_sig(arena, sig) {
            SigMatch::Real(r) => r as f32,
            SigMatch::Int(i) => i as f32,
            SigMatch::Input(idx) => inputs[idx as usize],
            SigMatch::Proj(slot, group)
                if tlib::match_de_bruijn_ref(arena, group) == Some(1) =>
            {
                prev_state[slot as usize]
            }
            SigMatch::BinOp(BinOp::Add, x, y) => {
                eval_branch(arena, x, inputs, prev_state)
                    + eval_branch(arena, y, inputs, prev_state)
            }
            SigMatch::BinOp(BinOp::Sub, x, y) => {
                eval_branch(arena, x, inputs, prev_state)
                    - eval_branch(arena, y, inputs, prev_state)
            }
            SigMatch::BinOp(BinOp::Mul, x, y) => {
                eval_branch(arena, x, inputs, prev_state)
                    * eval_branch(arena, y, inputs, prev_state)
            }
            SigMatch::BinOp(BinOp::Div, x, y) => {
                eval_branch(arena, x, inputs, prev_state)
                    / eval_branch(arena, y, inputs, prev_state)
            }
            other => panic!(
                "eval_branch: unsupported node {other:?} (test interpreter is intentionally narrow)"
            ),
        }
    }

    /// Evaluates one recursive group forward in time over `frames` frames.
    /// `inputs_per_frame[n]` lists the `input(_)` lanes at frame `n`.
    fn evaluate_recursion(
        arena: &TreeArena,
        group: SigId,
        inputs_per_frame: &[Vec<f32>],
    ) -> Vec<Vec<f32>> {
        let body = match_de_bruijn_rec(arena, group).expect("recursive group");
        let branches = list_to_vec(arena, body).expect("body list");
        let arity = branches.len();
        let mut prev_state = vec![0.0_f32; arity];
        let mut history = Vec::with_capacity(inputs_per_frame.len());
        for inputs in inputs_per_frame {
            let mut next_state = Vec::with_capacity(arity);
            for &branch in &branches {
                next_state.push(eval_branch(arena, branch, inputs, &prev_state));
            }
            history.push(next_state.clone());
            prev_state = next_state;
        }
        history
    }

    /// Evaluates the transposed recursive group **in reverse time** over the
    /// same number of frames, with terminal adjoint state set to zero. The
    /// returned history is in forward-time order (so `history[n]` is the
    /// adjoint state at frame `n`).
    fn evaluate_transposed_reverse(
        arena: &TreeArena,
        transposed: SigId,
        cotangents_per_frame: &[Vec<f32>],
    ) -> Vec<Vec<f32>> {
        let body = match_de_bruijn_rec(arena, transposed).expect("transposed group");
        let branches = list_to_vec(arena, body).expect("body list");
        let arity = branches.len();
        let frame_count = cotangents_per_frame.len();
        // Terminal boundary: y_bar[N] = 0 (block-local boundary, plan §19).
        let mut next_state = vec![0.0_f32; arity];
        let mut adjoints = vec![vec![0.0_f32; arity]; frame_count];
        for n in (0..frame_count).rev() {
            // In the transposed group, "prev_state" semantically means
            // "the adjoint at frame n+1" because evaluation runs in reverse
            // time. The branch's `input(i)` carries the cotangent at the
            // current frame `n`.
            let mut current_state = Vec::with_capacity(arity);
            for &branch in &branches {
                current_state.push(eval_branch(
                    arena,
                    branch,
                    &cotangents_per_frame[n],
                    &next_state,
                ));
            }
            adjoints[n] = current_state.clone();
            next_state = current_state;
        }
        adjoints
    }

    #[test]
    fn scaffold_first_order_lti_matches_analytic_seed_adjoint() {
        // Canonical first-order LTI: y[n] = p · y[n-1] + x[n]
        // Body shape after propagation: `+ ~ *(p)` ⇒
        //   branch[0] = input(0) + p * proj(0, ref(1))
        //
        // For a constant input x[n] = c, the closed form is
        //   y[n] = c · (1 - p^(n+1)) / (1 - p)        for p ≠ 1.
        // With all-ones cotangent over an N-frame block, the implicit-sum
        // RAD seed adjoint w.r.t. p is
        //   p_bar = sum_{n=0}^{N-1} ∂y[n]/∂p
        //         = sum_{n=0}^{N-1} sum_{k=0}^{n-1} (k+1) · p^k · y[n-1-k]
        // The transposed group gives an alternative computation:
        //   p_bar = sum_{n=1}^{N-1} y_bar[n] · y[n-1]
        // where y_bar runs the transposed recurrence in reverse time with
        // terminal y_bar[N] = 0.
        //
        // Both expressions must agree numerically. This test pins that
        // identity for p = 0.6, x = 1.0, N = 6.
        let mut arena = TreeArena::new();
        let p = 0.6_f32;
        let frames = 6;

        // Build `+ ~ *(p)` body: input(0) + p · proj(0, ref(1))
        let group = {
            let ref1 = de_bruijn_ref(&mut arena, 1);
            let mut b = SigBuilder::new(&mut arena);
            let p_sig = b.real(f64::from(p));
            let prev = b.proj(0, ref1);
            let scaled = b.mul(p_sig, prev);
            let input = b.input(0);
            let branch = b.add(input, scaled);
            rec_group(&mut arena, &[branch])
        };
        let transposed =
            transpose_lti_de_bruijn_rec_scaffold(&mut arena, group).expect("LTI group");

        // Forward-time primal evaluation: x[n] = 1 for every frame.
        let primal_inputs: Vec<Vec<f32>> = (0..frames).map(|_| vec![1.0_f32]).collect();
        let primal = evaluate_recursion(&arena, group, &primal_inputs);

        // Reverse-time adjoint evaluation: cotangent input(0)=1 every frame
        // (implicit all-ones cotangent for the single primal output).
        let cotangents: Vec<Vec<f32>> = (0..frames).map(|_| vec![1.0_f32]).collect();
        let adjoints = evaluate_transposed_reverse(&arena, transposed, &cotangents);

        // Seed adjoint via the transposed path:
        //   p_bar = sum_{n=1}^{N-1} y_bar[n] · y[n-1]
        let p_bar_transposed: f32 = (1..frames).map(|n| adjoints[n][0] * primal[n - 1][0]).sum();

        // Analytical reference: differentiate y[n] = (1 - p^(n+1)) / (1 - p)
        // w.r.t. p, sum over n = 0..N-1.
        let p_bar_analytic: f32 = (0..frames)
            .map(|n| {
                let n_plus_1 = (n + 1) as f32;
                let p_n_plus_1 = p.powi(n_plus_1 as i32);
                let p_n = p.powi(n as i32);
                let one_minus_p = 1.0 - p;
                // d/dp [(1 - p^(n+1)) / (1 - p)]
                // = [-(n+1) · p^n · (1 - p) - (1 - p^(n+1)) · (-1)] / (1 - p)²
                // = [(1 - p^(n+1)) - (n+1) · p^n · (1 - p)] / (1 - p)²
                let num = (1.0 - p_n_plus_1) - n_plus_1 * p_n * one_minus_p;
                num / (one_minus_p * one_minus_p)
            })
            .sum();

        assert!(
            (p_bar_transposed - p_bar_analytic).abs() < 1.0e-4,
            "transposed-path p_bar = {p_bar_transposed} differs from analytic {p_bar_analytic}"
        );
    }

    #[test]
    fn scaffold_diagonal_two_state_lti_matches_independent_analytic_adjoints() {
        // Two independent first-order LTI states, no cross-coupling:
        //   y0[n] = p0 · y0[n-1] + x0[n]
        //   y1[n] = p1 · y1[n-1] + x1[n]
        // The transposed group must keep them independent. With cotangent
        // c0=1, c1=0 at every frame, only y0_bar is non-zero and the
        // computed seed adjoints w.r.t. p0 and p1 are independent.
        let mut arena = TreeArena::new();
        let p0 = 0.5_f32;
        let p1 = 0.3_f32;
        let frames = 5;

        let group = {
            let ref1 = de_bruijn_ref(&mut arena, 1);
            let mut b = SigBuilder::new(&mut arena);
            let p0_sig = b.real(f64::from(p0));
            let p1_sig = b.real(f64::from(p1));
            let prev0 = b.proj(0, ref1);
            let prev1 = b.proj(1, ref1);
            let scaled0 = b.mul(p0_sig, prev0);
            let scaled1 = b.mul(p1_sig, prev1);
            let in0 = b.input(0);
            let in1 = b.input(1);
            let br0 = b.add(in0, scaled0);
            let br1 = b.add(in1, scaled1);
            rec_group(&mut arena, &[br0, br1])
        };
        let transposed =
            transpose_lti_de_bruijn_rec_scaffold(&mut arena, group).expect("LTI group");

        let primal_inputs: Vec<Vec<f32>> = (0..frames).map(|_| vec![1.0_f32, 1.0_f32]).collect();
        let primal = evaluate_recursion(&arena, group, &primal_inputs);

        // Cotangent only on lane 0 — lane 1 should stay at zero through the
        // transposed evaluation because the diagonal group has no cross-
        // coupling.
        let cotangents: Vec<Vec<f32>> = (0..frames).map(|_| vec![1.0_f32, 0.0_f32]).collect();
        let adjoints = evaluate_transposed_reverse(&arena, transposed, &cotangents);

        for (n, frame_adj) in adjoints.iter().enumerate() {
            assert!(
                frame_adj[1].abs() < 1.0e-6,
                "diagonal LTI: lane-1 adjoint must stay at 0 (frame {n}, got {})",
                frame_adj[1]
            );
        }

        // Lane-0 seed adjoint via the transposed path agrees with the
        // first-order analytic result (same closed form as above).
        let p0_bar_transposed: f32 = (1..frames).map(|n| adjoints[n][0] * primal[n - 1][0]).sum();
        let p0_bar_analytic: f32 = (0..frames)
            .map(|n| {
                let n1 = (n + 1) as f32;
                let pn1 = p0.powi(n1 as i32);
                let pn = p0.powi(n as i32);
                let omp = 1.0 - p0;
                ((1.0 - pn1) - n1 * pn * omp) / (omp * omp)
            })
            .sum();
        assert!(
            (p0_bar_transposed - p0_bar_analytic).abs() < 1.0e-4,
            "diagonal-LTI p0_bar = {p0_bar_transposed} differs from analytic {p0_bar_analytic}"
        );
    }
}
