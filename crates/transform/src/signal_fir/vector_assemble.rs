//! Verified P6.3b assembly of vector state phases and clock islands.
//!
//! # C++ provenance and adaptation
//! State words follow `DAGInstructionsCompiler::generateDlineLoop` and
//! `generateDelayAccess` in `compiler/generator/dag_instructions_compiler.cpp`:
//! short delays copy `_perm` history into `_tmp`, execute the chunk, then copy
//! the tail back; long delays advance a masked ring index before the chunk and
//! save the chunk count afterwards. Recursive projections are captured into
//! stack temporaries before any projection storage is updated, preserving the
//! simultaneous `sigRec`/`sigProj` step.
//!
//! Clock guards follow the scalar `SignalFIRLowerer` implementation of OD, US,
//! and DS. Unlike the C++ compiler's mutable `CodeLoop` tree, Rust assembles an
//! immutable, checked artifact from the accepted P4.4/P5/P6.1/P6.2 artifacts.
//! This module remains additive: final module lifecycle placement and backend
//! activation are later phases.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fir::{AccessType, FirBinOp, FirBuilder, FirId, FirMatch, FirStore, FirType, match_fir};

use super::vector_clock_ad::{
    ClockGuard, ClockIsland, ClockTransportMode, VerifiedVectorClockAdPlan,
};
use super::vector_route::{RoutedDefinition, VectorRegion, VerifiedRoutedFir};
use super::vector_state::{
    DelayTransition, RecursionTransition, VectorDelayStorage, VectorStateAction,
    VerifiedVectorStatePlan,
};
use super::vector_verify::{ValueType, VectorPlan};

/// Current canonical P6.3b assembly schema.
pub const VECTOR_FIR_ASSEMBLY_VERSION: u32 = 1;

/// Already-lowered non-state statements owned by one checked P4 loop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorLoopFirInput {
    pub loop_id: u64,
    pub statements: Vec<FirId>,
}

/// Concrete statement implementing one accepted P6.1 action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorStateFirAction {
    pub action: VectorStateAction,
    pub statement: FirId,
}

/// One loop body after state-phase materialization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssembledVectorLoop {
    pub loop_id: u64,
    pub pre: Vec<VectorStateFirAction>,
    pub exec: Vec<FirId>,
    pub exec_actions: Vec<VectorStateFirAction>,
    pub post: Vec<VectorStateFirAction>,
    /// Complete outer-chunk execution: `pre; for i0; post`.
    pub chunk_statement: FirId,
    /// One serial iteration used when this loop is nested below a clock guard.
    pub iteration_statement: FirId,
}

/// One nested serial clock domain after guard construction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssembledClockIsland {
    pub domain_id: u64,
    pub parent_domain: Option<u64>,
    pub guard: ClockGuard,
    pub nested_loop_ids: Vec<u64>,
    /// P6.2 `IslandScalar` declarations whose lifetime begins below this guard.
    pub local_declarations: Vec<FirId>,
    pub statement: FirId,
}

/// Finite FIR assembly accepted before final module placement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorFirAssembly {
    pub schema_version: u32,
    pub local_declarations: Vec<FirId>,
    pub state_declarations: Vec<FirId>,
    pub clear_statements: Vec<FirId>,
    pub loops: Vec<AssembledVectorLoop>,
    pub islands: Vec<AssembledClockIsland>,
    pub top_level_statement: FirId,
}

/// Opaque evidence that the P6.3b checker accepted an assembly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedVectorFirAssembly {
    assembly: VectorFirAssembly,
    vector_plan: VectorPlan,
}

impl VerifiedVectorFirAssembly {
    #[must_use]
    pub fn assembly(&self) -> &VectorFirAssembly {
        &self.assembly
    }

    #[must_use]
    pub fn vector_plan(&self) -> &VectorPlan {
        &self.vector_plan
    }

    #[must_use]
    pub fn into_assembly(self) -> VectorFirAssembly {
        self.assembly
    }
}

/// Typed producer/checker failure at the P6.3b boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum VectorFirAssemblyError {
    PlanMismatch {
        artifact: &'static str,
    },
    ReverseAdRequiresScalar {
        signal_id: u64,
    },
    LoopInputCoverage {
        loop_id: u64,
    },
    DuplicateLoopInput {
        loop_id: u64,
    },
    MissingDefinition {
        signal_id: u64,
        loop_id: u64,
    },
    MissingRecursionProjection {
        group: u64,
        index: u64,
    },
    LoopStateCoverage {
        loop_id: u64,
    },
    ClockLoopOwnership {
        loop_id: u64,
    },
    MissingClockValue {
        domain_id: u64,
        signal_id: u64,
    },
    MissingClockParent {
        domain_id: u64,
        parent_id: u64,
    },
    ArithmeticOverflow {
        what: &'static str,
        value: u64,
    },
    UnsupportedValueType {
        signal_id: u64,
    },
    DeclarationShape {
        name: String,
    },
    ActionShape {
        loop_id: u64,
        action: VectorStateAction,
    },
    IslandShape {
        domain_id: u64,
    },
    TopLevelShape,
}

impl fmt::Display for VectorFirAssemblyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlanMismatch { artifact } => {
                write!(f, "{artifact} does not belong to the routed vector plan")
            }
            Self::ReverseAdRequiresScalar { signal_id } => write!(
                f,
                "signal {signal_id} requires the certified scalar reverse-AD window"
            ),
            Self::LoopInputCoverage { loop_id } => {
                write!(f, "loop FIR inputs do not cover loop {loop_id} exactly")
            }
            Self::DuplicateLoopInput { loop_id } => {
                write!(f, "loop FIR input {loop_id} is duplicated")
            }
            Self::MissingDefinition { signal_id, loop_id } => write!(
                f,
                "stateful signal {signal_id} has no routed definition in loop {loop_id}"
            ),
            Self::MissingRecursionProjection { group, index } => write!(
                f,
                "recursion group {group} projection {index} has no routed definition"
            ),
            Self::LoopStateCoverage { loop_id } => {
                write!(f, "assembled state actions disagree for loop {loop_id}")
            }
            Self::ClockLoopOwnership { loop_id } => {
                write!(f, "loop {loop_id} belongs to more than one clock island")
            }
            Self::MissingClockValue {
                domain_id,
                signal_id,
            } => write!(
                f,
                "clock island {domain_id} cannot resolve clock signal {signal_id}"
            ),
            Self::MissingClockParent {
                domain_id,
                parent_id,
            } => write!(
                f,
                "clock island {domain_id} references missing parent {parent_id}"
            ),
            Self::ArithmeticOverflow { what, value } => {
                write!(f, "{what} value {value} does not fit FIR i32 arithmetic")
            }
            Self::UnsupportedValueType { signal_id } => {
                write!(f, "signal {signal_id} has tuple-valued state storage")
            }
            Self::DeclarationShape { name } => {
                write!(f, "assembled declaration {name} has an invalid FIR shape")
            }
            Self::ActionShape { loop_id, action } => {
                write!(f, "loop {loop_id} action {action:?} has invalid FIR")
            }
            Self::IslandShape { domain_id } => {
                write!(f, "clock island {domain_id} has an invalid FIR guard")
            }
            Self::TopLevelShape => write!(f, "assembled top-level FIR is not a block"),
        }
    }
}

impl std::error::Error for VectorFirAssemblyError {}

