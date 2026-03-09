//! Box-to-signal propagation (Phase 4, section 2.4).
//!
//! # Source provenance (C++)
//! - `/Users/letz/Developpements/RUST/faust/compiler/propagate/propagate.hh`
//! - `/Users/letz/Developpements/RUST/faust/compiler/propagate/propagate.cpp`
//! - `/Users/letz/Developpements/RUST/faust/compiler/boxes/boxtype.cpp`
//!
//! # Current scope
//! - Core box arity inference for supported box families.
//! - Primitive lowering from `boxes::BoxMatch` to `signals::SigBuilder`.
//! - Composition algebra: `seq`, `par`, `split`, `merge`.
//! - Explicit typed errors for unsupported nodes and arity mismatches.
//! - Recursive composition lowering with De Bruijn-style placeholders (`sigRec/sigProj` shape).
//! - Typed `FlatBoxId` boundary that validates the post-`eval/a2sb` flat box subset.
//!
//! # Public API mapping status
//! - `box_arity(...)` mirrors the C++ `getBoxType(...)` role for the supported subset.
//! - `propagate(...)` mirrors C++ `propagate(...)` on the supported subset.
//! - `make_sig_input_list(...)` mirrors C++ `makeSigInputList(...)`.
//! - `FlatBoxId` / [`try_build_flat_box`] are an adapted Rust boundary: they make the
//!   C++ post-`evalprocess -> a2sb -> propagate` flat-box contract explicit while
//!   preserving `TreeArena` node sharing through `TreeId`.
//!
//! # Integer convention
//! - Integer signals emitted by this pass are `i32`-semantic.
//! - Conversions from container sizes/indices (`usize`) are explicit and
//!   fallible to preserve deterministic diagnostics on overflow.

use std::fmt::{Display, Formatter};

use ahash::AHashMap;
use boxes::{BoxId, BoxMatch, match_box};
use errors::codes;
use errors::{Diagnostic, IntoDiagnostic, Severity, Stage};
use signals::{SigBuilder, SigId};
use tlib::{NodeKind, TreeArena, TreeId, tree_to_int};

/// Memoization cache for [`box_arity`] / [`box_arity_flat`] results, keyed by validated flat boxes.
pub type ArityCache = AHashMap<FlatBoxId, Result<BoxArity, PropagateError>>;
type SlotEnv = AHashMap<BoxId, SigId>;

pub const CRATE_NAME: &str = "propagate";
const DEBRUIJN_TAG: &str = "DEBRUIJN";
const DEBRUIJNREF_TAG: &str = "DEBRUIJNREF";

/// Stable crate identifier used in workspace-level tooling and diagnostics.
#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}

/// Input/output arity of one box expression.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoxArity {
    /// Number of required input signals.
    pub inputs: usize,
    /// Number of produced output signals.
    pub outputs: usize,
}

/// Typed handle for the flat post-eval box subset accepted at the propagation boundary.
///
/// # Source provenance (C++)
/// - `compiler/evaluate/eval.cpp`
/// - `evalprocess(...)`
/// - `a2sb(...)`
/// - `compiler/propagate/propagate.cpp`
/// - `realPropagate(...)`
///
/// The C++ production pipeline does not feed arbitrary box syntax into
/// propagation. It first evaluates the program and lowers residual
/// lambda/pattern forms through `a2sb(...)`, then propagates the resulting
/// first-order box tree.
///
/// Rust keeps the same tree arena and structural sharing model: `FlatBoxId` is
/// a validated `TreeId` wrapper, not a copied owned IR. Its role is to make the
/// post-eval contract explicit:
///
/// - accepted: flat first-order box families such as primitives, symbolic
///   slots, widgets, groups, route, and composition algebra,
/// - rejected: evaluator-only syntax that must have disappeared before
///   propagation, such as `abstr`, `case`, `pattern_var`, and local-definition
///   shells.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FlatBoxId(TreeId);

impl FlatBoxId {
    #[must_use]
    pub fn as_tree_id(self) -> TreeId {
        self.0
    }

    fn from_tree_id(id: TreeId) -> Self {
        Self(id)
    }
}

/// Boundary validation error while converting a generic `BoxId` into a [`FlatBoxId`].
///
/// This error means the caller attempted to feed propagation a box family that
/// is not valid after the C++ `evalprocess -> a2sb` lowering boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlatBoxBuildError {
    UnexpectedPostEvalBox { node: TreeId, kind: &'static str },
}

impl Display for FlatBoxBuildError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedPostEvalBox { node, kind } => write!(
                f,
                "box node {} ({kind}) is not valid in the post-eval flat box subset",
                node.as_u32()
            ),
        }
    }
}

impl std::error::Error for FlatBoxBuildError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FlatNodeKind {
    Int,
    Real,
    Wire,
    Cut,
    Slot,
    Symbolic { body: FlatBoxId },
    Metadata { body: FlatBoxId },
    Prim1,
    Prim2,
    Prim3,
    Prim4,
    Prim5,
    FFun,
    FConst,
    FVar,
    Button,
    Checkbox,
    VSlider,
    HSlider,
    NumEntry,
    VBargraph,
    HBargraph,
    Soundfile,
    Waveform,
    VGroup { body: FlatBoxId },
    HGroup { body: FlatBoxId },
    TGroup { body: FlatBoxId },
    Seq(FlatBoxId, FlatBoxId),
    Par(FlatBoxId, FlatBoxId),
    Split(FlatBoxId, FlatBoxId),
    Merge(FlatBoxId, FlatBoxId),
    Rec(FlatBoxId, FlatBoxId),
    Environment,
    Route,
    Inputs,
    Outputs,
    Ondemand(FlatBoxId),
    Upsampling(FlatBoxId),
    Downsampling(FlatBoxId),
}

/// Validates that `root` belongs to the flat post-eval box subset and returns a typed handle.
///
/// This is a structural contract check only. It does not evaluate, simplify, or
/// normalize the tree. Callers should use it at the `eval/a2sb -> propagate`
/// boundary to guarantee that propagation never sees residual evaluator syntax.
pub fn try_build_flat_box(
    arena: &TreeArena,
    root: BoxId,
) -> Result<FlatBoxId, FlatBoxBuildError> {
    validate_flat_box(arena, root)
}

fn validate_flat_box(arena: &TreeArena, node: BoxId) -> Result<FlatBoxId, FlatBoxBuildError> {
    let flat = FlatBoxId::from_tree_id(node);
    let _ = flat_node_kind(arena, flat)?;
    Ok(flat)
}

