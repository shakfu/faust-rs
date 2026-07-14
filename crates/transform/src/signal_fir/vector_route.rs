//! P5.1 region-aware vector routing and routed-FIR verification.
//!
//! # C++ provenance and adaptation
//! C++ `DAGInstructionsCompiler` combines loop ownership, value caching, and
//! vector buffer allocation while recursively compiling signals. Rust keeps
//! those concerns explicit: P4.4 freezes a [`VectorPlan`], this module resolves
//! values through three visibility scopes, and later lowering emits the loop
//! bodies. A cross-loop read can only use a transport already named by P4.4;
//! no buffer identity is allocated on demand.
//!
//! This is an additive P5 routing gate. It emits real FIR declarations,
//! stores, and loads for planned transports and independently verifies them,
//! but it is not connected to `build_module` yet. Stateful delay, recursion,
//! clock, and AD loop transitions remain P6 and must not silently reuse the
//! scalar cache through this API.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fir::{AccessType, FirBinOp, FirBuilder, FirId, FirMatch, FirStore, FirType, match_fir};

use crate::schedule::SchedulingStrategy;

use super::vector_plan::VerifiedVectorPlan;
use super::vector_schedule::{VectorScheduleError, schedule_vector_plan};
use super::vector_verify::{
    LoopKind, Placement, TransportRecord, ValueType, VectorPlan, VectorPlanError,
    verify_vector_plan,
};

/// Lexical visibility scope used by the P5 value cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VectorRegion {
    /// Fixed control/lifecycle scope, visible from every loop.
    Control,
    /// One vector-plan loop region.
    Loop(u64),
}

/// One materialized loop region in strategy-dependent execution order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorLoopRegion {
    /// Stable loop id from the vector plan.
    pub loop_id: u64,
    /// Epoch containing the loop.
    pub epoch_id: u64,
    /// Dependency rank of the epoch.
    pub epoch_rank: u64,
    /// Vectorizable, recursive, or scalar-island loop classification.
    pub kind: LoopKind,
    /// Canonically ordered signal roots owned by the loop.
    pub roots: Vec<u64>,
}

/// Scheduled region layout. The plan remains strategy independent; only this
/// ordered projection varies with `-ss`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorRegionLayout {
    strategy: SchedulingStrategy,
    loops: Vec<VectorLoopRegion>,
}

impl VectorRegionLayout {
    #[must_use]
    pub fn strategy(&self) -> SchedulingStrategy {
        self.strategy
    }

    #[must_use]
    pub fn loops(&self) -> &[VectorLoopRegion] {
        &self.loops
    }
}

/// Result of one region-aware cache lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteResolution {
    /// A visible FIR value, direct or loaded from a planned transport.
    Value(FirId),
    /// An `Inline` signal has no value in this exact loop yet and must be
    /// lowered locally before being recorded with [`VectorRouteSession::define_in_loop`].
    NeedsInlineLowering,
}

/// Source selected for one recorded signal use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutedUseSource {
    Direct(VectorRegion),
    Transport(u64),
}

/// One FIR definition recorded at its legal visibility scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoutedDefinition {
    /// Stable signal id.
    pub signal_id: u64,
    /// Region in which the FIR value is defined.
    pub region: VectorRegion,
    /// FIR value implementing the signal in that region.
    pub value: FirId,
}

/// One cache resolution made from a loop region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoutedUse {
    /// Stable signal id.
    pub signal_id: u64,
    /// Loop reading the signal.
    pub consumer_loop: u64,
    /// Visibility path selected for the read.
    pub source: RoutedUseSource,
    /// Direct value or concrete transport load.
    pub value: FirId,
}

/// Concrete FIR evidence for one P4.4 transport.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutedTransport {
    /// Stable transport id from P4.4.
    pub transport_id: u64,
    /// Stack-array declaration with the canonical transport name and type.
    pub declaration: FirId,
    /// Value defined by the producer loop.
    pub producer_value: Option<FirId>,
    /// Producer-side table store.
    pub store: Option<FirId>,
    /// Consumer-side table load.
    pub load: Option<FirId>,
}

/// Canonical routed-FIR trace checked after lowering and before emission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoutedFirTrace {
    definitions: Vec<RoutedDefinition>,
    uses: Vec<RoutedUse>,
    transports: Vec<RoutedTransport>,
}

impl RoutedFirTrace {
    #[must_use]
    pub fn definitions(&self) -> &[RoutedDefinition] {
        &self.definitions
    }

    #[must_use]
    pub fn uses(&self) -> &[RoutedUse] {
        &self.uses
    }

    #[must_use]
    pub fn transports(&self) -> &[RoutedTransport] {
        &self.transports
    }
}

/// Opaque evidence that [`verify_routed_fir`] accepted the routing trace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedRoutedFir {
    plan: VectorPlan,
    layout: VectorRegionLayout,
    trace: RoutedFirTrace,
}

impl VerifiedRoutedFir {
    /// Returns the exact verified plan whose stable identities own this route.
    #[must_use]
    pub fn plan(&self) -> &VectorPlan {
        &self.plan
    }

    #[must_use]
    pub fn layout(&self) -> &VectorRegionLayout {
        &self.layout
    }

    #[must_use]
    pub fn trace(&self) -> &RoutedFirTrace {
        &self.trace
    }

