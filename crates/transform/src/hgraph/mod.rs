//! Hierarchical dependency graph and schedule (roadmap P1.2).
//!
//! # Source provenance (C++)
//! - `compiler/Dependencies/DependenciesGraph.cpp` (`dependenciesGraphs`,
//!   branch `master-dev-ocpp-od-fir-2-FIR19`, commit `8eebea429`)
//! - `compiler/Dependencies/DependenciesUtils.cpp` (`needSubGraph`,
//!   `isExternal`, `getSignalDependencies`)
//! - `compiler/Dependencies/DependenciesScheduling.hh` (`Hsched`)
//! - `compiler/Dependencies/DependenciesAudit.hh` (`auditHgraph`)
//!
//! # What this module computes
//! A **partition** of the reachable prepared signals into per-domain
//! dependency graphs, driven by the clock-environment inference of
//! [`crate::clk_env`]:
//!
//! - the top-level graph holds audio-rate (`nil`-domain) signals;
//! - each OD/US/DS wrapper node keys a **subgraph** holding the signals of
//!   its inner domain (the clock deliberately stays outside — it is a block
//!   *precondition*, not block content);
//! - a signal whose domain is a *strict ancestor* of the current traversal
//!   domain is **external**: it surfaces as an edge `wrapper → external` in
//!   the graph holding the wrapper ("the block needs this computed first"),
//!   and its own computation is placed in the graph of its own domain.
//!
//! [`schedule`] then serializes each graph with one deterministic depth-first
//! toposort on *immediate* edges (C++ `dfschedule`; the pluggable
//! `bf`/`sp`/`rb` strategies of `-ss` are deferred until a consumer needs
//! them).
//!
//! # Immediate vs delayed dependencies
//! A `delay ≥ 1` dependency imposes no intra-tick ordering (the value is read
//! from state) but is still traversed so its defining computation lands in
//! the right domain. `Seq(od, y)` depends **only on `od`** — reading the held
//! `PermVar` is free once the block ran (plan §3.7).
//!
//! # Adaptation status
//! - C++ keys graphs by `Tree` and attaches results as properties; Rust keys
//!   by [`GraphKey`] (`Top` or the wrapper `SigId`) and returns owned values.
//! - Node membership is partitioned (checked by [`audit_hgraph`], mirroring
//!   C++ `auditHgraph`, and run as a debug assertion by [`build_hgraph`]);
//!   edge *targets* may be foreign (externals owned by another graph).
//! - C++ `controls` (slower-than-sample signals hoisted before the loop) is
//!   not populated in this slice: variability-driven hoisting stays in
//!   `signal_fir` placement until backend emission (P3) unifies the two.

use std::fmt;

use ahash::{AHashMap, AHashSet};
use propagate::{ClockDomainId, ClockDomainTable};
use signals::{SigId, SigMatch, match_sig};
use tlib::{TreeArena, list_to_vec, match_sym_rec, match_sym_ref};

use crate::clk_env::{ClkEnv, ClkEnvMap, is_ancestor_clk_env};

/// Key of one per-domain dependency graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GraphKey {
    /// The top-level (audio-rate) graph.
    Top,
    /// Subgraph keyed by its OD/US/DS wrapper node.
    Wrapper(SigId),
}

/// One directed dependency edge annotated with its temporal class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    /// Dependency target (the signal that must be available first).
    pub to: SigId,
    /// `true` when the dependency crosses at least one sample of state
    /// (`delay ≥ 1`): traversed for placement, ignored for ordering.
    pub delayed: bool,
}

/// Small deterministic digraph: owned nodes in first-visit order, adjacency
/// per node. Edge targets may be foreign (signals owned by another graph).
#[derive(Clone, Debug)]
pub struct Digraph {
    nodes: Vec<SigId>,
    node_set: AHashSet<SigId>,
    edges: AHashMap<SigId, Vec<Edge>>,
}

impl Default for Digraph {
    fn default() -> Self {
        Self {
            nodes: Vec::new(),
            node_set: AHashSet::new(),
            edges: AHashMap::new(),
        }
    }
}

impl Digraph {
    fn add_node(&mut self, sig: SigId) {
        if self.node_set.insert(sig) {
            self.nodes.push(sig);
        }
    }

    /// Adds `from → to`. Only `from` becomes an owned node; `to` may be
    /// foreign (an external precondition).
    fn add_edge(&mut self, from: SigId, to: SigId, delayed: bool) {
        self.add_node(from);
        let list = self.edges.entry(from).or_default();
        if !list.iter().any(|e| e.to == to && e.delayed == delayed) {
            list.push(Edge { to, delayed });
        }
    }

