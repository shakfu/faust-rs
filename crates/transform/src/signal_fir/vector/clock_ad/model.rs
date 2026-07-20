//! Clock-island / AD policy vocabulary and error taxonomy (schema v1).

use crate::clk_env::ClkEnvError;
use crate::signal_fir::vector::analysis::{EffectAtom, StateCell, StateResource};
#[cfg(test)]
use crate::signal_fir::vector::plan::VerifiedVectorPlan;
use crate::signal_fir::vector::verify::{VectorPlan, VectorPlanError};
use propagate::ClockDomainKind;
use std::collections::BTreeSet;
use std::fmt;

/// Current canonical P6.2 clock/AD-plan schema.
pub const VECTOR_CLOCK_AD_PLAN_VERSION: u32 = 1;
/// Concrete guard shape used to implement `fires(c, i)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClockGuard {
    BooleanOnDemand,
    CountedOnDemand,
    CountedUpsampling,
    DownsampleModulo,
}
/// Whether a pre-planned P5 transport may use the outer chunk index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClockTransportMode {
    /// Audio-rate value: the P5 `i0 - vindex` chunk route remains valid.
    OuterChunk,
    /// Per-sample value crossing logical loops in one certified fused group.
    /// The route is a scalar stack temporary inside the group's sole sample
    /// envelope, either a top-rate loop or one exact guarded clock island,
    /// never a chunk array filled by a preceding loop.
    FusedScalar { group_id: u64 },
    /// Domain-rate value: route below the serial guard, one fire at a time.
    IslandScalar { domain_id: u64 },
    /// Persistent `PermVar` value exported from a domain to an ancestor.
    HeldOutput { domain_id: u64 },
}
/// Complete policy for one existing P5 transport.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClockTransportPolicy {
    pub transport_id: u64,
    pub mode: ClockTransportMode,
}
/// One serial OD/US/DS region and the P4 loops nested below it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClockIsland {
    pub domain_id: u64,
    pub parent_domain: Option<u64>,
    pub kind: ClockDomainKind,
    pub clock_signal_id: u64,
    pub wrapper_signal_id: u64,
    pub boundary_loop_id: u64,
    pub guard: ClockGuard,
    /// Exact signals whose inferred clock environment is this domain.
    pub signal_ids: Vec<u64>,
    /// Signals that own clock guard/counter state for this domain.
    pub clock_state_signal_ids: Vec<u64>,
    /// Existing P4 loops with at least one root in this domain.
    pub nested_loop_ids: Vec<u64>,
}
/// FAD policy at the prepared-signal boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForwardAdPolicy {
    /// Propagation expanded FAD into ordinary primal/tangent signals.
    ExpandedSignalGraph,
}
/// Reverse-mode carrier requiring a block-local reverse window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ReverseAdKind {
    ReverseTimeRec,
    BlockReverseAd,
}
/// Semantically fixed reverse-mode epoch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AdEpoch {
    Forward,
    Reverse,
}
/// Stable reason why reverse-mode execution is not admitted to vector mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReverseAdDiagnostic {
    ScalarReverseWindowRequired,
}
impl ReverseAdDiagnostic {
    /// Stable internal diagnostic code for a future CLI/backend activation gate.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::ScalarReverseWindowRequired => "FRS-VEC-RAD-SCALAR",
        }
    }

    /// User-facing reason that vector reverse-mode execution is refused.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::ScalarReverseWindowRequired => {
                "reverse AD requires a scalar forward/tape/reverse window; vector chunk semantics are not enabled"
            }
        }
    }
}
/// One explicit scalar fallback for a reverse-time Signal-IR carrier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReverseAdFallback {
    pub signal_id: u64,
    pub owner_loop_id: u64,
    pub kind: ReverseAdKind,
    pub epochs: Vec<AdEpoch>,
    pub diagnostic: ReverseAdDiagnostic,
}
/// Canonical finite P6.2 artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorClockAdPlan {
    pub schema_version: u32,
    pub vec_size: u64,
    pub clock_islands: Vec<ClockIsland>,
    pub transports: Vec<ClockTransportPolicy>,
    pub forward_ad: ForwardAdPolicy,
    pub reverse_ad_fallbacks: Vec<ReverseAdFallback>,
}
/// Opaque evidence that P6.2 construction passed its checker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedVectorClockAdPlan {
    pub(super) plan: VectorClockAdPlan,
    pub(super) vector_plan: VectorPlan,
}
impl VerifiedVectorClockAdPlan {
    #[must_use]
    pub fn plan(&self) -> &VectorClockAdPlan {
        &self.plan
    }