    #[must_use]
    pub fn into_trace(self) -> RoutedFirTrace {
        self.trace
    }
}

/// Typed P5 routing or routed-FIR verification failure.
#[derive(Clone, Debug, PartialEq)]
pub enum VectorRouteError {
    Plan(VectorPlanError),
    Schedule(VectorScheduleError),
    UnknownSignal {
        signal_id: u64,
    },
    UnknownLoop {
        loop_id: u64,
    },
    WrongRegion {
        signal_id: u64,
        expected: Placement,
        actual: VectorRegion,
    },
    DuplicateDefinition {
        signal_id: u64,
        region: VectorRegion,
    },
    MissingDefinition {
        signal_id: u64,
        region: VectorRegion,
    },
    MissingInlineDefinition {
        signal_id: u64,
    },
    MissingTransport {
        signal_id: u64,
        producer_loop: u64,
        consumer_loop: u64,
    },
    UnsupportedTupleTransport {
        signal_id: u64,
    },
    ValueTypeMismatch {
        signal_id: u64,
        expected: FirType,
        actual: Option<FirType>,
    },
    DefinitionCoverage {
        signal_id: u64,
    },
    TransportCoverage {
        transport_id: u64,
    },
    TransportDeclaration {
        transport_id: u64,
    },
    TransportStore {
        transport_id: u64,
    },
    TransportLoad {
        transport_id: u64,
    },
    InvalidUse {
        signal_id: u64,
        consumer_loop: u64,
    },
}

impl fmt::Display for VectorRouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(error) => write!(f, "vector plan verification failed: {error}"),
            Self::Schedule(error) => write!(f, "vector scheduling failed: {error}"),
            Self::UnknownSignal { signal_id } => write!(f, "unknown signal {signal_id}"),
            Self::UnknownLoop { loop_id } => write!(f, "unknown loop {loop_id}"),
            Self::WrongRegion {
                signal_id,
                expected,
                actual,
            } => write!(
                f,
                "signal {signal_id} has placement {expected:?}, not region {actual:?}"
            ),
            Self::DuplicateDefinition { signal_id, region } => {
                write!(f, "signal {signal_id} is defined twice in {region:?}")
            }
            Self::MissingDefinition { signal_id, region } => {
                write!(f, "signal {signal_id} has no definition in {region:?}")
            }
            Self::MissingInlineDefinition { signal_id } => {
                write!(f, "inline signal {signal_id} was never lowered")
            }
            Self::MissingTransport {
                signal_id,
                producer_loop,
                consumer_loop,
            } => write!(
                f,
                "no planned transport for signal {signal_id} from loop {producer_loop} to loop {consumer_loop}"
            ),
            Self::UnsupportedTupleTransport { signal_id } => {
                write!(f, "tuple transport for signal {signal_id} is not lowered")
            }
            Self::ValueTypeMismatch {
                signal_id,
                expected,
                actual,
            } => write!(
                f,
                "signal {signal_id} FIR type {actual:?} does not match {expected:?}"
            ),
            Self::DefinitionCoverage { signal_id } => {
                write!(f, "routed definitions do not cover signal {signal_id}")
            }
            Self::TransportCoverage { transport_id } => {
                write!(
                    f,
                    "routed trace does not exactly cover transport {transport_id}"
                )
            }
            Self::TransportDeclaration { transport_id } => {
                write!(f, "transport {transport_id} declaration is invalid")
            }
            Self::TransportStore { transport_id } => {
                write!(
                    f,
                    "transport {transport_id} producer store is invalid or missing"
                )
            }
            Self::TransportLoad { transport_id } => {
                write!(
                    f,
                    "transport {transport_id} consumer load is invalid or missing"
                )
            }
            Self::InvalidUse {
                signal_id,
                consumer_loop,
            } => write!(
                f,
                "signal {signal_id} is not visible from loop {consumer_loop} through the recorded source"
            ),
        }
    }
}

impl std::error::Error for VectorRouteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plan(error) => Some(error),
            Self::Schedule(error) => Some(error),
            _ => None,
        }
    }
}

impl From<VectorPlanError> for VectorRouteError {
    fn from(value: VectorPlanError) -> Self {
        Self::Plan(value)
    }
}

impl From<VectorScheduleError> for VectorRouteError {
    fn from(value: VectorScheduleError) -> Self {
        Self::Schedule(value)
    }
}

#[derive(Default)]
struct VectorValueCache {
    control: BTreeMap<u64, FirId>,
    owned: BTreeMap<u64, (u64, FirId)>,
    inline: BTreeMap<(u64, u64), FirId>,
    transport_loads: BTreeMap<u64, FirId>,
}

/// Stateful P5 routing surface used by a signal lowerer.
pub struct VectorRouteSession<'a> {
    plan: &'a VectorPlan,
    layout: VectorRegionLayout,
    real_type: FirType,
    cache: VectorValueCache,
    trace: RoutedFirTrace,
    transport_by_route: BTreeMap<(u64, u64, u64), usize>,
    loop_ids: BTreeSet<u64>,
}