    /// Owned nodes in deterministic first-visit order.
    #[must_use]
    pub fn nodes(&self) -> &[SigId] {
        &self.nodes
    }

    /// Outgoing edges of one node.
    #[must_use]
    pub fn edges(&self, sig: SigId) -> &[Edge] {
        self.edges.get(&sig).map_or(&[], Vec::as_slice)
    }

    /// Whether `sig` is an owned node of this graph.
    #[must_use]
    pub fn contains(&self, sig: SigId) -> bool {
        self.node_set.contains(&sig)
    }

    /// Number of owned nodes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the graph owns no node.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

/// Hierarchical dependency graph: one digraph per clock-domain instance.
#[derive(Debug)]
pub struct Hgraph {
    /// Per-domain graphs in deterministic creation order (`Top` first, then
    /// wrappers in traversal order).
    graphs: Vec<(GraphKey, Digraph)>,
    index: AHashMap<GraphKey, usize>,
}

impl Default for Hgraph {
    fn default() -> Self {
        Self {
            graphs: Vec::new(),
            index: AHashMap::new(),
        }
    }
}

impl Hgraph {
    fn graph_mut(&mut self, key: GraphKey) -> &mut Digraph {
        if let Some(&slot) = self.index.get(&key) {
            return &mut self.graphs[slot].1;
        }
        self.index.insert(key, self.graphs.len());
        self.graphs.push((key, Digraph::default()));
        &mut self.graphs.last_mut().expect("just pushed").1
    }

    /// Returns one graph by key.
    #[must_use]
    pub fn graph(&self, key: GraphKey) -> Option<&Digraph> {
        self.index.get(&key).map(|&slot| &self.graphs[slot].1)
    }

    /// All graphs in deterministic creation order.
    #[must_use]
    pub fn graphs(&self) -> &[(GraphKey, Digraph)] {
        &self.graphs
    }
}

/// Errors raised while building or scheduling the hierarchical graph.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HgraphError {
    /// A reachable signal carries no inferred clock environment.
    MissingEnv { sig: SigId },
    /// The partition property is violated (a signal owned by two graphs).
    PartitionViolated { sig: SigId },
    /// An instantaneous (immediate-edge) cycle inside one domain: a
    /// causality error, exactly as in classic Faust.
    InstantaneousCycle { key: GraphKey, sig: SigId },
    /// Structural error while walking the prepared forest.
    Malformed { sig: SigId, detail: String },
}

impl fmt::Display for HgraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnv { sig } => write!(
                f,
                "signal {} is reachable but has no inferred clock environment",
                sig.as_u32()
            ),
            Self::PartitionViolated { sig } => write!(
                f,
                "signal {} is owned by more than one dependency graph",
                sig.as_u32()
            ),
            Self::InstantaneousCycle { key, sig } => write!(
                f,
                "instantaneous cycle through signal {} in graph {key:?} (causality error)",
                sig.as_u32()
            ),
            Self::Malformed { sig, detail } => {
                write!(f, "malformed signal {}: {detail}", sig.as_u32())
            }
        }
    }
}

impl std::error::Error for HgraphError {}

/// `needSubGraph(sig)` ⇔ the node is an OD/US/DS wrapper.
#[must_use]
pub fn needs_subgraph(arena: &TreeArena, sig: SigId) -> bool {
    matches!(
        match_sig(arena, sig),
        SigMatch::OnDemand(_) | SigMatch::Upsampling(_) | SigMatch::Downsampling(_)
    )
}

/// `isExternal(cur_env, sig)` ⇔ the signal's domain is a *strict* ancestor of
/// the current traversal domain: computed elsewhere, visible here.
#[must_use]
pub fn is_external(
    domains: &ClockDomainTable,
    envs: &ClkEnvMap,
    cur_env: ClkEnv,
    sig: SigId,
) -> bool {
    let Some(sig_env) = envs.env(sig) else {
        return false;
    };
    sig_env != cur_env && is_ancestor_clk_env(domains, sig_env, cur_env)
}

