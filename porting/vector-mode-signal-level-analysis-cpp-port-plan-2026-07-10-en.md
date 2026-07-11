# Vector-mode signal-level analysis: C++ study and port plan

**Date:** 2026-07-10

**Scheduling analysis update and design review:** 2026-07-11

**Formal specification update:** 2026-07-11

**Status:** proposed replacement for FIR-level loop discovery

**Studied Rust branch:** `ondemand-vec-fad-synthesis`

**C++ reference:** `master-dev-ocpp-od-fir-2-FIR19`, commit `8eebea429`

**Related documents:**
[initial `-vec` analysis](vector-mode-analysis-port-plan-2026-06-10-en.md),
[current loop-separation design](vector-mode-loop-separation-plan-2026-07-09-en.md),
[FIR-model hardening](vector-mode-fir-model-hardening-plan-2026-07-10-en.md),
and the [signal-to-FIR rewriting calculus](signal-to-fir-rewriting-calculus-2026-06-20-en.md).

## 1. Conclusion

Yes: vectorization decisions should primarily be made on the prepared signal
graph, before signals are fused into FIR statements.

The current FIR partition was useful for validating the chunk driver,
cross-loop buffers, and backend support. It can extract a pure tail from an
already-fused recursive loop. It is not a general model, however: it has to
reconstruct dependencies that were explicit in the signal graph, and it does
not cleanly cover pure prefixes, all delay shapes, multiple recursion groups,
or clocked blocks.

The C++ compiler confirms this direction, with an important nuance: Faust C++
does not first compute a complete immutable loop plan in one standalone pass.
It combines:

1. prior signal analyses (type, recursiveness, occurrences, delays, and
   execution conditions);
2. incremental loop-graph construction while signals are lowered;
3. immediate materialization of values that cross a loop boundary.

The recommended Rust port preserves those semantics while making the boundary
more explicit: a pure `VectorPlan` over `SigId`s decides loop boundaries,
dependencies, and transports; a separate scheduling pass serializes the
resulting execution DAG; signal-to-FIR lowering then emits the planned regions.
FIR remains the emission and storage IR. It must no longer be the source of
truth used to discover vectorization dependencies.

faust-rs will expose one general `-ss` option in scalar and vector modes, backed
by one `SchedulingStrategy` enum and one family of graph algorithms. The node
type scheduled by that policy is mode-specific:

- scalar mode schedules the control and per-domain signal DAGs;
- vector mode schedules the completed `LoopGraph` induced inside each fixed
  execution epoch produced by signal-level vector analysis.

This is one public scheduling contract, not two coupled scheduling passes. In
particular, vector mode does not first serialize the complete signal graph and
then serialize the loop graph. Inline signals have no unique loop owner and may
be lowered independently in several loop regions, so such a global signal
schedule cannot be filtered into loop schedules without inventing per-region
signal instances. A separate public `-dfs` option is not part of the Rust
design.

## 2. What Faust C++ actually does

This analysis uses `DAGInstructionsCompiler` as the primary reference because
it feeds the maintained FIR backends. The older `VectorCompiler` used by the
`-ocpp` path implements nearly the same algorithm and is useful as a cross-check,
but `-ocpp` is outside the Rust port scope.

### 2.1 Signal-graph preparation

`InstructionsCompiler::prepare` in
`compiler/generator/instructions_compiler.cpp` prepares a shared signal forest
before any loop is built:

- normalization and constant propagation;
- simplification and optional signal-level FIR/IIR reconstruction;
- execution-condition annotation;
- recursiveness annotation;
- type annotation and causality checking;
- `OccMarkup::mark`, which computes context-sensitive occurrences.

`Occurrences` is not a plain parent count. It records the variability of use
contexts, delayed uses, `min/maxDelay`, the number of delayed reads, and the
execution condition. `hasMultiOccurrences()` also becomes true when a value is
used from a faster context or under different execution conditions.

This is already a semantic analysis of the signal graph. It provides the facts
later consumed by vector lowering.

### 2.2 Online loop construction

`DAGInstructionsCompiler::compileMultiSignal` calls `prepare`, opens a loop from
each output, and descends through the signals with `CS(sig)`.

For each signal that has not been compiled yet:

- `generateCodeRecursions` first finds recursion groups and opens one dedicated
  loop per group;
- `generateLoopCode` calls `needSeparateLoop(sig)`;
- when the signal must be separated, a `CodeLoop` is opened, the signal is
  lowered, and the loop is closed;
- `CodeContainer::closeLoop` either keeps that loop or absorbs it into its
  enclosing loop when it is empty or when separation would cut an active
  recursive dependency.

The C++ `needSeparateLoop` rule, in exact precedence order, is:

| Priority | Signal property | Decision |
|---|---|---|
| 1 | `maxDelay > 0` | separate loop |
| 2 | `verySimple` or variability `< kSamp` | inline |
| 3 | `sigDelay` read | inline at the use site |
| 4 | recursive projection | separate serial loop |
| 5 | multiple occurrences | separate loop |
| 6 | other sample expression | inline into the consumer |

The order is semantic. In particular, a simple or slow value that is used with
a delay first matches `maxDelay > 0`, because its sample history still has to be
produced one sample at a time.

### 2.3 Dependencies and value transport

The `CS` cache is also where C++ constructs loop-graph edges. When an already
compiled signal is reused, the current loop depends on its defining loop.
Additional cases cover:

- a delayed read whose carried signal owns a loop;
- a delayed read of a recursive projection;
- a projection whose recursion group owns a loop.

`CodeContainer::closeLoop` completes missing edges by scanning sub-signals and
their loop properties before deciding whether to absorb or retain the loop. The
topology is therefore not inferred from generated FIR instructions.

`generateCacheCode` simultaneously selects how a value is transported:

- a scalar for slow values with no history;
- a block array (`Vector*`/`Zec*`) for a sample value shared across loops;
- a temporary plus permanent `Yec*` delay line for a short delay;
- a persistent ring buffer for a long delay.

Finally, `CodeLoop::sortGraph` orders the loops and `VectorCodeContainer` emits
the chunk driver and `-lv` variants.

### 2.4 Meaning of "signal-level analysis" in C++

C++ uses neither a FIR post-pass nor one pure pass equivalent to a complete
scheduler. It uses a hybrid model:

```text
prepared and annotated signals
        |
        v
CS(sig) traversal with memoization
        |
        +-- separation decision on the signal
        +-- CodeLoop open/absorb decision
        +-- edge on cross-loop reuse
        +-- transport-buffer selection
        v
CodeLoop graph -> sorting -> FIR/backend
```

The key invariant to port is not the exact C++ object structure. It is that loop
boundaries and dependencies are decided while signal identity and semantics are
still available.

### 2.5 Signal scheduling with `-ss`

`-ss <n>` selects a topological ordering policy for signal dependency graphs:

| CLI value | C++ function | Algorithm |
|---|---|---|
| `0` (default) | `dfschedule` | depth-first postorder from graph roots |
| `1` | `bfschedule` | dependency-level order from leaves to roots |
| `2` | `spschedule` | recursively interleaved branch order, then reverse deduplication |
| `3+` | `rbschedule` | levelize the reversed graph, then reverse the schedule |

The implementation lives in `compiler/DirectedGraph/Schedule.hh`. The graph
edge convention is `consumer -> dependency`; therefore every valid schedule
must place the edge destination before its source.

#### Depth-first (`-ss 0`)

`dfschedule` starts from graph roots, recursively visits all destinations, and
appends the current node after its dependencies. It tends to finish one
dependency chain before moving to a sibling chain. Shared dependencies are
emitted once through the visited set.