impl<'a> VectorRouteSession<'a> {
    /// Creates scheduled vector regions and emits every transport declaration.
    /// No transport is created after this operation.
    pub fn new(
        verified: &'a VerifiedVectorPlan,
        strategy: SchedulingStrategy,
        real_type: FirType,
        store: &mut FirStore,
    ) -> Result<(Self, Vec<FirId>), VectorRouteError> {
        let plan = verified.plan();
        verify_vector_plan(plan)?;
        let schedule = schedule_vector_plan(plan, strategy)?;
        let loop_by_id = plan
            .loops
            .iter()
            .map(|record| (record.loop_id, record))
            .collect::<BTreeMap<_, _>>();
        let mut loops = Vec::with_capacity(plan.loops.len());
        for epoch in &schedule.epochs {
            for &loop_id in &epoch.loops {
                let record = loop_by_id[&loop_id];
                loops.push(VectorLoopRegion {
                    loop_id,
                    epoch_id: epoch.epoch_id,
                    epoch_rank: epoch.rank,
                    kind: record.kind,
                    roots: record.roots.clone(),
                });
            }
        }
        let layout = VectorRegionLayout { strategy, loops };
        let mut declarations = Vec::with_capacity(plan.transports.len());
        let mut routed_transports = Vec::with_capacity(plan.transports.len());
        let mut transport_by_route = BTreeMap::new();
        for (index, transport) in plan.transports.iter().enumerate() {
            let elem = transport_fir_type(transport, real_type.clone())?;
            let length = usize::try_from(transport.length).map_err(|_| {
                VectorRouteError::TransportDeclaration {
                    transport_id: transport.transport_id,
                }
            })?;
            let declaration = FirBuilder::new(store).declare_var(
                transport.stable_name.clone(),
                FirType::Array(Box::new(elem), length),
                AccessType::Stack,
                None,
            );
            declarations.push(declaration);
            routed_transports.push(RoutedTransport {
                transport_id: transport.transport_id,
                declaration,
                producer_value: None,
                store: None,
                load: None,
            });
            transport_by_route.insert(
                (
                    transport.signal_id,
                    transport.producer_loop,
                    transport.consumer_loop,
                ),
                index,
            );
        }
        let loop_ids = plan.loops.iter().map(|record| record.loop_id).collect();
        Ok((
            Self {
                plan,
                layout,
                real_type,
                cache: VectorValueCache::default(),
                trace: RoutedFirTrace {
                    definitions: Vec::new(),
                    uses: Vec::new(),
                    transports: routed_transports,
                },
                transport_by_route,
                loop_ids,
            },
            declarations,
        ))
    }

    #[must_use]
    pub fn layout(&self) -> &VectorRegionLayout {
        &self.layout
    }

    /// Returns the immutable P4.4 plan that owns all route identities.
    #[must_use]
    pub fn plan(&self) -> &VectorPlan {
        self.plan
    }

    /// Records a control value, visible from every vector loop.
    pub fn define_control(
        &mut self,
        signal_id: u64,
        value: FirId,
        store: &FirStore,
    ) -> Result<(), VectorRouteError> {
        let signal = self.signal(signal_id)?;
        if signal.placement != Placement::Control {
            return Err(VectorRouteError::WrongRegion {
                signal_id,
                expected: signal.placement,
                actual: VectorRegion::Control,
            });
        }
        check_value_type(signal_id, &signal.value_type, &self.real_type, value, store)?;
        if self.cache.control.insert(signal_id, value).is_some() {
            return Err(VectorRouteError::DuplicateDefinition {
                signal_id,
                region: VectorRegion::Control,
            });
        }
        self.trace.definitions.push(RoutedDefinition {
            signal_id,
            region: VectorRegion::Control,
            value,
        });
        Ok(())
    }