    #[must_use]
    pub fn vector_plan(&self) -> &VectorPlan {
        &self.vector_plan
    }

    #[must_use]
    pub fn into_plan(self) -> VectorClockAdPlan {
        self.plan
    }

    /// State resources whose execution is completely owned by P6.2 guards or
    /// persistent held-output transports.
    #[must_use]
    pub fn managed_state_resources(&self) -> BTreeSet<StateResource> {
        self.vector_plan
            .signals
            .iter()
            .flat_map(|signal| signal.effects.iter())
            .filter_map(|effect| match effect {
                EffectAtom::ReadState(resource) | EffectAtom::WriteState(resource)
                    if matches!(
                        resource,
                        StateResource::Signal {
                            cell: StateCell::Clock | StateCell::Hold,
                            ..
                        }
                    ) =>
                {
                    Some(resource.clone())
                }
                _ => None,
            })
            .collect()
    }
}
#[cfg(test)]
pub(crate) fn verified_vector_clock_ad_plan_for_test(
    plan: VectorClockAdPlan,
    vector_plan: &VerifiedVectorPlan,
) -> VerifiedVectorClockAdPlan {
    assert_eq!(plan.schema_version, VECTOR_CLOCK_AD_PLAN_VERSION);
    assert_eq!(plan.vec_size, vector_plan.plan().vec_size);
    assert_eq!(plan.transports.len(), vector_plan.plan().transports.len());
    for (policy, transport) in plan.transports.iter().zip(&vector_plan.plan().transports) {
        assert_eq!(policy.transport_id, transport.transport_id);
    }
    VerifiedVectorClockAdPlan {
        plan,
        vector_plan: vector_plan.plan().clone(),
    }
}
/// Typed producer/checker failure at the P6.2 boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VectorClockAdError {
    Plan(VectorPlanError),
    ClockInference(ClkEnvError),
    UnsupportedSchema {
        found: u32,
    },
    VecSizeMismatch {
        declared: u64,
        actual: u64,
    },
    SignalCoverageMismatch {
        signal_id: u64,
    },
    ClockFactMismatch {
        signal_id: u64,
    },
    DomainParentUnknown {
        domain_id: u64,
        parent_id: u64,
    },
    DomainCycle {
        domain_id: u64,
    },
    ClockSignalUnknown {
        domain_id: u64,
        signal_id: u64,
    },
    UnsupportedClockType {
        domain_id: u64,
        kind: ClockDomainKind,
    },
    WrapperCoverageMismatch {
        domain_id: u64,
    },
    WrapperKindMismatch {
        domain_id: u64,
        table: ClockDomainKind,
        signal: ClockDomainKind,
    },
    ClockStateDomainUnknown {
        signal_id: u64,
    },
    BoundaryNotOwned {
        domain_id: u64,
        signal_id: u64,
    },
    BoundaryNotSerial {
        domain_id: u64,
        loop_id: u64,
    },
    IslandCoverageMismatch,
    TransportCoverageMismatch,
    ReverseCarrierNotOwned {
        signal_id: u64,
    },
    ReverseAdCoverageMismatch,
    ClockValueKindMismatch {
        guard: ClockGuard,
    },
    InvalidDownsampleFactor {
        factor: i64,
    },
    UnadoptedStatefulRead {
        domain_id: u64,
        consumer: u64,
        producer: u64,
    },
}
impl fmt::Display for VectorClockAdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(error) => write!(f, "vector plan verification failed: {error}"),
            Self::ClockInference(error) => write!(f, "clock inference failed: {error}"),
            Self::UnsupportedSchema { found } => {
                write!(f, "unsupported vector clock/AD schema {found}")
            }
            Self::VecSizeMismatch { declared, actual } => write!(
                f,
                "clock/AD plan vector size {declared} differs from vector plan {actual}"
            ),
            Self::SignalCoverageMismatch { signal_id } => {
                write!(f, "clock/AD source facts do not cover signal {signal_id}")
            }
            Self::ClockFactMismatch { signal_id } => {
                write!(f, "clock facts disagree for signal {signal_id}")
            }
            Self::DomainParentUnknown {
                domain_id,
                parent_id,
            } => write!(
                f,
                "clock domain {domain_id} names unknown parent {parent_id}"
            ),
            Self::DomainCycle { domain_id } => {
                write!(
                    f,
                    "clock domain hierarchy cycles through domain {domain_id}"
                )
            }
            Self::ClockSignalUnknown {
                domain_id,
                signal_id,
            } => write!(
                f,
                "clock domain {domain_id} names unknown clock signal {signal_id}"
            ),
            Self::UnsupportedClockType { domain_id, kind } => write!(
                f,
                "clock domain {domain_id} ({kind:?}) has no supported boolean/integer guard"
            ),
            Self::WrapperCoverageMismatch { domain_id } => write!(
                f,
                "clock domain {domain_id} does not have exactly one matching wrapper"
            ),
            Self::WrapperKindMismatch {
                domain_id,
                table,
                signal,
            } => write!(
                f,
                "clock domain {domain_id} is {table:?} in the table but {signal:?} in Signal IR"
            ),
            Self::ClockStateDomainUnknown { signal_id } => write!(
                f,
                "clock-state signal {signal_id} has no decodable domain token"
            ),
            Self::BoundaryNotOwned {
                domain_id,
                signal_id,
            } => write!(
                f,
                "clock boundary {domain_id} signal {signal_id} has no owned loop"
            ),
            Self::BoundaryNotSerial { domain_id, loop_id } => write!(
                f,
                "clock boundary {domain_id} is owned by non-serial loop {loop_id}"
            ),
            Self::IslandCoverageMismatch => write!(f, "clock-island facts are not exact"),
            Self::TransportCoverageMismatch => {
                write!(f, "clock transport policies are not exact")
            }
            Self::ReverseCarrierNotOwned { signal_id } => write!(
                f,
                "reverse-mode carrier {signal_id} has no owned scalar loop"
            ),
            Self::ReverseAdCoverageMismatch => {
                write!(f, "reverse-mode fallback facts are not exact")
            }
            Self::ClockValueKindMismatch { guard } => {
                write!(f, "runtime clock value does not match {guard:?}")
            }
            Self::InvalidDownsampleFactor { factor } => {
                write!(f, "downsampling factor must be positive, got {factor}")
            }
            Self::UnadoptedStatefulRead {
                domain_id,
                consumer,
                producer,
            } => {
                write!(
                    f,
                    "clock domain {domain_id}: signal {consumer} reads audio-rate stateful \
                     signal {producer} from fire time; rate-polymorphic state under a clock \
                     wrapper is not adopted into the domain yet and would advance at the \
                     wrong rate"
                )
            }
        }
    }
}
impl std::error::Error for VectorClockAdError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plan(error) => Some(error),
            Self::ClockInference(error) => Some(error),
            _ => None,
        }
    }
}
impl From<VectorPlanError> for VectorClockAdError {
    fn from(value: VectorPlanError) -> Self {
        Self::Plan(value)
    }
}
impl From<ClkEnvError> for VectorClockAdError {
    fn from(value: ClkEnvError) -> Self {
        Self::ClockInference(value)
    }
}