#### Breadth-first (`-ss 1`)

`bfschedule` calls `parallelize(G)`. A leaf with no dependencies has level 0;
every other node has `1 + max(level(dependency))`. Levels are emitted from 0
upward. This groups independent nodes at the same dependency depth and exposes
the graph's parallel width, but it can keep intermediate values live longer
than a depth-first order.

#### Special (`-ss 2`)

`spschedule` recursively builds root-to-dependency lists with duplicates,
interleaves sibling lists, then scans the result backwards while retaining only
the first occurrence of each node. This produces a dependencies-first order
while mixing independent branches more than DFS. It is a fixed heuristic, not
an automatic cost optimizer: `schedulingcost` exists in the same header, but no
compiler path invokes it to select or tune a strategy.

#### Reverse breadth-first (`-ss 3+`)

`rbschedule` levelizes `reverse(G)`, so levels are measured from the original
roots rather than from dependency leaves. It then reverses the complete result
to restore dependencies-first execution. This gives another valid lifetime and
locality profile without changing graph semantics.

#### Hierarchical application

`dependenciesGraphs` builds an `Hgraph` with a control graph and one signal
graph per clock domain or OD/US/DS wrapper. Immediate dependencies create
ordering edges. Delayed dependencies are traversed for placement but create no
same-tick ordering edge.

`scheduleSigList` applies the selected function independently to:

- the control graph;
- the top-rate signal graph;
- every nested clock-domain subgraph.

`ScalarCompiler` and `InstructionsCompiler` then compile controls first, walk
the top schedule, and walk a wrapper's own sub-schedule inside its guard. The
schedule changes statement order and temporary lifetime; it does not change
domain ownership, causality, or the set of computations.

#### Interaction with C++ vector mode

On the reference commit, `-ss` does **not** schedule vector loops.
`DAGInstructionsCompiler::compileMultiSignal`, selected by maintained backends
when `gVectorSwitch` is set, overrides the scalar method and never calls
`scheduleSigList`. It constructs `CodeLoop`s directly through the memoized
`CS(sig)` traversal described above. The old `VectorCompiler` follows the same
pattern. Consequently, `-ss` is parsed globally but has no effective role in
the C++ `-vec` path studied here.

C++ has a separate vector-loop ordering switch, `-dfs`:

- by default, `CodeLoop::sortGraph` propagates levels from the synthetic output
  root through `fBackwardLoopDependencies`, keeps the greatest root distance
  reached for a shared node, and emits levels in reverse order;
- with `-dfs`, `sortDeepFirstDAG` performs a dependency-first DFS over
  `CodeLoop::fBackwardLoopDependencies`.

With the common `consumer -> dependency` edge orientation, the default
`sortGraph` level partition is closest to `rbschedule`, not `bfschedule`:

- `bfschedule` groups nodes by longest distance from dependency leaves;
- `sortGraph` and `rbschedule` group nodes by longest distance from output
  roots, then emit dependencies first;
- their within-level order is not identical: `sortGraph` uses pointer-ordered
  sets while `rbschedule` reverses the flattened level order. Only the level
  policy, not textual output order, is a C++ parity claim.

faust-rs deliberately does not preserve the C++ option split. The single public
`-ss` strategy selects the serialization of the active execution DAG:

```text
scalar mode:
    -ss -> control and per-domain signal DAGs

vector mode:
    fixed epoch order -> -ss on each completed epoch LoopGraph
    canonical recursive lowering -> inline expressions inside each LoopNode
```

The exact cross-mode mapping is:

| `-ss` | Scalar DAG | Vector `LoopGraph` |
|---|---|---|
| `0` (default) | C++ `dfschedule` parity target | subsumes C++ `-dfs` |
| `1` | C++ `bfschedule` parity target | new dependency-leaf level order |
| `2` | C++ `spschedule` parity target | new interleaved order |
| `3+` | C++ `rbschedule` parity target | closest to default C++ `sortGraph` levels |

Therefore the faust-rs vector default intentionally changes from the C++
vector default: Rust `-ss 0` means DFS in both modes. Users wanting the closest
equivalent to the C++ default vector levelization select `-ss 3`. Loop levels
may still be retained as internal parallelism metadata for future OMP or task
scheduling; they do not require another user-facing option.

This is an intentional adapted behavior, not strict reproduction of the current
C++ `-vec` path. It gives `-ss` one consistent meaning across faust-rs modes.
Lifecycle ordering and semantic barriers, such as constants before compute or a
RAD forward sweep before its reverse sweep, remain fixed constraints rather
than scheduling preferences.

The initial port does not expose a second, intra-loop signal scheduling knob in
vector mode. Each `LoopNode` owns materialized signal roots; lowering those roots
recursively emits their inline dependency closures in deterministic child order.
If later profiling justifies scheduling statements inside a loop, that requires
an explicit graph of per-region signal instances such as `(LoopId, SigId)` and a
separate design decision. It must not be implemented by filtering a global
`Hsched` because one inline `SigId` can legitimately occur in several regions.

## 3. Current faust-rs state

faust-rs already has several components at the correct level:

- [`signal_prepare`](../crates/transform/src/signal_prepare/mod.rs) produces a
  private, canonical, typed forest;
- [`placement.rs`](../crates/transform/src/signal_fir/placement.rs) computes
  reference counts and variability boundaries;
- [`delay/plan.rs`](../crates/transform/src/signal_fir/delay/plan.rs) analyzes
  delays over `SigId`s without FIR side effects;
- [`loop_graph.rs`](../crates/transform/src/signal_fir/loop_graph.rs) contains
  `LoopGraph`, the separation predicate, an `assign_loops` prototype, and chunk
  buffers;
- [`region.rs`](../crates/transform/src/signal_fir/module/region.rs) already
  provides one routing surface for `compute` instructions.

These pieces do not yet form one coherent vector model:

1. `assign_loops` is only called by its tests. It does not drive
   `SignalToFirLower`.
2. It receives synthetic properties through a closure; no production path
   currently combines types, occurrences, and delays into `SignalLoopProps`.
3. `signal_value_children` is intentionally incomplete for clock domains and
   several specialized signal forms.
4. It assigns even inline signals to the first visited loop. A trivial inline
   signal has no owning loop: it may be duplicated in several consumers.
   Therefore, `SigId -> LoopId` is too strong a representation for every signal.
5. The current `needs_separate_loop` precedence differs from C++: it tests
   variability, delay-read shape, and `verySimple` before `max_delay`.
6. The lowering cache is only `SigId -> FirId`; it records neither the producing
   region nor cross-region transport.
7. `build_module` first flattens regions, runs CSE on the fused loop, and then
   builds a `LoopGraph` from FIR slices.
8. Effective separation uses `partition_recursive_body`, which recovers a
   serial core and pure tail from FIR temporaries after fusion.
9. [`hgraph::schedule`](../crates/transform/src/hgraph/mod.rs) implements only a
   deterministic DFS order, corresponding to C++ `-ss 0`.
10. `Hgraph` is currently built only when the propagated clock-domain table is
    non-empty. Unlike C++, it has no separate control graph yet; slower-than-
    sample placement remains owned by `signal_fir::placement`.
11. The resulting `Hsched` is currently used as a clock-domain causality and
    partition validation gate and then discarded; demand-driven region lowering
    does not consume its order.
12. Neither `SignalFirOptions` nor the CLI/FFI option surfaces expose `-ss`.
    `LoopGraph::topological_order` is a deterministic Kahn serialization with a
    `LoopId`-ordered ready set; it is not one of the four C++ `-ss` algorithms.
    faust-rs has no existing `-dfs` compatibility surface, so `-ss` can become
    the sole public scheduling option without deprecation work.