fn flat_node_kind(arena: &TreeArena, node: FlatBoxId) -> Result<FlatNodeKind, FlatBoxBuildError> {
    let node_id = node.as_tree_id();
    match match_box(arena, node_id) {
        BoxMatch::Int(_) => Ok(FlatNodeKind::Int),
        BoxMatch::Real(_) => Ok(FlatNodeKind::Real),
        BoxMatch::Wire => Ok(FlatNodeKind::Wire),
        BoxMatch::Cut => Ok(FlatNodeKind::Cut),
        BoxMatch::Slot(_) => Ok(FlatNodeKind::Slot),
        BoxMatch::Symbolic(_, body) => Ok(FlatNodeKind::Symbolic {
            body: validate_flat_box(arena, body)?,
        }),
        BoxMatch::Metadata(body, _) => Ok(FlatNodeKind::Metadata {
            body: validate_flat_box(arena, body)?,
        }),
        BoxMatch::Add
        | BoxMatch::Sub
        | BoxMatch::Mul
        | BoxMatch::Div
        | BoxMatch::Rem
        | BoxMatch::And
        | BoxMatch::Or
        | BoxMatch::Xor
        | BoxMatch::Lsh
        | BoxMatch::Rsh
        | BoxMatch::Lt
        | BoxMatch::Le
        | BoxMatch::Gt
        | BoxMatch::Ge
        | BoxMatch::Eq
        | BoxMatch::Ne
        | BoxMatch::Pow
        | BoxMatch::Atan2
        | BoxMatch::Fmod
        | BoxMatch::Remainder
        | BoxMatch::Delay
        | BoxMatch::Min
        | BoxMatch::Max
        | BoxMatch::Prefix
        | BoxMatch::Attach
        | BoxMatch::Enable
        | BoxMatch::Control => Ok(FlatNodeKind::Prim2),
        BoxMatch::Delay1
        | BoxMatch::IntCast
        | BoxMatch::FloatCast
        | BoxMatch::Acos
        | BoxMatch::Asin
        | BoxMatch::Atan
        | BoxMatch::Cos
        | BoxMatch::Sin
        | BoxMatch::Tan
        | BoxMatch::Exp
        | BoxMatch::Log
        | BoxMatch::Log10
        | BoxMatch::Sqrt
        | BoxMatch::Abs
        | BoxMatch::Floor
        | BoxMatch::Ceil
        | BoxMatch::Rint
        | BoxMatch::Round
        | BoxMatch::Lowest
        | BoxMatch::Highest => Ok(FlatNodeKind::Prim1),
        BoxMatch::ReadOnlyTable | BoxMatch::Select2 | BoxMatch::AssertBounds => {
            Ok(FlatNodeKind::Prim3)
        }
        BoxMatch::Select3 => Ok(FlatNodeKind::Prim4),
        BoxMatch::WriteReadTable => Ok(FlatNodeKind::Prim5),
        BoxMatch::FFun(_) => Ok(FlatNodeKind::FFun),
        BoxMatch::FConst(_, _, _) => Ok(FlatNodeKind::FConst),
        BoxMatch::FVar(_, _, _) => Ok(FlatNodeKind::FVar),
        BoxMatch::Button(_) => Ok(FlatNodeKind::Button),
        BoxMatch::Checkbox(_) => Ok(FlatNodeKind::Checkbox),
        BoxMatch::VSlider(_, _, _, _, _) => Ok(FlatNodeKind::VSlider),
        BoxMatch::HSlider(_, _, _, _, _) => Ok(FlatNodeKind::HSlider),
        BoxMatch::NumEntry(_, _, _, _, _) => Ok(FlatNodeKind::NumEntry),
        BoxMatch::VBargraph(_, _, _) => Ok(FlatNodeKind::VBargraph),
        BoxMatch::HBargraph(_, _, _) => Ok(FlatNodeKind::HBargraph),
        BoxMatch::Soundfile(_, _) => Ok(FlatNodeKind::Soundfile),
        BoxMatch::Waveform(_) => Ok(FlatNodeKind::Waveform),
        BoxMatch::VGroup(_, body) => Ok(FlatNodeKind::VGroup {
            body: validate_flat_box(arena, body)?,
        }),
        BoxMatch::HGroup(_, body) => Ok(FlatNodeKind::HGroup {
            body: validate_flat_box(arena, body)?,
        }),
        BoxMatch::TGroup(_, body) => Ok(FlatNodeKind::TGroup {
            body: validate_flat_box(arena, body)?,
        }),
        BoxMatch::Seq(left, right) => Ok(FlatNodeKind::Seq(
            validate_flat_box(arena, left)?,
            validate_flat_box(arena, right)?,
        )),
        BoxMatch::Par(left, right) => Ok(FlatNodeKind::Par(
            validate_flat_box(arena, left)?,
            validate_flat_box(arena, right)?,
        )),
        BoxMatch::Split(left, right) => Ok(FlatNodeKind::Split(
            validate_flat_box(arena, left)?,
            validate_flat_box(arena, right)?,
        )),
        BoxMatch::Merge(left, right) => Ok(FlatNodeKind::Merge(
            validate_flat_box(arena, left)?,
            validate_flat_box(arena, right)?,
        )),
        BoxMatch::Rec(left, right) => Ok(FlatNodeKind::Rec(
            validate_flat_box(arena, left)?,
            validate_flat_box(arena, right)?,
        )),
        BoxMatch::Environment => Ok(FlatNodeKind::Environment),
        BoxMatch::Route(_, _, _) => Ok(FlatNodeKind::Route),
        BoxMatch::Inputs(_) => Ok(FlatNodeKind::Inputs),
        BoxMatch::Outputs(_) => Ok(FlatNodeKind::Outputs),
        BoxMatch::Ondemand(body) => Ok(FlatNodeKind::Ondemand(validate_flat_box(arena, body)?)),
        BoxMatch::Upsampling(body) => {
            Ok(FlatNodeKind::Upsampling(validate_flat_box(arena, body)?))
        }
        BoxMatch::Downsampling(body) => {
            Ok(FlatNodeKind::Downsampling(validate_flat_box(arena, body)?))
        }
        BoxMatch::Ffunction(_, _, _) => Err(flat_box_unexpected(node_id, "ffunction")),
        BoxMatch::Unknown => Err(flat_box_unexpected(node_id, "unknown")),
        BoxMatch::Ident(_) => Err(flat_box_unexpected(node_id, "ident")),
        BoxMatch::Appl(_, _) => Err(flat_box_unexpected(node_id, "appl")),
        BoxMatch::Access(_, _) => Err(flat_box_unexpected(node_id, "access")),
        BoxMatch::IPar(_, _, _) => Err(flat_box_unexpected(node_id, "ipar")),
        BoxMatch::ISeq(_, _, _) => Err(flat_box_unexpected(node_id, "iseq")),
        BoxMatch::ISum(_, _, _) => Err(flat_box_unexpected(node_id, "isum")),
        BoxMatch::IProd(_, _, _) => Err(flat_box_unexpected(node_id, "iprod")),
        BoxMatch::WithLocalDef(_, _) => Err(flat_box_unexpected(node_id, "withlocaldef")),
        BoxMatch::ModifLocalDef(_, _) => Err(flat_box_unexpected(node_id, "modiflocaldef")),
        BoxMatch::WithRecDef(_, _, _) => Err(flat_box_unexpected(node_id, "withrecdef")),
        BoxMatch::Component(_) => Err(flat_box_unexpected(node_id, "component")),
        BoxMatch::Library(_) => Err(flat_box_unexpected(node_id, "library")),
        BoxMatch::Case(_) => Err(flat_box_unexpected(node_id, "case")),
        BoxMatch::PatternVar(_) => Err(flat_box_unexpected(node_id, "patternvar")),
        BoxMatch::Abstr(_, _) => Err(flat_box_unexpected(node_id, "abstr")),
        BoxMatch::Modulation(_, _) => Err(flat_box_unexpected(node_id, "modulation")),
    }
}

fn flat_box_unexpected(node: TreeId, kind: &'static str) -> FlatBoxBuildError {
    FlatBoxBuildError::UnexpectedPostEvalBox { node, kind }
}

/// Propagation/arity inference error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PropagateError {
    UnsupportedBox {
        node: TreeId,
        kind: &'static str,
    },
    InvalidIntegerValue {
        node: TreeId,
        field: &'static str,
    },
    NegativeIntegerValue {
        field: &'static str,
        value: i64,
    },
    IntegerTooLarge {
        field: &'static str,
        value: usize,
    },
    InputArityMismatch {
        node: TreeId,
        expected: usize,
        got: usize,
    },
    OutputArityMismatch {
        node: TreeId,
        expected: usize,
        got: usize,
    },
    SeqArityMismatch {
        node: TreeId,
        left_outputs: usize,
        right_inputs: usize,
    },
    SplitArityMismatch {
        node: TreeId,
        left_outputs: usize,
        right_inputs: usize,
    },
    MergeArityMismatch {
        node: TreeId,
        left_outputs: usize,
        right_inputs: usize,
    },
    RecArityMismatch {
        node: TreeId,
        left_inputs: usize,
        left_outputs: usize,
        right_inputs: usize,
        right_outputs: usize,
    },
}

