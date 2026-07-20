//! Pure-vector program artifacts: region bodies, the verified program
//! wrapper, the lowering context, and the error taxonomy.

use crate::schedule::SchedulingStrategy;
use crate::signal_fir::vector::analysis::EffectAtom;
use crate::signal_fir::vector::route::{VectorRegion, VectorRouteError, VerifiedRoutedFir};
use crate::signal_fir::vector::verify::ValueType;
use crate::signal_prepare::SimpleSigType;
use fir::{FirId, FirMathOp, FirStore, FirType};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fmt;
/// One scheduled vector loop and its final CSE-rewritten FIR body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PureVectorRegionBody {
    pub(super) loop_id: u64,
    pub(super) statements: Vec<FirId>,
}
impl PureVectorRegionBody {
    /// Stable P4.4 loop identity.
    #[must_use]
    pub fn loop_id(&self) -> u64 {
        self.loop_id
    }

    /// Final statements in execution order.
    #[must_use]
    pub fn statements(&self) -> &[FirId] {
        &self.statements
    }
}
/// Opaque P5.2/P6.5 result accepted by routing and region-body verification.
///
/// The historical `Pure` name is retained for source compatibility. The
/// representation now also carries programs accepted through explicit P6.1
/// state and P6.2 clock policies; it does not imply that those programs are
/// pure.
pub struct VerifiedPureVectorProgram {
    pub(super) store: FirStore,
    pub(super) static_declarations: Vec<FirId>,
    pub(super) table_declarations: Vec<FirId>,
    pub(super) table_init_statements: Vec<FirId>,
    pub(super) mutable_tables: BTreeMap<u64, (String, usize, FirType)>,
    pub(super) transport_declarations: Vec<FirId>,
    pub(super) control_statements: Vec<FirId>,
    pub(super) regions: Vec<PureVectorRegionBody>,
    pub(super) routed: VerifiedRoutedFir,
    pub(super) math_ops: HashSet<FirMathOp>,
    pub(super) int_helpers: BTreeSet<&'static str>,
}
impl VerifiedPureVectorProgram {
    /// FIR store owning every returned id.
    #[must_use]
    pub fn store(&self) -> &FirStore {
        &self.store
    }

    /// Mutable store access reserved for the checked final-module assembler.
    pub(crate) fn store_mut(&mut self) -> &mut FirStore {
        &mut self.store
    }

    /// Consumes the checked program after final module assembly.
    pub(crate) fn into_store(self) -> FirStore {
        self.store
    }

    /// Canonical transport declarations emitted before region bodies.
    #[must_use]
    pub fn transport_declarations(&self) -> &[FirId] {
        &self.transport_declarations
    }

    /// Immutable literal tables required by checked waveform reads.
    #[must_use]
    pub fn static_declarations(&self) -> &[FirId] {
        &self.static_declarations
    }

    /// Mutable table DSP-struct field declarations.
    #[must_use]
    pub fn table_declarations(&self) -> &[FirId] {
        &self.table_declarations
    }

    /// Element-wise mutable-table initialization for `instanceConstants`.
    #[must_use]
    pub fn table_init_statements(&self) -> &[FirId] {
        &self.table_init_statements
    }

    /// Accepted mutable tables by signal id: field name, length, element type.
    #[must_use]
    pub fn mutable_tables(&self) -> &BTreeMap<u64, (String, usize, FirType)> {
        &self.mutable_tables
    }

    /// Fixed control-scope statements, including input pointer aliases.
    #[must_use]
    pub fn control_statements(&self) -> &[FirId] {
        &self.control_statements
    }

    /// Loop bodies in the selected strategy-dependent schedule order.
    #[must_use]
    pub fn regions(&self) -> &[PureVectorRegionBody] {
        &self.regions
    }

    /// Independently accepted P5.1 route evidence.
    #[must_use]
    pub fn routed(&self) -> &VerifiedRoutedFir {
        &self.routed
    }

    /// Math prototypes required when this artifact is assembled as a module.
    #[must_use]
    pub fn math_ops(&self) -> &HashSet<FirMathOp> {
        &self.math_ops
    }

