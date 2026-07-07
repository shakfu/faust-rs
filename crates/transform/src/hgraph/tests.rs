//! Unit tests for the hierarchical dependency graph and schedule (P1.2).
//!
//! Fixtures mirror the shapes `propagate_clocked_wrapper` emits:
//! `Seq(OnDemand([Clocked(env, clock), PermVar(Clocked(env, body))...]), permvar_i)`.

use propagate::{ClockDomain, ClockDomainId, ClockDomainKind, ClockDomainTable};
use signals::{BinOp, SigBuilder, SigId};
use tlib::TreeArena;

use super::{GraphKey, HgraphError, audit_hgraph, build_hgraph, needs_subgraph, schedule};
use crate::clk_env::annotate;

fn make_domains(parents: &[Option<usize>], arena: &mut TreeArena) -> ClockDomainTable {
    let placeholder_clock = SigBuilder::new(arena).int(1);
    let placeholder_box = arena.nil();
    let mut table = ClockDomainTable::new();
    for &parent in parents {
        let parent = parent
            .map(|index| ClockDomainId::from_u32(u32::try_from(index).expect("small test index")));
        table.alloc(ClockDomain {
            parent,
            kind: ClockDomainKind::OnDemand,
            clock: placeholder_clock,
            wrapper_box: placeholder_box,
            inputs: Vec::new(),
        });
    }
    table
}

fn token(arena: &mut TreeArena, index: usize) -> SigId {
    SigBuilder::new(arena).clock_env_token(u32::try_from(index).expect("small test index"))
}

/// Builds the full propagation shape for a one-output ondemand block:
/// returns `(seq_output, wrapper, clocked_clock, held_output, body)`.
fn make_od_program(
    arena: &mut TreeArena,
    env_index: usize,
    clock: SigId,
    body: SigId,
) -> (SigId, SigId, SigId, SigId) {
    let env = token(arena, env_index);
    let mut b = SigBuilder::new(arena);
    let clocked_clock = b.clocked(env, clock);
    let clocked_body = b.clocked(env, body);
    let held = b.perm_var(clocked_body);
    let od = b.on_demand(&[clocked_clock, held]);
    let seq = b.seq(od, held);
    (seq, od, clocked_clock, held)
}

#[test]
fn ondemand_partitions_top_and_subgraph() {
    let mut arena = TreeArena::new();
    let nil = arena.nil();
    let domains = make_domains(&[None], &mut arena);

    // The body reads the outer input through the boundary glue:
    // Clocked(d0, Clocked(nil, TempVar(x))) * 2.
    let env = token(&mut arena, 0);
    let mut b = SigBuilder::new(&mut arena);
    let clock = b.button(0);
    let x = b.input(0);
    let two = b.int(2);
    let temp = b.temp_var(x);
    let outer_clocked = b.clocked(nil, temp);
    let inner_clocked = b.clocked(env, outer_clocked);
    let body_in_domain = b.binop(BinOp::Mul, inner_clocked, two);

    let (seq, od, _clocked_clock, held) = make_od_program(&mut arena, 0, clock, body_in_domain);

    let envs = annotate(&arena, &domains, &[seq]).expect("well-clocked fixture");
    let hgraph = build_hgraph(&arena, &domains, &envs, &[seq]).expect("hgraph builds");

    // Partition property holds.
    audit_hgraph(&hgraph).expect("every signal owned by exactly one graph");

    // The wrapper is a subgraph key.
    assert!(needs_subgraph(&arena, od));
    let top = hgraph.graph(GraphKey::Top).expect("top graph exists");
    let sub = hgraph
        .graph(GraphKey::Wrapper(od))
        .expect("wrapper subgraph exists");

    // Top owns the seq output, the wrapper node, and the raw clock (the
    // clock stays outside the block).
    assert!(top.contains(seq));
    assert!(top.contains(od));
    assert!(
        top.contains(clock),
        "raw clock must stay in the outer graph"
    );

    // The subgraph owns the held output and the domain-internal body.
    assert!(sub.contains(held));
    assert!(sub.contains(body_in_domain));
    assert!(!top.contains(body_in_domain));

    // `Seq(od, y)` depends only on `od`.
    let seq_edges = top.edges(seq);
    assert_eq!(seq_edges.len(), 1);
    assert_eq!(seq_edges[0].to, od);
    assert!(!seq_edges[0].delayed);
}

