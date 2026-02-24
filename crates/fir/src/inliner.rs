//! FIR function inliner scaffolding (Milestones 1–2).
//!
//! # Scope
//! This module currently implements:
//! - function indexing from a FIR `Module`,
//! - call graph extraction,
//! - SCC detection,
//! - simple callee size metrics,
//! - candidate selection decisions (legality/profitability pre-checks).
//! - hygienic FIR subtree cloning with local-variable renaming (future inlining substrate).
//!
//! It still does **not** inline `FunCall` nodes yet. Callsite rewriting and
//! statement splicing will be layered on top of these analysis/clone utilities
//! in later milestones.
//!
//! # Source provenance (C++)
//! - `compiler/generator/fir_to_fir.cpp` (`FunctionInliner`, `FunctionCallInliner`)
//! - `compiler/generator/fir_to_fir.hh`
//!
//! # Public API mapping status
//! - `adapted`: the C++ code exposes inlining as visitor-side rewriting helpers.
//!   Rust starts with a module-level analysis API to make legality/profitability
//!   decisions explicit and testable before implementing rewriting.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::{AccessType, FirBuilder, FirId, FirMatch, FirStore, NamedType, SliderRange, match_fir};

const RESERVED_DSP_API_FUNCTIONS: &[&str] = &[
    "classInit",
    "instanceInit",
    "init",
    "getSampleRate",
    "instanceConstants",
    "instanceResetUserInterface",
    "instanceClear",
    "buildUserInterface",
    "compute",
    "metadata",
];

/// Analysis-time configuration for FIR function inlining candidate selection.
///
/// These options are used by [`analyze_fir_inliner`] to classify module
/// functions as "eligible" or "skipped" candidates without rewriting code yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirInlineOptions {
    /// Master switch for candidate selection.
    ///
    /// When `false`, all functions are analyzed but marked non-eligible with a
    /// `Disabled` reason.
    pub enabled: bool,
    /// Inline only functions explicitly flagged `is_inline`.
    pub inline_marked_only: bool,
    /// Maximum allowed unique FIR nodes reachable from a callee body.
    ///
    /// Functions whose body exceeds this threshold are marked as skipped with
    /// `TooLarge`.
    pub max_callee_nodes: usize,
    /// Reserved for later rewrite phase (depth-limited inlining).
    ///
    /// The analysis pass records the option for future compatibility but does
    /// not consume it yet.
    pub max_inline_depth: usize,
    /// Reserved for later rewrite phase (caller expansion budget).
    ///
    /// The analysis pass records the option for future compatibility but does
    /// not consume it yet.
    pub max_expansion_factor: usize,
    /// Whether recursive SCCs may be considered eligible.
    ///
    /// Default is `false`; recursive/self-recursive functions are skipped.
    pub allow_recursive: bool,
    /// Reserved for later rewrite phase integration with checker-driven
    /// validation after each transformed function.
    pub verify_after_each_function: bool,
}

impl Default for FirInlineOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            inline_marked_only: false,
            max_callee_nodes: 64,
            max_inline_depth: 8,
            max_expansion_factor: 8,
            allow_recursive: false,
            verify_after_each_function: false,
        }
    }
}

/// Stable analysis error for malformed FIR module inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirInlineAnalysisError {
    /// Root node is not `FirMatch::Module`.
    RootNotModule,
    /// One module section is not a `Block`.
    InvalidModuleSection { section: &'static str, node: FirId },
    /// Duplicate function names make call graph resolution ambiguous.
    DuplicateFunctionName {
        name: String,
        first: FirId,
        second: FirId,
    },
}

impl std::fmt::Display for FirInlineAnalysisError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RootNotModule => write!(f, "FIR inliner analysis requires a Module root"),
            Self::InvalidModuleSection { section, node } => write!(
                f,
                "FIR inliner analysis expected '{section}' to be a Block (node={})",
                node.as_u32()
            ),
            Self::DuplicateFunctionName {
                name,
                first,
                second,
            } => write!(
                f,
                "duplicate FIR function name '{name}' (nodes {} and {})",
                first.as_u32(),
                second.as_u32()
            ),
        }
    }
}

impl std::error::Error for FirInlineAnalysisError {}

/// Location of a function declaration inside a FIR module.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FirFunctionSection {
    /// Function declared in `Module.globals` (often extern prototypes).
    Globals,
    /// Function declared in `Module.declarations`.
    Declarations,
}

/// Per-function summary extracted during module analysis.
#[derive(Clone, Debug, PartialEq)]
pub struct FirFunctionSummary {
    /// Function name.
    pub name: String,
    /// FIR node id of the `DeclareFun`.
    pub decl_id: FirId,
    /// Origin module section.
    pub section: FirFunctionSection,
    /// Parameter list copied from `DeclareFun`.
    pub params: Vec<NamedType>,
    /// Function has a body and is therefore rewriteable in principle.
    pub has_body: bool,
    /// Original `DeclareFun.is_inline` flag.
    pub is_inline: bool,
    /// Number of unique FIR nodes reachable from the body (0 for prototypes).
    pub body_node_count: usize,
    /// Unique direct callees referenced by `FunCall` nodes in the body.
    pub direct_callees: BTreeSet<String>,
    /// SCC index in [`FirInlineAnalysis::sccs`].
    pub scc_index: usize,
    /// `true` for self-recursive or mutually recursive SCC members.
    pub is_recursive_scc: bool,
    /// `true` when the name matches a reserved DSP API function.
    pub is_reserved_api: bool,
}

/// Why a function was excluded from inlining candidates in analysis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirInlineSkipReason {
    /// Global pass is disabled in options.
    Disabled,
    /// `inline_marked_only=true` and `DeclareFun.is_inline=false`.
    NotMarkedInline,
    /// Function has no body (`DeclareFun` prototype / extern declaration).
    PrototypeOnly,
    /// Default policy excludes DSP API entry points from inlining.
    ReservedApi,
    /// Function belongs to a recursive SCC and recursion is disabled.
    RecursiveScc,
    /// Body node count exceeded `max_callee_nodes`.
    TooLarge { body_nodes: usize, max: usize },
}

/// Candidate decision produced for each analyzed function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirInlineCandidateDecision {
    /// Function name the decision applies to.
    pub function: String,
    /// Whether the function is eligible as an inlining callee in this phase.
    pub eligible: bool,
    /// Explanatory skip reasons (empty iff `eligible=true`).
    pub reasons: Vec<FirInlineSkipReason>,
}

/// One strongly-connected component in the function call graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirInlineScc {
    /// Stable index used by [`FirFunctionSummary::scc_index`].
    pub index: usize,
    /// Functions in deterministic order.
    pub functions: Vec<String>,
    /// `true` if the SCC is recursive (`len>1` or self-edge).
    pub is_recursive: bool,
}