The FIR partition should be treated as a validation prototype. It proved the
chunk driver, the possible SIMD gain, and backend portability of local buffers.
It should not become the general semantic model.

## 4. Recommended architecture boundary

"At signal level" does not mean placing `-vec` policy in the `signals` crate.
That crate should remain the owner of the canonical IR, builders, and matchers.
Vectorization depends on variability, occurrences, delay strategies, clock
domains, and lowering options, so it belongs under
`crates/transform/src/signal_fir/`.

The proposed ownership boundary is:

| Layer | Responsibility |
|---|---|
| `signals` | canonical node shapes and inspection |
| `signal_prepare` | prepared forest, types, causality, and recursion invariants |
| generic `schedule(strategy, dag)` | deterministic DFS/BFS/special/reverse-BFS serialization |
| `hgraph` | scalar control and per-domain signal DAGs and schedules |
| `signal_fir::vector_analysis` | uses, loop boundaries, loop graph, and required transports |
| `LoopGraph` | strategy-independent vector execution dependencies |
| `ExecutionSchedule` | selected scalar signal orders or per-epoch vector loop orders |
| `SignalToFirLower` | value/instruction emission into planned regions |
| FIR | backend-neutral loops, arrays, accesses, and phases |
| backends | mechanical FIR translation with no vectorization re-analysis |

The vector analysis and the selected schedule must be separate values. This
makes it structurally impossible for `-ss` to alter vector partitioning:

```rust
struct VectorPlan {
    uses: HashMap<SigId, SignalUseInfo>,
    placement: HashMap<SigId, SignalPlacement>,
    loops: LoopGraph,
    epochs: Vec<ExecutionEpoch>,
    transports: Vec<ValueTransport>,
}

enum SignalPlacement {
    Inline,
    Control,
    Owned(LoopId),
}

enum ExecutionSchedule {
    Scalar(Hsched),
    Vector(Vec<ScheduledEpoch>),
}

struct ScheduledEpoch {
    epoch: EpochId,
    loops: Vec<LoopId>,
}

struct ValueTransport {
    signal: SigId,
    producer: LoopId,
    consumer: LoopId,
    kind: TransportKind,
}
```

`Inline` is essential. Forcing every signal into a `LoopId` would misrepresent
the C++ model and make assignment depend on DFS order.

Each `LoopNode` must retain an ordered list of materialized `SigId` roots. A
region-aware lowerer recursively expands inline dependencies from those roots.
Its cache key must include the producing region (or encode equivalent
visibility), because an inline signal may be recomputed in sibling regions.

The plan must also retain recursion-group identity on recursive loops, for
example `LoopKind::Recursive { group }`. The current `Recursive` flag is enough
for emission, but not for absorption analysis and special projection cases.

The selected strategy must not affect `placement`, loop identity, loop roots,
transport allocation, buffer names, or loop-graph edges. In scalar mode it
chooses among valid signal-DAG orders; in vector mode it chooses among valid
orders of each already-completed epoch subgraph. The same `SchedulingStrategy`
type, edge convention, validation rules, and algorithms apply to both node
types.

### 4.1 Generic scheduling contract

All scheduler adapters must present the same graph contract:

- an edge `A -> B` means "A depends on B", so B must occur before A;
- `nodes()` and `dependencies(node)` expose all nodes and edges in stable-key
  order (`SigId` for signals, `LoopId` for loops);
- a root is a terminal consumer with no incoming edge; DFS and Special visit all
  roots in stable order, including roots of disconnected components;
- a successful schedule contains every node exactly once;
- for every edge `A -> B`, `position(B) < position(A)`;
- a cycle returns a typed error listing the remaining stable node ids; no
  strategy may recurse forever or return a partial order.

Same-loop dependencies are removed while constructing `LoopGraph`, before it is
adapted to this contract. Any self-edge that reaches the generic scheduler is an
instantaneous cycle and must be rejected.

The C++ implementations assume a DAG. The Rust port must validate this contract
for all four strategies. Exact Rust output is deterministic, but C++ tie order
is not a cross-language compatibility promise: C++ signal ties follow `Tree`
ordering and vector loop ties are pointer-ordered. Differential tests compare
edge validity and level membership first; Rust-only snapshots pin exact tie
order.

`spschedule` deserves an explicit complexity guardrail. Its literal C++ form
constructs duplicate root-to-leaf lists and can grow with the number of graph
paths. The first port should preserve exact ordering on the focused corpus and
benchmark path-heavy DAGs. Any compact rewrite must prove order equivalence
against the literal algorithm before replacing it.

### 4.2 Scheduling scope and hard barriers

`-ss` serializes only nodes inside one schedulable DAG. It never reorders these
outer phases:

1. class and instance lifecycle code;
2. compute controls;
3. forward sample/chunk execution;
4. reverse-time AD execution;
5. post-compute maintenance.

Clock-domain guards and vector scalar islands are structural regions, not
ordinary independent nodes that may escape their parent. Within vector mode,
the `LoopGraph` must also contain every data and effect dependency needed to
make arbitrary topological orders legal. Initially, signals with unknown or
observable effects (foreign calls without purity information, mutable table
access, UI writes, or state shared outside one recursion owner) must be
co-located or chained conservatively. Only proven-independent loops may be
reordered by `-ss`.

## 5. Formal specification layer

This section extends, rather than replaces, the
[signal-to-FIR rewriting calculus](signal-to-fir-rewriting-calculus-2026-06-20-en.md).
That document specifies the complete prepared signal algebra and signal-to-FIR
translation. The present layer specifies only the additional facts required for
scheduling and vector loop planning.

The formulas are design contracts. The first implementation target is an
executable specification plus property tests and independent verifiers, not a
mechanized proof of the whole compiler.

### 5.1 Semantic domains and analysis judgment

For one prepared output forest, define:

```text
S       finite set of reachable prepared SigId values
L       finite set of allocated LoopId values
C       rooted tree of clock domains, with ancestor order <=c
Rate    {Konst <=r Block <=r Samp}
Vec     {Vect <=v Scal <=v TrueScal}
Theta   existing SigType value shapes and numeric qualifiers
Res     set of abstract mutable/effect resources
Eff     finite sets of effect atoms over Res
```

The canonical type map from `sigtype`, the clock-environment map, and the new
effect analysis jointly establish the decoration judgment:

```text
Gamma; Omega; Epsilon |- s : theta @ rate [clock] ! effects
```

where `Gamma(s) = theta`, `rate = variability(theta)`, `Omega(s) = clock`, and
`Epsilon(s) = effects`. The required properties are:

```text
Totality:     forall s in S, exactly one judgment exists.
Stability:    the judgment depends only on the prepared forest and options
              that affect semantics, never on -ss or traversal order.
Consistency:  Gamma agrees with the existing total SigType map.
Domain:       Omega agrees with ClkEnvMap and the clock-domain tree.
Effects:      Epsilon over-approximates every observable read/write of s.
```

Representative rules are shown below. They are not a replacement for the full
constructor rules in `sigtype`; they make explicit the facts consumed by this
plan.