#[test]
fn schedule_orders_clock_before_wrapper_before_seq() {
    let mut arena = TreeArena::new();
    let domains = make_domains(&[None], &mut arena);

    let mut b = SigBuilder::new(&mut arena);
    let clock = b.button(0);
    let x = b.input(0);
    let env = token(&mut arena, 0);
    let mut b = SigBuilder::new(&mut arena);
    let lifted = b.clocked(env, x);
    let (seq, od, _clocked_clock, _held) = make_od_program(&mut arena, 0, clock, lifted);

    let envs = annotate(&arena, &domains, &[seq]).expect("well-clocked fixture");
    let hgraph = build_hgraph(&arena, &domains, &envs, &[seq]).expect("hgraph builds");
    let sched = schedule(&hgraph).expect("acyclic per-domain graphs");

    let top_order = sched.schedule(GraphKey::Top).expect("top schedule");
    let pos = |sig: SigId| {
        top_order
            .iter()
            .position(|&s| s == sig)
            .unwrap_or_else(|| panic!("signal {} missing from top schedule", sig.as_u32()))
    };
    assert!(pos(clock) < pos(od), "clock is a precondition of the block");
    assert!(
        pos(od) < pos(seq),
        "the block runs before its output is read"
    );

    // Determinism: rebuilding gives the identical schedule.
    let hgraph2 = build_hgraph(&arena, &domains, &envs, &[seq]).expect("hgraph rebuilds");
    let sched2 = schedule(&hgraph2).expect("still acyclic");
    assert_eq!(sched.schedules.len(), sched2.schedules.len());
    for ((k1, s1), (k2, s2)) in sched.schedules.iter().zip(sched2.schedules.iter()) {
        assert_eq!(k1, k2);
        assert_eq!(s1, s2);
    }
}

#[test]
fn external_dependency_becomes_wrapper_precondition() {
    let mut arena = TreeArena::new();
    let nil = arena.nil();
    let domains = make_domains(&[None], &mut arena);

    // The block body reads an outer-domain computation `g = input0 * 0.5`
    // through the boundary glue: Clocked(d0, Clocked(nil, TempVar(g))).
    let env = token(&mut arena, 0);
    let mut b = SigBuilder::new(&mut arena);
    let clock = b.button(0);
    let x = b.input(0);
    let half = b.real(0.5);
    let g = b.binop(BinOp::Mul, x, half);
    let temp = b.temp_var(g);
    let outer_clocked = b.clocked(nil, temp);
    let boundary = b.clocked(env, outer_clocked);
    let (seq, od, _cc, _held) = make_od_program(&mut arena, 0, clock, boundary);

    let envs = annotate(&arena, &domains, &[seq]).expect("well-clocked fixture");
    let hgraph = build_hgraph(&arena, &domains, &envs, &[seq]).expect("hgraph builds");
    audit_hgraph(&hgraph).expect("partition holds");

    // `g` and its temp-var snapshot are owned by the top graph.
    let top = hgraph.graph(GraphKey::Top).expect("top graph");
    assert!(top.contains(g), "external computation lands at top");

    // The wrapper carries a precondition edge to the external chain.
    let od_edges = top.edges(od);
    assert!(
        od_edges.len() >= 2,
        "wrapper must depend on its clock and on the external input chain, got {od_edges:?}"
    );
}

#[test]
fn instantaneous_cycle_is_reported_as_causality_error() {
    // Build an artificial immediate cycle: a + (a * 1) where the graph edges
    // are forced circular by aliasing — simplest is a direct self-edge via a
    // hand-built graph. We go through the public API with a degenerate
    // fixture: BinOp whose operand is itself is impossible under hash-consing
    // from builders, so this test drives `schedule` directly.
    use super::{Digraph, Hgraph};

    let mut arena = TreeArena::new();
    let mut b = SigBuilder::new(&mut arena);
    let a = b.input(0);
    let one = b.int(1);
    let prod = b.binop(BinOp::Mul, a, one);

    let mut graph = Digraph::default();
    graph.add_edge(prod, a, false);
    graph.add_edge(a, prod, false); // artificial back-edge: instantaneous cycle
    let mut hgraph = Hgraph::default();
    let slot = hgraph.graph_mut(GraphKey::Top);
    *slot = graph;

    let err = schedule(&hgraph).expect_err("cycle must be a causality error");
    assert!(
        matches!(err, HgraphError::InstantaneousCycle { .. }),
        "{err}"
    );
}

#[test]
fn delayed_edges_do_not_order_the_tick() {
    let mut arena = TreeArena::new();
    let domains = make_domains(&[], &mut arena);

    // y = x + y' : a same-tick read of `x` plus a delayed self-read. The
    // recursion is expressed through SYMREC as in the prepared forest.
    let var = arena.int(1000);
    let reference = tlib::sym_ref(&mut arena, var);
    let mut b = SigBuilder::new(&mut arena);
    let back = b.proj(0, reference);
    let delayed = b.delay1(back);
    let x = b.input(0);
    let def = b.binop(BinOp::Add, x, delayed);
    let body = tlib::vec_to_list(&mut arena, &[def]);
    let group = tlib::sym_rec(&mut arena, var, body);
    let out = SigBuilder::new(&mut arena).proj(0, group);

    let envs = annotate(&arena, &domains, &[out]).expect("well-clocked fixture");
    let hgraph = build_hgraph(&arena, &domains, &envs, &[out]).expect("hgraph builds");
    // The recursion is broken by the delayed edge: scheduling must succeed.
    schedule(&hgraph).expect("state-breaking recursion is acyclic on immediate edges");
}