impl Display for PropagateError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedBox { node, kind } => {
                write!(f, "unsupported box node {} ({kind})", node.as_u32())
            }
            Self::InvalidIntegerValue { node, field } => {
                write!(
                    f,
                    "invalid integer value for `{field}` at node {}",
                    node.as_u32()
                )
            }
            Self::NegativeIntegerValue { field, value } => {
                write!(f, "negative integer value for `{field}`: {value}")
            }
            Self::IntegerTooLarge { field, value } => {
                write!(f, "integer value too large for `{field}`: {value}")
            }
            Self::InputArityMismatch {
                node,
                expected,
                got,
            } => write!(
                f,
                "input arity mismatch at node {}: expected {expected}, got {got}",
                node.as_u32()
            ),
            Self::OutputArityMismatch {
                node,
                expected,
                got,
            } => write!(
                f,
                "output arity mismatch at node {}: expected {expected}, got {got}",
                node.as_u32()
            ),
            Self::SeqArityMismatch {
                node,
                left_outputs,
                right_inputs,
            } => write!(
                f,
                "sequential composition mismatch at node {}: left outputs ({left_outputs}) != right inputs ({right_inputs})",
                node.as_u32()
            ),
            Self::SplitArityMismatch {
                node,
                left_outputs,
                right_inputs,
            } => write!(
                f,
                "split composition mismatch at node {}: left outputs ({left_outputs}) must divide right inputs ({right_inputs})",
                node.as_u32()
            ),
            Self::MergeArityMismatch {
                node,
                left_outputs,
                right_inputs,
            } => write!(
                f,
                "merge composition mismatch at node {}: left outputs ({left_outputs}) must be a multiple of right inputs ({right_inputs})",
                node.as_u32()
            ),
            Self::RecArityMismatch {
                node,
                left_inputs,
                left_outputs,
                right_inputs,
                right_outputs,
            } => write!(
                f,
                "recursive composition mismatch at node {}: right inputs ({right_inputs}) <= left outputs ({left_outputs}) and right outputs ({right_outputs}) <= left inputs ({left_inputs}) are required",
                node.as_u32()
            ),
        }
    }
}

impl std::error::Error for PropagateError {}

impl From<FlatBoxBuildError> for PropagateError {
    fn from(value: FlatBoxBuildError) -> Self {
        match value {
            FlatBoxBuildError::UnexpectedPostEvalBox { node, kind } => {
                Self::UnsupportedBox { node, kind }
            }
        }
    }
}

/// Converts propagation errors into structured diagnostics used by the compiler facade.
impl IntoDiagnostic for PropagateError {
    fn into_diagnostic(self) -> Diagnostic {
        let message = self.to_string();
        match self {
            Self::UnsupportedBox { .. } => {
                Diagnostic::new(Severity::Error, Stage::Propagate, codes::PROP_UNSUPPORTED_BOX, message)
                    .with_note(
                        "cause: encountered box node family is not supported in current propagation phase",
                    )
                    .with_help("evaluate box expression first or add propagation support for this node family")
            }
            Self::InputArityMismatch { expected, got, .. }
            | Self::OutputArityMismatch { expected, got, .. } => {
                Diagnostic::new(Severity::Error, Stage::Propagate, codes::PROP_ARITY_MISMATCH, message)
                    .with_note("cause: propagated bus width differs from required arity")
                    .with_note(
                        "rule: input/output arities at composition boundary must match target signature",
                    )
                    .with_note(format!("expected {expected}, got {got}"))
                    .with_help("adjust composition so input/output bus widths match")
            }
            Self::SeqArityMismatch {
                left_outputs,
                right_inputs,
                ..
            } => Diagnostic::new(Severity::Error, Stage::Propagate, codes::PROP_ARITY_MISMATCH, message)
                .with_note("cause: sequential composition bus widths do not match")
                .with_note("rule: seq(A, B) requires outputs(A) == inputs(B)")
                .with_note(format!(
                    "sequential composition requires left outputs ({left_outputs}) == right inputs ({right_inputs})"
                ))
                .with_note(format!(
                    "computed: {left_outputs} == {right_inputs} -> {}",
                    left_outputs == right_inputs
                ))
                .with_note(format!(
                    "suggested target: make outputs(A) and inputs(B) equal (common target: {})",
                    left_outputs.max(right_inputs)
                ))
                .with_help("for `A : B`, enforce outputs(A) == inputs(B)")
                .with_help("fix: adjust A or B channel count to same bus width")
                .with_help("template: process = A : B; // outputs(A) == inputs(B)"),
            Self::SplitArityMismatch {
                left_outputs,
                right_inputs,
                ..
            } => Diagnostic::new(Severity::Error, Stage::Propagate, codes::PROP_ARITY_MISMATCH, message)
                .with_note("cause: split composition divisibility rule is violated")
                .with_note("rule: split(A, B) requires inputs(B) % outputs(A) == 0")
                .with_note(format!(
                    "split composition requires right inputs ({right_inputs}) to be divisible by left outputs ({left_outputs})"
                ))
                .with_note(if left_outputs == 0 {
                    "computed: divisor outputs(A)=0 is invalid".to_owned()
                } else {
                    format!(
                        "computed: {right_inputs} % {left_outputs} = {}",
                        right_inputs % left_outputs
                    )
                })
                .with_note(if left_outputs == 0 {
                    "suggested target: outputs(A) must be > 0 before divisibility can be satisfied".to_owned()
                } else {
                    let next = right_inputs
                        .saturating_add(left_outputs - 1)
                        / left_outputs
                        * left_outputs;
                    format!(
                        "suggested target: set inputs(B) to {next} (next multiple of outputs(A)={left_outputs})"
                    )
                })
                .with_help("for `A <: B`, enforce inputs(B) % outputs(A) == 0")
                .with_help("fix: make B inputs a multiple of A outputs")
                .with_help("template: process = A <: B; // inputs(B) % outputs(A) == 0"),
            Self::MergeArityMismatch {
                left_outputs,
                right_inputs,
                ..
            } => Diagnostic::new(Severity::Error, Stage::Propagate, codes::PROP_ARITY_MISMATCH, message)
                .with_note("cause: merge composition multiple rule is violated")
                .with_note("rule: merge(A, B) requires outputs(A) % inputs(B) == 0")
                .with_note(format!(
                    "merge composition requires left outputs ({left_outputs}) to be a multiple of right inputs ({right_inputs})"
                ))
                .with_note(if right_inputs == 0 {
                    "computed: divisor inputs(B)=0 is invalid".to_owned()
                } else {
                    format!(
                        "computed: {left_outputs} % {right_inputs} = {}",
                        left_outputs % right_inputs
                    )
                })
                .with_note(if right_inputs == 0 {
                    "suggested target: inputs(B) must be > 0 before multiple constraints can be satisfied".to_owned()
                } else {
                    let next = left_outputs
                        .saturating_add(right_inputs - 1)
                        / right_inputs
                        * right_inputs;
                    format!(
                        "suggested target: set outputs(A) to {next} (next multiple of inputs(B)={right_inputs})"
                    )
                })
                .with_help("for `A :> B`, enforce outputs(A) % inputs(B) == 0")
                .with_help("fix: make A outputs a multiple of B inputs")
                .with_help("template: process = A :> B; // outputs(A) % inputs(B) == 0"),
            Self::RecArityMismatch {
                left_inputs,
                left_outputs,
                right_inputs,
                right_outputs,
                ..
            } => Diagnostic::new(
                Severity::Error,
                Stage::Propagate,
                codes::PROP_RECURSION_MISMATCH,
                message,
            )
            .with_note("cause: recursive feedback arity constraints are not satisfied")
            .with_note(
                "rule: rec(A, B) requires right_inputs <= left_outputs and right_outputs <= left_inputs",
            )
            .with_note(format!(
                "required: right_inputs ({right_inputs}) <= left_outputs ({left_outputs}) and right_outputs ({right_outputs}) <= left_inputs ({left_inputs})"
            ))
            .with_note(format!(
                "computed: {} <= {} is {}, {} <= {} is {}",
                right_inputs,
                left_outputs,
                right_inputs <= left_outputs,
                right_outputs,
                left_inputs,
                right_outputs <= left_inputs
            ))
            .with_note(format!(
                "suggested target: set outputs(A) >= {} and inputs(A) >= {}",
                right_inputs, right_outputs
            ))
            .with_help(
                "for `A ~ B`, enforce inputs(B) <= outputs(A) and outputs(B) <= inputs(A)",
            )
            .with_help("fix: reduce B feedback bus or widen matching A arities")
            .with_help(
                "template: process = A ~ B; // inputs(B)<=outputs(A), outputs(B)<=inputs(A)",
            ),
            Self::InvalidIntegerValue { field, .. } => Diagnostic::new(
                Severity::Error,
                Stage::Propagate,
                codes::PROP_GENERIC_FAILURE,
                message,
            )
            .with_note("cause: integer-valued propagation field has invalid runtime representation")
            .with_note(format!("invalid integer for field `{field}`")),
            Self::NegativeIntegerValue { field, value } => Diagnostic::new(
                Severity::Error,
                Stage::Propagate,
                codes::PROP_GENERIC_FAILURE,
                message,
            )
            .with_note(
                "cause: field constrained to non-negative integer received a negative value",
            )
            .with_note(format!("field `{field}` is negative: {value}")),
            Self::IntegerTooLarge { field, value } => Diagnostic::new(
                Severity::Error,
                Stage::Propagate,
                codes::PROP_GENERIC_FAILURE,
                message,
            )
            .with_note("cause: integer field exceeds propagation-supported bounds")
            .with_note(format!("field `{field}` exceeds target range: {value}")),
        }
    }
}