/// `getSignalDependencies`: immediate/delayed dependency split (plan §3.7).
///
/// Returns the dependency targets of `sig` in deterministic child order. The
/// opaque clock-env child of `Clocked` is never a dependency.
pub fn get_signal_dependencies(arena: &TreeArena, sig: SigId) -> Result<Vec<Edge>, HgraphError> {
    let imm = |to: SigId| Edge { to, delayed: false };
    let del = |to: SigId| Edge { to, delayed: true };

    if let Some((_, body_list)) = match_sym_rec(arena, sig) {
        // The group's projections read state: the definitions impose no
        // same-tick ordering but still need placement.
        let defs = list_to_vec(arena, body_list).ok_or_else(|| HgraphError::Malformed {
            sig,
            detail: "malformed SYMREC body list".to_owned(),
        })?;
        return Ok(defs.into_iter().map(del).collect());
    }
    if match_sym_ref(arena, sig).is_some() {
        return Ok(Vec::new());
    }

    Ok(match match_sig(arena, sig) {
        // Leaves.
        SigMatch::Int(_)
        | SigMatch::Real(_)
        | SigMatch::Input(_)
        | SigMatch::Button(_)
        | SigMatch::Checkbox(_)
        | SigMatch::VSlider(_)
        | SigMatch::HSlider(_)
        | SigMatch::NumEntry(_)
        | SigMatch::Soundfile(_)
        | SigMatch::Waveform(_)
        | SigMatch::FConst(_, _, _)
        | SigMatch::FVar(_, _, _)
        | SigMatch::ClockEnvToken(_)
        | SigMatch::Unknown => Vec::new(),

        // `Seq(od, y)` depends only on `od`: once the block ran, reading the
        // held perm var is free.
        SigMatch::Seq(x, _) => vec![imm(x)],

        // One-sample delay: state read.
        SigMatch::Delay1(x) => vec![del(x)],
        // General delay: state read when the amount is a constant ≥ 1; the
        // amount itself is an immediate dependency.
        SigMatch::Delay(x, amount) => {
            let x_edge = match match_sig(arena, amount) {
                SigMatch::Int(n) if n >= 1 => del(x),
                _ => imm(x),
            };
            vec![x_edge, imm(amount)]
        }
        // Prefix reads its own state after the first sample; init immediate.
        SigMatch::Prefix(init, x) => vec![imm(init), del(x)],

        // Annotation: depend on the wrapped signal only.
        SigMatch::Clocked(_, y) => vec![imm(y)],

        // Boundary glue.
        SigMatch::TempVar(x) | SigMatch::PermVar(x) => vec![imm(x)],
        SigMatch::ZeroPad(x, h) => vec![imm(x), imm(h)],

        // Wrapper: all children immediate — the builder dispatches the clock
        // to the outer graph and the held outputs to the subgraph.
        SigMatch::OnDemand(children)
        | SigMatch::Upsampling(children)
        | SigMatch::Downsampling(children) => children.iter().copied().map(imm).collect(),

        // Projections resolve through their group.
        SigMatch::Proj(_, group) => vec![imm(group)],

        // Unary pass-throughs.
        SigMatch::Output(_, x)
        | SigMatch::IntCast(x)
        | SigMatch::BitCast(x)
        | SigMatch::FloatCast(x)
        | SigMatch::Gen(x)
        | SigMatch::Lowest(x)
        | SigMatch::Highest(x)
        | SigMatch::Acos(x)
        | SigMatch::Asin(x)
        | SigMatch::Atan(x)
        | SigMatch::Cos(x)
        | SigMatch::Sin(x)
        | SigMatch::Tan(x)
        | SigMatch::Exp(x)
        | SigMatch::Exp10(x)
        | SigMatch::Log(x)
        | SigMatch::Log10(x)
        | SigMatch::Sqrt(x)
        | SigMatch::Abs(x)
        | SigMatch::Floor(x)
        | SigMatch::Ceil(x)
        | SigMatch::Rint(x)
        | SigMatch::Round(x)
        | SigMatch::VBargraph(_, x)
        | SigMatch::HBargraph(_, x)
        | SigMatch::ReverseTimeRec(x) => vec![imm(x)],

        // Binary and ternary composites.
        SigMatch::RdTbl(x, y)
        | SigMatch::Pow(x, y)
        | SigMatch::Min(x, y)
        | SigMatch::Max(x, y)
        | SigMatch::Atan2(x, y)
        | SigMatch::Fmod(x, y)
        | SigMatch::Remainder(x, y)
        | SigMatch::Attach(x, y)
        | SigMatch::Enable(x, y)
        | SigMatch::Control(x, y)
        | SigMatch::SoundfileLength(x, y)
        | SigMatch::SoundfileRate(x, y) => vec![imm(x), imm(y)],
        SigMatch::BinOp(_, x, y) => vec![imm(x), imm(y)],
        SigMatch::Select2(a, b, c) | SigMatch::AssertBounds(a, b, c) => {
            vec![imm(a), imm(b), imm(c)]
        }
        SigMatch::SoundfileBuffer(a, b, c, d) => vec![imm(a), imm(b), imm(c), imm(d)],

        SigMatch::WrTbl(size, generator, wi, ws) => {
            let mut deps = vec![imm(size), imm(generator)];
            if !arena.is_nil(wi) {
                deps.push(imm(wi));
            }
            if !arena.is_nil(ws) {
                deps.push(imm(ws));
            }
            deps
        }

        SigMatch::FFun(_, largs) => {
            let args = list_to_vec(arena, largs).ok_or_else(|| HgraphError::Malformed {
                sig,
                detail: "malformed FFUN argument list".to_owned(),
            })?;
            args.into_iter().map(imm).collect()
        }

        SigMatch::Fir(coefs) | SigMatch::Iir(coefs) => coefs.iter().copied().map(imm).collect(),

        SigMatch::BlockReverseAD {
            body,
            seeds,
            cotangents,
            ..
        } => {
            let mut deps = Vec::new();
            for list in [body, seeds, cotangents] {
                let items = list_to_vec(arena, list).ok_or_else(|| HgraphError::Malformed {
                    sig,
                    detail: "malformed BlockReverseAD child list".to_owned(),
                })?;
                deps.extend(items.into_iter().map(imm));
            }
            deps
        }

        SigMatch::Rec(_) => {
            return Err(HgraphError::Malformed {
                sig,
                detail: "legacy SIGREC form is not supported by hgraph".to_owned(),
            });
        }
    })
}