/// Result of Milestone-1 FIR inliner analysis.
#[derive(Clone, Debug, PartialEq)]
pub struct FirInlineAnalysis {
    /// Options used to compute candidate decisions.
    pub options: FirInlineOptions,
    /// Per-function summaries keyed by function name.
    pub functions: BTreeMap<String, FirFunctionSummary>,
    /// Direct call graph adjacency list (callee names filtered to known functions).
    pub call_graph: BTreeMap<String, BTreeSet<String>>,
    /// SCC decomposition of the call graph.
    pub sccs: Vec<FirInlineScc>,
    /// Callee candidate decisions keyed by function name.
    pub candidate_decisions: BTreeMap<String, FirInlineCandidateDecision>,
}

impl FirInlineAnalysis {
    /// Returns `true` when `function` is an eligible inlining callee.
    #[must_use]
    pub fn is_callee_candidate(&self, function: &str) -> bool {
        self.candidate_decisions
            .get(function)
            .map(|d| d.eligible)
            .unwrap_or(false)
    }
}

#[derive(Clone, Debug)]
struct RawFunctionInfo {
    decl_id: FirId,
    section: FirFunctionSection,
    params: Vec<NamedType>,
    body: Option<FirId>,
    is_inline: bool,
}

#[derive(Default)]
struct BodyMetrics {
    node_count: usize,
    direct_callees: BTreeSet<String>,
}

/// Analyze a FIR module for future function-inlining transformations.
///
/// This function performs **no rewrites**. It builds a function index and call
/// graph, computes SCCs, collects body-size metrics, and classifies callee
/// candidates using [`FirInlineOptions`].
///
/// # Errors
/// Returns [`FirInlineAnalysisError`] when the input is not a valid FIR module
/// shape for analysis (non-`Module` root, non-block sections, duplicate
/// function names).
pub fn analyze_fir_inliner(
    store: &FirStore,
    module: FirId,
    options: &FirInlineOptions,
) -> Result<FirInlineAnalysis, FirInlineAnalysisError> {
    let FirMatch::Module {
        globals,
        declarations,
        ..
    } = match_fir(store, module)
    else {
        return Err(FirInlineAnalysisError::RootNotModule);
    };

    let mut raw_functions: BTreeMap<String, RawFunctionInfo> = BTreeMap::new();
    collect_functions_from_section(
        store,
        globals,
        FirFunctionSection::Globals,
        &mut raw_functions,
    )?;
    collect_functions_from_section(
        store,
        declarations,
        FirFunctionSection::Declarations,
        &mut raw_functions,
    )?;

    let mut call_graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut body_metrics: BTreeMap<String, BodyMetrics> = BTreeMap::new();
    for (name, info) in &raw_functions {
        let metrics = if let Some(body) = info.body {
            collect_body_metrics(store, body)
        } else {
            BodyMetrics::default()
        };
        let known_callees = metrics
            .direct_callees
            .iter()
            .filter(|callee| raw_functions.contains_key(callee.as_str()))
            .cloned()
            .collect::<BTreeSet<_>>();
        call_graph.insert(name.clone(), known_callees);
        body_metrics.insert(name.clone(), metrics);
    }

    let (sccs, scc_index_by_name) = tarjan_sccs(&call_graph);

    let mut functions = BTreeMap::new();
    let mut candidate_decisions = BTreeMap::new();
    for (name, info) in &raw_functions {
        let metrics = body_metrics
            .get(name)
            .expect("metrics collected for all funcs");
        let scc_index = *scc_index_by_name
            .get(name)
            .expect("scc index computed for all funcs");
        let scc = &sccs[scc_index];
        let summary = FirFunctionSummary {
            name: name.clone(),
            decl_id: info.decl_id,
            section: info.section,
            params: info.params.clone(),
            has_body: info.body.is_some(),
            is_inline: info.is_inline,
            body_node_count: metrics.node_count,
            direct_callees: metrics.direct_callees.clone(),
            scc_index,
            is_recursive_scc: scc.is_recursive,
            is_reserved_api: RESERVED_DSP_API_FUNCTIONS.contains(&name.as_str()),
        };
        let decision = decide_callee_candidate(&summary, options);
        candidate_decisions.insert(name.clone(), decision);
        functions.insert(name.clone(), summary);
    }

    Ok(FirInlineAnalysis {
        options: options.clone(),
        functions,
        call_graph,
        sccs,
        candidate_decisions,
    })
}

/// Collects all `DeclareFun` nodes from a module section (`globals`/`declarations`).
fn collect_functions_from_section(
    store: &FirStore,
    section_id: FirId,
    section: FirFunctionSection,
    out: &mut BTreeMap<String, RawFunctionInfo>,
) -> Result<(), FirInlineAnalysisError> {
    let section_name = match section {
        FirFunctionSection::Globals => "globals",
        FirFunctionSection::Declarations => "declarations",
    };
    let FirMatch::Block(items) = match_fir(store, section_id) else {
        return Err(FirInlineAnalysisError::InvalidModuleSection {
            section: section_name,
            node: section_id,
        });
    };

    for item in items {
        let FirMatch::DeclareFun {
            name,
            args,
            body,
            is_inline,
            ..
        } = match_fir(store, item)
        else {
            continue;
        };

        if let Some(prev) = out.get(&name) {
            return Err(FirInlineAnalysisError::DuplicateFunctionName {
                name,
                first: prev.decl_id,
                second: item,
            });
        }

        out.insert(
            name,
            RawFunctionInfo {
                decl_id: item,
                section,
                params: args,
                body,
                is_inline,
            },
        );
    }
    Ok(())
}

/// Traverses a function body and extracts a size metric + direct `FunCall` names.
fn collect_body_metrics(store: &FirStore, root: FirId) -> BodyMetrics {
    let mut metrics = BodyMetrics::default();
    let mut stack = vec![root];
    let mut seen = HashSet::new();

    while let Some(id) = stack.pop() {
        if !seen.insert(id) {
            continue;
        }
        metrics.node_count += 1;
        let node = match_fir(store, id);
        if let FirMatch::FunCall { name, .. } = &node {
            metrics.direct_callees.insert(name.clone());
        }
        stack.extend(child_ids(&node));
    }

    metrics
}