/// Creates `n` canonical `sigInput(i)` signals.
///
/// Output order is stable and follows input bus index order: `0..n-1`.
#[must_use]
pub fn make_sig_input_list(arena: &mut TreeArena, n: usize) -> Vec<SigId> {
    let mut b = SigBuilder::new(arena);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let index = i32::try_from(i).unwrap_or(i32::MAX);
        out.push(b.input(index));
    }
    out
}

/// Infers input/output arity of one flat post-eval box expression (memoized).
///
/// This is the typed entry point for post-`eval/a2sb` callers that already
/// hold a validated [`FlatBoxId`].
pub fn box_arity_flat(
    arena: &TreeArena,
    box_tree: FlatBoxId,
    cache: &mut ArityCache,
) -> Result<BoxArity, PropagateError> {
    if let Some(cached) = cache.get(&box_tree) {
        return cached.clone();
    }
    let result = box_arity_flat_inner(arena, box_tree, cache);
    cache.insert(box_tree, result.clone());
    result
}

/// Infers input/output arity of one box expression (memoized).
///
/// This mirrors C++ `getBoxType(...)` behavior for the currently supported subset.
/// Unsupported box families return [`PropagateError::UnsupportedBox`].
///
/// Callers should create one [`ArityCache`] and pass it through to amortise
/// repeated sub-expression visits across multiple calls.
pub fn box_arity(
    arena: &TreeArena,
    box_tree: BoxId,
    cache: &mut ArityCache,
) -> Result<BoxArity, PropagateError> {
    let flat = try_build_flat_box(arena, box_tree)?;
    box_arity_flat(arena, flat, cache)
}

/// Core arity inference logic, called only on cache miss.
fn box_arity_flat_inner(
    arena: &TreeArena,
    box_tree: FlatBoxId,
    cache: &mut ArityCache,
) -> Result<BoxArity, PropagateError> {
    match flat_node_kind(arena, box_tree)? {
        FlatNodeKind::Int | FlatNodeKind::Real => Ok(BoxArity {
            inputs: 0,
            outputs: 1,
        }),
        FlatNodeKind::Slot => Ok(BoxArity {
            inputs: 0,
            outputs: 1,
        }),
        FlatNodeKind::Metadata { body } => box_arity_flat(arena, body, cache),
        FlatNodeKind::Wire => Ok(BoxArity {
            inputs: 1,
            outputs: 1,
        }),
        FlatNodeKind::Cut => Ok(BoxArity {
            inputs: 1,
            outputs: 0,
        }),
        FlatNodeKind::Prim2 => Ok(BoxArity {
            inputs: 2,
            outputs: 1,
        }),
        FlatNodeKind::Prim1 => Ok(BoxArity {
            inputs: 1,
            outputs: 1,
        }),
        FlatNodeKind::Prim3 => Ok(BoxArity {
            inputs: 3,
            outputs: 1,
        }),
        FlatNodeKind::Prim4 => Ok(BoxArity {
            inputs: 4,
            outputs: 1,
        }),
        FlatNodeKind::Prim5 => Ok(BoxArity {
            inputs: 5,
            outputs: 1,
        }),
        FlatNodeKind::FConst | FlatNodeKind::FVar => Ok(BoxArity {
            inputs: 0,
            outputs: 1,
        }),
        FlatNodeKind::FFun => Err(PropagateError::UnsupportedBox {
            node: box_tree.as_tree_id(),
            kind: "ffun",
        }),
        FlatNodeKind::Button
        | FlatNodeKind::Checkbox
        | FlatNodeKind::VSlider
        | FlatNodeKind::HSlider
        | FlatNodeKind::NumEntry => Ok(BoxArity {
            inputs: 0,
            outputs: 1,
        }),
        FlatNodeKind::VBargraph | FlatNodeKind::HBargraph => Ok(BoxArity {
            inputs: 1,
            outputs: 1,
        }),
        FlatNodeKind::Soundfile => {
            let BoxMatch::Soundfile(_, chan) = match_box(arena, box_tree.as_tree_id()) else {
                unreachable!("flat soundfile node must decode to BoxMatch::Soundfile")
            };
            let chan = usize_from_int_node(arena, chan, "soundfile channels")?;
            Ok(BoxArity {
                inputs: 2,
                outputs: 2 + chan,
            })
        }
        FlatNodeKind::Waveform => {
            let BoxMatch::Waveform(values) = match_box(arena, box_tree.as_tree_id()) else {
                unreachable!("flat waveform node must decode to BoxMatch::Waveform")
            };
            let _ = list_length(arena, values).ok_or(PropagateError::UnsupportedBox {
                node: box_tree.as_tree_id(),
                kind: "waveform-list",
            })?;
            Ok(BoxArity {
                inputs: 0,
                outputs: 2,
            })
        }
        FlatNodeKind::VGroup { body }
        | FlatNodeKind::HGroup { body }
        | FlatNodeKind::TGroup { body } => box_arity_flat(arena, body, cache),
        FlatNodeKind::Symbolic { body } => {
            let inner = box_arity_flat(arena, body, cache)?;
            Ok(BoxArity {
                inputs: inner.inputs + 1,
                outputs: inner.outputs,
            })
        }
        FlatNodeKind::Seq(left, right) => {
            let left_arity = box_arity_flat(arena, left, cache)?;
            let right_arity = box_arity_flat(arena, right, cache)?;
            if left_arity.outputs != right_arity.inputs {
                return Err(PropagateError::SeqArityMismatch {
                    node: box_tree.as_tree_id(),
                    left_outputs: left_arity.outputs,
                    right_inputs: right_arity.inputs,
                });
            }
            Ok(BoxArity {
                inputs: left_arity.inputs,
                outputs: right_arity.outputs,
            })
        }
        FlatNodeKind::Par(left, right) => {
            let left_arity = box_arity_flat(arena, left, cache)?;
            let right_arity = box_arity_flat(arena, right, cache)?;
            Ok(BoxArity {
                inputs: left_arity.inputs + right_arity.inputs,
                outputs: left_arity.outputs + right_arity.outputs,
            })
        }
        FlatNodeKind::Split(left, right) => {
            let left_arity = box_arity_flat(arena, left, cache)?;
            let right_arity = box_arity_flat(arena, right, cache)?;
            if !split_compatible(left_arity.outputs, right_arity.inputs) {
                return Err(PropagateError::SplitArityMismatch {
                    node: box_tree.as_tree_id(),
                    left_outputs: left_arity.outputs,
                    right_inputs: right_arity.inputs,
                });
            }
            Ok(BoxArity {
                inputs: left_arity.inputs,
                outputs: right_arity.outputs,
            })
        }
        FlatNodeKind::Merge(left, right) => {
            let left_arity = box_arity_flat(arena, left, cache)?;
            let right_arity = box_arity_flat(arena, right, cache)?;
            if !merge_compatible(left_arity.outputs, right_arity.inputs) {
                return Err(PropagateError::MergeArityMismatch {
                    node: box_tree.as_tree_id(),
                    left_outputs: left_arity.outputs,
                    right_inputs: right_arity.inputs,
                });
            }
            Ok(BoxArity {
                inputs: left_arity.inputs,
                outputs: right_arity.outputs,
            })
        }
        FlatNodeKind::Rec(left, right) => {
            let left_arity = box_arity_flat(arena, left, cache)?;
            let right_arity = box_arity_flat(arena, right, cache)?;
            if right_arity.inputs > left_arity.outputs || right_arity.outputs > left_arity.inputs {
                return Err(PropagateError::RecArityMismatch {
                    node: box_tree.as_tree_id(),
                    left_inputs: left_arity.inputs,
                    left_outputs: left_arity.outputs,
                    right_inputs: right_arity.inputs,
                    right_outputs: right_arity.outputs,
                });
            }
            Ok(BoxArity {
                inputs: left_arity.inputs - right_arity.outputs,
                outputs: left_arity.outputs,
            })
        }
        FlatNodeKind::Environment => Ok(BoxArity {
            inputs: 0,
            outputs: 0,
        }),
        FlatNodeKind::Route => {
            let BoxMatch::Route(ins, outs, _) = match_box(arena, box_tree.as_tree_id()) else {
                unreachable!("flat route node must decode to BoxMatch::Route")
            };
            Ok(BoxArity {
                inputs: usize_from_int_node(arena, ins, "route inputs")?,
                outputs: usize_from_int_node(arena, outs, "route outputs")?,
            })
        }
        FlatNodeKind::Inputs | FlatNodeKind::Outputs => Ok(BoxArity {
            inputs: 0,
            outputs: 1,
        }),
        FlatNodeKind::Ondemand(expr)
        | FlatNodeKind::Upsampling(expr)
        | FlatNodeKind::Downsampling(expr) => {
            let inner = box_arity_flat(arena, expr, cache)?;
            Ok(BoxArity {
                inputs: inner.inputs + 1,
                outputs: inner.outputs,
            })
        }
    }
}