```text
Gamma |- x : tx @ rx [c] ! ex     Gamma |- y : ty @ ry [c] ! ey
promote(op, tx, ty) = t
---------------------------------------------------------------- (T-BIN)
Gamma |- BinOp(op,x,y) : t @ (rx join ry) [c] ! (ex union ey)

Gamma |- x : t @ rx [c] ! ex      Gamma |- n : Int @ rn [c] ! en
interval(n) subseteq [0,+infinity)
state_resource(Delay(x,n)) = k
---------------------------------------------------------------- (T-DELAY)
Gamma |- Delay(x,n) : t @ (rx join rn) [c]
         ! (ex union en union {ReadState(k),WriteState(k)})

Gamma |- g : Tuple(t0,...,tk,...) @ Samp [c] ! eg
---------------------------------------------------------------- (T-PROJ)
Gamma |- Proj(k,g) : tk @ Samp [c] ! eg

Gamma |- x : t @ r [c] ! e
---------------------------------------------------------------- (T-CLOCKED)
Gamma |- Clocked(c,x) : t @ r [c] ! e
```

`Clocked`'s environment token is an annotation, not a value dependency. For a
general delay, the carried edge is classified `Delayed(n)` only when analysis
proves a constant `n >= 1`; otherwise it remains `Immediate`, conservatively
preserving possible same-tick dependence.

The effect decoration adds constructor-owned effects in addition to child
effects:

```text
Delay/Prefix/recursion state k   {ReadState(k), WriteState(k)}
RdTbl(table k,...)                {ReadTable(k)}
WrTbl(table k,...)                {WriteTable(k)}
Bargraph(control k,...)           {WriteUi(k)}
Output(channel k,...)              {WriteOutput(k)}
FFun(name,...)                    {Foreign(name,purity(name))}
```

Here `k` is a stable abstract resource identity, not a generated FIR variable
name. A zero-history delay may discharge its state effects after delay analysis.
Every other constructor that lowers to persistent state, including waveform,
clock-domain, hold, and table index/counter state, receives the corresponding
`ReadState/WriteState` atoms by the same rule.

The existing `Vectorability` qualifier is an analysis input, but it is not
identified with `LoopKind`. Define a separate witness:

```text
VecSafe(l) :=
    no loop-carried dependence remains inside l
    and all effects in l are reorderable across samples
    and every non-Vect operation has a dedicated SIMD-safety rule.
```

`vectorability(s) = Vect` is a local sufficient fact for an otherwise pure
operation. `Scal` or `TrueScal` requires a dedicated discharge rule or forces a
serial loop/island. The mandatory implication is one-way:

```text
LoopKind(l) = Vectorizable  ==>  VecSafe(l)
```

The converse is an optimization choice, not a correctness requirement.
This conservative rule can initially classify more loops as serial than C++;
that is an explicit `adapted` safety boundary, to be relaxed only by adding and
testing a new discharge rule.

### 5.2 Dependency relations and causality

The structured dependency analysis returns labelled edges:

```text
Dep subseteq S x DepKind x S

DepKind = Immediate
        | Delayed(n), n >= 1
        | Control
        | ClockBoundary
        | Effect(resource, mode)
```

An edge `(u, Immediate, v)` means that consumer `u` needs the current-tick value
of dependency `v`. Scheduling therefore uses the orientation `u -> v` and must
emit `v` first. A delayed edge is traversed for ownership, history sizing, and
state allocation, but is excluded from the same-tick ordering graph.

For each scalar execution region `R` (controls, top domain, or one nested clock
domain), define:

```text
G_R = (S_R, E_R)
E_R = immediate same-region edges union required effect-order edges
```

The causality obligation is:

```text
WF-Causality:  forall R, G_R is a finite DAG.
```

A cycle in the full signal relation is legal only if every cyclic path is cut
by at least one `Delayed(n)` edge or represented by an explicit recursion group.
An immediate self-edge or an immediate SCC is a causality error.

Clock-domain consistency is:

```text
(u, Immediate, v) and Omega(u) = c_u and Omega(v) = c_v

either c_v = c_u,
or     c_v <=c c_u and v is an external precondition of c_u,
or     the dependency crosses an explicit wrapper boundary;
otherwise the forest is ill-clocked.
```

This prevents a value owned by a child or sibling domain from being silently
scheduled in an ancestor domain.

### 5.3 Effects and commutation

Use at least these abstract effect atoms:

```text
ReadState(k)    WriteState(k)
ReadTable(k)    WriteTable(k)
WriteUi(k)
WriteOutput(k)
Foreign(name, Pure | Impure | Unknown)
```

Two effect sets conflict, written `e1 # e2`, when they touch the same mutable
resource and at least one writes. `Impure` and `Unknown` foreign effects conflict
with every non-local effect unless a stronger contract is available.

For two loops `a` and `b` that are incomparable in `LoopGraph`, require:

```text
Commute(a,b) :=
    forall ea in Effects(a), eb in Effects(b), not (ea # eb)
```

If `Commute(a,b)` cannot be proved, the planner must add a directed effect edge
that preserves the semantic order, co-locate both computations, or place them in
one serial island. This is the condition that makes different topological
schedules observationally equivalent rather than merely data-flow-correct.

### 5.4 Formal scheduling contract

For a finite dependency DAG `G = (V,E)`, a schedule is a bijection:

```text
pi : V -> {0,...,|V|-1}
```

It is valid exactly when:

```text
Valid(G,pi) := forall (u,v) in E, pi(v) < pi(u).
```

Let `key(v)` be the stable Rust rank. Let `deps(v)` and `roots(G)` be ordered by
that key. The four strategies are specified as follows.

```text
DFS:
    postorder visit of roots(G), recursively visiting deps(v).

BFS:
    h(v) = 0                              if deps(v) is empty
    h(v) = 1 + max { h(d) | d in deps(v) } otherwise
    order by (h(v) ascending, key(v) ascending).

SPECIAL:
    rec(v) = [v] ++ fold(interleave, [], [rec(d) | d in deps(v)])
    raw(G) = fold(interleave, [], [rec(r) | r in roots(G)])
    scan reverse(raw(G)); retain the first occurrence of each node.

REVERSE-BFS:
    users(v) = { u | (u,v) in E }
    r(v) = 0                                if users(v) is empty
    r(v) = 1 + max { r(u) | u in users(v) } otherwise
    reverse the sequence ordered by
        (r(v) ascending, key(v) ascending).
```

Here `++` is list concatenation. `interleave` alternates elements from two
lists, appending the remainder of the longer list, exactly as C++
`DirectedGraphAlgorythm.hh` does.

Every implementation must satisfy four scheduler obligations:

```text
S-Sound:         schedule(G,s) = pi  ==>  Valid(G,pi)
S-Complete:      G is a DAG           ==>  pi contains every node exactly once
S-Deterministic: same (G,s,key)       ==>  same pi
S-Terminating:   finite G             ==>  success or typed Cycle error
```

`Special`'s path-list expansion additionally has a resource obligation: either
its measured growth is accepted for the supported corpus, or a compact
implementation must be proved order-equivalent to the literal definition.

### 5.5 Separation, ownership, and loop-graph construction

For each signal `s`, let:

```text
d(s)       maximum delayed use of s
simple(s)  C++ verySimple predicate
slow(s)    variability(s) < Samp
read(s)    s is a sigDelay read node
proj(s)    s is a recursive-group projection
multi(s)   context-sensitive multiple-occurrence predicate
```

The loop-boundary decision is the following ordered function; the first matching
line wins:

```text
Separate(s) =
    Yes       if d(s) > 0
    No        if simple(s) or slow(s)
    No        if read(s)
    Yes       if proj(s)
    Yes       if multi(s)
    No        otherwise
```

This order is normative. In particular, `d(s) > 0` dominates both `simple(s)`
and `slow(s)`.

Placement is a partial ownership discipline:

```text
Place(s) in { Control, Inline, Owned(l) }

Control:  evaluated in a fixed slower lifecycle/domain region.
Inline:   no unique loop owner; may have instances (l,s) in several loops.
Owned(l): exactly one materialized producer loop l.
```