    /// Records an `Inline` instance or the unique `Owned(loop_id)` producer.
    /// Returns all pre-planned transport stores that the producer loop must
    /// append immediately after computing this value.
    pub fn define_in_loop(
        &mut self,
        loop_id: u64,
        signal_id: u64,
        value: FirId,
        store: &mut FirStore,
    ) -> Result<Vec<FirId>, VectorRouteError> {
        self.ensure_loop(loop_id)?;
        let signal = self.signal(signal_id)?;
        check_value_type(signal_id, &signal.value_type, &self.real_type, value, store)?;
        match signal.placement {
            Placement::Inline => {
                if self
                    .cache
                    .inline
                    .insert((loop_id, signal_id), value)
                    .is_some()
                {
                    return Err(VectorRouteError::DuplicateDefinition {
                        signal_id,
                        region: VectorRegion::Loop(loop_id),
                    });
                }
            }
            Placement::Owned(owner) if owner == loop_id => {
                if self
                    .cache
                    .owned
                    .insert(signal_id, (loop_id, value))
                    .is_some()
                {
                    return Err(VectorRouteError::DuplicateDefinition {
                        signal_id,
                        region: VectorRegion::Loop(loop_id),
                    });
                }
            }
            placement => {
                return Err(VectorRouteError::WrongRegion {
                    signal_id,
                    expected: placement,
                    actual: VectorRegion::Loop(loop_id),
                });
            }
        }
        self.trace.definitions.push(RoutedDefinition {
            signal_id,
            region: VectorRegion::Loop(loop_id),
            value,
        });

        let outgoing = self
            .plan
            .transports
            .iter()
            .enumerate()
            .filter(|(_, transport)| {
                transport.signal_id == signal_id && transport.producer_loop == loop_id
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let mut stores = Vec::with_capacity(outgoing.len());
        for index in outgoing {
            let transport = &self.plan.transports[index];
            let routed = &mut self.trace.transports[index];
            if routed.store.is_some() {
                return Err(VectorRouteError::TransportStore {
                    transport_id: transport.transport_id,
                });
            }
            let chunk_index = chunk_index(store);
            let statement = FirBuilder::new(store).store_table(
                transport.stable_name.clone(),
                AccessType::Stack,
                chunk_index,
                value,
            );
            routed.producer_value = Some(value);
            routed.store = Some(statement);
            stores.push(statement);
        }
        Ok(stores)
    }

    /// Resolves one signal use from `consumer_loop` according to P5 visibility.
    pub fn resolve_in_loop(
        &mut self,
        consumer_loop: u64,
        signal_id: u64,
        store: &mut FirStore,
    ) -> Result<RouteResolution, VectorRouteError> {
        self.ensure_loop(consumer_loop)?;
        let signal = self.signal(signal_id)?;
        let (resolution, source) = match signal.placement {
            Placement::Control => {
                let value = self.cache.control.get(&signal_id).copied().ok_or(
                    VectorRouteError::MissingDefinition {
                        signal_id,
                        region: VectorRegion::Control,
                    },
                )?;
                (
                    RouteResolution::Value(value),
                    RoutedUseSource::Direct(VectorRegion::Control),
                )
            }
            Placement::Inline => {
                let Some(value) = self.cache.inline.get(&(consumer_loop, signal_id)).copied()
                else {
                    return Ok(RouteResolution::NeedsInlineLowering);
                };
                (
                    RouteResolution::Value(value),
                    RoutedUseSource::Direct(VectorRegion::Loop(consumer_loop)),
                )
            }
            Placement::Owned(owner) if owner == consumer_loop => {
                let value = self
                    .cache
                    .owned
                    .get(&signal_id)
                    .map(|(_, value)| *value)
                    .ok_or(VectorRouteError::MissingDefinition {
                        signal_id,
                        region: VectorRegion::Loop(owner),
                    })?;
                (
                    RouteResolution::Value(value),
                    RoutedUseSource::Direct(VectorRegion::Loop(owner)),
                )
            }
            Placement::Owned(owner) => {
                if !self.cache.owned.contains_key(&signal_id) {
                    return Err(VectorRouteError::MissingDefinition {
                        signal_id,
                        region: VectorRegion::Loop(owner),
                    });
                }
                let index = self
                    .transport_by_route
                    .get(&(signal_id, owner, consumer_loop))
                    .copied()
                    .ok_or(VectorRouteError::MissingTransport {
                        signal_id,
                        producer_loop: owner,
                        consumer_loop,
                    })?;
                let transport = &self.plan.transports[index];
                let value = if let Some(value) = self
                    .cache
                    .transport_loads
                    .get(&transport.transport_id)
                    .copied()
                {
                    value
                } else {
                    let elem = transport_fir_type(transport, self.real_type.clone())?;
                    let index_value = chunk_index(store);
                    let value = FirBuilder::new(store).load_table(
                        transport.stable_name.clone(),
                        AccessType::Stack,
                        index_value,
                        elem,
                    );
                    self.cache
                        .transport_loads
                        .insert(transport.transport_id, value);
                    self.trace.transports[index].load = Some(value);
                    value
                };
                (
                    RouteResolution::Value(value),
                    RoutedUseSource::Transport(transport.transport_id),
                )
            }
        };
        let RouteResolution::Value(value) = resolution else {
            unreachable!()
        };
        self.trace.uses.push(RoutedUse {
            signal_id,
            consumer_loop,
            source,
            value,
        });
        Ok(resolution)
    }

    /// Closes the route and independently verifies FIR visibility and all
    /// planned transport declarations/store/load pairs.
    pub fn finish(mut self, store: &FirStore) -> Result<VerifiedRoutedFir, VectorRouteError> {
        self.trace
            .definitions
            .sort_by_key(|definition| (definition.region, definition.signal_id));
        verify_routed_fir(self.plan, &self.trace, &self.real_type, store)?;
        Ok(VerifiedRoutedFir {
            plan: self.plan.clone(),
            layout: self.layout,
            trace: self.trace,
        })
    }

    fn signal(
        &self,
        signal_id: u64,
    ) -> Result<&super::vector_verify::SignalRecord, VectorRouteError> {
        self.plan
            .signals
            .iter()
            .find(|signal| signal.signal_id == signal_id)
            .ok_or(VectorRouteError::UnknownSignal { signal_id })
    }

    fn ensure_loop(&self, loop_id: u64) -> Result<(), VectorRouteError> {
        if self.loop_ids.contains(&loop_id) {
            Ok(())
        } else {
            Err(VectorRouteError::UnknownLoop { loop_id })
        }
    }
}

/// Independently checks a complete P5 routed-FIR trace.
pub fn verify_routed_fir(
    plan: &VectorPlan,
    trace: &RoutedFirTrace,
    real_type: &FirType,
    store: &FirStore,
) -> Result<(), VectorRouteError> {
    verify_vector_plan(plan)?;
    let signal_by_id = plan
        .signals
        .iter()
        .map(|signal| (signal.signal_id, signal))
        .collect::<BTreeMap<_, _>>();
    let loop_ids = plan
        .loops
        .iter()
        .map(|record| record.loop_id)
        .collect::<BTreeSet<_>>();

    let mut seen_definitions = BTreeSet::new();
    let mut signals_with_definition = BTreeSet::new();
    for definition in &trace.definitions {
        let signal = signal_by_id.get(&definition.signal_id).copied().ok_or(
            VectorRouteError::UnknownSignal {
                signal_id: definition.signal_id,
            },
        )?;
        if !seen_definitions.insert((definition.region, definition.signal_id)) {
            return Err(VectorRouteError::DuplicateDefinition {
                signal_id: definition.signal_id,
                region: definition.region,
            });
        }
        let legal = match (signal.placement, definition.region) {
            (Placement::Control, VectorRegion::Control) => true,
            (Placement::Inline, VectorRegion::Loop(loop_id)) => loop_ids.contains(&loop_id),
            (Placement::Owned(owner), VectorRegion::Loop(loop_id)) => owner == loop_id,
            _ => false,
        };
        if !legal {
            return Err(VectorRouteError::WrongRegion {
                signal_id: definition.signal_id,
                expected: signal.placement,
                actual: definition.region,
            });
        }
        check_value_type(
            definition.signal_id,
            &signal.value_type,
            real_type,
            definition.value,
            store,
        )?;
        signals_with_definition.insert(definition.signal_id);
    }
    for signal in &plan.signals {
        if !signals_with_definition.contains(&signal.signal_id) {
            return match signal.placement {
                Placement::Inline => Err(VectorRouteError::MissingInlineDefinition {
                    signal_id: signal.signal_id,
                }),
                Placement::Control | Placement::Owned(_) => {
                    Err(VectorRouteError::DefinitionCoverage {
                        signal_id: signal.signal_id,
                    })
                }
            };
        }
    }

    if trace.transports.len() != plan.transports.len() {
        let transport_id = plan
            .transports
            .get(trace.transports.len())
            .map_or(u64::MAX, |transport| transport.transport_id);
        return Err(VectorRouteError::TransportCoverage { transport_id });
    }
    for (transport, routed) in plan.transports.iter().zip(&trace.transports) {
        if routed.transport_id != transport.transport_id {
            return Err(VectorRouteError::TransportCoverage {
                transport_id: transport.transport_id,
            });
        }
        verify_transport(transport, routed, real_type, store)?;
        let producer_value_is_declared = trace.definitions.iter().any(|definition| {
            definition.signal_id == transport.signal_id
                && definition.region == VectorRegion::Loop(transport.producer_loop)
                && Some(definition.value) == routed.producer_value
        });
        if !producer_value_is_declared {
            return Err(VectorRouteError::TransportStore {
                transport_id: transport.transport_id,
            });
        }
        let load_is_consumed = trace.uses.iter().any(|routed_use| {
            routed_use.signal_id == transport.signal_id
                && routed_use.consumer_loop == transport.consumer_loop
                && routed_use.source == RoutedUseSource::Transport(transport.transport_id)
                && Some(routed_use.value) == routed.load
        });
        if !load_is_consumed {
            return Err(VectorRouteError::TransportLoad {
                transport_id: transport.transport_id,
            });
        }
    }

    for routed_use in &trace.uses {
        let signal = signal_by_id.get(&routed_use.signal_id).copied().ok_or(
            VectorRouteError::UnknownSignal {
                signal_id: routed_use.signal_id,
            },
        )?;
        if !loop_ids.contains(&routed_use.consumer_loop) {
            return Err(VectorRouteError::UnknownLoop {
                loop_id: routed_use.consumer_loop,
            });
        }
        let valid = match routed_use.source {
            RoutedUseSource::Direct(VectorRegion::Control) => {
                signal.placement == Placement::Control
                    && trace.definitions.iter().any(|definition| {
                        definition.signal_id == routed_use.signal_id
                            && definition.region == VectorRegion::Control
                            && definition.value == routed_use.value
                    })
            }
            RoutedUseSource::Direct(VectorRegion::Loop(source)) => {
                source == routed_use.consumer_loop
                    && (signal.placement == Placement::Inline
                        || signal.placement == Placement::Owned(source))
                    && trace.definitions.iter().any(|definition| {
                        definition.signal_id == routed_use.signal_id
                            && definition.region == VectorRegion::Loop(source)
                            && definition.value == routed_use.value
                    })
            }
            RoutedUseSource::Transport(transport_id) => plan.transports.iter().any(|transport| {
                transport.transport_id == transport_id
                    && transport.signal_id == routed_use.signal_id
                    && transport.consumer_loop == routed_use.consumer_loop
                    && trace.transports.iter().any(|routed| {
                        routed.transport_id == transport_id && routed.load == Some(routed_use.value)
                    })
            }),
        };
        if !valid {
            return Err(VectorRouteError::InvalidUse {
                signal_id: routed_use.signal_id,
                consumer_loop: routed_use.consumer_loop,
            });
        }
    }
    Ok(())
}

fn verify_transport(
    transport: &TransportRecord,
    routed: &RoutedTransport,
    real_type: &FirType,
    store: &FirStore,
) -> Result<(), VectorRouteError> {
    let elem = transport_fir_type(transport, real_type.clone())?;
    let length =
        usize::try_from(transport.length).map_err(|_| VectorRouteError::TransportDeclaration {
            transport_id: transport.transport_id,
        })?;
    match match_fir(store, routed.declaration) {
        FirMatch::DeclareVar {
            name,
            typ: FirType::Array(actual_elem, actual_length),
            access: AccessType::Stack,
            init: None,
        } if name == transport.stable_name && *actual_elem == elem && actual_length == length => {}
        _ => {
            return Err(VectorRouteError::TransportDeclaration {
                transport_id: transport.transport_id,
            });
        }
    }
    let Some(producer_value) = routed.producer_value else {
        return Err(VectorRouteError::TransportStore {
            transport_id: transport.transport_id,
        });
    };
    match routed.store.map(|statement| match_fir(store, statement)) {
        Some(FirMatch::StoreTable {
            name,
            access: AccessType::Stack,
            index,
            value,
        }) if name == transport.stable_name
            && value == producer_value
            && is_chunk_index(store, index)
            && store.value_type(value) == Some(elem.clone()) => {}
        _ => {
            return Err(VectorRouteError::TransportStore {
                transport_id: transport.transport_id,
            });
        }
    }
    match routed.load.map(|value| (value, match_fir(store, value))) {
        Some((
            value,
            FirMatch::LoadTable {
                name,
                access: AccessType::Stack,
                index,
                typ,
            },
        )) if name == transport.stable_name
            && typ == elem
            && is_chunk_index(store, index)
            && store.value_type(value) == Some(elem) =>
        {
            Ok(())
        }
        _ => Err(VectorRouteError::TransportLoad {
            transport_id: transport.transport_id,
        }),
    }
}

fn transport_fir_type(
    transport: &TransportRecord,
    real_type: FirType,
) -> Result<FirType, VectorRouteError> {
    value_fir_type(&transport.element_type, real_type).ok_or(
        VectorRouteError::UnsupportedTupleTransport {
            signal_id: transport.signal_id,
        },
    )
}

fn value_fir_type(value_type: &ValueType, real_type: FirType) -> Option<FirType> {
    match value_type {
        ValueType::Int => Some(FirType::Int32),
        ValueType::Real => Some(real_type),
        ValueType::Tuple(_) => None,
    }
}

fn check_value_type(
    signal_id: u64,
    value_type: &ValueType,
    real_type: &FirType,
    value: FirId,
    store: &FirStore,
) -> Result<(), VectorRouteError> {
    let expected = value_fir_type(value_type, real_type.clone())
        .ok_or(VectorRouteError::UnsupportedTupleTransport { signal_id })?;
    let actual = store.value_type(value);
    if actual == Some(expected.clone()) {
        Ok(())
    } else {
        Err(VectorRouteError::ValueTypeMismatch {
            signal_id,
            expected,
            actual,
        })
    }
}

fn chunk_index(store: &mut FirStore) -> FirId {
    let mut builder = FirBuilder::new(store);
    let i0 = builder.load_var("i0", AccessType::Loop, FirType::Int32);
    let vindex = builder.load_var("vindex", AccessType::Loop, FirType::Int32);
    builder.binop(FirBinOp::Sub, i0, vindex, FirType::Int32)
}

fn is_chunk_index(store: &FirStore, value: FirId) -> bool {
    let FirMatch::BinOp {
        op: FirBinOp::Sub,
        lhs,
        rhs,
        typ: FirType::Int32,
    } = match_fir(store, value)
    else {
        return false;
    };
    matches!(
        match_fir(store, lhs),
        FirMatch::LoadVar {
            name,
            access: AccessType::Loop,
            typ: FirType::Int32,
        } if name == "i0"
    ) && matches!(
        match_fir(store, rhs),
        FirMatch::LoadVar {
            name,
            access: AccessType::Loop,
            typ: FirType::Int32,
        } if name == "vindex"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signal_fir::vector_plan::verified_vector_plan_for_test;
    use crate::signal_fir::vector_verify::{
        EpochRecord, LoopEdge, LoopRecord, Rate, SignalRecord, VecSafeWitness, Vectorability,
        WitnessKind,
    };

    fn pure_shared_plan() -> VerifiedVectorPlan {
        verified_vector_plan_for_test(VectorPlan {
            vec_size: 16,
            signals: vec![
                SignalRecord {
                    signal_id: 8,
                    value_type: ValueType::Real,
                    rate: Rate::Block,
                    vectorability: Vectorability::Vect,
                    clock_id: 0,
                    effects: vec![],
                    placement: Placement::Control,
                    duplicable: true,
                },
                SignalRecord {
                    signal_id: 9,
                    value_type: ValueType::Real,
                    rate: Rate::Samp,
                    vectorability: Vectorability::Vect,
                    clock_id: 0,
                    effects: vec![],
                    placement: Placement::Inline,
                    duplicable: true,
                },
                SignalRecord {
                    signal_id: 10,
                    value_type: ValueType::Real,
                    rate: Rate::Samp,
                    vectorability: Vectorability::Vect,
                    clock_id: 0,
                    effects: vec![],
                    placement: Placement::Owned(0),
                    duplicable: true,
                },
                SignalRecord {
                    signal_id: 11,
                    value_type: ValueType::Real,
                    rate: Rate::Samp,
                    vectorability: Vectorability::Vect,
                    clock_id: 0,
                    effects: vec![],
                    placement: Placement::Owned(1),
                    duplicable: true,
                },
            ],
            loops: vec![
                LoopRecord {
                    loop_id: 0,
                    stable_name: "loop_owns_shared".to_owned(),
                    kind: LoopKind::Vectorizable,
                    roots: vec![10],
                    epoch_id: 0,
                },
                LoopRecord {
                    loop_id: 1,
                    stable_name: "loop_consumes_shared".to_owned(),
                    kind: LoopKind::Vectorizable,
                    roots: vec![11],
                    epoch_id: 0,
                },
            ],
            epochs: vec![EpochRecord {
                epoch_id: 0,
                rank: 0,
                loops: vec![0, 1],
            }],
            transports: vec![TransportRecord {
                transport_id: 0,
                stable_name: "transport_s10_l0_l1".to_owned(),
                signal_id: 10,
                producer_loop: 0,
                consumer_loop: 1,
                element_type: ValueType::Real,
                length: 16,
            }],
            data_edges: vec![LoopEdge {
                consumer: 1,
                dependency: 0,
            }],
            effect_edges: vec![],
            vec_safe_witnesses: vec![
                VecSafeWitness {
                    loop_id: 0,
                    witness_kind: WitnessKind::Pointwise,
                },
                VecSafeWitness {
                    loop_id: 1,
                    witness_kind: WitnessKind::Pointwise,
                },
            ],
        })
    }

    fn value_for(
        signal: &super::super::vector_verify::SignalRecord,
        store: &mut FirStore,
    ) -> FirId {
        let mut builder = FirBuilder::new(store);
        match &signal.value_type {
            ValueType::Int => builder.int32(0),
            ValueType::Real => builder.float32(0.0),
            ValueType::Tuple(_) => panic!("pure fixture has no tuple"),
        }
    }

    fn define_complete<'a>(
        session: &mut VectorRouteSession<'a>,
        store: &mut FirStore,
    ) -> Vec<FirId> {
        let signals = session.plan.signals.clone();
        for signal in &signals {
            if signal.placement == Placement::Control {
                let value = value_for(signal, store);
                session
                    .define_control(signal.signal_id, value, store)
                    .unwrap();
            }
        }
        let loop_order = session
            .layout()
            .loops()
            .iter()
            .map(|region| region.loop_id)
            .collect::<Vec<_>>();
        let mut stores = Vec::new();
        let mut inline_defined = BTreeSet::new();
        for loop_id in loop_order {
            for signal in &signals {
                let should_define = match signal.placement {
                    Placement::Owned(owner) => owner == loop_id,
                    Placement::Inline => inline_defined.insert(signal.signal_id),
                    Placement::Control => false,
                };
                if should_define {
                    let value = value_for(signal, store);
                    stores.extend(
                        session
                            .define_in_loop(loop_id, signal.signal_id, value, store)
                            .unwrap(),
                    );
                }
            }
        }
        stores
    }

    #[test]
    fn pure_plan_routes_all_preplanned_transports_and_verifies() {
        let verified_plan = pure_shared_plan();
        let routes = verified_plan.plan().transports.clone();
        let mut store = FirStore::new();
        let (mut session, declarations) = VectorRouteSession::new(
            &verified_plan,
            SchedulingStrategy::DepthFirst,
            FirType::Float32,
            &mut store,
        )
        .unwrap();
        assert_eq!(declarations.len(), routes.len());
        let stores = define_complete(&mut session, &mut store);
        assert_eq!(stores.len(), routes.len());
        for transport in &routes {
            assert!(matches!(
                session
                    .resolve_in_loop(transport.consumer_loop, transport.signal_id, &mut store)
                    .unwrap(),
                RouteResolution::Value(_)
            ));
        }
        let routed = session.finish(&store).unwrap();
        assert_eq!(routed.trace().transports().len(), routes.len());
        assert!(
            routed
                .trace()
                .transports()
                .iter()
                .all(|transport| transport.store.is_some() && transport.load.is_some())
        );
    }

    #[test]
    fn inline_cache_is_loop_local_and_control_cache_is_ancestor_visible() {
        let verified_plan = pure_shared_plan();
        let mut store = FirStore::new();
        let (mut session, _) = VectorRouteSession::new(
            &verified_plan,
            SchedulingStrategy::DepthFirst,
            FirType::Float32,
            &mut store,
        )
        .unwrap();
        let loops = session
            .layout()
            .loops()
            .iter()
            .map(|region| region.loop_id)
            .collect::<Vec<_>>();
        let inline = session
            .plan
            .signals
            .iter()
            .find(|signal| signal.placement == Placement::Inline)
            .unwrap()
            .clone();
        let value = value_for(&inline, &mut store);
        session
            .define_in_loop(loops[0], inline.signal_id, value, &mut store)
            .unwrap();
        assert_eq!(
            session
                .resolve_in_loop(loops[0], inline.signal_id, &mut store)
                .unwrap(),
            RouteResolution::Value(value)
        );
        assert_eq!(
            session
                .resolve_in_loop(loops[1], inline.signal_id, &mut store)
                .unwrap(),
            RouteResolution::NeedsInlineLowering
        );

        let control = session
            .plan
            .signals
            .iter()
            .find(|signal| signal.placement == Placement::Control)
            .unwrap()
            .clone();
        let control_value = value_for(&control, &mut store);
        session
            .define_control(control.signal_id, control_value, &store)
            .unwrap();
        for loop_id in loops {
            assert_eq!(
                session
                    .resolve_in_loop(loop_id, control.signal_id, &mut store)
                    .unwrap(),
                RouteResolution::Value(control_value)
            );
        }
    }

    #[test]
    fn owned_definition_in_a_sibling_region_is_rejected() {
        let verified_plan = pure_shared_plan();
        let mut store = FirStore::new();
        let (mut session, _) = VectorRouteSession::new(
            &verified_plan,
            SchedulingStrategy::DepthFirst,
            FirType::Float32,
            &mut store,
        )
        .unwrap();
        let owned = session
            .plan
            .signals
            .iter()
            .find_map(|signal| match signal.placement {
                Placement::Owned(owner) => Some((signal.clone(), owner)),
                _ => None,
            })
            .unwrap();
        let sibling = session
            .layout()
            .loops()
            .iter()
            .map(|region| region.loop_id)
            .find(|loop_id| *loop_id != owned.1)
            .unwrap();
        let value = value_for(&owned.0, &mut store);
        assert!(matches!(
            session.define_in_loop(sibling, owned.0.signal_id, value, &mut store),
            Err(VectorRouteError::WrongRegion { .. })
        ));
    }

    #[test]
    fn finish_fails_closed_when_a_planned_transport_has_no_load() {
        let verified_plan = pure_shared_plan();
        let expected = verified_plan.plan().transports[0].transport_id;
        let mut store = FirStore::new();
        let (mut session, _) = VectorRouteSession::new(
            &verified_plan,
            SchedulingStrategy::DepthFirst,
            FirType::Float32,
            &mut store,
        )
        .unwrap();
        define_complete(&mut session, &mut store);
        assert_eq!(
            session.finish(&store),
            Err(VectorRouteError::TransportLoad {
                transport_id: expected
            })
        );
    }

    #[test]
    fn independent_verifier_rejects_a_mutated_transport_load() {
        let verified_plan = pure_shared_plan();
        let expected = verified_plan.plan().transports[0].transport_id;
        let mut store = FirStore::new();
        let (mut session, _) = VectorRouteSession::new(
            &verified_plan,
            SchedulingStrategy::DepthFirst,
            FirType::Float32,
            &mut store,
        )
        .unwrap();
        define_complete(&mut session, &mut store);
        for transport in &verified_plan.plan().transports {
            session
                .resolve_in_loop(transport.consumer_loop, transport.signal_id, &mut store)
                .unwrap();
        }
        let mut trace = session.finish(&store).unwrap().into_trace();
        trace.transports[0].load = Some(FirBuilder::new(&mut store).int32(0));
        assert_eq!(
            verify_routed_fir(verified_plan.plan(), &trace, &FirType::Float32, &store),
            Err(VectorRouteError::TransportLoad {
                transport_id: expected
            })
        );
    }

    #[test]
    fn independent_verifier_rejects_a_mutated_direct_use() {
        let verified_plan = pure_shared_plan();
        let mut store = FirStore::new();
        let (mut session, _) = VectorRouteSession::new(
            &verified_plan,
            SchedulingStrategy::DepthFirst,
            FirType::Float32,
            &mut store,
        )
        .unwrap();
        define_complete(&mut session, &mut store);
        assert!(matches!(
            session.resolve_in_loop(0, 10, &mut store).unwrap(),
            RouteResolution::Value(_)
        ));
        session.resolve_in_loop(1, 10, &mut store).unwrap();
        let mut trace = session.finish(&store).unwrap().into_trace();
        trace.uses[0].value = FirBuilder::new(&mut store).float32(1.0);
        assert_eq!(
            verify_routed_fir(verified_plan.plan(), &trace, &FirType::Float32, &store),
            Err(VectorRouteError::InvalidUse {
                signal_id: 10,
                consumer_loop: 0
            })
        );
    }

    #[test]
    fn all_strategies_change_only_region_order_not_transport_identity() {
        let verified_plan = pure_shared_plan();
        let expected = verified_plan
            .plan()
            .transports
            .iter()
            .map(|transport| {
                (
                    transport.transport_id,
                    transport.stable_name.clone(),
                    transport.signal_id,
                )
            })
            .collect::<Vec<_>>();
        for strategy in [
            SchedulingStrategy::DepthFirst,
            SchedulingStrategy::BreadthFirst,
            SchedulingStrategy::Special,
            SchedulingStrategy::ReverseBreadthFirst,
        ] {
            let mut store = FirStore::new();
            let (session, declarations) =
                VectorRouteSession::new(&verified_plan, strategy, FirType::Float32, &mut store)
                    .unwrap();
            assert_eq!(session.layout().strategy(), strategy);
            assert_eq!(declarations.len(), expected.len());
            assert_eq!(
                session
                    .plan
                    .transports
                    .iter()
                    .map(|transport| (
                        transport.transport_id,
                        transport.stable_name.clone(),
                        transport.signal_id
                    ))
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
}