struct Builder<'a> {
    arena: &'a TreeArena,
    domains: &'a ClockDomainTable,
    envs: &'a ClkEnvMap,
    hgraph: Hgraph,
    /// Which graph owns each domain's signals. `None → Top`; a wrapper's
    /// inner domain maps to its subgraph once the wrapper is visited.
    domain_key: AHashMap<ClkEnv, GraphKey>,
    /// Global visited set: with the env dispatch, guarantees the partition
    /// property (plan §4.2).
    visited: AHashSet<SigId>,
}

impl<'a> Builder<'a> {
    fn env_of(&self, sig: SigId) -> Result<ClkEnv, HgraphError> {
        self.envs.env(sig).ok_or(HgraphError::MissingEnv { sig })
    }

    /// Graph owning signals of `env`. Signals of a clocked domain are only
    /// reachable through their wrapper (`Seq(od, permvar)` depends on `od`
    /// alone), so the wrapper — which registers the mapping — is always
    /// visited first.
    fn key_for(&self, env: ClkEnv) -> GraphKey {
        self.domain_key.get(&env).copied().unwrap_or(GraphKey::Top)
    }

    fn visit(&mut self, sig: SigId) -> Result<(), HgraphError> {
        if !self.visited.insert(sig) {
            return Ok(());
        }
        let env = self.env_of(sig)?;

        if needs_subgraph(self.arena, sig) {
            // The wrapper node itself belongs to the outer graph (R_CD gives
            // it the parent env); its contents populate the subgraph.
            let outer_key = self.key_for(env);
            self.hgraph.graph_mut(outer_key).add_node(sig);
            let sub_key = GraphKey::Wrapper(sig);
            let _ = self.hgraph.graph_mut(sub_key);
            let inner_env = wrapper_inner_env(self.arena, sig);
            self.domain_key.insert(inner_env, sub_key);

            let deps = get_signal_dependencies(self.arena, sig)?;
            let Some((clock_edge, outputs)) = deps.split_first() else {
                return Ok(());
            };
            // The clock stays outside: block precondition in the outer graph.
            self.hgraph
                .graph_mut(outer_key)
                .add_edge(sig, clock_edge.to, clock_edge.delayed);
            self.visit(clock_edge.to)?;
            for edge in outputs {
                self.visit(edge.to)?;
            }
            return Ok(());
        }

        let own_key = self.key_for(env);
        self.hgraph.graph_mut(own_key).add_node(sig);

        for edge in get_signal_dependencies(self.arena, sig)? {
            // Wrapper deps carry the parent env; ordinary deps their own.
            self.visit(edge.to)?;
            let dep_env = self.env_of(edge.to)?;
            if dep_env == env {
                // Same-domain: ordering edge (immediate) or placement-only
                // edge (delayed) inside the same graph.
                self.hgraph
                    .graph_mut(own_key)
                    .add_edge(sig, edge.to, edge.delayed);
            } else if is_ancestor_clk_env(self.domains, dep_env, env) {
                // External (strict-ancestor) dependency: no intra-graph
                // ordering edge; surface the block precondition on the
                // enclosing wrapper in the wrapper's own graph.
                if let GraphKey::Wrapper(w) = own_key {
                    let wrapper_env = self.env_of(w)?;
                    let wrapper_key = self.key_for(wrapper_env);
                    self.hgraph
                        .graph_mut(wrapper_key)
                        .add_edge(w, edge.to, false);
                }
            } else {
                // Deeper/sibling deps only occur through wrapper nodes; a
                // well-clocked forest cannot reach here (inference would have
                // rejected it). Record the edge to stay conservative.
                self.hgraph
                    .graph_mut(own_key)
                    .add_edge(sig, edge.to, edge.delayed);
            }
        }
        Ok(())
    }
}