Define `Duplicable(s)` to mean that reevaluating `s` in another region at the
same logical sample produces the same bits and observations. It requires no
write effect, no read from mutable state/table/UI resources that may change
between the instances, no unknown foreign effect, and recursively duplicable
operands. Immutable constants, current-sample inputs, and pure arithmetic over
duplicable operands satisfy it.

Required ownership invariants are:

```text
P-Unique:    Place(s)=Owned(l1) and Place(s)=Owned(l2) ==> l1=l2
P-Inline:    Place(s)=Inline ==> no global cross-region FirId cache entry
P-Duplicate: Place(s)=Inline ==> Duplicable(s)
P-Root:      each Owned(l) signal appears exactly once among roots(l)
P-Control:   a Control value is available before every sample/chunk consumer
P-Strategy:  Place, roots, epochs, edges, transports, LoopId allocation,
             and names are independent of -ss
```

Recursion-group identity is preserved by construction. All projections of one
active group are owned by, or absorbed into, one serial recursive loop. A cycle
must never survive as a cycle between `LoopId` nodes.

Write the region-aware lowering judgment as:

```text
Plan; l |- s => value ; delta
```

where `delta` is the finite set of emitted local code, loop edges, and transports
required by this use. Its core rewrite rules are:

```text
Place(s)=Inline       Plan;l |- each immediate dependency of s
---------------------------------------------------------------- (R-INLINE)
Plan;l |- s => rebuild s in region l ; union dependency deltas

Place(s)=Owned(l)
---------------------------------------------------------------- (R-LOCAL)
Plan;l |- s => compute-or-reuse the materialized value in l
              ; local code on first region-scoped visit

Place(s)=Owned(m)     m != l     Gamma(s)=theta
---------------------------------------------------------------- (R-CROSS)
Plan;l |- s => load T(s,m,l) : lower_type(theta)
              ; { edge l->m, typed transport T(s,m,l) }

Place(s)=Control      control value dominates region l
---------------------------------------------------------------- (R-CONTROL)
Plan;l |- s => load the lifecycle/domain materialization ; empty
```

`R-INLINE` uses a cache scoped by `(l,s)` and is applicable only under
`P-Duplicate`. `R-CROSS` is the only rule that creates cross-loop value reuse.
All four rules carry these preservation obligations:

```text
R-Type:    Gamma(s)=theta ==> FIRType(value)=lower_type(theta)
R-Effects: every non-duplicable effect of s is emitted exactly once, and the
           relative order of conflicting effects is preserved
R-Value:   storage insertion does not alter the per-sample value bits
```

Let `epoch : L -> EpochId` partition loops into a fixed ordered sequence such as
forward and reverse AD execution. For each epoch `e`, the schedulable loop graph
is the induced graph:

```text
L_e   = { l in L | epoch(l)=e }
G_L^e = (L_e, (E_data union E_effect) restricted to L_e)
```

The complete plan must satisfy:

```text
L-DAG:       every G_L^e is acyclic.
L-Complete:  every cross-loop current-sample read has a producer edge and
             a transport.
L-Effects:   loops incomparable inside the same epoch commute.
L-Barriers:  epoch order is explicit and every cross-epoch edge (u,v), where
             u depends on v, satisfies epoch(v) < epoch(u).
```

`-ss` schedules each `G_L^e` independently. It cannot reorder epochs.

### 5.6 Typed transports and region visibility

Let `lower_type(theta)` be the existing signal-type-to-FIR-type mapping and `q`
the vector chunk size. A cross-loop transport is well typed when:

```text
Gamma(s) = theta
lower_type(theta) = tau
T(s,p,c) : Array(q,tau)
```

Its operational rule is:

```text
j = i0 - vindex,  0 <= j < q

producer p:  T(s,p,c)[j] := value(s,i0)
consumer c:  load T(s,p,c)[j] : tau
```

The producer store and every consumer load must use the same chunk-local index.
`Int32`, `Float32`, `Float64`, and `FaustFloat` boundaries must preserve the
existing FIR cast rules; scheduling cannot introduce a new numeric conversion.
These rewrites may move a value through storage but may not reassociate,
contract, or otherwise change its arithmetic expression.

Region visibility is a lexical preorder `<=reg` where ancestors outlive their
descendants:

```text
Reusable(value produced in R, requested in Q) := R <=reg Q
```

Sibling regions are incomparable. Reuse across siblings therefore requires
named transport or persistent storage. The cache is sound only if its key or
entry records enough information to prove `Reusable`.

### 5.7 Loop-fission rewrite rule

Let scalar execution order events by sample first:

```text
(i,a) <scalar (j,b) iff i < j or (i=j and a precedes b in the scalar body).
```

Let vector execution order events by scheduled loop first, then by that loop's
sample direction:

```text
(i,a) <vec (j,b) iff
    schedule(loop(a)) < schedule(loop(b)),
    or loop(a)=loop(b) and the loop-local sample/statement order says so.
```

Let `D` be the true dependence relation containing data, state, control, and
effect dependencies between dynamic events. Loop fission is legal exactly when
the transformed order preserves every dependence:

```text
FissionSafe := forall (x,y) in D, x <scalar y ==> x <vec y.
```

The implementation does not enumerate dynamic events. It checks the following
finite sufficient condition:

```text
StaticFissionSafe(plan) :=
    L-DAG and L-Complete and L-Effects and L-Barriers
    and every loop-carried dependence is internal to a serial LoopNode
    and every cross-loop current-sample dependence has a typed transport.

Proof obligation: StaticFissionSafe(plan) ==> FissionSafe(plan).
```

This criterion explains the key implementation rules:

- a current-sample producer/consumer dependence can cross loops through a chunk
  buffer because producer loop precedes consumer loop;
- a loop-carried dependence must stay inside a serial loop whose sample order is
  preserved;
- if `A(i+1)` depends on `B(i)`, fissioning all `A` iterations before all `B`
  iterations is illegal, so A and B must be co-located or serialized differently;
- two effectful loops may be reordered only when their effects commute.

The FIR-level `partition_recursive_body` is accepted only as a transitional
rewrite with the same `FissionSafe` obligation. The signal-level planner must
eventually be the unique producer of this proof witness.

### 5.8 Semantic preservation and schedule independence

Let the scalar DSP transition be:

```text
Step : State x InputSample -> State x OutputSample x Observations
Run(n, state0, inputs) = n repeated Step transitions
```

Let vector execution with chunk size `q`, loop variant `lv`, and one valid
schedule `pi_e` per epoch be `VecRun(q,lv,{pi_e},n,state0,inputs)`.

The main port correctness theorem is the following proof obligation:

```text
V-Simulation:
forall well-typed prepared programs P,
forall state0, inputs, n, q>0, lv in {0,1},
forall {pi_e} such that for every epoch e, Valid(G_L^e,pi_e),

VecRun(q,lv,{pi_e},n,state0,inputs)
    = Run(n,state0,inputs)
```

Equality covers output samples, final persistent state, tables, UI zones, and
declared external observations. For current impulse gates, the intended
refinement is bit equality, not approximate real-number equality.

Schedule independence is the corollary required by `-ss`:

```text
SS-Independent:
Valid(G,pi1) and Valid(G,pi2)
    ==> Obs(Execute(G,pi1)) = Obs(Execute(G,pi2)).
```

This corollary is valid only if `L-Complete`, `L-Effects`, and `L-Barriers` hold.
A successful topological sort alone is not a proof of semantic equivalence.

Scalar scheduling has the analogous obligation over each `G_R`; fixed outer
lifecycle and domain nesting compose the per-region simulations.