/// Returns child FIR ids for recursive traversal.
///
/// This is intentionally local to the inliner module so analysis remains
/// independent from dump helpers in `lib.rs`.
fn child_ids(node: &FirMatch) -> Vec<FirId> {
    match node {
        FirMatch::Unknown
        | FirMatch::Int32 { .. }
        | FirMatch::Int64 { .. }
        | FirMatch::Float32 { .. }
        | FirMatch::Float64 { .. }
        | FirMatch::Bool { .. }
        | FirMatch::Quad { .. }
        | FirMatch::FixedPoint { .. }
        | FirMatch::Int32Array { .. }
        | FirMatch::Float32Array { .. }
        | FirMatch::Float64Array { .. }
        | FirMatch::QuadArray { .. }
        | FirMatch::FixedPointArray { .. }
        | FirMatch::LoadVar { .. }
        | FirMatch::LoadVarAddress { .. }
        | FirMatch::NullValue { .. }
        | FirMatch::NewDsp { .. }
        | FirMatch::NullDeclareVar
        | FirMatch::DeclareStructType { .. }
        | FirMatch::DeclareBufferIterators { .. }
        | FirMatch::ShiftArrayVar { .. }
        | FirMatch::NullStatement
        | FirMatch::OpenBox { .. }
        | FirMatch::CloseBox
        | FirMatch::AddButton { .. }
        | FirMatch::AddSlider { .. }
        | FirMatch::AddBargraph { .. }
        | FirMatch::AddSoundfile { .. }
        | FirMatch::AddMetaDeclare { .. }
        | FirMatch::Label(_) => Vec::new(),
        FirMatch::ValueArray { values, .. }
        | FirMatch::FunCall { args: values, .. }
        | FirMatch::DeclareTable { values, .. }
        | FirMatch::Block(values) => values.clone(),
        FirMatch::LoadTable { index, .. }
        | FirMatch::TeeVar { value: index, .. }
        | FirMatch::Neg { value: index, .. }
        | FirMatch::Cast { value: index, .. }
        | FirMatch::Bitcast { value: index, .. }
        | FirMatch::StoreVar { value: index, .. }
        | FirMatch::SimpleForLoop { upper: index, .. }
        | FirMatch::Drop(index) => vec![*index],
        FirMatch::BinOp { lhs, rhs, .. } => vec![*lhs, *rhs],
        FirMatch::Select2 {
            cond,
            then_value,
            else_value,
            ..
        } => vec![*cond, *then_value, *else_value],
        FirMatch::DeclareVar { init, .. } => init.iter().copied().collect(),
        FirMatch::DeclareFun { body: Some(b), .. } => vec![*b],
        FirMatch::DeclareFun { body: None, .. } => vec![],
        FirMatch::StoreTable { index, value, .. } => vec![*index, *value],
        FirMatch::Return(value) => value.iter().copied().collect(),
        FirMatch::If {
            cond,
            then_block,
            else_block,
        } => {
            let mut out = vec![*cond, *then_block];
            out.extend(else_block.iter().copied());
            out
        }
        FirMatch::Control { cond, stmt } => vec![*cond, *stmt],
        FirMatch::ForLoop {
            init,
            end,
            step,
            body,
            ..
        } => vec![*init, *end, *step, *body],
        FirMatch::IteratorForLoop { body, .. } => vec![*body],
        FirMatch::WhileLoop { cond, body } => vec![*cond, *body],
        FirMatch::Switch {
            cond,
            cases,
            default,
        } => {
            let mut out = vec![*cond];
            out.extend(cases.iter().map(|(_, block)| *block));
            out.extend(default.iter().copied());
            out
        }
        FirMatch::Module {
            dsp_struct,
            globals,
            declarations,
            ..
        } => vec![*dsp_struct, *globals, *declarations],
    }
}

/// Computes a deterministic SCC decomposition of the function call graph.
///
/// The graph is keyed by function name and edges should only target known keys.
/// SCCs are returned in a stable order derived from sorted node iteration.
fn tarjan_sccs(
    graph: &BTreeMap<String, BTreeSet<String>>,
) -> (Vec<FirInlineScc>, BTreeMap<String, usize>) {
    struct TarjanState {
        index: usize,
        stack: Vec<String>,
        on_stack: HashSet<String>,
        index_map: HashMap<String, usize>,
        lowlink_map: HashMap<String, usize>,
        components: Vec<Vec<String>>,
    }

    fn strong_connect(
        node: &str,
        graph: &BTreeMap<String, BTreeSet<String>>,
        st: &mut TarjanState,
    ) {
        let node_s = node.to_string();
        st.index_map.insert(node_s.clone(), st.index);
        st.lowlink_map.insert(node_s.clone(), st.index);
        st.index += 1;
        st.stack.push(node_s.clone());
        st.on_stack.insert(node_s.clone());

        if let Some(neighbors) = graph.get(node) {
            for next in neighbors {
                if !st.index_map.contains_key(next) {
                    strong_connect(next, graph, st);
                    let low_n = *st.lowlink_map.get(node).expect("node lowlink set");
                    let low_next = *st.lowlink_map.get(next).expect("next lowlink set");
                    st.lowlink_map.insert(node_s.clone(), low_n.min(low_next));
                } else if st.on_stack.contains(next) {
                    let low_n = *st.lowlink_map.get(node).expect("node lowlink set");
                    let idx_next = *st.index_map.get(next).expect("next index set");
                    st.lowlink_map.insert(node_s.clone(), low_n.min(idx_next));
                }
            }
        }

        let low = *st.lowlink_map.get(node).expect("node lowlink set");
        let idx = *st.index_map.get(node).expect("node index set");
        if low == idx {
            let mut component = Vec::new();
            loop {
                let w = st.stack.pop().expect("stack contains SCC members");
                st.on_stack.remove(&w);
                let done = w == node;
                component.push(w);
                if done {
                    break;
                }
            }
            component.sort();
            st.components.push(component);
        }
    }

    let mut st = TarjanState {
        index: 0,
        stack: Vec::new(),
        on_stack: HashSet::new(),
        index_map: HashMap::new(),
        lowlink_map: HashMap::new(),
        components: Vec::new(),
    };
    for node in graph.keys() {
        if !st.index_map.contains_key(node) {
            strong_connect(node, graph, &mut st);
        }
    }

    st.components.sort_by(|a, b| a[0].cmp(&b[0]));

    let mut sccs = Vec::with_capacity(st.components.len());
    let mut scc_index_by_name = BTreeMap::new();
    for (idx, functions) in st.components.into_iter().enumerate() {
        let is_recursive = if functions.len() > 1 {
            true
        } else {
            let f = &functions[0];
            graph.get(f).is_some_and(|edges| edges.contains(f))
        };
        for name in &functions {
            scc_index_by_name.insert(name.clone(), idx);
        }
        sccs.push(FirInlineScc {
            index: idx,
            functions,
            is_recursive,
        });
    }

    (sccs, scc_index_by_name)
}

/// Applies the current (conservative) callee-eligibility policy.
fn decide_callee_candidate(
    summary: &FirFunctionSummary,
    options: &FirInlineOptions,
) -> FirInlineCandidateDecision {
    let mut reasons = Vec::new();

    if !options.enabled {
        reasons.push(FirInlineSkipReason::Disabled);
    }
    if !summary.has_body {
        reasons.push(FirInlineSkipReason::PrototypeOnly);
    }
    if summary.is_reserved_api {
        reasons.push(FirInlineSkipReason::ReservedApi);
    }
    if summary.is_recursive_scc && !options.allow_recursive {
        reasons.push(FirInlineSkipReason::RecursiveScc);
    }
    if options.inline_marked_only && !summary.is_inline {
        reasons.push(FirInlineSkipReason::NotMarkedInline);
    }
    if summary.has_body && summary.body_node_count > options.max_callee_nodes {
        reasons.push(FirInlineSkipReason::TooLarge {
            body_nodes: summary.body_node_count,
            max: options.max_callee_nodes,
        });
    }

    FirInlineCandidateDecision {
        function: summary.name.clone(),
        eligible: reasons.is_empty(),
        reasons,
    }
}