/// Propagates input signals through one evaluated box expression (memoized arity).
///
/// This function validates input/output arity using [`box_arity`].
///
/// Precondition: `box_tree` should already be in the evaluated box domain
/// (typically output of `eval::eval_process` / `eval::eval_box`).
///
/// Callers should create one [`ArityCache`] and pass it through to amortise
/// repeated sub-expression arity lookups.
pub fn propagate(
    arena: &mut TreeArena,
    box_tree: BoxId,
    inputs: &[SigId],
    cache: &mut ArityCache,
) -> Result<Vec<SigId>, PropagateError> {
    let mut slot_env = SlotEnv::default();
    propagate_in_slot_env(arena, box_tree, inputs, cache, &mut slot_env)
}

/// Propagates one box tree with an explicit slot environment.
///
/// Source provenance (C++):
/// - `compiler/propagate/propagate.cpp`
/// - `propagate(...)`
///
/// C++ threads a dedicated `slotenv` alongside the normal recursion so
/// `boxSymbolic(slot, body)` can bind the first input bus to `boxSlot(slot)`.
/// Rust keeps the same semantic mechanism but uses a local hash map keyed by the
/// canonical `BoxId` of each slot node instead of global tree properties.
///
/// This helper is also the point where Rust enforces the public
/// `propagate(...)` contract: callers may only enter `propagate_inner(...)`
/// through a path that has already checked both input and output bus widths.
fn propagate_in_slot_env(
    arena: &mut TreeArena,
    box_tree: BoxId,
    inputs: &[SigId],
    cache: &mut ArityCache,
    slot_env: &mut SlotEnv,
) -> Result<Vec<SigId>, PropagateError> {
    let arity = box_arity(arena, box_tree, cache)?;
    if inputs.len() != arity.inputs {
        return Err(PropagateError::InputArityMismatch {
            node: box_tree,
            expected: arity.inputs,
            got: inputs.len(),
        });
    }
    let outputs = propagate_inner(arena, box_tree, inputs, cache, slot_env)?;
    if outputs.len() != arity.outputs {
        return Err(PropagateError::OutputArityMismatch {
            node: box_tree,
            expected: arity.outputs,
            got: outputs.len(),
        });
    }
    Ok(outputs)
}