### 5.9 State-transition refinements

For a carried value of type `tau` with maximum history `D > 0`, use the abstract
history state (the `D=0` case has no history state):

```text
H in tau^D, with H[0] the previous sample.

delay_read(0,x,H) = x
delay_read(n,x,H) = H[n-1]              for 1 <= n <= D
history_step(x,H) = [x,H[0],...,H[D-2]]
```

Short copy buffers and long ring buffers are concrete representations of `H`.
Each implementation needs an abstraction function `alpha` from concrete memory
and cursor state to `H`, with the simulation obligation:

```text
DelaySim:
alpha(concrete_step(memory,cursor,x)) = history_step(x,alpha(memory,cursor))
and every concrete read n equals delay_read(n,x,H).
```

For a recursion group with state tuple `R`, define one scalar transition:

```text
(R_i, outputs_i) = RecStep(R_(i-1), inputs_i).
```

All projections at sample `i` observe components of the same `R_i`. The group
must remain inside one serial `LoopNode`; buffering `R_i` for pure downstream
loops is legal, but splitting the computation of `R_i` itself is not.

For a clock domain `c`, let `fires(c,i)` be the wrapper-defined number of inner
transitions at outer sample `i` (zero, one, or a counted amount):

```text
ClockStep(c,i,state) = Step_c repeated fires(c,i) times.
```

When `fires(c,i)=0`, domain-owned state is unchanged and held outputs preserve
their prior values. A scalar island is correct when its concrete guarded loop
simulates this equation and ancestor-domain values are read only through the
declared external preconditions.

For reverse AD, define a forward transition that produces primal outputs and a
tape, followed by a reverse transition that consumes that tape:

```text
(state_f, primal, tape) = Forward(state0, inputs)
(state_r, adjoints)     = Reverse(state_f, tape, seeds)
```

The epoch order `Forward < Reverse` is part of the semantics. No `-ss` strategy
may interleave these epochs. FAD remains an ordinary pointwise signal
transformation once its generated recursion and effects satisfy the preceding
rules.

### 5.10 Executable certificates and proof obligations by phase

The implementation should materialize three independently checkable
certificates:

```text
ScheduleCertificate {
    strategy, node_count, ordered_nodes, edge_hash
}

VectorPlanCertificate {
    placement, loop_roots, loop_kinds, epochs, data_edges, effect_edges,
    barriers, transports, vec_safe_witnesses, stable_names
}

RoutedFirCertificate {
    value_regions, fir_types, transport_stores, transport_loads,
    emitted_effects, epoch_bodies
}
```

`verify_schedule` checks `S-Sound` and `S-Complete` without reusing the selected
scheduling algorithm. `verify_vector_plan` checks `P-*`, `L-*`, transport typing,
region visibility, and `VecSafe` witnesses before FIR emission.
`verify_routed_fir` checks `R-Type`, `R-Effects`, and structural evidence for
`R-Value` after lowering and before backend emission.

The phase-level formal obligations are:

| Phase | Required obligations |
|---|---|
| P0 | C++ observations and abstract DAG fixtures are reproducible |
| P1 | `S-Sound`, `S-Complete`, `S-Deterministic`, `S-Terminating` |
| P2 | CLI/FFI mapping is total on documented inputs and canonical by enum |
| P3 | `WF-Causality`, clock/effect completeness, scalar `SS-Independent` |
| P4 | analysis totality/stability, exact `Separate`, `P-*`, strategy independence |
| P5 | `L-*`, `R-*`, typed transports, `VecSafe`, `StaticFissionSafe => FissionSafe` |
| P6 | `DelaySim`, `RecStep`, `ClockStep`, island nesting, AD epoch simulation |
| P7 | `V-Simulation` and cross-backend translation validation on the corpus |

The practical verification ladder is:

1. pure reference functions for the mathematical definitions;
2. exhaustive enumeration of small finite DAGs plus generated larger DAGs;
3. independent certificate verifiers run in tests and debug builds;
4. differential traces against C++ and scalar faust-rs;
5. optional bounded or deductive mechanization of the generic scheduler and
   transport index lemmas once the executable model stabilizes.

This order avoids proving a moving implementation while still turning every
formal statement above into an executable acceptance condition.

## 6. Port plan

Each step must preserve scalar semantics and be validated before the next step
becomes active. `-ss 0` is the faust-rs default, but activating `Hsched` may
legitimately change textual FIR relative to the current demand-driven lowerer.
The implementation must first run the new order in shadow mode, classify any
difference, and refresh goldens only for audited ordering changes. Byte identity
must not be promised before that comparison; runtime semantics must remain
bit-exact.

The integration order is strict:

1. build and validate semantic dependency graphs;
2. build scalar execution regions or the strategy-independent `VectorPlan`;
3. complete loop roots, data/effect edges, transports, ids, and names;
4. only then call `schedule(strategy, dag)` on the active execution DAG;
5. lower regions in the returned order while preserving fixed lifecycle and AD
   epochs.

### P0 - Structural C++ oracle and focused corpus

**Formal gate:** each fixture has a reproducible labelled dependency graph and
an expected relation-level observation; pointer-dependent tie order is excluded
from the oracle.

- Pin the references above and document the observed C++ functions: `prepare`,
  `OccMarkup`, `dependenciesGraphs`, `scheduleSigList`, `dfschedule`,
  `bfschedule`, `spschedule`, `rbschedule`, `needSeparateLoop`, `CS`,
  `generateCacheCode`, `closeLoop`, and `sortGraph`.
- Build a minimal corpus covering a shared expression, a simple value used with
  delay, a pure prefix, a pure tail, multiple outputs, multiple recursion
  groups, short/long delays, tables, FAD, and a clocked block.
- Capture C++ loop count, recursive classification, edges, and buffers in
  addition to audio output. The research branch rejects `-vec` with ondemand;
  those cases use the scalar oracle and Rust invariants instead of claiming a
  dynamic C++ vector oracle.
- Capture `-phs` schedules for `-ss 0`, `1`, `2`, and `3` on asymmetric
  fork/join graphs, shared dependencies, controls, and nested clock domains.
- Capture C++ vector loop orders both with default `sortGraph` and `-dfs`.
  Record that changing C++ `-ss` alone does not change this vector order.
- Add abstract labelled DAG fixtures (chain, diamond, asymmetric fork/join,
  disconnected roots, and path-heavy shared DAG) so strategy behavior can be
  compared independently of signal preparation and code generation.

**Exit criterion:** a versioned matrix of expected shapes and minimal DSPs
exists before the architecture changes.

### P1 - Generic scheduler core

**Formal gate:** `schedule` produces an independently verified
`ScheduleCertificate` satisfying `S-Sound`, `S-Complete`, `S-Deterministic`, and
`S-Terminating`.

- Add one transform-owned `SchedulingStrategy` enum with
  `DepthFirst`, `BreadthFirst`, `Special`, and `ReverseBreadthFirst` variants.
- Add one generic dependency-DAG adapter used by both `hgraph::Digraph` and
  `LoopGraph`; do not copy the four algorithms into both modules.
- Port the C++ algorithms literally first, including root selection, sibling
  interleaving for `Special`, and full-list reversal for
  `ReverseBreadthFirst`.
- Define stable ascending ranks for roots, adjacency lists, and level members.
- Return typed cycle errors consistently for all strategies and run the common
  postcondition verifier on every successful result.
- Test exact Rust orders on the P0 abstract DAGs, plus node coverage, edge order,
  disconnected components, same-loop edge normalization, retained self-edge
  rejection, and cycle diagnostics.
