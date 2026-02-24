//! FIR function inliner analysis scaffolding (Milestone 1).
//!
//! # Scope
//! This module intentionally implements **analysis only**:
//! - function indexing from a FIR `Module`,
//! - call graph extraction,
//! - SCC detection,
//! - simple callee size metrics,
//! - candidate selection decisions (legality/profitability pre-checks).
//!
//! It does **not** rewrite FIR yet. Rewriting/inlining will be layered on top of
//! this analysis in later milestones.
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

use crate::{FirId, FirMatch, FirStore, NamedType, match_fir};

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FirBuilder, FirType, NamedType};

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
}