/// Decodes the inner domain of one wrapper from its first payload child.
fn wrapper_inner_env(arena: &TreeArena, sig: SigId) -> ClkEnv {
    let children = match match_sig(arena, sig) {
        SigMatch::OnDemand(c) | SigMatch::Upsampling(c) | SigMatch::Downsampling(c) => c,
        _ => return None,
    };
    let &first = children.first()?;
    let SigMatch::Clocked(env, _) = match_sig(arena, first) else {
        return None;
    };
    match match_sig(arena, env) {
        SigMatch::ClockEnvToken(id) => Some(ClockDomainId::from_u32(id)),
        _ => None,
    }
}

/// Builds the hierarchical dependency graph from the prepared outputs.
pub fn build_hgraph(
    arena: &TreeArena,
    domains: &ClockDomainTable,
    envs: &ClkEnvMap,
    outputs: &[SigId],
) -> Result<Hgraph, HgraphError> {
    let mut builder = Builder {
        arena,
        domains,
        envs,
        hgraph: Hgraph::default(),
        domain_key: AHashMap::from_iter([(None, GraphKey::Top)]),
        visited: AHashSet::new(),
    };
    // Materialize the top graph first for deterministic ordering.
    let _ = builder.hgraph.graph_mut(GraphKey::Top);
    for &out in outputs {
        builder.visit(out)?;
    }
    let hgraph = builder.hgraph;
    debug_assert!(
        audit_hgraph(&hgraph).is_ok(),
        "hgraph partition property violated: {:?}",
        audit_hgraph(&hgraph)
    );
    Ok(hgraph)
}

/// C++ `auditHgraph`: every signal is *owned* by exactly one graph.
pub fn audit_hgraph(hgraph: &Hgraph) -> Result<(), HgraphError> {
    let mut seen: AHashSet<SigId> = AHashSet::new();
    for (_, graph) in hgraph.graphs() {
        for &sig in graph.nodes() {
            if !seen.insert(sig) {
                return Err(HgraphError::PartitionViolated { sig });
            }
        }
    }
    Ok(())
}

/// One serialized per-graph schedule.
#[derive(Debug, Default)]
pub struct Hsched {
    /// `(key, schedule)` pairs in the same deterministic order as
    /// [`Hgraph::graphs`]. Each schedule lists the graph's owned signals in
    /// dependencies-first order on immediate edges.
    pub schedules: Vec<(GraphKey, Vec<SigId>)>,
}

impl Hsched {
    /// Returns the schedule of one graph.
    #[must_use]
    pub fn schedule(&self, key: GraphKey) -> Option<&[SigId]> {
        self.schedules
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, s)| s.as_slice())
    }
}

/// Serializes each per-domain graph with a deterministic depth-first
/// toposort on immediate edges (C++ `dfschedule` strategy).
///
/// Instantaneous in-graph cycles are causality errors, reported per graph.
pub fn schedule(hgraph: &Hgraph) -> Result<Hsched, HgraphError> {
    fn dfs(
        graph: &Digraph,
        sig: SigId,
        key: GraphKey,
        state: &mut AHashMap<SigId, u8>,
        order: &mut Vec<SigId>,
    ) -> Result<(), HgraphError> {
        match state.get(&sig) {
            Some(2) => return Ok(()),
            Some(1) => return Err(HgraphError::InstantaneousCycle { key, sig }),
            _ => {}
        }
        state.insert(sig, 1);
        for edge in graph.edges(sig) {
            // Only immediate edges order the tick; only owned targets
            // participate (externals are preconditions handled upstream).
            if !edge.delayed && graph.contains(edge.to) {
                dfs(graph, edge.to, key, state, order)?;
            }
        }
        state.insert(sig, 2);
        order.push(sig);
        Ok(())
    }

    let mut out = Hsched::default();
    for (key, graph) in hgraph.graphs() {
        let mut order = Vec::with_capacity(graph.len());
        let mut state: AHashMap<SigId, u8> = AHashMap::new();
        for &sig in graph.nodes() {
            dfs(graph, sig, *key, &mut state, &mut order)?;
        }
        out.schedules.push((*key, order));
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