- Exhaustively enumerate all upper-triangular dependency DAGs up to six nodes,
  then relabel representative graphs to detect accidental dependence on insertion
  order. Check every result with the independent certificate verifier.
- Compare normalized level membership and dependency order with C++; treat tie
  order as an adapted deterministic Rust behavior.
- Benchmark `Special` on path-heavy DAGs before accepting its literal duplicate
  list construction as production-safe.

**Exit criterion:** both signal and loop graph adapters pass the same scheduler
conformance suite; no compiler output changes yet.

### P2 - Public option and configuration plumbing

**Formal gate:** option decoding implements the total documented function
`0 -> DFS`, `1 -> BFS`, `2 -> Special`, and `n>=3 -> ReverseBFS`; rejected inputs
produce no compiler or factory state.

- Add `--scheduling-strategy <n>` to the `clap` CLI and normalize legacy Faust
  spelling `-ss <n>` to it.
- Accept non-negative integers with `0 -> DepthFirst`, `1 -> BreadthFirst`,
  `2 -> Special`, and `n >= 3 -> ReverseBreadthFirst`; reject missing,
  non-integer, and negative values. This is an adapted, stricter contract than
  C++ `atoi` fallback behavior.
- Keep `DepthFirst` (`-ss 0`) as the default in scalar and vector modes.
- Thread the enum through `Compiler`, a public
  `with_scheduling_strategy(...)` builder, `SignalLoweringContext`, and
  `SignalFirOptions` without coupling it to `ComputeMode`.
- Extend shared FFI argv parsing with `-ss`, then thread the selected strategy
  through every factory path. Include its canonical enum value in factory cache
  identity and compile-option metadata where those surfaces exist.
- Add parser, default, facade, FFI, and cache-key tests. `-ss` must be accepted
  without `-vec`; `-vec` must not alter its default or parsing.
- Document public API mapping as `adapted`: CLI behavior matches documented C++
  values, while the Rust enum and strict parse errors are idiomatic Rust APIs.

**Exit criterion:** all entry points retain and report the selected strategy,
but scheduling is still behaviorally inactive.

### P3 - Scalar scheduling activation

**Formal gate:** every scalar region satisfies `WF-Causality`, clock consistency,
effect completeness, and scalar `SS-Independent` before a selected schedule is
allowed to drive lowering.

- Make signal dependency graph construction available for every prepared forest,
  not only when clock domains exist.
- Complete the current Rust `Hgraph` parity gap before activation: represent the
  C++ control graph explicitly, preserve top and nested domain graphs, and keep
  delayed edges placement-only rather than same-tick ordering edges.
- Introduce the common conservative effect classification here, before enabling
  schedule-dependent emission. Add effect-order edges or fixed source-order
  chains whenever commutation is not proved.
- Audit graph membership against existing `Variability::{Konst, Block, Samp}`
  placement. Lifecycle sections remain fixed even when one schedule visits
  signals that lower into different sections.
- Change `hgraph::schedule` to accept `SchedulingStrategy` and run the generic
  scheduler independently on controls, top rate, and each wrapper subgraph.
- Run `Hsched` in shadow mode against demand-driven lowering first. Record
  statement-order, naming, and CSE differences for `-ss 0` before making it
  authoritative.
- Make scalar lowering visit the selected controls and top schedule; when a
  wrapper opens its guarded region, visit that wrapper's selected sub-schedule.
- Preserve region redirection, held-payload exceptions, initialization order,
  delay maintenance, and output-store order as hard rules outside `-ss`.
- Run CSE only after statements have been routed to their lifecycle/domain
  regions.

**Exit criterion:** `-ss 0/1/2/3` produce valid distinct scalar schedules and
bit-exact runtime results. Any textual golden changes for `-ss 0` are individually
audited and documented rather than assumed absent.

### P4 - Unified signal-use analysis and strategy-independent vector plan

**Formal gate:** the analysis judgment is total and stable, the ordered
`Separate` function is exact, and the provisional `VectorPlanCertificate`
satisfies all `P-*` obligations independently of `-ss`.

- Add `vector_analysis.rs` under `signal_fir`.
- Consolidate the dependency/effect API introduced for scalar scheduling with
  delay, occurrence, and vector-placement facts. It must distinguish at least
  `Immediate`, `Delayed(n)`, `Control`, `Effect`, and `ClockBoundary` without
  maintaining a second child walk.
- Produce `SignalUseInfo` per `SigId`: variability, context-sensitive uses,
  `max_delay`, delay-read shape, projection/group identity, triviality, effects,
  and clock domain.
- Reuse results from `placement` and `delay::plan`, or merge them into one
  traversal. Do not maintain divergent definitions of sharing and `max_delay`.
- Port the exact `needSeparateLoop` precedence, especially
  `verySimple + maxDelay > 0` and `Block + maxDelay > 0`.
- Replace `assign_loops` with a pure `VectorPlan` builder. Represent inline
  signals as `Inline`, controls as `Control`, and only materialized sample
  values as `Owned(LoopId)`.
- Allocate one loop per separated value and one serial loop per recursion group;
  retain explicit group identity and deterministic materialized roots.
- Port the four edge families added by C++ `CS` and the recursive closure/
  absorption invariant from `CodeContainer::closeLoop`.
- Allocate loop ids, epoch membership, transport ids, and buffer names before
  scheduling. The plan builder must not accept a `SchedulingStrategy` argument.

**Exit criterion:** deterministic `SignalUseInfo` and `VectorPlan` snapshots
reproduce the P0 topology cases. A structural test proves that changing the
configured `-ss` value leaves the serialized plan snapshot byte-identical.

### P5 - LoopGraph completion, scheduling, and region routing

**Formal gate:** before emission, `verify_vector_plan` establishes `L-*`, typed
transports, region visibility, `VecSafe`, and `StaticFissionSafe`; after routed
lowering, an independent FIR check establishes `R-Type`, `R-Effects`, and
`R-Value` before backend code generation.

- Add every cross-loop data dependency before scheduling. For each immediate
  sample value, record a typed chunk transport from producer to each consumer.
- Add or conservatively co-locate effect dependencies for mutable tables,
  impure/unknown foreign calls, UI writes, and shared state. A data-only DAG is
  insufficient for arbitrary legal reordering.
- Add a verifier that checks: acyclicity, edge endpoints, complete transports,
  region visibility, one owner for every materialized value, no owner for inline
  values, and fixed forward/reverse epoch ordering.
- Add a bounded event-order model that builds `<scalar`, `<vec`, and `D` for
  small plans. Exhaustively check that accepted certificates satisfy
  `FissionSafe`, including counterexamples with cross-loop carried state and
  conflicting effects.
- Extend `RegionTree` with one vector region per `LoopId`. Store materialized
  roots on each node and recursively lower their inline closures in canonical
  child order; do not filter a global `Hsched`.
- Replace the raw `SigId -> FirId` cache with a region-aware value cache. Reuse
  across siblings requires named transport storage; inline values may be
  recomputed.
- Run CSE independently inside each routed region.
- After the graph, epochs, and all names are frozen, call the generic scheduler
  on each induced epoch subgraph and emit epochs in fixed order. Do not add
  `-dfs` or a second loop strategy enum.
- Verify `-ss 0` against C++ `-dfs` topology and `-ss 3` against default C++
  `sortGraph` level membership. `-ss 1/2` are new vector policies validated by
  the generic graph contract.
- Assert that changing `-ss` changes only the per-epoch orders in
  `ExecutionSchedule::Vector`, never `VectorPlan`, epoch membership, ids,
  transports, or buffer names.