/// Internal propagation dispatcher once input arity has been validated.
///
/// Unlike `box_arity(...)`, this function is intentionally operational rather
/// than declarative: it builds actual signal nodes, threads slot bindings, and
/// recursively performs composition rewrites. Unsupported box families here are
/// therefore genuine lowering gaps, not just missing arity metadata.
fn propagate_inner(
    arena: &mut TreeArena,
    box_tree: BoxId,
    inputs: &[SigId],
    cache: &mut ArityCache,
    slot_env: &mut SlotEnv,
) -> Result<Vec<SigId>, PropagateError> {
    match match_box(arena, box_tree) {
        BoxMatch::Int(value) => {
            expect_input_arity(box_tree, inputs, 0)?;
            let mut b = SigBuilder::new(arena);
            Ok(vec![b.int(value)])
        }
        BoxMatch::Real(value) => {
            expect_input_arity(box_tree, inputs, 0)?;
            let mut b = SigBuilder::new(arena);
            Ok(vec![b.real(value)])
        }
        BoxMatch::Metadata(body, _) => propagate_inner(arena, body, inputs, cache, slot_env),
        BoxMatch::Slot(id) => {
            expect_input_arity(box_tree, inputs, 0)?;
            if let Some(sig) = slot_env.get(&box_tree).copied() {
                Ok(vec![sig])
            } else {
                let mut b = SigBuilder::new(arena);
                Ok(vec![b.input(id)])
            }
        }
        BoxMatch::Wire => {
            expect_input_arity(box_tree, inputs, 1)?;
            Ok(vec![inputs[0]])
        }
        BoxMatch::Cut => {
            expect_input_arity(box_tree, inputs, 1)?;
            Ok(Vec::new())
        }
        BoxMatch::Add => binary_prim(arena, box_tree, inputs, |b, x, y| b.add(x, y)),
        BoxMatch::Sub => binary_prim(arena, box_tree, inputs, |b, x, y| b.sub(x, y)),
        BoxMatch::Mul => binary_prim(arena, box_tree, inputs, |b, x, y| b.mul(x, y)),
        BoxMatch::Div => binary_prim(arena, box_tree, inputs, |b, x, y| b.div(x, y)),
        BoxMatch::Rem => binary_prim(arena, box_tree, inputs, |b, x, y| b.rem(x, y)),
        BoxMatch::And => binary_prim(arena, box_tree, inputs, |b, x, y| b.and(x, y)),
        BoxMatch::Or => binary_prim(arena, box_tree, inputs, |b, x, y| b.or(x, y)),
        BoxMatch::Xor => binary_prim(arena, box_tree, inputs, |b, x, y| b.xor(x, y)),
        BoxMatch::Lsh => binary_prim(arena, box_tree, inputs, |b, x, y| b.lsh(x, y)),
        BoxMatch::Rsh => binary_prim(arena, box_tree, inputs, |b, x, y| b.arsh(x, y)),
        BoxMatch::Lt => binary_prim(arena, box_tree, inputs, |b, x, y| b.lt(x, y)),
        BoxMatch::Le => binary_prim(arena, box_tree, inputs, |b, x, y| b.le(x, y)),
        BoxMatch::Gt => binary_prim(arena, box_tree, inputs, |b, x, y| b.gt(x, y)),
        BoxMatch::Ge => binary_prim(arena, box_tree, inputs, |b, x, y| b.ge(x, y)),
        BoxMatch::Eq => binary_prim(arena, box_tree, inputs, |b, x, y| b.eq(x, y)),
        BoxMatch::Ne => binary_prim(arena, box_tree, inputs, |b, x, y| b.ne(x, y)),
        BoxMatch::Pow => binary_prim(arena, box_tree, inputs, |b, x, y| b.pow(x, y)),
        BoxMatch::Atan2 => binary_prim(arena, box_tree, inputs, |b, x, y| b.atan2(x, y)),
        BoxMatch::Fmod => binary_prim(arena, box_tree, inputs, |b, x, y| b.fmod(x, y)),
        BoxMatch::Remainder => binary_prim(arena, box_tree, inputs, |b, x, y| b.remainder(x, y)),
        BoxMatch::Min => binary_prim(arena, box_tree, inputs, |b, x, y| b.min(x, y)),
        BoxMatch::Max => binary_prim(arena, box_tree, inputs, |b, x, y| b.max(x, y)),
        BoxMatch::Delay => binary_prim(arena, box_tree, inputs, |b, x, y| b.delay(x, y)),
        BoxMatch::Delay1 => unary_prim(arena, box_tree, inputs, |b, x| b.delay1(x)),
        BoxMatch::Acos => unary_prim(arena, box_tree, inputs, |b, x| b.acos(x)),
        BoxMatch::Asin => unary_prim(arena, box_tree, inputs, |b, x| b.asin(x)),
        BoxMatch::Atan => unary_prim(arena, box_tree, inputs, |b, x| b.atan(x)),
        BoxMatch::Cos => unary_prim(arena, box_tree, inputs, |b, x| b.cos(x)),
        BoxMatch::Sin => unary_prim(arena, box_tree, inputs, |b, x| b.sin(x)),
        BoxMatch::Tan => unary_prim(arena, box_tree, inputs, |b, x| b.tan(x)),
        BoxMatch::Exp => unary_prim(arena, box_tree, inputs, |b, x| b.exp(x)),
        BoxMatch::Log => unary_prim(arena, box_tree, inputs, |b, x| b.log(x)),
        BoxMatch::Log10 => unary_prim(arena, box_tree, inputs, |b, x| b.log10(x)),
        BoxMatch::Sqrt => unary_prim(arena, box_tree, inputs, |b, x| b.sqrt(x)),
        BoxMatch::Abs => unary_prim(arena, box_tree, inputs, |b, x| b.abs(x)),
        BoxMatch::Floor => unary_prim(arena, box_tree, inputs, |b, x| b.floor(x)),
        BoxMatch::Ceil => unary_prim(arena, box_tree, inputs, |b, x| b.ceil(x)),
        BoxMatch::Rint => unary_prim(arena, box_tree, inputs, |b, x| b.rint(x)),
        BoxMatch::Round => unary_prim(arena, box_tree, inputs, |b, x| b.round(x)),
        BoxMatch::Prefix => binary_prim(arena, box_tree, inputs, |b, x, y| b.prefix(x, y)),
        BoxMatch::IntCast => unary_prim(arena, box_tree, inputs, |b, x| b.int_cast(x)),
        BoxMatch::FloatCast => unary_prim(arena, box_tree, inputs, |b, x| b.float_cast(x)),
        BoxMatch::ReadOnlyTable => ternary_prim(arena, box_tree, inputs, |b, x, y, z| {
            b.read_only_table(x, y, z)
        }),
        BoxMatch::WriteReadTable => quinary_prim(arena, box_tree, inputs, |b, s, i, wi, ws, ri| {
            b.write_read_table(s, i, wi, ws, ri)
        }),
        BoxMatch::Select2 => ternary_prim(arena, box_tree, inputs, |b, x, y, z| b.select2(x, y, z)),
        BoxMatch::Select3 => quaternary_prim(arena, box_tree, inputs, |b, x, y, z, w| {
            b.select3(x, y, z, w)
        }),
        BoxMatch::AssertBounds => ternary_prim(arena, box_tree, inputs, |b, x, y, z| {
            b.assert_bounds(x, y, z)
        }),
        BoxMatch::Lowest => unary_prim(arena, box_tree, inputs, |b, x| b.lowest(x)),
        BoxMatch::Highest => unary_prim(arena, box_tree, inputs, |b, x| b.highest(x)),
        BoxMatch::Attach => binary_prim(arena, box_tree, inputs, |b, x, y| b.attach(x, y)),
        BoxMatch::Enable => binary_prim(arena, box_tree, inputs, |b, x, y| b.enable(x, y)),
        BoxMatch::Control => binary_prim(arena, box_tree, inputs, |b, x, y| b.control(x, y)),
        BoxMatch::FConst(ty, name, file) => {
            expect_input_arity(box_tree, inputs, 0)?;
            let mut b = SigBuilder::new(arena);
            Ok(vec![b.fconst(ty, name, file)])
        }
        BoxMatch::FVar(ty, name, file) => {
            expect_input_arity(box_tree, inputs, 0)?;
            let mut b = SigBuilder::new(arena);
            Ok(vec![b.fvar(ty, name, file)])
        }
        BoxMatch::Button(label) => {
            expect_input_arity(box_tree, inputs, 0)?;
            let mut b = SigBuilder::new(arena);
            Ok(vec![b.button(label)])
        }
        BoxMatch::Checkbox(label) => {
            expect_input_arity(box_tree, inputs, 0)?;
            let mut b = SigBuilder::new(arena);
            Ok(vec![b.checkbox(label)])
        }
        BoxMatch::VSlider(label, cur, min, max, step) => {
            expect_input_arity(box_tree, inputs, 0)?;
            let mut b = SigBuilder::new(arena);
            Ok(vec![b.vslider(label, cur, min, max, step)])
        }
        BoxMatch::HSlider(label, cur, min, max, step) => {
            expect_input_arity(box_tree, inputs, 0)?;
            let mut b = SigBuilder::new(arena);
            Ok(vec![b.hslider(label, cur, min, max, step)])
        }
        BoxMatch::NumEntry(label, cur, min, max, step) => {
            expect_input_arity(box_tree, inputs, 0)?;
            let mut b = SigBuilder::new(arena);
            Ok(vec![b.numentry(label, cur, min, max, step)])
        }
        BoxMatch::VBargraph(label, min, max) => {
            expect_input_arity(box_tree, inputs, 1)?;
            let mut b = SigBuilder::new(arena);
            Ok(vec![b.vbargraph(label, min, max, inputs[0])])
        }
        BoxMatch::HBargraph(label, min, max) => {
            expect_input_arity(box_tree, inputs, 1)?;
            let mut b = SigBuilder::new(arena);
            Ok(vec![b.hbargraph(label, min, max, inputs[0])])
        }
        BoxMatch::Waveform(values) => {
            expect_input_arity(box_tree, inputs, 0)?;
            let values = list_to_vec(arena, values).ok_or(PropagateError::UnsupportedBox {
                node: box_tree,
                kind: "waveform-list",
            })?;
            let mut b = SigBuilder::new(arena);
            let n = i32_from_usize(values.len(), "waveform size")?;
            let size = b.int(n);
            let waveform = b.waveform(&values);
            Ok(vec![size, waveform])
        }
        BoxMatch::VGroup(_, expr) | BoxMatch::HGroup(_, expr) | BoxMatch::TGroup(_, expr) => {
            propagate_in_slot_env(arena, expr, inputs, cache, slot_env)
        }
        BoxMatch::Symbolic(slot, body) => {
            if inputs.is_empty() {
                return Err(PropagateError::InputArityMismatch {
                    node: box_tree,
                    expected: 1,
                    got: 0,
                });
            }
            let previous = slot_env.insert(slot, inputs[0]);
            let result = propagate_in_slot_env(arena, body, &inputs[1..], cache, slot_env);
            if let Some(sig) = previous {
                slot_env.insert(slot, sig);
            } else {
                slot_env.remove(&slot);
            }
            result
        }
        BoxMatch::Seq(left, right) => {
            let left_arity = box_arity(arena, left, cache)?;
            let right_arity = box_arity(arena, right, cache)?;
            if left_arity.outputs != right_arity.inputs {
                return Err(PropagateError::SeqArityMismatch {
                    node: box_tree,
                    left_outputs: left_arity.outputs,
                    right_inputs: right_arity.inputs,
                });
            }
            let mid = propagate_in_slot_env(arena, left, inputs, cache, slot_env)?;
            propagate_in_slot_env(arena, right, &mid, cache, slot_env)
        }
        BoxMatch::Par(left, right) => {
            let left_arity = box_arity(arena, left, cache)?;
            let right_arity = box_arity(arena, right, cache)?;
            let left_out =
                propagate_in_slot_env(arena, left, &inputs[..left_arity.inputs], cache, slot_env)?;
            let mut right_out = propagate_in_slot_env(
                arena,
                right,
                &inputs[left_arity.inputs..left_arity.inputs + right_arity.inputs],
                cache,
                slot_env,
            )?;
            let mut out = left_out;
            out.append(&mut right_out);
            Ok(out)
        }
        BoxMatch::Split(left, right) => {
            let left_arity = box_arity(arena, left, cache)?;
            let right_arity = box_arity(arena, right, cache)?;
            if !split_compatible(left_arity.outputs, right_arity.inputs) {
                return Err(PropagateError::SplitArityMismatch {
                    node: box_tree,
                    left_outputs: left_arity.outputs,
                    right_inputs: right_arity.inputs,
                });
            }
            let left_out = propagate_in_slot_env(arena, left, inputs, cache, slot_env)?;
            let split_in = split_signals(&left_out, right_arity.inputs);
            propagate_in_slot_env(arena, right, &split_in, cache, slot_env)
        }
        BoxMatch::Merge(left, right) => {
            let left_arity = box_arity(arena, left, cache)?;
            let right_arity = box_arity(arena, right, cache)?;
            if !merge_compatible(left_arity.outputs, right_arity.inputs) {
                return Err(PropagateError::MergeArityMismatch {
                    node: box_tree,
                    left_outputs: left_arity.outputs,
                    right_inputs: right_arity.inputs,
                });
            }
            let left_out = propagate_in_slot_env(arena, left, inputs, cache, slot_env)?;
            let merge_in = mix_signals(arena, &left_out, right_arity.inputs);
            propagate_in_slot_env(arena, right, &merge_in, cache, slot_env)
        }
        BoxMatch::Rec(left, right) => {
            let left_arity = box_arity(arena, left, cache)?;
            let right_arity = box_arity(arena, right, cache)?;
            if right_arity.inputs > left_arity.outputs || right_arity.outputs > left_arity.inputs {
                return Err(PropagateError::RecArityMismatch {
                    node: box_tree,
                    left_inputs: left_arity.inputs,
                    left_outputs: left_arity.outputs,
                    right_inputs: right_arity.inputs,
                    right_outputs: right_arity.outputs,
                });
            }

            let l0 = make_mem_sig_proj_list(arena, right_arity.inputs)?;
            let l1 = propagate_in_slot_env(arena, right, &l0, cache, slot_env)?;

            let mut rec_inputs = l1;
            rec_inputs.extend(lift_signals(arena, inputs));

            let l2 = propagate_in_slot_env(arena, left, &rec_inputs, cache, slot_env)?;
            let group_body = vec_to_list(arena, &l2);
            let group = debruijn_rec(arena, group_body);

            let mut outputs = Vec::with_capacity(l2.len());
            for (index, expr) in l2.iter().copied().enumerate() {
                if aperture(arena, expr) > 0 {
                    let idx = i32_from_usize(index, "rec projection index")?;
                    let mut b = SigBuilder::new(arena);
                    outputs.push(b.proj(idx, group));
                } else {
                    outputs.push(expr);
                }
            }
            Ok(outputs)
        }
        BoxMatch::Inputs(expr) => {
            expect_input_arity(box_tree, inputs, 0)?;
            let arity = box_arity(arena, expr, cache)?;
            let value = i32_from_usize(arity.inputs, "inputs")?;
            let mut b = SigBuilder::new(arena);
            Ok(vec![b.int(value)])
        }
        BoxMatch::Outputs(expr) => {
            expect_input_arity(box_tree, inputs, 0)?;
            let arity = box_arity(arena, expr, cache)?;
            let value = i32_from_usize(arity.outputs, "outputs")?;
            let mut b = SigBuilder::new(arena);
            Ok(vec![b.int(value)])
        }
        BoxMatch::Environment => {
            expect_input_arity(box_tree, inputs, 0)?;
            Ok(Vec::new())
        }
        BoxMatch::Unknown => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "unknown",
        }),
        BoxMatch::Ident(_) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "ident",
        }),
        BoxMatch::Appl(_, _) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "appl",
        }),
        BoxMatch::Access(_, _) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "access",
        }),
        BoxMatch::IPar(_, _, _) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "ipar",
        }),
        BoxMatch::ISeq(_, _, _) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "iseq",
        }),
        BoxMatch::ISum(_, _, _) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "isum",
        }),
        BoxMatch::IProd(_, _, _) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "iprod",
        }),
        BoxMatch::WithLocalDef(_, _) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "withlocaldef",
        }),
        BoxMatch::ModifLocalDef(_, _) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "modiflocaldef",
        }),
        BoxMatch::WithRecDef(_, _, _) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "withrecdef",
        }),
        BoxMatch::Component(_) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "component",
        }),
        BoxMatch::Library(_) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "library",
        }),
        BoxMatch::Route(_, _, _) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "route",
        }),
        BoxMatch::Ffunction(_, _, _) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "ffunction",
        }),
        BoxMatch::FFun(_) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "ffun",
        }),
        BoxMatch::Case(_) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "case",
        }),
        BoxMatch::PatternVar(_) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "patternvar",
        }),
        BoxMatch::Abstr(_, _) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "abstr",
        }),
        BoxMatch::Modulation(_, _) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "modulation",
        }),
        BoxMatch::Ondemand(_) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "ondemand",
        }),
        BoxMatch::Upsampling(_) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "upsampling",
        }),
        BoxMatch::Downsampling(_) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "downsampling",
        }),
        BoxMatch::Soundfile(_, _) => Err(PropagateError::UnsupportedBox {
            node: box_tree,
            kind: "soundfile",
        }),
    }
}