/// Options controlling hygienic FIR subtree cloning for future inlining.
///
/// This is the Milestone-2 rename engine used to clone callee bodies into a
/// destination store while avoiding local variable name capture/collisions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirHygienicCloneOptions {
    /// Prefix used for generated fresh local names.
    ///
    /// Generated names are of the form `<prefix><counter>_<original>`.
    pub local_prefix: String,
}

impl Default for FirHygienicCloneOptions {
    fn default() -> Self {
        Self {
            local_prefix: "__fir_inl".to_string(),
        }
    }
}

/// Reusable freshness state for hygienic cloning across multiple callsites.
///
/// Reusing one state instance across repeated clones guarantees distinct fresh
/// local names across all those clones, which is required for future inlining
/// of the same callee multiple times into one caller block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirHygienicCloneState {
    /// Clone options (notably fresh-name prefix).
    pub options: FirHygienicCloneOptions,
    /// Next fresh local id.
    pub next_local_id: usize,
}

impl Default for FirHygienicCloneState {
    fn default() -> Self {
        Self {
            options: FirHygienicCloneOptions::default(),
            next_local_id: 0,
        }
    }
}

/// Kind of local binding that was renamed during hygienic cloning.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirLocalRenameKind {
    /// Local `DeclareVar` (`kStack` or `kLoop`).
    DeclareVar,
    /// Local `DeclareTable` (`kStack` or `kLoop`).
    DeclareTable,
    /// `ForLoop.var` / `SimpleForLoop.var` loop control variable.
    LoopVar,
    /// `IteratorForLoop.iterators[*]`.
    IteratorVar,
    /// `DeclareBufferIterators` generated locals.
    BufferIterator,
}

/// One recorded local rename performed by the hygienic clone engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirLocalRename {
    /// Source FIR node that introduced the binding.
    pub origin_node: FirId,
    /// Original local symbol name.
    pub original: String,
    /// Fresh cloned symbol name.
    pub renamed: String,
    /// Access class of the renamed local.
    pub access: AccessType,
    /// Syntactic origin category.
    pub kind: FirLocalRenameKind,
}

/// Result of a hygienic subtree clone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FirHygienicCloneResult {
    /// Root node id in the destination store.
    pub root: FirId,
    /// Local symbol renames performed during the clone.
    pub local_renames: Vec<FirLocalRename>,
}

/// Errors returned by the hygienic clone engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FirHygienicCloneError {
    /// Source node could not be decoded by `match_fir`.
    UnknownNode(FirId),
}

impl std::fmt::Display for FirHygienicCloneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownNode(id) => write!(
                f,
                "hygienic FIR clone cannot clone unknown node {}",
                id.as_u32()
            ),
        }
    }
}

impl std::error::Error for FirHygienicCloneError {}

/// Hygienically clones a FIR subtree into `dst_store` using a fresh local-state.
///
/// This convenience wrapper is suitable for one-off clones. For repeated clones
/// into the same destination (future inlining of the same callee in multiple
/// callsites), prefer [`clone_fir_hygienic_with_state`] so fresh local names
/// remain unique across clones.
///
/// # Errors
/// Returns [`FirHygienicCloneError`] if the source subtree contains an unknown
/// FIR node.
pub fn clone_fir_hygienic(
    src_store: &FirStore,
    root: FirId,
    dst_store: &mut FirStore,
) -> Result<FirHygienicCloneResult, FirHygienicCloneError> {
    let mut state = FirHygienicCloneState::default();
    clone_fir_hygienic_with_state(src_store, root, dst_store, &mut state)
}

/// Hygienically clones a FIR subtree into `dst_store` using caller-provided freshness state.
///
/// Local symbols declared in the cloned subtree (`kStack`, `kLoop`, loop
/// iterators, and buffer iterators) are renamed to fresh names and all matching
/// references are rewritten consistently.
///
/// The clone is scope-aware:
/// - `Block` introduces a new lexical frame,
/// - `If`/`Switch` branches are cloned in isolated branch frames,
/// - loop constructs introduce loop frames so iterator/loop-variable names do
///   not leak outside the cloned loop.
///
/// # Errors
/// Returns [`FirHygienicCloneError`] if the source subtree contains an unknown
/// FIR node.
pub fn clone_fir_hygienic_with_state(
    src_store: &FirStore,
    root: FirId,
    dst_store: &mut FirStore,
    state: &mut FirHygienicCloneState,
) -> Result<FirHygienicCloneResult, FirHygienicCloneError> {
    let mut cloner = HygienicCloner::new(src_store, dst_store, state);
    cloner.push_scope();
    let root = cloner.clone_node(root)?;
    cloner.pop_scope();
    Ok(FirHygienicCloneResult {
        root,
        local_renames: cloner.local_renames,
    })
}

struct HygienicCloner<'a, 'b> {
    src: &'a FirStore,
    dst: &'b mut FirStore,
    state: &'b mut FirHygienicCloneState,
    scopes: Vec<HashMap<String, String>>,
    local_renames: Vec<FirLocalRename>,
}