**Exit criterion:** shared expressions and pure prefixes/tails are separated
without inspecting FIR statements; all four strategies execute bit-exactly for
both `-lv 0` and `-lv 1`.

### P6 - Vector recursions, delays, clock domains, and AD

**Formal gate:** recursive and clock-island state transitions refine scalar
`Step`; forward/reverse AD epochs have an explicit simulation relation and may
not be justified by topological order alone.

- Route each recursion group into its serial loop and pure consumers into
  vectorizable loops.
- Connect the delay plan to `LoopNode` `pre/exec/post` phases: temporary and
  permanent copies for short delays, ring buffers for long delays.
- Cover multiple projections, multiple recursion groups, and recursive values
  read at several delays.
- For bounded histories and input sequences, exhaustively compare copy and ring
  representations through `alpha` against `history_step`/`delay_read`.
- Compare topology, storage sizes, and outputs with C++.
- Disable and then remove `partition_recursive_body` once all its tests are
  reproduced by the signal-level vector plan.
- Compose `VectorPlan` with `ClkEnvMap`: each OD/US/DS boundary becomes a
  serial `Island`, and only top-rate signals use buffers indexed by the outer
  chunk.
- Nest existing guarded regions below their `Island` region without duplicating
  counters or state.
- Treat FAD as an ordinary signal graph after transformation; pointwise tangent
  work can then be separated like primal work.
- Keep RAD/BRA scalar until reverse-window semantics under chunking are
  explicitly specified and tested. Forward and reverse execution remain
  separate fixed epochs even after vector support is enabled.

**Exit criterion:** no loop separation is discovered from a fused FIR body;
scalar/`-vec` bit-exactness holds for supported FAD and clocked islands, with an
explicit diagnostic for RAD shapes forced to scalar.

### P7 - Backend matrix, cost model, and prototype removal

**Formal gate:** translation validation checks `V-Simulation` for all supported
mode/strategy/backend combinations, including final state and observable effects.

- Generate and verify scalar artifacts with `-ss 0/1/2/3` for every CLI FIR
  consumer: C, C++, interpreter, Cranelift, WASM/WAST, AssemblyScript, and Julia.
- Generate and verify vector artifacts for `-lv 0/1` crossed with
  `-ss 0/1/2/3` on the same emitters, in single and double precision where
  supported.
- Execute the full impulse matrix on the six backends with existing runtime
  harnesses: C, C++, interpreter, Cranelift, WASM, and AssemblyScript. FIR,
  WAST, and Julia remain artifact/structural gates until they have equivalent
  impulse runners.
- Add unoptimized/optimized execution parity on a representative sub-corpus.
- Verify that all `-ss` strategies are bit-exact in scalar and vector modes. In
  vector mode they must produce identical `LoopGraph`/transport snapshots while
  allowing different loop orders. Also compare state after multiple blocks,
  tables, UI zones, and effectful cases, not only the first audio impulse.
- Add CLI and FFI integration cases proving that `-ss` reaches every backend and
  that unsupported or malformed values fail before factory creation.
- Measure buffer cost, then add a signal-level cost model to avoid splits whose
  transport costs more than the vectorizable work.
- Benchmark scheduling cost, temporary live ranges, and generated-code
  performance separately from loop-partition cost. Do not choose a default from
  the unused C++ `schedulingcost` heuristic without measurements.
- Remove transitional FIR paths and redundant implementation tests while
  retaining their DSP cases as signal-schedule regression tests.

**Exit criterion:** every emitter passes its artifact gate; every executable
impulse backend passes scalar `-ss 0/1/2/3` and `vec0`/`vec1` crossed with
`-ss 0/1/2/3`; optimized and unoptimized execution agree on the representative
corpus; vector mode has one strategy-independent source of truth for its loop
graph.

## 7. Risks and guardrails

### Occurrence semantics

The current Rust parent count is not equivalent to `OccMarkup`. The main risk is
under-materializing a value used across variability or execution-condition
contexts. Tests must cover contexts, not only raw edge counts.

### Cache visibility

A `FirId` cached in one region cannot be reused freely in a sibling region.
Every cross-region reuse must either be a rematerializable trivial expression
or use named storage. A verifier must enforce this before module emission.

### Recursion

A generic SCC over FIR is not a substitute for a signal recursion group. Group
identity and projections must remain visible through loop planning, especially
to reproduce `closeLoop` absorption.

### Delays

`max_delay` describes delayed use of the carried signal, not merely the presence
of a `Delay` node. The delay plan and loop plan must consume the same data to
avoid inconsistent storage geometry.

### Cost

Correctness does not depend on the cost model. First produce a correct split
that can be disabled, then decide whether it is profitable. Existing FIR-tail
measurements (`0.92x` for a simple multiply and `1.15x` for a heavier tail) show
that systematic separation is not always profitable.

### Scheduling stability

The C++ graph containers use ordered sets/maps whose ties ultimately follow
`Tree` ordering. Rust must define stable tie-breaking explicitly with `SigId` or
canonical insertion order. Strategy changes may reorder statements, but they
must not renumber loop identities or buffers; otherwise `-ss` would create
irrelevant golden churn and make performance comparisons ambiguous.

### Scheduling scope

Using one public `-ss` option does not imply scheduling the raw signal graph and
the loop graph consecutively in vector mode. Scalar and vector modes expose
different execution-unit DAGs. Applying one enum and one graph contract to those
two DAG types is the generalization; adding an undocumented second vector
scheduling layer would couple two performance variables and mishandle duplicated
inline expressions.

### Effects and state

Topological validity over data edges alone is insufficient when loop fission
changes sample-major execution into loop-major execution. Unknown foreign calls,
mutable tables, UI writes, and shared state need effect edges or conservative
co-location. The loop verifier must reject a plan that allows two conflicting
effects to be reordered, even if their audio values are data-independent.

### Formal-model boundary

`ScheduleCertificate` and `VectorPlanCertificate` establish finite structural
properties; they do not by themselves prove `V-Simulation`. Until a mechanized
semantics exists, semantic preservation remains a translation-validation
obligation discharged by scalar/vector traces, final-state comparison, C++
differential tests, and optimized/unoptimized execution parity. Conservative
effect or `VecSafe` fallbacks may reduce vectorization but must never be relaxed
only to satisfy a performance target.

### Default behavior

`-ss 0` remains the global Rust default. This matches C++ scalar scheduling but
maps vector mode to the C++ `-dfs` family rather than C++ default `sortGraph`.
That difference is intentional and user-visible. `-ss 3` is the closest vector
levelization compatibility setting, with deterministic Rust tie ordering.

## 8. Proposed decision

Adopt a strategy-independent signal-level `VectorPlan` as the target
architecture and retain the FIR partition only until P6 covers its regression
cases.

Expose one pluggable `-ss` option backed by one `SchedulingStrategy` and one
generic scheduler implementation. In scalar mode it serializes the hierarchical
signal DAGs. In vector mode it serializes each completed epoch subgraph of the
`LoopGraph`, without changing fixed epoch order. Keep vector partitioning,
epochs, ids, transports, and names strategy-independent, and do not port `-dfs`
as a separate public option.

This is closer to Faust C++ on the important point - dependencies remain signal
dependencies - while being more explicit and testable than its online loop
construction. It avoids a false equivalence between signal scheduling and loop
scheduling: the policy and graph contract are shared, while the scheduled node
granularity follows the active execution model. It also uses faust-rs strengths:
an immutable prepared forest, pure analyses, routed emission regions, and one
backend-neutral FIR shared by all backends. The formal contracts in section 5
and their executable certificates are mandatory acceptance gates for the port,
not optional documentation.