/// Validates that a primitive receives exactly the expected number of inputs.
fn expect_input_arity(
    node: TreeId,
    inputs: &[SigId],
    expected: usize,
) -> Result<(), PropagateError> {
    if inputs.len() == expected {
        Ok(())
    } else {
        Err(PropagateError::InputArityMismatch {
            node,
            expected,
            got: inputs.len(),
        })
    }
}

/// Lowers one unary primitive and returns a single output signal.
fn unary_prim(
    arena: &mut TreeArena,
    node: TreeId,
    inputs: &[SigId],
    f: impl FnOnce(&mut SigBuilder<'_>, SigId) -> SigId,
) -> Result<Vec<SigId>, PropagateError> {
    expect_input_arity(node, inputs, 1)?;
    let mut b = SigBuilder::new(arena);
    Ok(vec![f(&mut b, inputs[0])])
}

/// Lowers one binary primitive and returns a single output signal.
fn binary_prim(
    arena: &mut TreeArena,
    node: TreeId,
    inputs: &[SigId],
    f: impl FnOnce(&mut SigBuilder<'_>, SigId, SigId) -> SigId,
) -> Result<Vec<SigId>, PropagateError> {
    expect_input_arity(node, inputs, 2)?;
    let mut b = SigBuilder::new(arena);
    Ok(vec![f(&mut b, inputs[0], inputs[1])])
}

/// Lowers one ternary primitive and returns a single output signal.
fn ternary_prim(
    arena: &mut TreeArena,
    node: TreeId,
    inputs: &[SigId],
    f: impl FnOnce(&mut SigBuilder<'_>, SigId, SigId, SigId) -> SigId,
) -> Result<Vec<SigId>, PropagateError> {
    expect_input_arity(node, inputs, 3)?;
    let mut b = SigBuilder::new(arena);
    Ok(vec![f(&mut b, inputs[0], inputs[1], inputs[2])])
}

/// Lowers one quaternary primitive and returns a single output signal.
fn quaternary_prim(
    arena: &mut TreeArena,
    node: TreeId,
    inputs: &[SigId],
    f: impl FnOnce(&mut SigBuilder<'_>, SigId, SigId, SigId, SigId) -> SigId,
) -> Result<Vec<SigId>, PropagateError> {
    expect_input_arity(node, inputs, 4)?;
    let mut b = SigBuilder::new(arena);
    Ok(vec![f(&mut b, inputs[0], inputs[1], inputs[2], inputs[3])])
}

/// Lowers one quinary primitive and returns a single output signal.
fn quinary_prim(
    arena: &mut TreeArena,
    node: TreeId,
    inputs: &[SigId],
    f: impl FnOnce(&mut SigBuilder<'_>, SigId, SigId, SigId, SigId, SigId) -> SigId,
) -> Result<Vec<SigId>, PropagateError> {
    expect_input_arity(node, inputs, 5)?;
    let mut b = SigBuilder::new(arena);
    Ok(vec![f(
        &mut b, inputs[0], inputs[1], inputs[2], inputs[3], inputs[4],
    )])
}

/// Returns whether `split` wiring law is satisfied.
///
/// C++ parity rule:
/// - exact match, or
/// - right inputs is an integer multiple of left outputs.
fn split_compatible(left_outputs: usize, right_inputs: usize) -> bool {
    (left_outputs == right_inputs)
        || (left_outputs != 0 && right_inputs.is_multiple_of(left_outputs))
}

/// Returns whether `merge` wiring law is satisfied.
///
/// C++ parity rule:
/// - exact match, or
/// - left outputs is an integer multiple of right inputs.
fn merge_compatible(left_outputs: usize, right_inputs: usize) -> bool {
    (left_outputs == right_inputs)
        || (right_inputs != 0 && left_outputs.is_multiple_of(right_inputs))
}

/// Replicates input buses cyclically to feed `split` right-side arity.
fn split_signals(inputs: &[SigId], nbus: usize) -> Vec<SigId> {
    if nbus == 0 || inputs.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(nbus);
    for b in 0..nbus {
        out.push(inputs[b % inputs.len()]);
    }
    out
}

/// Mixes grouped buses by summing channels modulo `nbus` (merge semantics).
fn mix_signals(arena: &mut TreeArena, inputs: &[SigId], nbus: usize) -> Vec<SigId> {
    if nbus == 0 {
        return Vec::new();
    }

    let mut b = SigBuilder::new(arena);
    let mut out = Vec::with_capacity(nbus);

    for bus in 0..nbus {
        let mut acc = if bus < inputs.len() {
            inputs[bus]
        } else {
            b.int(0)
        };
        let mut idx = bus + nbus;
        while idx < inputs.len() {
            acc = b.add(acc, inputs[idx]);
            idx += nbus;
        }
        out.push(acc);
    }

    out
}

/// Returns list length for a `cons`/`nil` encoded list.
fn list_length(arena: &TreeArena, mut list: TreeId) -> Option<usize> {
    let mut len = 0usize;
    while !arena.is_nil(list) {
        let _ = arena.hd(list)?;
        list = arena.tl(list)?;
        len = len.checked_add(1)?;
    }
    Some(len)
}

/// Converts a `cons`/`nil` list into a vector preserving order.
fn list_to_vec(arena: &TreeArena, mut list: TreeId) -> Option<Vec<TreeId>> {
    let mut out = Vec::new();
    while !arena.is_nil(list) {
        out.push(arena.hd(list)?);
        list = arena.tl(list)?;
    }
    Some(out)
}

/// Reads a non-negative integer node and converts it to `usize`.
fn usize_from_int_node(
    arena: &TreeArena,
    node: TreeId,
    field: &'static str,
) -> Result<usize, PropagateError> {
    let Some(value) = tree_to_int(arena, node) else {
        return Err(PropagateError::InvalidIntegerValue { node, field });
    };
    if value < 0 {
        return Err(PropagateError::NegativeIntegerValue { field, value });
    }
    usize::try_from(value).map_err(|_| PropagateError::InvalidIntegerValue { node, field })
}

/// Fallible `usize -> i32` conversion used for stable signal-index nodes.
fn i32_from_usize(value: usize, field: &'static str) -> Result<i32, PropagateError> {
    i32::try_from(value).map_err(|_| PropagateError::IntegerTooLarge { field, value })
}

/// Seeds recursive feedback inputs with `delay1(proj(i, DEBRUIJNREF(1)))`.
fn make_mem_sig_proj_list(arena: &mut TreeArena, n: usize) -> Result<Vec<SigId>, PropagateError> {
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let idx = i32_from_usize(i, "rec projection seed index")?;
        let rg = debruijn_ref(arena, 1);
        let mut b = SigBuilder::new(arena);
        let proj = b.proj(idx, rg);
        out.push(b.delay1(proj));
    }
    Ok(out)
}

/// Lifts De Bruijn references of input signals by one recursion level.
fn lift_signals(arena: &mut TreeArena, inputs: &[SigId]) -> Vec<SigId> {
    let mut out = Vec::with_capacity(inputs.len());
    for sig in inputs.iter().copied() {
        out.push(liftn(arena, sig, 1));
    }
    out
}

/// Converts a vector to a `cons`/`nil` list preserving order.
fn vec_to_list(arena: &mut TreeArena, values: &[TreeId]) -> TreeId {
    let mut list = arena.nil();
    for value in values.iter().rev().copied() {
        list = arena.cons(value, list);
    }
    list
}

/// Builds one recursive signal group wrapper (`DEBRUIJN(body)`).
fn debruijn_rec(arena: &mut TreeArena, body: TreeId) -> TreeId {
    intern_tag(arena, DEBRUIJN_TAG, &[body])
}

/// Builds one De Bruijn reference node (`DEBRUIJNREF(level)`).
fn debruijn_ref(arena: &mut TreeArena, level: i64) -> TreeId {
    let lvl = arena.int(level);
    intern_tag(arena, DEBRUIJNREF_TAG, &[lvl])
}

/// Recursively lifts De Bruijn reference levels starting at `threshold`.
fn liftn(arena: &mut TreeArena, root: TreeId, threshold: i64) -> TreeId {
    if let Some(level) = debruijn_ref_level(arena, root) {
        if level < threshold {
            return root;
        }
        return debruijn_ref(arena, level + 1);
    }

    if let Some(body) = debruijn_body(arena, root) {
        let lifted_body = liftn(arena, body, threshold + 1);
        return debruijn_rec(arena, lifted_body);
    }

    let Some(node) = arena.node(root).cloned() else {
        return root;
    };
    if node.children.is_empty() {
        return root;
    }

    let original_children = node.children.as_slice();
    let mut rebuilt = Vec::with_capacity(original_children.len());
    let mut changed = false;
    for child in original_children.iter().copied() {
        let lifted = liftn(arena, child, threshold);
        if lifted != child {
            changed = true;
        }
        rebuilt.push(lifted);
    }
    if changed {
        arena.intern(node.kind, &rebuilt)
    } else {
        root
    }
}

/// Computes free-recursion aperture used to decide `sigProj` re-emission.
fn aperture(arena: &TreeArena, root: TreeId) -> i64 {
    if let Some(level) = debruijn_ref_level(arena, root) {
        return level;
    }

    if let Some(body) = debruijn_body(arena, root) {
        return aperture(arena, body) - 1;
    }

    let Some(children) = arena.children(root) else {
        return 0;
    };
    let mut max_aperture = 0;
    for child in children.iter().copied() {
        max_aperture = max_aperture.max(aperture(arena, child));
    }
    max_aperture
}

/// Returns De Bruijn level for a reference node, if `root` is `DEBRUIJNREF`.
fn debruijn_ref_level(arena: &TreeArena, root: TreeId) -> Option<i64> {
    let (tag, children) = tag_and_children(arena, root)?;
    if tag != DEBRUIJNREF_TAG {
        return None;
    }
    let [level_node] = children else {
        return None;
    };
    tree_to_int(arena, *level_node)
}

/// Returns recursive group body when `root` is a `DEBRUIJN` node.
fn debruijn_body(arena: &TreeArena, root: TreeId) -> Option<TreeId> {
    let (tag, children) = tag_and_children(arena, root)?;
    if tag != DEBRUIJN_TAG {
        return None;
    }
    let [body] = children else {
        return None;
    };
    Some(*body)
}

/// Helper to decode `(tag_name, children)` from one tagged node.
fn tag_and_children(arena: &TreeArena, root: TreeId) -> Option<(&str, &[TreeId])> {
    let node = arena.node(root)?;
    let NodeKind::Tag(tag_id) = &node.kind else {
        return None;
    };
    let tag = arena.tag_name(*tag_id)?;
    Some((tag, node.children.as_slice()))
}

/// Interns one tag node with children in the arena.
fn intern_tag(arena: &mut TreeArena, tag: &str, children: &[TreeId]) -> TreeId {
    let tag_id = arena.intern_tag(tag);
    arena.intern(NodeKind::Tag(tag_id), children)
}