    /// Integer helper prototypes required by `min`, `max`, or `abs`.
    #[must_use]
    pub fn int_helpers(&self) -> &BTreeSet<&'static str> {
        &self.int_helpers
    }
}
/// P5.2 lowering or final-body verification failure.
#[derive(Clone, Debug, PartialEq)]
pub enum PureVectorLowerError {
    /// P5.1 route construction or verification failed.
    Route(VectorRouteError),
    /// Internal real precision is outside the active fast-lane contract.
    InvalidRealType(FirType),
    /// A P4.4 signal id is absent from the verified prepared forest.
    MissingPreparedSignal { signal_id: u64 },
    /// Prepared and planned scalar types disagree.
    PlannedTypeMismatch {
        signal_id: u64,
        planned: ValueType,
        prepared: Option<SimpleSigType>,
    },
    /// The pure P5.2 slice cannot execute an effect-bearing signal.
    EffectfulSignal {
        signal_id: u64,
        expression: String,
        effects: Vec<EffectAtom>,
    },
    /// The pure P5.2 slice has no state/effect semantics for this node.
    UnsupportedSignal { signal_id: u64, expression: String },
    /// A control expression depended on a sample-region value.
    InvalidControlDependency { signal_id: u64 },
    /// A pure signal cycle escaped the P4/P6 recursion boundary.
    PureCycle {
        signal_id: u64,
        region: VectorRegion,
    },
    /// Audio input index is invalid for the declared module arity.
    InputIndexOutOfRange { index: i32, num_inputs: usize },
    /// FIR operands or result violate the prepared typing contract.
    FirTypeMismatch {
        signal_id: u64,
        expected: FirType,
        actual: Option<FirType>,
    },
    /// Region-local CSE did not preserve one sink per requested root.
    CseRootCoverage { region: VectorRegion },
    /// Final bodies do not contain the evidence accepted by P5.1.
    BodyEvidence { detail: String },
}
impl fmt::Display for PureVectorLowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Route(error) => write!(f, "vector routing failed: {error}"),
            Self::InvalidRealType(typ) => write!(f, "unsupported vector real type {typ:?}"),
            Self::MissingPreparedSignal { signal_id } => {
                write!(
                    f,
                    "vector plan signal {signal_id} is absent from the prepared forest"
                )
            }
            Self::PlannedTypeMismatch {
                signal_id,
                planned,
                prepared,
            } => write!(
                f,
                "signal {signal_id} planned type {planned:?} disagrees with prepared type {prepared:?}"
            ),
            Self::EffectfulSignal {
                signal_id,
                expression,
                effects,
            } => {
                write!(
                    f,
                    "signal {signal_id} is effectful and cannot enter pure P5.2 lowering: {expression}; effects={effects:?}"
                )
            }
            Self::UnsupportedSignal {
                signal_id,
                expression,
            } => write!(
                f,
                "signal {signal_id} is outside the pure P5.2 node set: {expression}"
            ),
            Self::InvalidControlDependency { signal_id } => {
                write!(f, "control lowering reached sample signal {signal_id}")
            }
            Self::PureCycle { signal_id, region } => {
                write!(f, "pure signal cycle at signal {signal_id} in {region:?}")
            }
            Self::InputIndexOutOfRange { index, num_inputs } => {
                write!(f, "input index {index} is outside num_inputs={num_inputs}")
            }
            Self::FirTypeMismatch {
                signal_id,
                expected,
                actual,
            } => write!(
                f,
                "signal {signal_id} FIR type {actual:?} does not match {expected:?}"
            ),
            Self::CseRootCoverage { region } => {
                write!(f, "CSE changed root-sink coverage in {region:?}")
            }
            Self::BodyEvidence { detail } => {
                write!(f, "routed region-body verification failed: {detail}")
            }
        }
    }
}
impl std::error::Error for PureVectorLowerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Route(error) => Some(error),
            _ => None,
        }
    }
}
impl From<VectorRouteError> for PureVectorLowerError {
    fn from(value: VectorRouteError) -> Self {
        Self::Route(value)
    }
}
/// Shared immutable configuration for one vector-region lowering pipeline.
///
/// State and clock certificates remain explicit arguments because they are
/// independently verified artifacts. This context groups the execution policy
/// and module-interface parameters consumed throughout lowering.
pub struct VectorLoweringContext<'a> {
    /// Canonical grouped UI program associated with the prepared forest.
    pub ui: &'a ui::UiProgram,
    /// Per-epoch scheduling strategy.
    pub strategy: SchedulingStrategy,
    /// Internal FIR real type.
    pub real_type: FirType,
    /// Number of audio inputs exposed by the module contract.
    pub num_inputs: usize,
}