/// Materializes checked P6.1 phases and P6.2 serial islands into concrete FIR.
pub fn assemble_vector_fir(
    routed: &VerifiedRoutedFir,
    state_plan: Option<&VerifiedVectorStatePlan>,
    clock_plan: Option<&VerifiedVectorClockAdPlan>,
    inputs: &[VectorLoopFirInput],
    real_type: FirType,
    store: &mut FirStore,
) -> Result<VerifiedVectorFirAssembly, VectorFirAssemblyError> {
    if let Some(state) = state_plan {
        require_same_plan(routed.plan(), state.vector_plan(), "P6.1 state plan")?;
    }
    if let Some(clock) = clock_plan {
        require_same_plan(routed.plan(), clock.vector_plan(), "P6.2 clock/AD plan")?;
        if let Some(fallback) = clock.plan().reverse_ad_fallbacks.first() {
            return Err(VectorFirAssemblyError::ReverseAdRequiresScalar {
                signal_id: fallback.signal_id,
            });
        }
    }

    let input_map = check_loop_inputs(routed, inputs)?;
    let transport_declarations = inspect_transport_declarations(routed, store)?;
    let assembly = {
        let mut builder = FirBuilder::new(store);
        let mut local_declarations = Vec::new();
        let mut state_declarations = Vec::new();
        let mut clear_statements = Vec::new();
        let mut island_declarations = BTreeMap::new();
        classify_transport_declarations(
            &transport_declarations,
            &mut builder,
            &mut local_declarations,
            &mut state_declarations,
            &mut clear_statements,
            &mut island_declarations,
        )?;

        let mut delays = BTreeMap::new();
        let mut recursions = BTreeMap::new();
        if let Some(state) = state_plan {
            materialize_state_storage(
                state,
                real_type.clone(),
                &mut builder,
                &mut state_declarations,
                &mut local_declarations,
                &mut clear_statements,
            )?;
            delays.extend(
                state
                    .plan()
                    .delays
                    .iter()
                    .map(|delay| (delay.signal_id, delay)),
            );
            recursions.extend(
                state
                    .plan()
                    .recursions
                    .iter()
                    .map(|recursion| (recursion.group, recursion)),
            );
        }

        let definitions = definition_map(routed.trace().definitions());
        let signal_types = routed
            .plan()
            .signals
            .iter()
            .map(|signal| (signal.signal_id, signal.value_type.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut loops = Vec::with_capacity(routed.layout().loops().len());
        for region in routed.layout().loops() {
            let phases = state_plan.and_then(|state| {
                state
                    .plan()
                    .loops
                    .iter()
                    .find(|phases| phases.loop_id == region.loop_id)
            });
            loops.push(materialize_loop(
                region.loop_id,
                input_map[&region.loop_id],
                phases,
                &delays,
                &recursions,
                &definitions,
                &signal_types,
                real_type.clone(),
                &mut builder,
            )?);
        }

        let islands = materialize_clock_islands(
            routed,
            clock_plan,
            &loops,
            &definitions,
            &island_declarations,
            &mut builder,
            &mut state_declarations,
            &mut clear_statements,
        )?;
        let top_level_statement = materialize_top_level(routed, &loops, &islands, &mut builder)?;
        VectorFirAssembly {
            schema_version: VECTOR_FIR_ASSEMBLY_VERSION,
            local_declarations,
            state_declarations,
            clear_statements,
            loops,
            islands,
            top_level_statement,
        }
    };
    verify_vector_fir_assembly(routed, state_plan, clock_plan, &assembly, store)?;
    Ok(VerifiedVectorFirAssembly {
        assembly,
        vector_plan: routed.plan().clone(),
    })
}

/// Independently validates P6.3b coverage and the concrete FIR word shapes.
pub fn verify_vector_fir_assembly(
    routed: &VerifiedRoutedFir,
    state_plan: Option<&VerifiedVectorStatePlan>,
    clock_plan: Option<&VerifiedVectorClockAdPlan>,
    assembly: &VectorFirAssembly,
    store: &FirStore,
) -> Result<(), VectorFirAssemblyError> {
    if assembly.schema_version != VECTOR_FIR_ASSEMBLY_VERSION {
        return Err(VectorFirAssemblyError::TopLevelShape);
    }
    if !matches!(
        match_fir(store, assembly.top_level_statement),
        FirMatch::Block(_)
    ) {
        return Err(VectorFirAssemblyError::TopLevelShape);
    }
    let expected_loops = routed
        .layout()
        .loops()
        .iter()
        .map(|region| region.loop_id)
        .collect::<Vec<_>>();
    let actual_loops = assembly
        .loops
        .iter()
        .map(|region| region.loop_id)
        .collect::<Vec<_>>();
    if actual_loops != expected_loops {
        let loop_id = expected_loops
            .iter()
            .zip(&actual_loops)
            .find_map(|(expected, actual)| (expected != actual).then_some(*expected))
            .or_else(|| expected_loops.last().copied())
            .unwrap_or(0);
        return Err(VectorFirAssemblyError::LoopInputCoverage { loop_id });
    }

    for assembled in &assembly.loops {
        let expected = state_plan
            .and_then(|state| {
                state
                    .plan()
                    .loops
                    .iter()
                    .find(|phases| phases.loop_id == assembled.loop_id)
            })
            .map(|phases| {
                phases
                    .pre
                    .iter()
                    .chain(&phases.exec)
                    .chain(&phases.post)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let actual = assembled
            .pre
            .iter()
            .chain(&assembled.exec_actions)
            .chain(&assembled.post)
            .map(|action| action.action.clone())
            .collect::<Vec<_>>();
        if expected != actual {
            return Err(VectorFirAssemblyError::LoopStateCoverage {
                loop_id: assembled.loop_id,
            });
        }
        for action in assembled
            .pre
            .iter()
            .chain(&assembled.exec_actions)
            .chain(&assembled.post)
        {
            verify_action_shape(assembled.loop_id, action, state_plan, store)?;
        }
        if !matches!(
            match_fir(store, assembled.chunk_statement),
            FirMatch::Block(_)
        ) || !matches!(
            match_fir(store, assembled.iteration_statement),
            FirMatch::Block(_)
        ) {
            return Err(VectorFirAssemblyError::LoopStateCoverage {
                loop_id: assembled.loop_id,
            });
        }
    }

    let expected_islands = clock_plan
        .map(|clock| clock.plan().clock_islands.as_slice())
        .unwrap_or(&[]);
    if assembly.islands.len() != expected_islands.len() {
        return Err(VectorFirAssemblyError::IslandShape { domain_id: 0 });
    }
    let assembled_loop_by_id = assembly
        .loops
        .iter()
        .map(|loop_| (loop_.loop_id, loop_))
        .collect::<BTreeMap<_, _>>();
    let assembled_island_by_id = assembly
        .islands
        .iter()
        .map(|island| (island.domain_id, island))
        .collect::<BTreeMap<_, _>>();
    for (actual, expected) in assembly.islands.iter().zip(expected_islands) {
        let local_declarations = expected_island_declarations(routed, expected.domain_id);
        let mut expected_body = local_declarations.clone();
        expected_body.extend(
            expected
                .nested_loop_ids
                .iter()
                .map(|loop_id| assembled_loop_by_id[loop_id].iteration_statement),
        );
        expected_body.extend(
            expected_islands
                .iter()
                .filter(|child| child.parent_domain == Some(expected.domain_id))
                .map(|child| assembled_island_by_id[&child.domain_id].statement),
        );
        if actual.domain_id != expected.domain_id
            || actual.parent_domain != expected.parent_domain
            || actual.guard != expected.guard
            || actual.nested_loop_ids != expected.nested_loop_ids
            || actual.local_declarations != local_declarations
            || !guard_shape_matches(expected, actual.statement, &expected_body, store)
        {
            return Err(VectorFirAssemblyError::IslandShape {
                domain_id: expected.domain_id,
            });
        }
    }
    Ok(())
}

fn require_same_plan(
    routed: &VectorPlan,
    artifact: &VectorPlan,
    name: &'static str,
) -> Result<(), VectorFirAssemblyError> {
    if routed == artifact {
        Ok(())
    } else {
        Err(VectorFirAssemblyError::PlanMismatch { artifact: name })
    }
}

fn check_loop_inputs<'a>(
    routed: &VerifiedRoutedFir,
    inputs: &'a [VectorLoopFirInput],
) -> Result<BTreeMap<u64, &'a [FirId]>, VectorFirAssemblyError> {
    let expected = routed
        .layout()
        .loops()
        .iter()
        .map(|region| region.loop_id)
        .collect::<BTreeSet<_>>();
    let mut result = BTreeMap::new();
    for input in inputs {
        if result
            .insert(input.loop_id, input.statements.as_slice())
            .is_some()
        {
            return Err(VectorFirAssemblyError::DuplicateLoopInput {
                loop_id: input.loop_id,
            });
        }
    }
    for loop_id in expected {
        if !result.contains_key(&loop_id) {
            return Err(VectorFirAssemblyError::LoopInputCoverage { loop_id });
        }
    }
    if let Some(extra) = result.keys().find(|loop_id| {
        !routed
            .layout()
            .loops()
            .iter()
            .any(|r| r.loop_id == **loop_id)
    }) {
        return Err(VectorFirAssemblyError::LoopInputCoverage { loop_id: *extra });
    }
    Ok(result)
}

struct TransportDeclaration {
    mode: ClockTransportMode,
    declaration: FirId,
    held: Option<(String, FirType)>,
}

fn inspect_transport_declarations(
    routed: &VerifiedRoutedFir,
    store: &FirStore,
) -> Result<Vec<TransportDeclaration>, VectorFirAssemblyError> {
    routed
        .trace()
        .transports()
        .iter()
        .map(|transport| {
            let held = if matches!(transport.mode, ClockTransportMode::HeldOutput { .. }) {
                match match_fir(store, transport.declaration) {
                    FirMatch::DeclareVar {
                        name,
                        typ,
                        access: AccessType::Struct,
                        init: None,
                    } => Some((name, typ)),
                    _ => {
                        return Err(VectorFirAssemblyError::DeclarationShape {
                            name: format!("transport_{}", transport.transport_id),
                        });
                    }
                }
            } else {
                None
            };
            Ok(TransportDeclaration {
                mode: transport.mode,
                declaration: transport.declaration,
                held,
            })
        })
        .collect()
}

fn classify_transport_declarations(
    transports: &[TransportDeclaration],
    builder: &mut FirBuilder<'_>,
    local: &mut Vec<FirId>,
    state: &mut Vec<FirId>,
    clear: &mut Vec<FirId>,
    island_declarations: &mut BTreeMap<u64, Vec<FirId>>,
) -> Result<(), VectorFirAssemblyError> {
    for transport in transports {
        match transport.mode {
            ClockTransportMode::OuterChunk => {
                local.push(transport.declaration);
            }
            ClockTransportMode::IslandScalar { domain_id } => {
                island_declarations
                    .entry(domain_id)
                    .or_default()
                    .push(transport.declaration);
            }
            ClockTransportMode::HeldOutput { .. } => {
                let (name, typ) = transport
                    .held
                    .as_ref()
                    .expect("held transport was inspected before FIR construction");
                state.push(transport.declaration);
                let zero = zero_value(builder, typ);
                clear.push(builder.store_var(name, AccessType::Struct, zero));
            }
        }
    }
    Ok(())
}

fn materialize_state_storage(
    state_plan: &VerifiedVectorStatePlan,
    real_type: FirType,
    builder: &mut FirBuilder<'_>,
    state: &mut Vec<FirId>,
    local: &mut Vec<FirId>,
    clear: &mut Vec<FirId>,
) -> Result<(), VectorFirAssemblyError> {
    for delay in &state_plan.plan().delays {
        let typ = state_fir_type(delay, real_type.clone())?;
        match &delay.storage {
            VectorDelayStorage::Copy {
                temporary_name,
                permanent_name,
                history_length,
                temporary_length,
            } => {
                let temporary_length_u64 = *temporary_length;
                let history_length_u64 = *history_length;
                let temporary_length = usize_value("temporary delay length", temporary_length_u64)?;
                let history_length = usize_value("permanent delay length", history_length_u64)?;
                local.push(builder.declare_var(
                    temporary_name,
                    FirType::Array(Box::new(typ.clone()), temporary_length),
                    AccessType::Stack,
                    None,
                ));
                state.push(builder.declare_var(
                    permanent_name,
                    FirType::Array(Box::new(typ.clone()), history_length),
                    AccessType::Struct,
                    None,
                ));
                clear.push(clear_table(
                    builder,
                    permanent_name,
                    AccessType::Struct,
                    &typ,
                    history_length_u64,
                )?);
            }
            VectorDelayStorage::Ring {
                buffer_name,
                index_name,
                index_save_name,
                capacity,
                ..
            } => {
                let capacity_u64 = *capacity;
                let capacity = usize_value("ring capacity", capacity_u64)?;
                state.push(builder.declare_var(
                    buffer_name,
                    FirType::Array(Box::new(typ.clone()), capacity),
                    AccessType::Struct,
                    None,
                ));
                state.push(builder.declare_var(
                    index_name,
                    FirType::Int32,
                    AccessType::Struct,
                    None,
                ));
                state.push(builder.declare_var(
                    index_save_name,
                    FirType::Int32,
                    AccessType::Struct,
                    None,
                ));
                clear.push(clear_table(
                    builder,
                    buffer_name,
                    AccessType::Struct,
                    &typ,
                    capacity_u64,
                )?);
                let zero = builder.int32(0);
                clear.push(builder.store_var(index_name, AccessType::Struct, zero));
                clear.push(builder.store_var(index_save_name, AccessType::Struct, zero));
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn materialize_loop(
    loop_id: u64,
    inputs: &[FirId],
    phases: Option<&super::vector_state::LoopStatePhases>,
    delays: &BTreeMap<u64, &DelayTransition>,
    recursions: &BTreeMap<u64, &RecursionTransition>,
    definitions: &BTreeMap<(VectorRegion, u64), FirId>,
    signal_types: &BTreeMap<u64, ValueType>,
    real_type: FirType,
    builder: &mut FirBuilder<'_>,
) -> Result<AssembledVectorLoop, VectorFirAssemblyError> {
    let mut recursion_values = BTreeMap::new();
    let mut pre = Vec::new();
    let mut exec_actions = Vec::new();
    let mut post = Vec::new();
    if let Some(phases) = phases {
        for action in &phases.pre {
            pre.push(materialize_action(
                loop_id,
                action,
                delays,
                recursions,
                definitions,
                signal_types,
                &mut recursion_values,
                real_type.clone(),
                builder,
            )?);
        }
        for action in &phases.exec {
            exec_actions.push(materialize_action(
                loop_id,
                action,
                delays,
                recursions,
                definitions,
                signal_types,
                &mut recursion_values,
                real_type.clone(),
                builder,
            )?);
        }
        for action in &phases.post {
            post.push(materialize_action(
                loop_id,
                action,
                delays,
                recursions,
                definitions,
                signal_types,
                &mut recursion_values,
                real_type.clone(),
                builder,
            )?);
        }
    }
    let mut exec = inputs.to_vec();
    exec.extend(exec_actions.iter().map(|action| action.statement));
    let iteration_statement = builder.block(&exec);
    let sample_loop = sample_loop(builder, iteration_statement);
    let mut chunk = pre
        .iter()
        .map(|action| action.statement)
        .collect::<Vec<_>>();
    chunk.push(sample_loop);
    chunk.extend(post.iter().map(|action| action.statement));
    let chunk_statement = builder.block(&chunk);
    Ok(AssembledVectorLoop {
        loop_id,
        pre,
        exec,
        exec_actions,
        post,
        chunk_statement,
        iteration_statement,
    })
}

#[allow(clippy::too_many_arguments)]
fn materialize_action(
    loop_id: u64,
    action: &VectorStateAction,
    delays: &BTreeMap<u64, &DelayTransition>,
    recursions: &BTreeMap<u64, &RecursionTransition>,
    definitions: &BTreeMap<(VectorRegion, u64), FirId>,
    signal_types: &BTreeMap<u64, ValueType>,
    recursion_values: &mut BTreeMap<u64, FirId>,
    real_type: FirType,
    builder: &mut FirBuilder<'_>,
) -> Result<VectorStateFirAction, VectorFirAssemblyError> {
    let statement = match action {
        VectorStateAction::DelayCopyIn { signal_id } => {
            let delay = delays[signal_id];
            let VectorDelayStorage::Copy {
                temporary_name,
                permanent_name,
                history_length,
                ..
            } = &delay.storage
            else {
                return Err(VectorFirAssemblyError::ActionShape {
                    loop_id,
                    action: action.clone(),
                });
            };
            let typ = state_fir_type(delay, real_type)?;
            let index = builder.load_var("vdelay_copy", AccessType::Loop, FirType::Int32);
            let value = builder.load_table(permanent_name, AccessType::Struct, index, typ);
            let store = builder.store_table(temporary_name, AccessType::Stack, index, value);
            let upper = fir_i32(builder, "copy history", *history_length)?;
            builder.simple_for_loop("vdelay_copy", upper, store, false)
        }
        VectorStateAction::DelayRingAdvance { signal_id } => {
            let delay = delays[signal_id];
            let VectorDelayStorage::Ring {
                index_name,
                index_save_name,
                mask,
                ..
            } = &delay.storage
            else {
                return Err(VectorFirAssemblyError::ActionShape {
                    loop_id,
                    action: action.clone(),
                });
            };
            let index = builder.load_var(index_name, AccessType::Struct, FirType::Int32);
            let saved = builder.load_var(index_save_name, AccessType::Struct, FirType::Int32);
            let added = builder.binop(FirBinOp::Add, index, saved, FirType::Int32);
            let mask = fir_i32(builder, "ring mask", *mask)?;
            let masked = builder.binop(FirBinOp::And, added, mask, FirType::Int32);
            builder.store_var(index_name, AccessType::Struct, masked)
        }
        VectorStateAction::RecursionStep { group } => {
            let recursion = recursions[group];
            let mut declarations = Vec::with_capacity(recursion.projections.len());
            for projection in &recursion.projections {
                let value = projection
                    .signal_ids
                    .iter()
                    .find_map(|signal_id| {
                        definitions
                            .get(&(VectorRegion::Loop(loop_id), *signal_id))
                            .copied()
                    })
                    .ok_or(VectorFirAssemblyError::MissingRecursionProjection {
                        group: *group,
                        index: projection.index,
                    })?;
                let signal_id = projection.signal_ids[0];
                let typ = value_type_to_fir(
                    signal_types.get(&signal_id).ok_or(
                        VectorFirAssemblyError::MissingRecursionProjection {
                            group: *group,
                            index: projection.index,
                        },
                    )?,
                    real_type.clone(),
                    signal_id,
                )
                .map_err(|_| {
                    VectorFirAssemblyError::MissingRecursionProjection {
                        group: *group,
                        index: projection.index,
                    }
                })?;
                let name = recursion_name(*group, projection.index);
                declarations.push(builder.declare_var(
                    &name,
                    typ.clone(),
                    AccessType::Stack,
                    Some(value),
                ));
                let load = builder.load_var(name, AccessType::Stack, typ);
                for signal_id in &projection.signal_ids {
                    recursion_values.insert(*signal_id, load);
                }
            }
            builder.block(&declarations)
        }
        VectorStateAction::DelayWrite { signal_id } => {
            let delay = delays[signal_id];
            let value = recursion_values.get(signal_id).copied().or_else(|| {
                definitions
                    .get(&(VectorRegion::Loop(loop_id), *signal_id))
                    .copied()
            });
            let value = value.ok_or(VectorFirAssemblyError::MissingDefinition {
                signal_id: *signal_id,
                loop_id,
            })?;
            let local = local_index(builder);
            match &delay.storage {
                VectorDelayStorage::Copy {
                    temporary_name,
                    history_length,
                    ..
                } => {
                    let history = fir_i32(builder, "copy history", *history_length)?;
                    let index = builder.binop(FirBinOp::Add, history, local, FirType::Int32);
                    builder.store_table(temporary_name, AccessType::Stack, index, value)
                }
                VectorDelayStorage::Ring {
                    buffer_name,
                    index_name,
                    mask,
                    ..
                } => {
                    let index = builder.load_var(index_name, AccessType::Struct, FirType::Int32);
                    let added = builder.binop(FirBinOp::Add, index, local, FirType::Int32);
                    let mask = fir_i32(builder, "ring mask", *mask)?;
                    let masked = builder.binop(FirBinOp::And, added, mask, FirType::Int32);
                    builder.store_table(buffer_name, AccessType::Struct, masked, value)
                }
            }
        }
        VectorStateAction::DelayCopyOut { signal_id } => {
            let delay = delays[signal_id];
            let VectorDelayStorage::Copy {
                temporary_name,
                permanent_name,
                history_length,
                ..
            } = &delay.storage
            else {
                return Err(VectorFirAssemblyError::ActionShape {
                    loop_id,
                    action: action.clone(),
                });
            };
            let typ = state_fir_type(delay, real_type)?;
            let index = builder.load_var("vdelay_copy", AccessType::Loop, FirType::Int32);
            let count = builder.load_var("vcount", AccessType::FunArgs, FirType::Int32);
            let source_index = builder.binop(FirBinOp::Add, count, index, FirType::Int32);
            let value = builder.load_table(temporary_name, AccessType::Stack, source_index, typ);
            let store = builder.store_table(permanent_name, AccessType::Struct, index, value);
            let upper = fir_i32(builder, "copy history", *history_length)?;
            builder.simple_for_loop("vdelay_copy", upper, store, false)
        }
        VectorStateAction::DelayRingSaveAdvance { signal_id } => {
            let delay = delays[signal_id];
            let VectorDelayStorage::Ring {
                index_save_name, ..
            } = &delay.storage
            else {
                return Err(VectorFirAssemblyError::ActionShape {
                    loop_id,
                    action: action.clone(),
                });
            };
            let count = builder.load_var("vcount", AccessType::FunArgs, FirType::Int32);
            builder.store_var(index_save_name, AccessType::Struct, count)
        }
    };
    Ok(VectorStateFirAction {
        action: action.clone(),
        statement,
    })
}

#[allow(clippy::too_many_arguments)]
fn materialize_clock_islands(
    routed: &VerifiedRoutedFir,
    clock_plan: Option<&VerifiedVectorClockAdPlan>,
    loops: &[AssembledVectorLoop],
    definitions: &BTreeMap<(VectorRegion, u64), FirId>,
    island_declarations: &BTreeMap<u64, Vec<FirId>>,
    builder: &mut FirBuilder<'_>,
    state_declarations: &mut Vec<FirId>,
    clear_statements: &mut Vec<FirId>,
) -> Result<Vec<AssembledClockIsland>, VectorFirAssemblyError> {
    let Some(clock_plan) = clock_plan else {
        return Ok(Vec::new());
    };
    let mut owner = BTreeMap::new();
    for island in &clock_plan.plan().clock_islands {
        for loop_id in &island.nested_loop_ids {
            if owner.insert(*loop_id, island.domain_id).is_some() {
                return Err(VectorFirAssemblyError::ClockLoopOwnership { loop_id: *loop_id });
            }
        }
    }
    let loop_by_id = loops
        .iter()
        .map(|assembled| (assembled.loop_id, assembled))
        .collect::<BTreeMap<_, _>>();
    let island_by_id = clock_plan
        .plan()
        .clock_islands
        .iter()
        .map(|island| (island.domain_id, island))
        .collect::<BTreeMap<_, _>>();
    for island in &clock_plan.plan().clock_islands {
        if let Some(parent) = island.parent_domain
            && !island_by_id.contains_key(&parent)
        {
            return Err(VectorFirAssemblyError::MissingClockParent {
                domain_id: island.domain_id,
                parent_id: parent,
            });
        }
    }
    let mut statements = BTreeMap::new();
    let mut pending = clock_plan.plan().clock_islands.len();
    while statements.len() < pending {
        let before = statements.len();
        for island in &clock_plan.plan().clock_islands {
            if statements.contains_key(&island.domain_id) {
                continue;
            }
            let children = clock_plan
                .plan()
                .clock_islands
                .iter()
                .filter(|child| child.parent_domain == Some(island.domain_id))
                .collect::<Vec<_>>();
            if children
                .iter()
                .any(|child| !statements.contains_key(&child.domain_id))
            {
                continue;
            }
            let local_declarations = island_declarations
                .get(&island.domain_id)
                .cloned()
                .unwrap_or_default();
            let mut body = local_declarations.clone();
            body.extend(
                island
                    .nested_loop_ids
                    .iter()
                    .map(|loop_id| loop_by_id[loop_id].iteration_statement),
            );
            body.extend(children.iter().map(|child| statements[&child.domain_id]));
            let body = builder.block(&body);
            let clock = resolve_clock_value(routed, definitions, island)?;
            let guarded = build_guard(
                island,
                clock,
                body,
                builder,
                state_declarations,
                clear_statements,
            );
            statements.insert(island.domain_id, guarded);
        }
        if statements.len() == before {
            let island = clock_plan
                .plan()
                .clock_islands
                .iter()
                .find(|island| !statements.contains_key(&island.domain_id))
                .expect("pending island count is nonzero");
            return Err(VectorFirAssemblyError::MissingClockParent {
                domain_id: island.domain_id,
                parent_id: island.parent_domain.unwrap_or(island.domain_id),
            });
        }
        pending = clock_plan.plan().clock_islands.len();
    }
    Ok(clock_plan
        .plan()
        .clock_islands
        .iter()
        .map(|island| AssembledClockIsland {
            domain_id: island.domain_id,
            parent_domain: island.parent_domain,
            guard: island.guard,
            nested_loop_ids: island.nested_loop_ids.clone(),
            local_declarations: island_declarations
                .get(&island.domain_id)
                .cloned()
                .unwrap_or_default(),
            statement: statements[&island.domain_id],
        })
        .collect())
}

fn materialize_top_level(
    routed: &VerifiedRoutedFir,
    loops: &[AssembledVectorLoop],
    islands: &[AssembledClockIsland],
    builder: &mut FirBuilder<'_>,
) -> Result<FirId, VectorFirAssemblyError> {
    let owned = islands
        .iter()
        .flat_map(|island| island.nested_loop_ids.iter().copied())
        .collect::<BTreeSet<_>>();
    let roots = islands
        .iter()
        .filter(|island| island.parent_domain.is_none())
        .map(|island| (island.nested_loop_ids.first().copied(), island.statement))
        .collect::<Vec<_>>();
    let loop_by_id = loops
        .iter()
        .map(|assembled| (assembled.loop_id, assembled))
        .collect::<BTreeMap<_, _>>();
    let mut body = Vec::new();
    for region in routed.layout().loops() {
        if !owned.contains(&region.loop_id) {
            body.push(loop_by_id[&region.loop_id].chunk_statement);
        }
        for (first_loop, statement) in &roots {
            if *first_loop == Some(region.loop_id) {
                body.push(sample_loop(builder, *statement));
            }
        }
    }
    if body.is_empty() && !roots.is_empty() {
        body.extend(
            roots
                .into_iter()
                .map(|(_, statement)| sample_loop(builder, statement)),
        );
    }
    Ok(builder.block(&body))
}

fn resolve_clock_value(
    routed: &VerifiedRoutedFir,
    definitions: &BTreeMap<(VectorRegion, u64), FirId>,
    island: &ClockIsland,
) -> Result<FirId, VectorFirAssemblyError> {
    routed
        .trace()
        .uses()
        .iter()
        .find(|use_| {
            use_.signal_id == island.clock_signal_id
                && use_.consumer_loop == island.boundary_loop_id
        })
        .map(|use_| use_.value)
        .or_else(|| {
            definitions
                .get(&(VectorRegion::Control, island.clock_signal_id))
                .copied()
        })
        .or_else(|| {
            definitions
                .get(&(
                    VectorRegion::Loop(island.boundary_loop_id),
                    island.clock_signal_id,
                ))
                .copied()
        })
        .ok_or(VectorFirAssemblyError::MissingClockValue {
            domain_id: island.domain_id,
            signal_id: island.clock_signal_id,
        })
}

fn build_guard(
    island: &ClockIsland,
    clock: FirId,
    body: FirId,
    builder: &mut FirBuilder<'_>,
    state_declarations: &mut Vec<FirId>,
    clear_statements: &mut Vec<FirId>,
) -> FirId {
    match island.guard {
        ClockGuard::BooleanOnDemand => {
            let zero = builder.int32(0);
            let cond = builder.binop(FirBinOp::Ne, clock, zero, FirType::Bool);
            builder.if_(cond, body, None)
        }
        ClockGuard::CountedOnDemand | ClockGuard::CountedUpsampling => builder.simple_for_loop(
            format!("vclock_d{}_fire", island.domain_id),
            clock,
            body,
            false,
        ),
        ClockGuard::DownsampleModulo => {
            let name = format!("vclock_d{}_counter", island.domain_id);
            state_declarations.push(builder.declare_var(
                &name,
                FirType::Int32,
                AccessType::Struct,
                None,
            ));
            let zero = builder.int32(0);
            clear_statements.push(builder.store_var(&name, AccessType::Struct, zero));
            let counter = builder.load_var(&name, AccessType::Struct, FirType::Int32);
            let zero = builder.int32(0);
            let cond = builder.binop(FirBinOp::Eq, counter, zero, FirType::Bool);
            let guarded = builder.if_(cond, body, None);
            let counter = builder.load_var(&name, AccessType::Struct, FirType::Int32);
            let one = builder.int32(1);
            let next = builder.binop(FirBinOp::Add, counter, one, FirType::Int32);
            let modulo = builder.binop(FirBinOp::Rem, next, clock, FirType::Int32);
            let update = builder.store_var(name, AccessType::Struct, modulo);
            builder.block(&[guarded, update])
        }
    }
}

fn verify_action_shape(
    loop_id: u64,
    action: &VectorStateFirAction,
    state_plan: Option<&VerifiedVectorStatePlan>,
    store: &FirStore,
) -> Result<(), VectorFirAssemblyError> {
    let state = state_plan.expect("actions require a state plan");
    let valid = match &action.action {
        VectorStateAction::DelayCopyIn { signal_id } => {
            let delay = find_delay(state, *signal_id);
            match &delay.storage {
                VectorDelayStorage::Copy {
                    temporary_name,
                    permanent_name,
                    history_length,
                    ..
                } => simple_copy_matches(
                    action.statement,
                    temporary_name,
                    AccessType::Stack,
                    permanent_name,
                    AccessType::Struct,
                    *history_length,
                    store,
                ),
                VectorDelayStorage::Ring { .. } => false,
            }
        }
        VectorStateAction::DelayRingAdvance { signal_id } => {
            let delay = find_delay(state, *signal_id);
            matches!(
                (&delay.storage, match_fir(store, action.statement)),
                (
                    VectorDelayStorage::Ring { index_name, .. },
                    FirMatch::StoreVar { name, access: AccessType::Struct, .. }
                ) if *index_name == name
            )
        }
        VectorStateAction::RecursionStep { group } => {
            let expected = state
                .plan()
                .recursions
                .iter()
                .find(|recursion| recursion.group == *group)
                .expect("verified recursion action has a transition");
            matches!(match_fir(store, action.statement), FirMatch::Block(body) if body.len() == expected.projections.len() && body.iter().zip(&expected.projections).all(|(id, projection)| matches!(match_fir(store, *id), FirMatch::DeclareVar { name, access: AccessType::Stack, init: Some(_), .. } if name == recursion_name(*group, projection.index))))
        }
        VectorStateAction::DelayWrite { signal_id } => {
            let delay = find_delay(state, *signal_id);
            match (&delay.storage, match_fir(store, action.statement)) {
                (
                    VectorDelayStorage::Copy { temporary_name, .. },
                    FirMatch::StoreTable {
                        name,
                        access: AccessType::Stack,
                        ..
                    },
                ) => *temporary_name == name,
                (
                    VectorDelayStorage::Ring { buffer_name, .. },
                    FirMatch::StoreTable {
                        name,
                        access: AccessType::Struct,
                        ..
                    },
                ) => *buffer_name == name,
                _ => false,
            }
        }
        VectorStateAction::DelayCopyOut { signal_id } => {
            let delay = find_delay(state, *signal_id);
            match &delay.storage {
                VectorDelayStorage::Copy {
                    temporary_name,
                    permanent_name,
                    history_length,
                    ..
                } => simple_copy_matches(
                    action.statement,
                    permanent_name,
                    AccessType::Struct,
                    temporary_name,
                    AccessType::Stack,
                    *history_length,
                    store,
                ),
                VectorDelayStorage::Ring { .. } => false,
            }
        }
        VectorStateAction::DelayRingSaveAdvance { signal_id } => {
            let delay = find_delay(state, *signal_id);
            matches!(
                (&delay.storage, match_fir(store, action.statement)),
                (
                    VectorDelayStorage::Ring { index_save_name, .. },
                    FirMatch::StoreVar { name, access: AccessType::Struct, .. }
                ) if *index_save_name == name
            )
        }
    };
    if valid {
        Ok(())
    } else {
        Err(VectorFirAssemblyError::ActionShape {
            loop_id,
            action: action.action.clone(),
        })
    }
}

fn simple_copy_matches(
    statement: FirId,
    target_name: &str,
    target_access: AccessType,
    source_name: &str,
    source_access: AccessType,
    history_length: u64,
    store: &FirStore,
) -> bool {
    let FirMatch::SimpleForLoop {
        upper,
        body,
        is_reverse: false,
        ..
    } = match_fir(store, statement)
    else {
        return false;
    };
    let Ok(history_length) = i32::try_from(history_length) else {
        return false;
    };
    if !matches!(match_fir(store, upper), FirMatch::Int32 { value, .. } if value == history_length)
    {
        return false;
    }
    let FirMatch::StoreTable {
        name,
        access,
        value,
        ..
    } = match_fir(store, body)
    else {
        return false;
    };
    name == target_name
        && access == target_access
        && matches!(match_fir(store, value), FirMatch::LoadTable { name, access, .. } if name == source_name && access == source_access)
}

fn guard_shape_matches(
    island: &ClockIsland,
    statement: FirId,
    expected_body: &[FirId],
    store: &FirStore,
) -> bool {
    let body = match island.guard {
        ClockGuard::BooleanOnDemand => match match_fir(store, statement) {
            FirMatch::If {
                cond,
                then_block,
                else_block: None,
            } if matches!(
                match_fir(store, cond),
                FirMatch::BinOp {
                    op: FirBinOp::Ne,
                    ..
                }
            ) =>
            {
                then_block
            }
            _ => return false,
        },
        ClockGuard::CountedOnDemand | ClockGuard::CountedUpsampling => {
            match match_fir(store, statement) {
                FirMatch::SimpleForLoop {
                    var,
                    body,
                    is_reverse: false,
                    ..
                } if var == format!("vclock_d{}_fire", island.domain_id) => body,
                _ => return false,
            }
        }
        ClockGuard::DownsampleModulo => match match_fir(store, statement) {
            FirMatch::Block(words) if words.len() == 2 => {
                let expected_counter = format!("vclock_d{}_counter", island.domain_id);
                if !matches!(match_fir(store, words[1]), FirMatch::StoreVar { name, access: AccessType::Struct, .. } if name == expected_counter)
                {
                    return false;
                }
                match match_fir(store, words[0]) {
                    FirMatch::If {
                        cond,
                        then_block,
                        else_block: None,
                    } if matches!(
                        match_fir(store, cond),
                        FirMatch::BinOp {
                            op: FirBinOp::Eq,
                            ..
                        }
                    ) =>
                    {
                        then_block
                    }
                    _ => return false,
                }
            }
            _ => return false,
        },
    };
    matches!(match_fir(store, body), FirMatch::Block(words) if words == expected_body)
}

fn expected_island_declarations(routed: &VerifiedRoutedFir, domain_id: u64) -> Vec<FirId> {
    routed
        .trace()
        .transports()
        .iter()
        .filter_map(|transport| {
            (transport.mode == ClockTransportMode::IslandScalar { domain_id })
                .then_some(transport.declaration)
        })
        .collect()
}

fn find_delay(state: &VerifiedVectorStatePlan, signal_id: u64) -> &DelayTransition {
    state
        .plan()
        .delays
        .iter()
        .find(|delay| delay.signal_id == signal_id)
        .expect("verified state action references a verified delay")
}

fn definition_map(definitions: &[RoutedDefinition]) -> BTreeMap<(VectorRegion, u64), FirId> {
    definitions
        .iter()
        .map(|definition| ((definition.region, definition.signal_id), definition.value))
        .collect()
}

fn state_fir_type(
    delay: &DelayTransition,
    real_type: FirType,
) -> Result<FirType, VectorFirAssemblyError> {
    match delay.value_type {
        ValueType::Int => Ok(FirType::Int32),
        ValueType::Real => Ok(real_type),
        ValueType::Tuple(_) => Err(VectorFirAssemblyError::UnsupportedValueType {
            signal_id: delay.signal_id,
        }),
    }
}

fn value_type_to_fir(
    value_type: &ValueType,
    real_type: FirType,
    signal_id: u64,
) -> Result<FirType, VectorFirAssemblyError> {
    match value_type {
        ValueType::Int => Ok(FirType::Int32),
        ValueType::Real => Ok(real_type),
        ValueType::Tuple(_) => Err(VectorFirAssemblyError::UnsupportedValueType { signal_id }),
    }
}

fn zero_value(builder: &mut FirBuilder<'_>, typ: &FirType) -> FirId {
    match typ {
        FirType::Float32 | FirType::FaustFloat => builder.float32(0.0),
        FirType::Float64 => builder.float64(0.0),
        FirType::Int64 => builder.int64(0),
        FirType::Bool => builder.bool_(false),
        _ => builder.int32(0),
    }
}

fn clear_table(
    builder: &mut FirBuilder<'_>,
    name: &str,
    access: AccessType,
    typ: &FirType,
    length: u64,
) -> Result<FirId, VectorFirAssemblyError> {
    let index = builder.load_var("vclear", AccessType::Loop, FirType::Int32);
    let zero = zero_value(builder, typ);
    let store = builder.store_table(name, access, index, zero);
    let upper = fir_i32(builder, "clear length", length)?;
    Ok(builder.simple_for_loop("vclear", upper, store, false))
}

fn sample_loop(builder: &mut FirBuilder<'_>, body: FirId) -> FirId {
    let start = builder.load_var("vindex", AccessType::FunArgs, FirType::Int32);
    let count = builder.load_var("vcount", AccessType::FunArgs, FirType::Int32);
    let end = builder.binop(FirBinOp::Add, start, count, FirType::Int32);
    let one = builder.int32(1);
    builder.for_loop("i0", start, end, one, body, false)
}

fn local_index(builder: &mut FirBuilder<'_>) -> FirId {
    let index = builder.load_var("i0", AccessType::Loop, FirType::Int32);
    let base = builder.load_var("vindex", AccessType::FunArgs, FirType::Int32);
    builder.binop(FirBinOp::Sub, index, base, FirType::Int32)
}

fn recursion_name(group: u64, index: u64) -> String {
    format!("vrec_g{group}_p{index}_next")
}

fn fir_i32(
    builder: &mut FirBuilder<'_>,
    what: &'static str,
    value: u64,
) -> Result<FirId, VectorFirAssemblyError> {
    let value = i32::try_from(value)
        .map_err(|_| VectorFirAssemblyError::ArithmeticOverflow { what, value })?;
    Ok(builder.int32(value))
}

fn usize_value(what: &'static str, value: u64) -> Result<usize, VectorFirAssemblyError> {
    usize::try_from(value).map_err(|_| VectorFirAssemblyError::ArithmeticOverflow { what, value })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schedule::SchedulingStrategy;
    use crate::signal_fir::vector_clock_ad::{
        ForwardAdPolicy, VECTOR_CLOCK_AD_PLAN_VERSION, VectorClockAdPlan,
        verified_vector_clock_ad_plan_for_test,
    };
    use crate::signal_fir::vector_plan::verified_vector_plan_for_test;
    use crate::signal_fir::vector_route::{RouteResolution, VectorRouteSession};
    use crate::signal_fir::vector_state::{
        DelayTransition, LoopStatePhases, RecursionProjectionTransition, RecursionTransition,
        VECTOR_STATE_PLAN_VERSION, VectorStatePlan, verified_vector_state_plan_for_test,
    };
    use crate::signal_fir::vector_verify::{
        EpochRecord, LoopEdge, LoopKind, LoopRecord, Placement, Rate, SignalRecord,
        TransportRecord, VecSafeWitness, VectorPlan, Vectorability, WitnessKind,
    };

    fn state_vector_plan() -> super::super::vector_plan::VerifiedVectorPlan {
        verified_vector_plan_for_test(VectorPlan {
            vec_size: 8,
            signals: vec![
                SignalRecord {
                    signal_id: 11,
                    value_type: ValueType::Real,
                    rate: Rate::Samp,
                    vectorability: Vectorability::Scal,
                    clock_id: 0,
                    effects: vec![],
                    placement: Placement::Owned(0),
                    duplicable: true,
                },
                SignalRecord {
                    signal_id: 12,
                    value_type: ValueType::Real,
                    rate: Rate::Samp,
                    vectorability: Vectorability::Scal,
                    clock_id: 0,
                    effects: vec![],
                    placement: Placement::Owned(0),
                    duplicable: true,
                },
            ],
            loops: vec![LoopRecord {
                loop_id: 0,
                stable_name: "recursive_1".to_owned(),
                kind: LoopKind::Recursive(1),
                roots: vec![11, 12],
                epoch_id: 0,
            }],
            epochs: vec![EpochRecord {
                epoch_id: 0,
                rank: 0,
                loops: vec![0],
            }],
            transports: vec![],
            data_edges: vec![],
            effect_edges: vec![],
            vec_safe_witnesses: vec![],
        })
    }

    fn state_plan(
        vector: &super::super::vector_plan::VerifiedVectorPlan,
    ) -> VerifiedVectorStatePlan {
        verified_vector_state_plan_for_test(
            VectorStatePlan {
                schema_version: VECTOR_STATE_PLAN_VERSION,
                vec_size: 8,
                max_copy_delay: 4,
                loops: vec![LoopStatePhases {
                    loop_id: 0,
                    pre: vec![
                        VectorStateAction::DelayCopyIn { signal_id: 11 },
                        VectorStateAction::DelayRingAdvance { signal_id: 12 },
                    ],
                    exec: vec![
                        VectorStateAction::RecursionStep { group: 1 },
                        VectorStateAction::DelayWrite { signal_id: 11 },
                        VectorStateAction::DelayWrite { signal_id: 12 },
                    ],
                    post: vec![
                        VectorStateAction::DelayCopyOut { signal_id: 11 },
                        VectorStateAction::DelayRingSaveAdvance { signal_id: 12 },
                    ],
                }],
                delays: vec![
                    DelayTransition {
                        signal_id: 11,
                        loop_id: 0,
                        value_type: ValueType::Real,
                        max_delay: 3,
                        storage: VectorDelayStorage::Copy {
                            temporary_name: "fVec11_tmp".to_owned(),
                            permanent_name: "fVec11_perm".to_owned(),
                            history_length: 4,
                            temporary_length: 12,
                        },
                    },
                    DelayTransition {
                        signal_id: 12,
                        loop_id: 0,
                        value_type: ValueType::Real,
                        max_delay: 5,
                        storage: VectorDelayStorage::Ring {
                            buffer_name: "fVec12".to_owned(),
                            index_name: "fVec12_idx".to_owned(),
                            index_save_name: "fVec12_idx_save".to_owned(),
                            capacity: 16,
                            mask: 15,
                        },
                    },
                ],
                recursions: vec![RecursionTransition {
                    group: 1,
                    loop_id: 0,
                    projections: vec![RecursionProjectionTransition {
                        index: 0,
                        signal_ids: vec![11],
                    }],
                }],
            },
            vector,
        )
    }

    #[test]
    fn materializes_copy_ring_and_simultaneous_recursion_words() {
        let vector = state_vector_plan();
        let state = state_plan(&vector);
        let mut store = FirStore::new();
        let value11 = FirBuilder::new(&mut store).float32(0.25);
        let value12 = FirBuilder::new(&mut store).float32(0.5);
        let (mut route, _) = VectorRouteSession::new(
            &vector,
            SchedulingStrategy::DepthFirst,
            FirType::Float32,
            &mut store,
        )
        .expect("route");
        route
            .define_in_loop(0, 11, value11, &mut store)
            .expect("define 11");
        route
            .define_in_loop(0, 12, value12, &mut store)
            .expect("define 12");
        let routed = route.finish(&store).expect("finish route");
        let input = VectorLoopFirInput {
            loop_id: 0,
            statements: vec![],
        };
        let verified = assemble_vector_fir(
            &routed,
            Some(&state),
            None,
            &[input],
            FirType::Float32,
            &mut store,
        )
        .expect("assemble state");
        let assembled = verified.assembly();
        assert_eq!(assembled.loops[0].pre.len(), 2);
        assert_eq!(assembled.loops[0].exec_actions.len(), 3);
        assert_eq!(assembled.loops[0].post.len(), 2);
        assert!(assembled.state_declarations.len() >= 4);
        assert!(matches!(
            match_fir(&store, assembled.loops[0].exec_actions[0].statement),
            FirMatch::Block(body) if body.len() == 1
        ));

        let mut forged = assembled.clone();
        forged.loops[0].exec_actions[1].statement = FirBuilder::new(&mut store).int32(0);
        assert!(matches!(
            verify_vector_fir_assembly(&routed, Some(&state), None, &forged, &store),
            Err(VectorFirAssemblyError::ActionShape {
                action: VectorStateAction::DelayWrite { signal_id: 11 },
                ..
            })
        ));
    }

    fn clock_vector_plan() -> super::super::vector_plan::VerifiedVectorPlan {
        verified_vector_plan_for_test(VectorPlan {
            vec_size: 8,
            signals: vec![
                SignalRecord {
                    signal_id: 1,
                    value_type: ValueType::Int,
                    rate: Rate::Block,
                    vectorability: Vectorability::Vect,
                    clock_id: 0,
                    effects: vec![],
                    placement: Placement::Control,
                    duplicable: true,
                },
                SignalRecord {
                    signal_id: 10,
                    value_type: ValueType::Real,
                    rate: Rate::Samp,
                    vectorability: Vectorability::Scal,
                    clock_id: 7,
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
                SignalRecord {
                    signal_id: 12,
                    value_type: ValueType::Real,
                    rate: Rate::Samp,
                    vectorability: Vectorability::Scal,
                    clock_id: 7,
                    effects: vec![],
                    placement: Placement::Owned(2),
                    duplicable: true,
                },
            ],
            loops: vec![
                LoopRecord {
                    loop_id: 0,
                    stable_name: "island_7".to_owned(),
                    kind: LoopKind::Island(7),
                    roots: vec![10],
                    epoch_id: 0,
                },
                LoopRecord {
                    loop_id: 1,
                    stable_name: "outer".to_owned(),
                    kind: LoopKind::Vectorizable,
                    roots: vec![11],
                    epoch_id: 0,
                },
                LoopRecord {
                    loop_id: 2,
                    stable_name: "island_7_consumer".to_owned(),
                    kind: LoopKind::Island(7),
                    roots: vec![12],
                    epoch_id: 0,
                },
            ],
            epochs: vec![EpochRecord {
                epoch_id: 0,
                rank: 0,
                loops: vec![0, 1, 2],
            }],
            transports: vec![
                TransportRecord {
                    transport_id: 0,
                    stable_name: "island_s10".to_owned(),
                    signal_id: 10,
                    producer_loop: 0,
                    consumer_loop: 2,
                    element_type: ValueType::Real,
                    length: 8,
                },
                TransportRecord {
                    transport_id: 1,
                    stable_name: "held_s12".to_owned(),
                    signal_id: 12,
                    producer_loop: 2,
                    consumer_loop: 1,
                    element_type: ValueType::Real,
                    length: 8,
                },
            ],
            data_edges: vec![
                LoopEdge {
                    consumer: 1,
                    dependency: 2,
                },
                LoopEdge {
                    consumer: 2,
                    dependency: 0,
                },
            ],
            effect_edges: vec![],
            vec_safe_witnesses: vec![VecSafeWitness {
                loop_id: 1,
                witness_kind: WitnessKind::Pointwise,
            }],
        })
    }

    #[test]
    fn nests_clock_loop_and_materializes_held_transport_lifetime() {
        let vector = clock_vector_plan();
        let clock = verified_vector_clock_ad_plan_for_test(
            VectorClockAdPlan {
                schema_version: VECTOR_CLOCK_AD_PLAN_VERSION,
                vec_size: 8,
                clock_islands: vec![ClockIsland {
                    domain_id: 7,
                    parent_domain: None,
                    kind: propagate::ClockDomainKind::OnDemand,
                    clock_signal_id: 1,
                    wrapper_signal_id: 10,
                    boundary_loop_id: 0,
                    guard: ClockGuard::CountedOnDemand,
                    signal_ids: vec![10],
                    clock_state_signal_ids: vec![],
                    nested_loop_ids: vec![0, 2],
                }],
                transports: vec![
                    super::super::vector_clock_ad::ClockTransportPolicy {
                        transport_id: 0,
                        mode: ClockTransportMode::IslandScalar { domain_id: 7 },
                    },
                    super::super::vector_clock_ad::ClockTransportPolicy {
                        transport_id: 1,
                        mode: ClockTransportMode::HeldOutput { domain_id: 7 },
                    },
                ],
                forward_ad: ForwardAdPolicy::ExpandedSignalGraph,
                reverse_ad_fallbacks: vec![],
            },
            &vector,
        );
        let mut store = FirStore::new();
        let clock_value = FirBuilder::new(&mut store).int32(2);
        let value = FirBuilder::new(&mut store).float32(0.5);
        let (mut route, _) = VectorRouteSession::new_with_clock_plan(
            &vector,
            &clock,
            SchedulingStrategy::DepthFirst,
            FirType::Float32,
            &mut store,
        )
        .expect("clock route");
        route
            .define_control(1, clock_value, &store)
            .expect("clock definition");
        let island_stores = route
            .define_in_loop(0, 10, value, &mut store)
            .expect("island definition");
        let island_value = match route
            .resolve_in_loop(2, 10, &mut store)
            .expect("island scalar load")
        {
            RouteResolution::Value(value) => value,
            RouteResolution::NeedsInlineLowering => panic!("unexpected inline"),
        };
        let held_stores = route
            .define_in_loop(2, 12, island_value, &mut store)
            .expect("held definition");
        let loaded = match route.resolve_in_loop(1, 12, &mut store).expect("held load") {
            RouteResolution::Value(value) => value,
            RouteResolution::NeedsInlineLowering => panic!("unexpected inline"),
        };
        route
            .define_in_loop(1, 11, loaded, &mut store)
            .expect("outer definition");
        let routed = route.finish(&store).expect("finish route");
        let inputs = vec![
            VectorLoopFirInput {
                loop_id: 0,
                statements: island_stores,
            },
            VectorLoopFirInput {
                loop_id: 1,
                statements: vec![FirBuilder::new(&mut store).drop_(loaded)],
            },
            VectorLoopFirInput {
                loop_id: 2,
                statements: held_stores,
            },
        ];
        let verified = assemble_vector_fir(
            &routed,
            None,
            Some(&clock),
            &inputs,
            FirType::Float32,
            &mut store,
        )
        .expect("assemble clock");
        let assembly = verified.assembly();
        assert_eq!(assembly.islands.len(), 1);
        assert_eq!(assembly.islands[0].local_declarations.len(), 1);
        assert!(assembly.local_declarations.is_empty());
        assert!(matches!(
            match_fir(&store, assembly.islands[0].statement),
            FirMatch::SimpleForLoop { .. }
        ));
        assert_eq!(assembly.state_declarations.len(), 1);
        assert_eq!(assembly.clear_statements.len(), 1);
        assert!(matches!(
            match_fir(&store, assembly.state_declarations[0]),
            FirMatch::DeclareVar {
                access: AccessType::Struct,
                ..
            }
        ));

        let mut forged = assembly.clone();
        forged.islands[0].statement = FirBuilder::new(&mut store).int32(0);
        assert!(matches!(
            verify_vector_fir_assembly(&routed, None, Some(&clock), &forged, &store),
            Err(VectorFirAssemblyError::IslandShape { domain_id: 7 })
        ));
    }
}