impl<'a, 'b> HygienicCloner<'a, 'b> {
    fn new(src: &'a FirStore, dst: &'b mut FirStore, state: &'b mut FirHygienicCloneState) -> Self {
        Self {
            src,
            dst,
            state,
            scopes: Vec::new(),
            local_renames: Vec::new(),
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        let _ = self.scopes.pop();
    }

    fn clone_in_new_scope(&mut self, id: FirId) -> Result<FirId, FirHygienicCloneError> {
        self.push_scope();
        let out = self.clone_node(id);
        self.pop_scope();
        out
    }

    fn lookup_local_rename(&self, name: &str) -> Option<&str> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).map(String::as_str))
    }

    fn maybe_renamed_unqualified(&self, name: &str) -> String {
        self.lookup_local_rename(name)
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| name.to_string())
    }

    fn remap_name_by_access(&self, name: &str, access: AccessType) -> String {
        if matches!(access, AccessType::Stack | AccessType::Loop) {
            self.lookup_local_rename(name)
                .map(ToOwned::to_owned)
                .unwrap_or_else(|| name.to_string())
        } else {
            name.to_string()
        }
    }

    fn fresh_local_name(&mut self, original: &str) -> String {
        let id = self.state.next_local_id;
        self.state.next_local_id += 1;
        format!("{}{}_{}", self.state.options.local_prefix, id, original)
    }

    fn bind_local_decl(
        &mut self,
        origin_node: FirId,
        original: &str,
        access: AccessType,
        kind: FirLocalRenameKind,
    ) -> String {
        let existing = self
            .scopes
            .last()
            .and_then(|scope| scope.get(original))
            .cloned();
        if let Some(existing) = existing {
            return existing;
        }
        let renamed = self.fresh_local_name(original);
        let Some(scope) = self.scopes.last_mut() else {
            return original.to_string();
        };
        scope.insert(original.to_string(), renamed.clone());
        self.local_renames.push(FirLocalRename {
            origin_node,
            original: original.to_string(),
            renamed: renamed.clone(),
            access,
            kind,
        });
        renamed
    }

    fn clone_node(&mut self, id: FirId) -> Result<FirId, FirHygienicCloneError> {
        let node = match_fir(self.src, id);
        let out = match node {
            FirMatch::Unknown => return Err(FirHygienicCloneError::UnknownNode(id)),
            FirMatch::Int32 { value, .. } => {
                let mut b = FirBuilder::new(self.dst);
                b.int32(value)
            }
            FirMatch::Int64 { value, .. } => {
                let mut b = FirBuilder::new(self.dst);
                b.int64(value)
            }
            FirMatch::Float32 { value, .. } => {
                let mut b = FirBuilder::new(self.dst);
                b.float32(value)
            }
            FirMatch::Float64 { value, .. } => {
                let mut b = FirBuilder::new(self.dst);
                b.float64(value)
            }
            FirMatch::Bool { value, .. } => {
                let mut b = FirBuilder::new(self.dst);
                b.bool_(value)
            }
            FirMatch::Quad { value, .. } => {
                let mut b = FirBuilder::new(self.dst);
                b.quad(value)
            }
            FirMatch::FixedPoint { value, .. } => {
                let mut b = FirBuilder::new(self.dst);
                b.fixed_point(value)
            }
            FirMatch::ValueArray { values, typ } => {
                let mut cloned = Vec::with_capacity(values.len());
                for v in values {
                    cloned.push(self.clone_node(v)?);
                }
                let mut b = FirBuilder::new(self.dst);
                b.value_array(&cloned, typ)
            }
            FirMatch::Int32Array { values, .. } => {
                let mut b = FirBuilder::new(self.dst);
                b.int32_array(&values)
            }
            FirMatch::Float32Array { values, .. } => {
                let mut b = FirBuilder::new(self.dst);
                b.float32_array(&values)
            }
            FirMatch::Float64Array { values, .. } => {
                let mut b = FirBuilder::new(self.dst);
                b.float64_array(&values)
            }
            FirMatch::QuadArray { values, .. } => {
                let mut b = FirBuilder::new(self.dst);
                b.quad_array(&values)
            }
            FirMatch::FixedPointArray { values, .. } => {
                let mut b = FirBuilder::new(self.dst);
                b.fixed_point_array(&values)
            }
            FirMatch::LoadVar { name, access, typ } => {
                let remap = self.remap_name_by_access(&name, access);
                let mut b = FirBuilder::new(self.dst);
                b.load_var(remap, access, typ)
            }
            FirMatch::LoadTable {
                name,
                access,
                index,
                typ,
            } => {
                let remap = self.remap_name_by_access(&name, access);
                let index = self.clone_node(index)?;
                let mut b = FirBuilder::new(self.dst);
                b.load_table(remap, access, index, typ)
            }
            FirMatch::LoadVarAddress { name, access, typ } => {
                let remap = self.remap_name_by_access(&name, access);
                let mut b = FirBuilder::new(self.dst);
                b.load_var_address(remap, access, typ)
            }
            FirMatch::TeeVar {
                name,
                access,
                value,
                typ,
            } => {
                let remap = self.remap_name_by_access(&name, access);
                let value = self.clone_node(value)?;
                let mut b = FirBuilder::new(self.dst);
                b.tee_var(remap, access, value, typ)
            }
            FirMatch::BinOp { op, lhs, rhs, typ } => {
                let lhs = self.clone_node(lhs)?;
                let rhs = self.clone_node(rhs)?;
                let mut b = FirBuilder::new(self.dst);
                b.binop(op, lhs, rhs, typ)
            }
            FirMatch::Neg { value, typ } => {
                let value = self.clone_node(value)?;
                let mut b = FirBuilder::new(self.dst);
                b.neg(value, typ)
            }
            FirMatch::Cast { typ, value } => {
                let value = self.clone_node(value)?;
                let mut b = FirBuilder::new(self.dst);
                b.cast(typ, value)
            }
            FirMatch::Bitcast { typ, value } => {
                let value = self.clone_node(value)?;
                let mut b = FirBuilder::new(self.dst);
                b.bitcast(typ, value)
            }
            FirMatch::Select2 {
                cond,
                then_value,
                else_value,
                typ,
            } => {
                let cond = self.clone_node(cond)?;
                let then_value = self.clone_node(then_value)?;
                let else_value = self.clone_node(else_value)?;
                let mut b = FirBuilder::new(self.dst);
                b.select2(cond, then_value, else_value, typ)
            }
            FirMatch::FunCall { name, args, typ } => {
                let mut cloned_args = Vec::with_capacity(args.len());
                for a in args {
                    cloned_args.push(self.clone_node(a)?);
                }
                let mut b = FirBuilder::new(self.dst);
                b.fun_call(name, &cloned_args, typ)
            }
            FirMatch::NullValue { typ } => {
                let mut b = FirBuilder::new(self.dst);
                b.null_value(typ)
            }
            FirMatch::NewDsp { name, typ } => {
                let mut b = FirBuilder::new(self.dst);
                b.new_dsp(name, typ)
            }
            FirMatch::DeclareVar {
                name,
                typ,
                access,
                init,
            } => {
                let init = match init {
                    Some(v) => Some(self.clone_node(v)?),
                    None => None,
                };
                let name = if matches!(access, AccessType::Stack | AccessType::Loop) {
                    self.bind_local_decl(id, &name, access, FirLocalRenameKind::DeclareVar)
                } else {
                    name
                };
                let mut b = FirBuilder::new(self.dst);
                b.declare_var(name, typ, access, init)
            }
            FirMatch::DeclareTable {
                name,
                access,
                elem_type,
                values,
            } => {
                let mut cloned_values = Vec::with_capacity(values.len());
                for v in values {
                    cloned_values.push(self.clone_node(v)?);
                }
                let name = if matches!(access, AccessType::Stack | AccessType::Loop) {
                    self.bind_local_decl(id, &name, access, FirLocalRenameKind::DeclareTable)
                } else {
                    name
                };
                let mut b = FirBuilder::new(self.dst);
                b.declare_table(name, access, elem_type, &cloned_values)
            }
            FirMatch::NullDeclareVar => {
                let mut b = FirBuilder::new(self.dst);
                b.null_declare_var()
            }
            FirMatch::DeclareFun {
                name,
                typ,
                args,
                body,
                is_inline,
            } => {
                let body = match body {
                    Some(body_id) => Some(self.clone_in_new_scope(body_id)?),
                    None => None,
                };
                let mut b = FirBuilder::new(self.dst);
                b.declare_fun(name, typ, &args, body, is_inline)
            }
            FirMatch::DeclareStructType { typ } => {
                let mut b = FirBuilder::new(self.dst);
                b.declare_struct_type(typ)
            }
            FirMatch::DeclareBufferIterators {
                name1,
                name2,
                channels,
                typ,
                mutable,
                chunk,
            } => {
                let name1 = self.bind_local_decl(
                    id,
                    &name1,
                    AccessType::Loop,
                    FirLocalRenameKind::BufferIterator,
                );
                let name2 = self.bind_local_decl(
                    id,
                    &name2,
                    AccessType::Loop,
                    FirLocalRenameKind::BufferIterator,
                );
                let mut b = FirBuilder::new(self.dst);
                b.declare_buffer_iterators(name1, name2, channels, typ, mutable, chunk)
            }
            FirMatch::StoreVar {
                name,
                access,
                value,
            } => {
                let remap = self.remap_name_by_access(&name, access);
                let value = self.clone_node(value)?;
                let mut b = FirBuilder::new(self.dst);
                b.store_var(remap, access, value)
            }
            FirMatch::StoreTable {
                name,
                access,
                index,
                value,
            } => {
                let remap = self.remap_name_by_access(&name, access);
                let index = self.clone_node(index)?;
                let value = self.clone_node(value)?;
                let mut b = FirBuilder::new(self.dst);
                b.store_table(remap, access, index, value)
            }
            FirMatch::ShiftArrayVar {
                name,
                access,
                delay,
            } => {
                let remap = self.remap_name_by_access(&name, access);
                let mut b = FirBuilder::new(self.dst);
                b.shift_array_var(remap, access, delay)
            }
            FirMatch::Drop(v) => {
                let v = self.clone_node(v)?;
                let mut b = FirBuilder::new(self.dst);
                b.drop_(v)
            }
            FirMatch::NullStatement => {
                let mut b = FirBuilder::new(self.dst);
                b.null_statement()
            }
            FirMatch::Return(v) => {
                let v = match v {
                    Some(v) => Some(self.clone_node(v)?),
                    None => None,
                };
                let mut b = FirBuilder::new(self.dst);
                b.ret(v)
            }
            FirMatch::Block(stmts) => {
                self.push_scope();
                let mut cloned = Vec::with_capacity(stmts.len());
                for s in stmts {
                    cloned.push(self.clone_node(s)?);
                }
                self.pop_scope();
                let mut b = FirBuilder::new(self.dst);
                b.block(&cloned)
            }
            FirMatch::If {
                cond,
                then_block,
                else_block,
            } => {
                let cond = self.clone_node(cond)?;
                let then_block = self.clone_in_new_scope(then_block)?;
                let else_block = match else_block {
                    Some(v) => Some(self.clone_in_new_scope(v)?),
                    None => None,
                };
                let mut b = FirBuilder::new(self.dst);
                b.if_(cond, then_block, else_block)
            }
            FirMatch::Control { cond, stmt } => {
                let cond = self.clone_node(cond)?;
                let stmt = self.clone_node(stmt)?;
                let mut b = FirBuilder::new(self.dst);
                b.control(cond, stmt)
            }
            FirMatch::ForLoop {
                var,
                init,
                end,
                step,
                body,
                is_reverse,
            } => {
                self.push_scope();
                let renamed_var =
                    self.bind_local_decl(id, &var, AccessType::Loop, FirLocalRenameKind::LoopVar);
                let init = self.clone_node(init)?;
                let end = self.clone_node(end)?;
                let step = self.clone_node(step)?;
                let body = self.clone_node(body)?;
                self.pop_scope();
                let mut b = FirBuilder::new(self.dst);
                b.for_loop(renamed_var, init, end, step, body, is_reverse)
            }
            FirMatch::SimpleForLoop {
                var,
                upper,
                body,
                is_reverse,
            } => {
                self.push_scope();
                let renamed_var =
                    self.bind_local_decl(id, &var, AccessType::Loop, FirLocalRenameKind::LoopVar);
                let upper = self.clone_node(upper)?;
                let body = self.clone_node(body)?;
                self.pop_scope();
                let mut b = FirBuilder::new(self.dst);
                b.simple_for_loop(renamed_var, upper, body, is_reverse)
            }
            FirMatch::IteratorForLoop {
                iterators,
                is_reverse,
                body,
            } => {
                self.push_scope();
                let mut renamed_iterators = Vec::with_capacity(iterators.len());
                for it in &iterators {
                    renamed_iterators.push(self.bind_local_decl(
                        id,
                        it,
                        AccessType::Loop,
                        FirLocalRenameKind::IteratorVar,
                    ));
                }
                let iter_refs: Vec<&str> = renamed_iterators.iter().map(String::as_str).collect();
                let body = self.clone_node(body)?;
                self.pop_scope();
                let mut b = FirBuilder::new(self.dst);
                b.iterator_for_loop(&iter_refs, is_reverse, body)
            }
            FirMatch::WhileLoop { cond, body } => {
                let cond = self.clone_node(cond)?;
                let body = self.clone_in_new_scope(body)?;
                let mut b = FirBuilder::new(self.dst);
                b.while_loop(cond, body)
            }
            FirMatch::Switch {
                cond,
                cases,
                default,
            } => {
                let cond = self.clone_node(cond)?;
                let mut cloned_cases = Vec::with_capacity(cases.len());
                for (value, body) in cases {
                    cloned_cases.push((value, self.clone_in_new_scope(body)?));
                }
                let default = match default {
                    Some(v) => Some(self.clone_in_new_scope(v)?),
                    None => None,
                };
                let mut b = FirBuilder::new(self.dst);
                b.switch(cond, &cloned_cases, default)
            }
            FirMatch::OpenBox { typ, label } => {
                let mut b = FirBuilder::new(self.dst);
                b.open_box(typ, label)
            }
            FirMatch::CloseBox => {
                let mut b = FirBuilder::new(self.dst);
                b.close_box()
            }
            FirMatch::AddButton { typ, label, var } => {
                let var = self.maybe_renamed_unqualified(&var);
                let mut b = FirBuilder::new(self.dst);
                b.add_button(typ, label, var)
            }
            FirMatch::AddSlider {
                typ,
                label,
                var,
                init,
                lo,
                hi,
                step,
            } => {
                let var = self.maybe_renamed_unqualified(&var);
                let mut b = FirBuilder::new(self.dst);
                b.add_slider(typ, label, var, SliderRange { init, lo, hi, step })
            }
            FirMatch::AddBargraph {
                typ,
                label,
                var,
                lo,
                hi,
            } => {
                let var = self.maybe_renamed_unqualified(&var);
                let mut b = FirBuilder::new(self.dst);
                b.add_bargraph(typ, label, var, lo, hi)
            }
            FirMatch::AddSoundfile { label, url, var } => {
                let var = self.maybe_renamed_unqualified(&var);
                let mut b = FirBuilder::new(self.dst);
                b.add_soundfile_with_url(label, url, var)
            }
            FirMatch::AddMetaDeclare { var, key, value } => {
                let var = self.maybe_renamed_unqualified(&var);
                let mut b = FirBuilder::new(self.dst);
                b.add_meta_declare(var, key, value)
            }
            FirMatch::Label(label) => {
                let mut b = FirBuilder::new(self.dst);
                b.label(label)
            }
            FirMatch::Module {
                name,
                dsp_struct,
                globals,
                declarations,
            } => {
                let dsp_struct = self.clone_node(dsp_struct)?;
                let globals = self.clone_node(globals)?;
                let declarations = self.clone_node(declarations)?;
                let mut b = FirBuilder::new(self.dst);
                b.module(name, dsp_struct, globals, declarations)
            }
        };
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checker::{Severity, verify_fir_module};
    use crate::{AccessType, FirBuilder, FirStore, FirType, NamedType, dump_fir};

    fn fun(
        b: &mut FirBuilder<'_>,
        name: &str,
        args: &[NamedType],
        ret: FirType,
        body: Option<FirId>,
        is_inline: bool,
    ) -> FirId {
        let sig = FirType::Fun {
            args: args.iter().map(|a| a.typ.clone()).collect(),
            ret: Box::new(ret),
        };
        b.declare_fun(name, sig, args, body, is_inline)
    }

    fn assert_no_checker_errors(store: &FirStore, module: FirId) {
        let report = verify_fir_module(store, module);
        let errors: Vec<_> = report
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "expected no FIR checker errors after hygienic clone, got: {errors:?}"
        );
    }

    #[test]
    fn analyzes_call_graph_sizes_and_candidates() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let ff = FirType::FaustFloat;
        let x_arg = NamedType {
            name: "x".to_string(),
            typ: ff.clone(),
        };
        let y_arg = NamedType {
            name: "y".to_string(),
            typ: ff.clone(),
        };

        let h_proto = fun(
            &mut b,
            "h",
            std::slice::from_ref(&x_arg),
            ff.clone(),
            None,
            false,
        );

        let g_body = {
            let x = b.load_var("x", crate::AccessType::FunArgs, ff.clone());
            let one = b.float64(1.0);
            let add = b.binop(crate::FirBinOp::Add, x, one, ff.clone());
            let ret = b.ret(Some(add));
            b.block(&[ret])
        };
        let g_fun = fun(
            &mut b,
            "g",
            std::slice::from_ref(&x_arg),
            ff.clone(),
            Some(g_body),
            true,
        );

        let f_body = {
            let x = b.load_var("x", crate::AccessType::FunArgs, ff.clone());
            let y = b.load_var("y", crate::AccessType::FunArgs, ff.clone());
            let call_g = b.fun_call("g", &[x], ff.clone());
            let call_h = b.fun_call("h", &[y], ff.clone());
            let add = b.binop(crate::FirBinOp::Add, call_g, call_h, ff.clone());
            let ret = b.ret(Some(add));
            b.block(&[ret])
        };
        let f_fun = fun(
            &mut b,
            "f",
            &[x_arg.clone(), y_arg.clone()],
            ff.clone(),
            Some(f_body),
            false,
        );

        let dsp_struct = b.block(&[]);
        let globals = b.block(&[h_proto]);
        let decls = b.block(&[g_fun, f_fun]);
        let module = b.module("mydsp", dsp_struct, globals, decls);

        let analysis = analyze_fir_inliner(&store, module, &FirInlineOptions::default())
            .expect("valid module should analyze");

        assert_eq!(analysis.functions.len(), 3);
        assert_eq!(
            analysis
                .call_graph
                .get("f")
                .expect("f in graph")
                .iter()
                .cloned()
                .collect::<Vec<_>>(),
            vec!["g".to_string(), "h".to_string()]
        );
        assert!(
            analysis
                .functions
                .get("g")
                .expect("g exists")
                .body_node_count
                > 0,
            "body node metric should be collected for defined functions"
        );
        assert_eq!(
            analysis
                .functions
                .get("h")
                .expect("h exists")
                .body_node_count,
            0,
            "prototype body metric should be zero"
        );
        assert!(
            analysis.is_callee_candidate("g"),
            "small non-recursive helper should be a candidate"
        );
        assert!(
            !analysis.is_callee_candidate("h"),
            "prototype-only extern should be skipped"
        );
    }

    #[test]
    fn detects_recursive_sccs_and_marks_skipped() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let ff = FirType::FaustFloat;
        let x_arg = NamedType {
            name: "x".to_string(),
            typ: ff.clone(),
        };

        let f_body = {
            let x = b.load_var("x", crate::AccessType::FunArgs, ff.clone());
            let call_g = b.fun_call("g", &[x], ff.clone());
            let ret = b.ret(Some(call_g));
            b.block(&[ret])
        };
        let g_body = {
            let x = b.load_var("x", crate::AccessType::FunArgs, ff.clone());
            let call_f = b.fun_call("f", &[x], ff.clone());
            let ret = b.ret(Some(call_f));
            b.block(&[ret])
        };
        let f_fun = fun(
            &mut b,
            "f",
            std::slice::from_ref(&x_arg),
            ff.clone(),
            Some(f_body),
            true,
        );
        let g_fun = fun(
            &mut b,
            "g",
            std::slice::from_ref(&x_arg),
            ff.clone(),
            Some(g_body),
            true,
        );
        let module = {
            let dsp_struct = b.block(&[]);
            let globals = b.block(&[]);
            let decls = b.block(&[f_fun, g_fun]);
            b.module("mydsp", dsp_struct, globals, decls)
        };

        let analysis = analyze_fir_inliner(&store, module, &FirInlineOptions::default())
            .expect("analysis should succeed");

        let scc_f = analysis.functions.get("f").unwrap().scc_index;
        let scc_g = analysis.functions.get("g").unwrap().scc_index;
        assert_eq!(
            scc_f, scc_g,
            "mutually recursive functions should share SCC"
        );
        assert!(analysis.sccs[scc_f].is_recursive);
        assert!(
            analysis
                .candidate_decisions
                .get("f")
                .unwrap()
                .reasons
                .contains(&FirInlineSkipReason::RecursiveScc)
        );
    }

    #[test]
    fn candidate_policy_respects_marked_only_size_and_reserved_api() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let ff = FirType::FaustFloat;
        let x_arg = NamedType {
            name: "x".to_string(),
            typ: ff.clone(),
        };

        let helper_body = {
            let x = b.load_var("x", crate::AccessType::FunArgs, ff.clone());
            let ret = b.ret(Some(x));
            b.block(&[ret])
        };
        let helper = fun(
            &mut b,
            "helper",
            std::slice::from_ref(&x_arg),
            ff.clone(),
            Some(helper_body),
            false,
        );

        let compute_body = {
            let x = b.load_var("x", crate::AccessType::FunArgs, ff.clone());
            let ret = b.ret(Some(x));
            b.block(&[ret])
        };
        let compute = fun(
            &mut b,
            "compute",
            std::slice::from_ref(&x_arg),
            ff.clone(),
            Some(compute_body),
            true,
        );

        let large_body = {
            let x = b.load_var("x", crate::AccessType::FunArgs, ff.clone());
            let c0 = b.float64(0.0);
            let c1 = b.float64(1.0);
            let c2 = b.float64(2.0);
            let a0 = b.binop(crate::FirBinOp::Add, x, c0, ff.clone());
            let a1 = b.binop(crate::FirBinOp::Add, a0, c1, ff.clone());
            let a2 = b.binop(crate::FirBinOp::Add, a1, c2, ff.clone());
            let ret = b.ret(Some(a2));
            b.block(&[ret])
        };
        let large = fun(
            &mut b,
            "large",
            std::slice::from_ref(&x_arg),
            ff.clone(),
            Some(large_body),
            true,
        );

        let module = {
            let dsp_struct = b.block(&[]);
            let globals = b.block(&[]);
            let decls = b.block(&[helper, compute, large]);
            b.module("mydsp", dsp_struct, globals, decls)
        };

        let opts = FirInlineOptions {
            inline_marked_only: true,
            max_callee_nodes: 4,
            ..FirInlineOptions::default()
        };
        let analysis = analyze_fir_inliner(&store, module, &opts).expect("analysis should succeed");

        let helper_dec = analysis.candidate_decisions.get("helper").unwrap();
        assert!(!helper_dec.eligible);
        assert!(
            helper_dec
                .reasons
                .contains(&FirInlineSkipReason::NotMarkedInline)
        );

        let compute_dec = analysis.candidate_decisions.get("compute").unwrap();
        assert!(!compute_dec.eligible);
        assert!(
            compute_dec
                .reasons
                .contains(&FirInlineSkipReason::ReservedApi)
        );

        let large_dec = analysis.candidate_decisions.get("large").unwrap();
        assert!(!large_dec.eligible);
        assert!(
            large_dec
                .reasons
                .iter()
                .any(|r| matches!(r, FirInlineSkipReason::TooLarge { .. }))
        );
    }

    #[test]
    fn hygienic_clone_renames_local_decls_and_rewrites_local_uses() {
        let mut src = FirStore::new();
        let src_block = {
            let mut b = FirBuilder::new(&mut src);
            let zero = b.int32(0);
            let decl = b.declare_var("tmp", FirType::Int32, AccessType::Stack, Some(zero));
            let load = b.load_var("tmp", AccessType::Stack, FirType::Int32);
            let dropv = b.drop_(load);
            b.block(&[decl, dropv])
        };

        let mut dst = FirStore::new();
        let cloned = clone_fir_hygienic(&src, src_block, &mut dst).expect("clone should succeed");

        assert_eq!(cloned.local_renames.len(), 1);
        let rename = &cloned.local_renames[0];
        assert_eq!(rename.original, "tmp");
        assert_ne!(rename.renamed, "tmp");
        assert!(rename.renamed.starts_with("__fir_inl"));

        let dump = dump_fir(&dst, cloned.root);
        assert!(dump.contains(&format!("DeclareVar {{ name: \"{}\"", rename.renamed)));
        assert!(dump.contains(&format!("LoadVar {{ name: \"{}\"", rename.renamed)));
        assert!(!dump.contains("DeclareVar { name: \"tmp\""));
    }

    #[test]
    fn hygienic_clone_state_avoids_name_collisions_across_repeated_clones() {
        let mut src = FirStore::new();
        let src_block = {
            let mut b = FirBuilder::new(&mut src);
            let zero = b.int32(0);
            let decl = b.declare_var("tmp", FirType::Int32, AccessType::Stack, Some(zero));
            let upper = b.int32(4);
            let body = {
                let i = b.load_var("i", AccessType::Loop, FirType::Int32);
                let st = b.store_var("tmp", AccessType::Stack, i);
                b.block(&[st])
            };
            let loop_stmt = b.simple_for_loop("i", upper, body, false);
            let load = b.load_var("tmp", AccessType::Stack, FirType::Int32);
            let dropv = b.drop_(load);
            b.block(&[decl, loop_stmt, dropv])
        };

        let mut dst = FirStore::new();
        let mut state = FirHygienicCloneState::default();
        let c1 = clone_fir_hygienic_with_state(&src, src_block, &mut dst, &mut state)
            .expect("first clone should succeed");
        let c2 = clone_fir_hygienic_with_state(&src, src_block, &mut dst, &mut state)
            .expect("second clone should succeed");

        let c1_names: HashSet<_> = c1.local_renames.iter().map(|r| r.renamed.clone()).collect();
        let c2_names: HashSet<_> = c2.local_renames.iter().map(|r| r.renamed.clone()).collect();
        assert!(
            c1_names.is_disjoint(&c2_names),
            "reused clone state should generate disjoint fresh locals"
        );

        let module = {
            let mut b = FirBuilder::new(&mut dst);
            let body = b.block(&[c1.root, c2.root]);
            let f = fun(&mut b, "helper", &[], FirType::Void, Some(body), false);
            let dsp_struct = b.block(&[]);
            let globals = b.block(&[]);
            let decls = b.block(&[f]);
            b.module("mydsp", dsp_struct, globals, decls)
        };
        assert_no_checker_errors(&dst, module);
    }

    #[test]
    fn hygienic_clone_renames_loop_vars_and_iterators_consistently() {
        let mut src = FirStore::new();
        let src_block = {
            let mut b = FirBuilder::new(&mut src);
            let zero = b.int32(0);
            let for_init = b.declare_var("j", FirType::Int32, AccessType::Loop, Some(zero));
            let end = b.int32(4);
            let j_load = b.load_var("j", AccessType::Loop, FirType::Int32);
            let one = b.int32(1);
            let j_next = b.binop(crate::FirBinOp::Add, j_load, one, FirType::Int32);
            let step = b.store_var("j", AccessType::Loop, j_next);
            let for_body = {
                let j = b.load_var("j", AccessType::Loop, FirType::Int32);
                let dj = b.drop_(j);
                b.block(&[dj])
            };
            let for_loop = b.for_loop("j", for_init, end, step, for_body, false);

            let iter_body = {
                let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
                let i1 = b.load_var("i1", AccessType::Loop, FirType::Int32);
                let sum = b.binop(crate::FirBinOp::Add, i0, i1, FirType::Int32);
                let ds = b.drop_(sum);
                b.block(&[ds])
            };
            let iter_loop = b.iterator_for_loop(&["i0", "i1"], false, iter_body);
            b.block(&[for_loop, iter_loop])
        };

        let mut dst = FirStore::new();
        let cloned = clone_fir_hygienic(&src, src_block, &mut dst).expect("clone should succeed");
        let renamed_originals: HashSet<_> = cloned
            .local_renames
            .iter()
            .map(|r| r.original.as_str())
            .collect();
        assert!(renamed_originals.contains("j"));
        assert!(renamed_originals.contains("i0"));
        assert!(renamed_originals.contains("i1"));

        let dump = dump_fir(&dst, cloned.root);
        assert!(!dump.contains("ForLoop { var: \"j\""));
        assert!(!dump.contains("IteratorForLoop { iterators: [\"i0\", \"i1\"]"));

        let module = {
            let mut b = FirBuilder::new(&mut dst);
            let body = b.block(&[cloned.root]);
            let f = fun(&mut b, "helper", &[], FirType::Void, Some(body), false);
            let dsp_struct = b.block(&[]);
            let globals = b.block(&[]);
            let decls = b.block(&[f]);
            b.module("mydsp", dsp_struct, globals, decls)
        };
        assert_no_checker_errors(&dst, module);
    }
}
