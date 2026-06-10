# On-Demand Computation (`ondemand` / `upsampling` / `downsampling`): C++ Analysis and Rust Port Plan

Date: 2026-06-10

Status: proposed

C++ reference: ` RUST/faust`, branch
`master-dev-ocpp-od-fir-2-FIR19`, commit `8eebea429` ("Rework
InstructionsCompiler::generateFIR"). This branch is research-grade and still
evolving (see commit `f696b1858` "Simplified ondemand, upsampling, and
downsampling signal structure"); the analysis below reflects the current
`(H, Y...)` signal encoding, not the older `(H, X..., NIL, Y...)` one.

## 1. Goal

Analyze the *on-demand computation* concept introduced in the C++ Faust
compiler — the `ondemand(C)`, `upsampling(C)`, and `downsampling(C)`
primitives, which all use a **clock signal** (their first input) to decide
*when* the wrapped circuit `C` executes — and define a porting plan for
faust-rs.

Two companion documents extend this plan and are summarized in §9 and §10:
[ondemand-fad-rad-cohabitation-2026-06-10-en.md](ondemand-fad-rad-cohabitation-2026-06-10-en.md)
(FAD/RAD × clock domains, referenced as "cohabitation §N") and
[vector-mode-analysis-port-plan-2026-06-10-en.md](vector-mode-analysis-port-plan-2026-06-10-en.md)
(`-vec` vector mode and its composition with clock domains, referenced as
"vector doc §N"). The consolidated, ordered landing plan across all three
documents is
[ondemand-vec-fad-implementation-roadmap-2026-06-10-en.md](ondemand-vec-fad-implementation-roadmap-2026-06-10-en.md)
(the *roadmap*, phases P0–P9).

## 2. The on-demand computation concept

### 2.1 Primitives, syntax, arity

Syntax (parser:
[faustparser.y:704-710](RUST/faust/compiler/parser/faustparser.y)):

```
ondemand     ( expression )   →  boxOndemand(expr)
upsampling   ( expression )   →  boxUpsampling(expr)
downsampling ( expression )   →  boxDownsampling(expr)
```

Box arity
([boxtype.cpp:374-402](RUST/faust/compiler/boxes/boxtype.cpp)):
if `C : u → v` then `ondemand(C) : u+1 → v`. **The extra first input is the
clock signal `H`.** Same rule for `upsampling`/`downsampling`.

### 2.2 Clock semantics

The clock `H` decides when (and how many times) the body executes per outer
tick:

| Primitive | Clock interpretation | Generated control structure |
|---|---|---|
| `ondemand`, boolean clock (interval ⊆ [0,1]) | execute iff `H ≠ 0` | `if (H) { ... }` |
| `ondemand`, integer clock | execute `H` times | `for (od = 0; od < H; od++) { ... }` |
| `upsampling` | execute `H` times per outer tick | `for (us = 0; us < H; us++) { ... }` |
| `downsampling` | execute every `H`-th outer tick | `if ((H != 0) & (DSCounter % H == 0)) { ... }` with per-clock `DSCounter++` |

The boolean/integer distinction for `ondemand` is made at code-generation time
from the inferred interval of the clock type
([compile_scal.cpp:2223-2248](RUST/faust/compiler/generator/compile_scal.cpp),
[instructions_compiler.cpp:3517](RUST/faust/compiler/generator/instructions_compiler.cpp)).

Constant clocks are optimized away during propagation
([propagate.cpp:651-679](RUST/faust/compiler/propagate/propagate.cpp)):

- `H == 0`: the body never runs; the `m` outputs are replaced by `0`.
- `H == 1`: the wrapper is transparent; the body is propagated inline.

### 2.3 Data-flow semantics at the domain boundary

- **Inputs** (outer → inner): snapshotted into *temp vars* (`sigTempVar`)
  computed in the outer domain. For `upsampling`, inputs are additionally
  zero-padded (`sigZeroPad(x, H)`): the actual sample is delivered only on the
  *last* inner iteration, zeros otherwise.
- **Outputs** (inner → outer): stored in *perm vars* (`sigPermVar`), i.e.
  class-level **sample-and-hold** state initialized to `0`. When the block does
  not fire, consumers read the last held value.
- **Time is local to each domain.** Delay lines inside a domain use a
  dedicated `IOTA` counter incremented only when the block executes
  ([compile_scal.cpp:1797-1811](RUST/faust/compiler/generator/compile_scal.cpp)).
- **Sample rate adapts.** Inside `upsampling`, `ma.SR` becomes `SR * H`;
  inside `downsampling`, `SR / H`; the whole stack of nested US/DS factors is
  unrolled
  ([propagate.cpp:282-306](RUST/faust/compiler/propagate/propagate.cpp)).

### 2.4 Reference generated code (captured from the branch binary)

`process = (button("gate"), _) : ondemand(@(44));` — note the *local* IOTA:

```c++
for (int i0 = 0; i0 < count; i0 = i0 + 1) {
    float fTemp0SE = static_cast<float>(input0[i0]);   // TempVar (outer domain)
    if (iSlow0BE) {                                    // OD block (boolean clock)
        fVec0SE[IOTA0 & 63] = fTemp0SE;
        fPermVar0SE = fVec0SE[(IOTA0 - 44) & 63];      // PermVar (sample & hold)
        IOTA0 = IOTA0 + 1;                             // local time advances only here
    }
    output0[i0] = static_cast<FAUSTFLOAT>(fPermVar0SE);
}
```

`process = (hslider("factor",2,1,8,1), _) : upsampling(+ ~ _);` — inner loop,
zero-padded input, inner recursive state crossing iterations:

```c++
if (iSlow1BE != 0) {
    for (int us0 = 0; us0 < iSlow1BE; us0 = us0 + 1) {
        float fRec0SE = ((us0 == (iSlow0BE - 1)) ? fTemp0SE : 0.0f) + fVec0SE[1];
        fVec0SE[0] = fRec0SE;
        fPermVar0SE = fVec0SE[0];
        fVec0SE[1] = fVec0SE[0];
    }
}
output0[i0] = static_cast<FAUSTFLOAT>(fPermVar0SE);
```

`downsampling(*(2))`:

```c++
if ((iSlow0BE != 0) & ((DSCounter0 % iSlow0BE) == 0)) {
    fPermVar0SE = 2.0f * fTemp0SE;
}
output0[i0] = static_cast<FAUSTFLOAT>(fPermVar0SE);
DSCounter0 = DSCounter0 + 1;
```

## 3. C++ implementation analysis, stage by stage

### 3.1 Box layer and evaluation

- Constructors/destructurers:
  [boxes.cpp:300-325](RUST/faust/compiler/boxes/boxes.cpp).
- Eval is purely structural — evaluate the body, rebuild the wrapper:
  [eval.cpp:622-629](RUST/faust/compiler/evaluate/eval.cpp).

### 3.2 Propagation: building the clocked signal graph

`propagate` gained a **first parameter** `clockenv` threaded through the whole
recursion, and the memoization key includes it
([propagate.cpp:918-929](RUST/faust/compiler/propagate/propagate.cpp)).

For `isBoxOndemand(box, C)` with `lsig = [H, x1..xn]`
([propagate.cpp:644-728](RUST/faust/compiler/propagate/propagate.cpp)):

1. Trivial cases `H==0` / `H==1` (§2.2).
2. `clockenv2 = makeClockEnv(clockenv, slotenv, path, box, lsig)` — a **new
   nested clock environment**, unique per instance (see §3.4).
3. Inputs: `X1[i] = sigTempVar(lsig[i+1])`, then
   `X2[i] = sigDoubleClocked(clockenv2, clockenv, X1[i])` — "computed in the
   outer env, used in the inner env"
   ([signals.cpp:1174-1187](RUST/faust/compiler/signals/signals.cpp)).
   For US: wrapped in `sigZeroPad(•, H)`.
4. Body propagated under `clockenv2` → `Y0`.
5. Outputs: `Y1[i] = sigPermVar(sigClocked(clockenv2, Y0[i]))`.
6. Block node: `OD = sigOD([sigClocked(clockenv2, H), Y1...])` — since commit
   `f696b1858` the node carries only `(H, Y...)`; the internal `X` signals are
   reached through hierarchical scheduling, not stored as subsignals.
7. Results: `Y2[i] = sigSeq(OD, Y1[i])` — a **sequencing constraint**: the OD
   block must be compiled/executed before its perm-var outputs are read.

US ([propagate.cpp:729-810](…)) and DS ([propagate.cpp:812-893](…)) follow the
identical scheme; only the input zero-padding (US) and the generated control
structure differ.

Two other propagation sites are clock-aware:

- `boxSymbolic`/slots: a signal bound to a slot is annotated
  `sigClocked(clockenv, sig)`; if later *used* under a deeper env, it is
  re-wrapped by `recTempVar(useEnv, defEnv, sig)` which builds a **stack of
  temp vars, one per env level**
  ([propagate.cpp:335-360](…), [signals.cpp:1418-1425](…)).
- `boxWaveform`: its first element gets `sigClocked(clockenv, …)` so the
  waveform index belongs to the right time reference
  ([propagate.cpp:269-277](…)).

### 3.3 New signal IR nodes

| Node | Shape | Role |
|---|---|---|
| `SIGOD` / `SIGUS` / `SIGDS` | variadic `(H, Y1..Ym)` | block marker; `H = sigClocked(clkenv2, clock)`, `Yi = PermVar` outputs |
| `SIGCLOCKED` | `(clkenv, sig)` | annotation "sig has time reference clkenv"; **the first child is a clock environment, not a signal** |
| `SIGTEMPVAR` | `(sig)` | local snapshot variable at a domain boundary |
| `SIGPERMVAR` | `(sig)` | persistent sample-and-hold variable, init 0 |
| `SIGZEROPAD` | `(sig, H)` | upsampling input: value on last inner iteration, else 0 |
| `SIGSEQ` | `(x, y)` | compile/execute `x` before `y`; value is `y` |

`sigClocked` canonicalization
([signals.cpp:1149-1172](…)): re-clocking with the same clock is idempotent;
clocking a `FIR`/`IIR` node pushes the annotation onto its input slot instead.

### 3.4 Clock environments (HE)

```
HE ::= nil | cons(parent, cons(slotenv, cons(path, cons(box-as-prim0, inputsigs...))))
```

([signals.cpp:1343-1416](…)). Key accessors: `getClockenvEnv` (parent, `hd`),
`getClockenvClock` (`nth(4)` = first input signal = the clock),
`getClockenvBox` (drives `isODClockenv` / `isUSClockenv` / `isDSClockenv`).

**Identity is the whole tuple.** Including `slotenv` and `path` is essential:
an earlier bug (documented in
[clkEnvInference.cpp:565-572](RUST/faust/compiler/signals/clkEnvInference.cpp))
made two structurally-identical but semantically-distinct ondemand instances
share one clkEnv under de Bruijn hash-consing, corrupting scheduling.

`nil` is the top-level (audio-rate) environment and the universal ancestor.

### 3.5 Typing rules

([sigtyperules.cpp:890-940](RUST/faust/compiler/signals/sigtyperules.cpp))

- `TempVar(x)`, `Clocked(_, x)`, `Seq(_, y)`: transparent (type of the payload).
- `PermVar(x)`, `ZeroPad(x, _)`: `sampCast(T(x))` with interval `∪ {0}`
  (initial value / padding is 0).
- `OD/US/DS`: type all subsignals for side effects, then return an artificial
  `kReal, kSamp, kExec` type with interval `[-1,1]` — a stand-in for a missing
  bottom type that must **not** be constant, or constant propagation would
  delete the block.

### 3.6 Clock environment inference (the formal core)

[clkEnvInference.cpp](RUST/faust/compiler/signals/clkEnvInference.cpp)
(638 lines) implements `C⟦sig⟧ᴴ·ᴹ = c` ("signal `sig` is computed in clock
environment `c`"), per a rules document `ClockEnvironmentInferenceRulesV2.md`
(not present in the repo). It answers: *for every signal, which domain
computes it?* — the information scheduling and code generation need.

- **Partial order**: `isAncestorClkEnv(c1, c2)` — reflexive, transitive,
  `nil ⊆ c` for all `c`; walks parent links.
- **Join**: `maxClkEnv{c1..cn}` = deepest env; all must be pairwise
  comparable, otherwise `ERROR: Incomparable clock environments` (parallel
  domains may not exchange un-annotated signals).
- **Rules** (`inferClkEnvWithHypothesis`):
  - `R_PROJ`: for `proj(i, W)`, seed the cache with `H(W)` for all
    projections of the group, infer each definition, result =
    `max` of the definitions' envs.
  - `R_CLOCKED(c, s)`: require `C⟦s⟧ ⊆ c`; result `c`. (Re-clocking moves a
    signal *deeper*, never shallower.)
  - `R_CD` (OD/US/DS): first child must be `sigClocked(c_inner, h)`; require
    `C⟦h⟧ ⊆ parent(c_inner)` (the clock is computed outside); every other
    child must live *exactly* in `c_inner` (exception: literal `0`); result =
    `parent(c_inner)` — the block as a whole belongs to the outer domain.
  - `R_SEQ(x, y)`: require `C⟦x⟧ ⊆ C⟦y⟧`; result `C⟦x⟧`.
  - Tables (`rdtable`, read-write): `max` of read/write index/signal envs.
  - `R_COMPOSITE` (default): `max` over subsignals.
- **Fixed point** (`findFixpoint`): hypothesis `H : RecGroup → ClkEnv`
  initialized to `nil` for every group collected by `collectRecGroups`;
  iterate `H(W) ← max(env of each definition)` until stable (safety bound
  1000 iterations). This handles recursions spanning domain boundaries
  without annotations.
- Results are attached as a tree property (`CLKENVPROPERTY`) and queried via
  `ClkEnvInference::getClkEnv(sig)` during graph construction.

### 3.7 Hierarchical dependency graph and scheduling

[DependenciesGraph.cpp](RUST/faust/compiler/Dependencies/DependenciesGraph.cpp)
builds:

```c++
struct Hgraph {
    Tree outSigList;                          // entry key
    digraph<Tree> controls;                   // < kSamp signals, scheduled before the loop
    std::map<Tree, digraph<Tree>> siggraph;   // one dependency graph per domain
};
```

`addDependencies(curClkEnv, …, curSig)` walks from the outputs:

- `needSubGraph(sig)` ⇔ `isSigOD/US/DS`
  ([DependenciesUtils.cpp:14-21](RUST/faust/compiler/Dependencies/DependenciesUtils.cpp)):
  the OD signal becomes the **key of a new subgraph**; its contents are
  populated by recursing under the inner env; the clock itself stays outside.
- `isExternal(clkEnv, sig)` ⇔ `getClkEnv(sig)` is a *strict ancestor* of the
  current env: the signal is computed elsewhere; inside a subgraph it becomes
  an edge `OD → external` in the *parent* graph; at top level it lands in the
  `controls` graph (compiled before the sample loop).
- `getSignalDependencies` distinguishes immediate vs delayed deps (a
  `delay ≥ 1` dependency imposes no intra-tick ordering);
  `sigSeq(od, y)` depends **only on `od`** — reading the perm var is free once
  the block ran.

[DependenciesScheduling](RUST/faust/compiler/Dependencies/DependenciesScheduling.hh)
converts `Hgraph` to `Hsched` (same shape, `schedule<Tree>` per graph) with a
pluggable strategy (`dfschedule`/`bfschedule`/`spschedule`/`rbschedule`,
selected by `-ss`).

(The older flat helpers in
[sigDependenciesGraph.cpp](RUST/faust/compiler/transform/sigDependenciesGraph.cpp)
— `ondemandGraph` stops at temp-var boundaries, `isSigOD` deps = clock only —
serve the single-signal/debug paths.)

### 3.8 Code generation

Both `ScalarCompiler` (`-ocpp`) and `InstructionsCompiler` (FIR backends)
share the flow
([compile_scal.cpp:592-642](RUST/faust/compiler/generator/compile_scal.cpp),
[instructions_compiler.cpp:649-700](RUST/faust/compiler/generator/instructions_compiler.cpp)):

1. `ClkEnvInference::annotate(L)`;
2. `fHschedule = scheduleSigList(L, strategy)`;
3. compile `controls` first, then `sigsched[L]` in order, then outputs.

Per-node generators
([compile_scal.cpp:2181-2284](…), [instructions_compiler.cpp:3479-3543](…)):

- `generateTempVar` → local variable store.
- `generatePermVar` → class field, cleared to 0, assigned inside the block.
- `generateZeroPad` → `((loopIdx == H-1) ? x : 0)` using the *current* inner
  loop index.
- `generateOD` → open `if`-block (boolean clock) or OD `for`-block, compile
  `fHschedule.sigsched[odSig]` elements inside, close. Returns no expression.
- `generateUS` / `generateDS` → same with US `for`-block / DS modulo-guard
  block (`declareRetrieveDSName` allocates the per-clock `DSCounter`).
- `sigSeq(od, y)` → `CS(od); return CS(y);`
- `sigClocked(c, y)` → `CS(y)` (annotation only at this stage — but the clock
  differentiates hash-consed delay lines across domains, and per-domain `IOTA`
  ring-buffer counters are keyed by the clock via `declareRetrieveIotaName`).

Block emission lives in
[loop.cpp:190-310](RUST/faust/compiler/parallelize/loop.cpp)
(`CodeIFblock`/`CodeODblock`/`CodeUSblock`/`CodeDSblock` on a code stack) for
ocpp, and equivalent structured instructions for the FIR path. `-vec` is
explicitly unsupported with ondemand.

## 4. The algorithms in detail: properties, correctness, complexity

### 4.1 Clock environment inference is a (simplified) clock calculus

The inference of §3.6 is best understood as a monomorphic **clock calculus**
in the tradition of synchronous languages (Lustre/Signal/Lucid Synchrone):
`ondemand` plays the role of Lustre's `when` (activation condition), the
`PermVar` outputs play the role of `current` (sample-and-hold when re-entering
the slower world), and the inference assigns every signal its clock. The C++
system is deliberately *much* simpler than a full clock calculus: no clock
polymorphism, no boolean-clock unification — domains must be **strictly
nested**, which turns clock checking into order checking on a tree.

**The abstract domain.** The set of clock environments created during
propagation, plus `nil`, ordered by `isAncestorClkEnv`. Since every env has
exactly one parent chain ending at `nil`
([signals.cpp:1350-1355](RUST/faust/compiler/signals/signals.cpp)),
the domain is a **finite tree rooted at `nil`** — `nil` is the bottom element
(audio rate, universally visible). Its height `h` is the maximal OD/US/DS
nesting depth of the program (in practice 1-3).

- `isAncestorClkEnv(c1, c2)` is the reflexive-transitive ancestor relation,
  computed by walking parent links: O(h) per query.
- `maxClkEnv{c1..cn}` is the join *restricted to chains*: it requires all
  arguments to be pairwise comparable and throws otherwise
  ([clkEnvInference.cpp:113-138](RUST/faust/compiler/signals/clkEnvInference.cpp)).
  This partiality is the **scoping rule** of the system: a computation may
  combine values from its own domain and from ancestor domains, never from a
  sibling domain. Sibling exchange must go through the explicit boundary
  machinery (PermVar held values read from the common ancestor).

**The transfer function.** `inferClkEnvWithHypothesis` is a bottom-up
synthesis over the signal DAG: composites take the `max` of their children;
`sigClocked(c, s)` *lifts* to `c` after checking `C⟦s⟧ ⊆ c` (annotations may
deepen a signal, never shallow it); OD/US/DS nodes pop back to
`parent(c_inner)`. Two consequences worth stating:

- The env of a signal is determined **only by its inputs**, never by its
  consumers. This is what makes inference well-defined under hash-consing:
  a shared subtree used by two domains has one env (the deepest *input*
  requirement), and each consumer either lives there or sees it as an
  external/ancestor signal.
- All domain *entries* are explicit (`sigClocked` wrappers inserted by
  propagation at boundaries), so the inference never has to guess: it only
  propagates and checks.

**The fixed point.** Recursive groups are the one place where bottom-up
synthesis is circular: a projection's env depends on its definitions, which
reference the projections. `findFixpoint`
([clkEnvInference.cpp:424-486](RUST/faust/compiler/signals/clkEnvInference.cpp))
runs a **Kleene iteration**: `H : Group → ClkEnv` starts at bottom
(`nil` for every group) and each round recomputes
`H'(W) = max{C⟦def_i⟧ under H}` until `H' = H`.

Properties:

- **Jacobi-style iteration**: within one round, every group is evaluated
  against the *previous* hypothesis (projections are seeded into the cache
  from `H` before inferring), so the result is independent of group
  processing order — deterministic.
- **Monotonicity**: `H` only appears positively (under `max`), so each
  round can only deepen the envs. The domain has finite height `h`, hence
  termination in at most `|groups| × h` rounds (the `MAX_ITERATIONS = 1000`
  bound is pure safety; real programs converge in ≤ 2-3 rounds).
- **Least fixed point**: starting at bottom yields the *shallowest*
  consistent assignment. Semantically: a recursive group is placed in the
  outermost domain compatible with its definitions, i.e. as visible as
  possible — a group not touching any domain-internal signal stays at audio
  rate; a group whose definitions read clocked inputs of domain `c` is pulled
  exactly into `c`, not deeper.
- **Complexity**: each round visits each signal once (memo cache `M`),
  so O(rounds × |signals|); the per-iteration cache reset makes it
  non-incremental but keeps the implementation trivially correct.

### 4.2 Hierarchical graph construction: a partition by domain

`dependenciesGraphs`
([DependenciesGraph.cpp:48-187](RUST/faust/compiler/Dependencies/DependenciesGraph.cpp))
is a **single DFS from the outputs** that *dispatches* each signal into the
dependency graph of its inferred domain:

- a global `visited` set + the env dispatch guarantee the **partition
  property**: each signal lands in exactly one graph (parent graph, one
  subgraph, or `controls`). This property is machine-checked by
  `auditHgraph`
  ([DependenciesAudit.hh:21-31](RUST/faust/compiler/Dependencies/DependenciesAudit.hh)).
- on meeting an OD/US/DS node, a **subgraph keyed by that node** is created
  and populated by recursing under the inner env; the clock is deliberately
  kept out of the subgraph (it is a block *precondition*, not block content).
- a signal whose env is a *strict ancestor* of the current env is
  **external**: inside a subgraph it surfaces as an edge `ODnode → external`
  in the *parent* graph ("the block needs this computed first"); at top level
  it lands in `controls` (compiled before the sample loop).
- **immediate vs delayed** dependencies
  ([DependenciesUtils.cpp:77-187](RUST/faust/compiler/Dependencies/DependenciesUtils.cpp)):
  a `delay ≥ 1` dependency creates no same-tick ordering edge (the value is
  read from state) but is still traversed so its defining computation lands
  in the right domain. `sigSeq(od, y)` depends only on `od` — once the block
  ran, reading the perm var is free.

**Correctness condition**: each per-domain graph must be a DAG on immediate
edges — instantaneous cycles inside a domain are causality errors, exactly as
in classic Faust; they are detected at schedule serialization. Cross-domain
cycles are impossible by construction (edges only point from a block node to
ancestor-domain signals).

**Complexity**: O(V + E), one traversal, no fixed point needed here — all the
hard decisions were made by the inference.

### 4.3 Scheduling and emission

`scheduleSigList` maps every graph of the `Hgraph` to a `schedule<Tree>`
through a pluggable topological-sort strategy (`df`/`bf`/`sp`/`rb` — standard
toposorts with different tie-breaking, selected by `-ss`). Determinism is per
strategy. Code emission then walks the *top-level* schedule; when `CS`
reaches an OD/US/DS node it opens the matching block, compiles **that node's
sub-schedule** recursively, and closes the block
([compile_scal.cpp:2223-2284](RUST/faust/compiler/generator/compile_scal.cpp)).
The LIFO code-stack in
[loop.cpp:190-310](RUST/faust/compiler/parallelize/loop.cpp)
guarantees structured (properly nested) output.

End-to-end ordering correctness rests on two pieces working together: the
graph gives `seq(OD, permvar)` consumers a path through the OD node, so every
toposort emits the block before its readers; and `sigSeq`'s code generator
(`CS(od); return CS(y)`) enforces the same order even on the single-signal
debug path.

## 5. Is the C++ implementation good? What should Rust copy, simplify, or drop?

### 5.1 What is genuinely good

- **The phase architecture.** Propagation marks boundaries → inference
  assigns domains → graph construction partitions → scheduling orders →
  emission materializes. Each phase has one responsibility, communicates
  through the signal tree plus one side map, and is independently testable.
  This mirrors how synchronous-language compilers are built, and it is the
  right shape; re-deriving it from scratch in Rust would most likely converge
  to the same design after re-discovering the same bugs.
- **The formal grounding.** The inference implements a written rule system
  (`ClockEnvironmentInferenceRulesV2.md`, referenced in
  [clkEnvInference.cpp:43](RUST/faust/compiler/signals/clkEnvInference.cpp));
  monotone fixed point over a finite domain, deterministic, terminating —
  the algorithmic core is principled and *small* (≈ 640 lines).
- **The strictly-nested domain restriction.** Renouncing clock polymorphism
  and sibling communication keeps checking decidable-by-construction and
  cheap, while covering the practical use cases (control-rate blocks,
  oversampled sections, conditional computation).
- **The `(H, Y...)` encoding** after commit `f696b1858`: block internals are
  reached through scheduling rather than stored as subsignals — less
  redundancy, fewer invariants to maintain.
- **Auditability**: `auditHgraph` and the (now disabled)
  `testClkEnvUniqueness` show the invariants were identified and checked.

### 5.2 Weak points (evidence in code)

1. **Clock env identity is encoded structurally**, as a hash-consed cons
   tuple — and the box is smuggled in via a *function-pointer cast*:
   `boxPrim0((prim0)box)`
   ([signals.cpp:1353](RUST/faust/compiler/signals/signals.cpp)).
   Structural identity for an entity whose whole point is *instance* identity
   is fragile: the de Bruijn collision bug (§3.4) happened precisely because
   the tuple was once too small. The fix was to enlarge the tuple
   (slotenv + path), not to change the representation.
2. **The pseudo-bottom type hack**: OD/US/DS nodes are typed
   `kReal/kSamp/interval(-1,1)` with an apologetic comment ("we lack a bottom
   type! But it must NOT be a constant type…",
   [sigtyperules.cpp:915-917](RUST/faust/compiler/signals/sigtyperules.cpp))
   so that constant propagation does not delete blocks.
3. **Residual layering debt**: the generic `getSignalDependencies` contains
   a branch for OD/US/DS marked "HACK, for test purposes only"
   ([DependenciesUtils.cpp:169-176](RUST/faust/compiler/Dependencies/DependenciesUtils.cpp));
   the older flat helpers (`ondemandGraph`, `compilationOrder` in
   [sigDependenciesGraph.cpp](RUST/faust/compiler/transform/sigDependenciesGraph.cpp))
   coexist with the hierarchical machinery.
4. **Global mutable state**: inference results are stored as tree properties
   (`CLKENVPROPERTY` via `setProperty`), the usual C++-Faust pattern.
5. **Error quality**: domain violations throw bare `faustexception`s with no
   source location ("ERROR: Incomparable clock environments\n").
6. **Research scaffolding**: commented-out debug blocks throughout, disabled
   audit calls, `-vec` unsupported. The branch is a working prototype, not a
   hardened subsystem.

None of these weaknesses are *algorithmic*: the rules, the fixed point, the
partition, and the scheduling are sound. They are representation and
engineering choices constrained by the legacy C++ infrastructure (everything
must be a `Tree`).

### 5.3 Could Rust simplify, or is a faithful port enough?

The recommendation is: **port the algorithms 1:1, modernize the
representations**. Concretely:

| C++ mechanism | Rust verdict |
|---|---|
| Inference rules + Kleene fixed point (§4.1) | **Port 1:1.** Small, principled, battle-tested. Keep rule names in comments for parity audits. |
| Two-phase design (infer, then partition) | **Keep.** It cannot be fused into propagation: hash-consing shares subtrees across domains and later transforms (promotion, FIR/IIR conversion) create new nodes, so envs must be (re)computable bottom-up after the fact. faust-rs has the same constraint (`TreeArena` interning). |
| Clock env as cons tuple + `(prim0)box` cast | **Replace** with a dedicated side-table arena: `struct ClockDomain { parent: ClockDomainId, kind: OdKind, clock: SigId, instance: UniquenessToken }` referenced by integer id. Same semantics, no structural-collision class of bugs, O(1) parent/depth access. The uniqueness token replaces slotenv+path. |
| `CLKENVPROPERTY` tree property | **Replace** with `HashMap<SigId, ClockDomainId>` returned by the inference — already the faust-rs house style. |
| Pseudo-bottom type for OD/US/DS | **Replace** with an explicit type variant (or a `is_block` flag) that simplification passes must skip. Cheap to do when the type already lives in a Rust enum. |
| `isAncestorClkEnv` chain walks | Keep the algorithm; storing `(parent, depth)` per domain makes ancestor checks O(Δdepth) with trivial code. Micro-level, not structural. |
| `Hgraph`/`Hsched` two-level structures | **Port 1:1** (`HashMap<SigId, Digraph>`); the partition + audit properties carry over directly. Port `auditHgraph` as debug assertions — it encodes the invariants. |
| 4 scheduling strategies | **Reduce** to one deterministic toposort (df) initially; the strategy interface can come later if needed. |
| Flat legacy helpers (`ondemandGraph`, `compilationOrder`) | **Drop.** Only the hierarchical path matters. |
| LIFO code-stack block emission | **Simplify naturally**: faust-rs lowers into a structured FIR; recursive lowering of sub-schedules into nested block nodes needs no stack at all. |
| Bare `faustexception` errors | **Improve**: structured `FRS-` diagnostics naming the two incomparable domains and the offending signal. |

Two things Rust should *not* attempt to simplify:

- **The double clocking of inputs** (`sigDoubleClocked`, §3.2). The
  inside/outside asymmetry between inputs (double-clocked) and outputs
  (single-clocked + PermVar) looks redundant but encodes a real distinction —
  where a value is *computed* vs where it is *used* — that the inference
  rules (`R_CLOCKED` chain) rely on. Changing it means re-deriving the rule
  system.
- **The least-fixed-point placement of recursive groups.** One could imagine
  annotating envs during propagation instead (propagation knows `clockenv`
  when building each node), but this is ill-defined under hash-consing (a
  shared node has many creation contexts) and breaks for nodes created by
  post-propagation transforms. The bottom-up inference is not an accident;
  it is the correct formulation.

In short: the C++ implementation is **algorithmically trustworthy and
architecturally right, but carries prototype-grade encodings**. A faithful
semantic port with idiomatic Rust data structures gets the best of both —
and the parity test surface (same rules, same fixtures, differential runs
against the branch binary) stays meaningful because the semantics are
unchanged.

## 6. Current faust-rs state

### 6.1 Already ported ✅

| Stage | Location | Notes |
|---|---|---|
| Lexer/parser | `crates/parser/src/grammar/faustlexer.l:138-140`, `faustparser.y:693-699` | `ondemand`/`upsampling`/`downsampling` keywords |
| Box nodes | `crates/boxes/src/matcher.rs:157-159` | `BoxMatch::Ondemand/Upsampling/Downsampling` + builders |
| Eval | `crates/eval/src/apply.rs:608` | structural rebuild, mirrors C++ |
| Signal nodes | `crates/signals/src/lib.rs` | `SIGOD/SIGUS/SIGDS/SIGCLOCKED/SIGTEMPVAR/SIGPERMVAR/SIGZEROPAD/SIGSEQ`, `clocked` canonicalization, `double_clocked` |
| Propagation | `crates/propagate/src/engine.rs:805-915` | `propagate_clocked_wrapper` mirrors C++ §3.2 including trivial clock cases |
| Typing | `crates/sigtype/src/rules.rs:710-747` | full parity incl. the non-constant pseudo-bottom type |

### 6.2 Missing ❌

1. **Clock environment inference** (§3.6) — nothing exists.
2. **Hierarchical dependency graph + scheduling** (§3.7) — `signal_fir` has no
   notion of per-domain subgraphs or block scheduling.
3. **signal_fir lowering** of `TempVar/PermVar/ZeroPad/Seq/Clocked/OD/US/DS` —
   today these nodes reach `signal_prepare`/`signal_fir` and fail.
4. **FIR structured blocks** — the FIR IR does have generic structured
   statements (`Block`, `If`, `SimpleForLoop`, `ForLoop` in
   [matcher.rs:173-205](RUST/faust-rs/crates/fir/src/matcher.rs)), but they
   are only used in init/clear/table-generator paths today; the compute
   lowering has no notion of guarded sub-blocks *inside the sample loop*,
   and no backend has ever consumed such control flow in `compute`
   (C/C++ emission, interp bytecode, cranelift, wasm, julia).
5. **Per-domain time** — per-clock `IOTA`/`DSCounter` counters and
   block-scoped delay-line updates.
6. **SR adaptation** under US/DS in propagation
   (C++ [propagate.cpp:282-306](…)) — to verify/port in the Rust FConst path.

### 6.3 Known divergences / bugs to fix

- **`signal_prepare` traverses the clock env as a signal.** In
  `verify_prepared_signal`
  ([signal_prepare.rs:560]( RUST/faust-rs/crates/transform/src/signal_prepare.rs)),
  `SigMatch::Clocked(x, y)` visits `x` — but `x` is a clock-environment tree
  list, producing `FRS-SFIR-0004 … unexpected list/nil node`. Reproduced with
  `process = (button("gate"), _) : ondemand(*(2));`. The clock env must be
  treated as an opaque annotation everywhere outside clock-specific code.
- **`make_clock_env` leaves `slotenv`/`path` nil**
  ([engine.rs:1086-1098]( RUST/faust-rs/crates/propagate/src/engine.rs)).
  This reintroduces exactly the uniqueness bug C++ fixed (§3.4): two ondemand
  instances of the same circuit with the same inputs in different contexts
  would share a clkEnv. Must be fixed before inference is built on top.
- **Propagate memoization key** must include the clock env (C++ does;
  verify the Rust `propagate_in_slot_env` cache key does too).

## 7. Port plan

Recommended order; each step is independently testable. The cross-document
landing order — interleaving these steps with the FAD/RAD phases and the
vector-mode steps — is maintained in the
[roadmap](ondemand-vec-fad-implementation-roadmap-2026-06-10-en.md)
(phases P0–P9); this section remains the technical reference for each step.

### Step 1 — Make clocked signals survive `signal_prepare` (small)

- Treat `Clocked(clkenv, y)` as annotation: never traverse `clkenv` as a
  signal (verification, simplification, occurrence analysis, CSE).
- Add `Seq/TempVar/PermVar/ZeroPad/OD/US/DS` cases where missing.
- Fix `make_clock_env` to include a uniqueness token equivalent to C++
  `slotenv`+`path` — or move directly to the `ClockDomain` side-table
  representation recommended in §5.3, which makes uniqueness a non-issue.
- Audit the propagate cache key for `clock_env`.
- Tests: structural pipeline tests asserting `signal_prepare` passes on
  od/us/ds fixtures and that two distinct instances yield distinct clk envs.

### Step 2 — Port clock environment inference (medium)

New module, e.g. `crates/transform/src/clk_env.rs` (or a dedicated crate):

- `is_ancestor_clk_env`, `max_clk_env` (error on incomparable),
  `collect_rec_groups`, `find_fixpoint`, `infer_clk_env_with_hypothesis`
  following §3.6 / §4.1 rule by rule (keep the C++ rule names — `R_PROJ`,
  `R_CLOCKED`, `R_CD`, `R_SEQ` — in comments for parity audits).
- Representation per §5.3: a `ClockDomain` arena (`parent`, `kind`, `clock`,
  uniqueness token) plus `HashMap<SigId, ClockDomainId>` returned by an
  `annotate(signals)` entry point (no tree-property mutation, matching
  faust-rs style).
- Tests: one unit test per rule (R_PROJ fixed point, R_CLOCKED violation,
  R_CD domain checks, R_SEQ, incomparable-domain error), mirroring C++
  behavior observed on the branch binary.

### Step 3 — Hierarchical dependency graph and schedule (medium)

Port `DependenciesUtils` + `DependenciesGraph` + `DependenciesScheduling`:

- `Hgraph { out_sigs, controls: Digraph, siggraph: HashMap<SigId, Digraph> }`,
  `needs_subgraph`, `is_external`, `get_signal_dependencies`
  (immediate/delayed split, `Seq` depending only on the block).
- A small deterministic digraph + topological schedule is enough to start
  (depth-first, matching `-ss 0`); pluggable strategies can come later.
- Tests: schedule snapshots on the §2.4 fixtures (block keyed subgraph,
  clock outside, externals hoisted to controls).

### Step 4 — FIR structured blocks (large, the architectural step)

Extend the FIR IR (`crates/fir`) with a block instruction:

- `CondBlock { cond: ValueId, body: Vec<Stmt> }` (OD boolean / DS guard) and
  `LoopBlock { count: ValueId, index: LocalId, body: Vec<Stmt> }` (OD integer
  / US), nestable. Before adding new node kinds, evaluate reusing the
  existing generic `If`/`SimpleForLoop`/`Block` FIR statements (§6.2 item 4;
  vector doc §4): the gap is in the compute-lowering organization, not necessarily
  in the IR vocabulary.
- Perm vars: struct fields cleared to 0; temp vars: locals in the outer body.
- Per-clock `IOTA`/`DSCounter` fields incremented in block post-code.
- CSE/occurrence passes (`signal_fir/cse.rs`) must become block-aware:
  expressions computed inside a block may not be hoisted out, and vice versa
  values used in a block but computed outside stay outside (that is exactly
  what `TempVar` encodes).

### Step 5 — `signal_fir` lowering of OD/US/DS (medium, depends on 2-4)

Mirror `generateOD/US/DS/TempVar/PermVar/ZeroPad` + `Seq`/`Clocked`
passthrough, driven by the `Hsched` from Step 3:

- compile `controls` first, then the top-level schedule; on reaching an
  OD/US/DS node, open the matching block, compile its sub-schedule, close.
- Delay lines inside a domain use that domain's IOTA.
- Boolean-vs-integer OD choice from the interval of the clock type (the
  interval crate is already wired into `signal_prepare`).

### Step 6 — Backend emission (per backend)

- **C/C++ backends** first: structured text emission of the new blocks is
  direct.
- **Interp bytecode**: needs conditional/loop opcodes (or block-call
  indirection) — the largest backend change.
- **Cranelift/wasm/julia**: native control flow exists; mostly lowering work.
- Until a backend supports blocks, emit a clear
  `FRS-SFIR` diagnostic ("ondemand not supported by backend X yet").

### Step 7 — SR adaptation + UI inside domains (small)

- Port the `ma.SR` US/DS adaptation in the Rust FConst propagation path.
- Verify sliders/buttons inside an ondemand body reach the UI tree with the
  right path (C++ threads `path` through the clock env for this reason).

### Step 8 — Differential validation (continuous)

- Golden corpus: stateless body, delay body (per-domain IOTA), recursive body
  (`+ ~ _`), nested domains (`ondemand` in `ondemand`, US in DS), constant
  clocks 0/1, `ma.SR` under US/DS, UI inside the body, multi-instance
  uniqueness.
- Differential harness against the branch binary
  (`RUST/faust/build/bin/faust`, version 2.84.3 on
  this branch) — impulse-response comparison like the existing
  `cpp_signal_differential` tests. The branch has dedicated ondemand
  impulse-test targets to reuse as corpus sources.

Suggested sizing: Steps 1-2 are self-contained and low-risk; Step 4 is the
critical path and should be designed against the existing
`faust-rust-fir-architecture-en.md` notes before coding.

## 8. Risks and open questions

1. **Reference instability.** The C++ branch is actively reworked (signal
   encoding changed in `f696b1858`). Pin parity tests to commit `8eebea429`
   and re-sync deliberately.
2. **Clock env uniqueness** is a correctness cliff (C++ already hit it);
   Step 1 makes it a precondition for everything else.
3. **Block-aware CSE/scheduling** in `signal_fir` may interact with the
   existing delay-merging and recursion passes; the `fLimitOndemand`
   ("don't go beyond tempvar") C++ graph mode shows where boundaries must cut.
4. **`-vec`-style optimizations** are explicitly unsupported with ondemand in
   C++ (the research branch disables `-vec` entirely). The vector doc (§10) analyzes the C++
   vector mode, why the conflict exists upstream, and a faust-rs design
   ("scalar islands") under which `-vec` + clock domains needs no rejection:
   clocked blocks compile as serial loop nodes inside the vector loop DAG
   with exact semantics.
5. **Interaction with FAD/RAD**: analyzed in depth in the cohabitation doc (§9). Short version:
   `fad` strictly *inside* one domain works with no new AD code once the base
   port lands; `fad` *across* a boundary currently produces **silently zero
   tangents** and must be guarded by a diagnostic landing together with
   Step 1; exact cross-boundary FAD is a well-defined structural rewrite
   (block augmentation, cohabitation §6); RAD correctly rejects today and stays
   rejected until a clock-aware tape exists (cohabitation §7).
6. **Incomparable-domain errors** need a good diagnostic (C++ throws a bare
   `faustexception`); Rust should produce a structured `FRS-` code with the
   two domains and the offending signal.

## 9. Cohabitation with FAD/RAD

Moved to the companion document
[ondemand-fad-rad-cohabitation-2026-06-10-en.md](ondemand-fad-rad-cohabitation-2026-06-10-en.md)
(referenced as "cohabitation §N"). Short version: the flagship FAD use
cases — control-rate in-graph learning (`ondemand(ad.fit_adam …)`),
event-triggered adaptation, decimated gradients, frame-rate DDSP
controllers, runtime-count Newton solvers — all place `fad` strictly
*inside* one domain and need **zero new AD code** once Steps 1-6 of §7
land (Phase A). Differentiation commutes with every boundary operator as
long as the clock is seed-independent, so exact cross-boundary forward AD
is a structural block-augmentation rewrite under the same clock env
(Phase B). Today `fad` across a boundary silently produces **zero
tangents**; a loud diagnostic must land together with Step 1 (see §8
item 5). RAD needs a clock-aware tape (Phase C) and keeps its loud
rejection until then.

## 10. Vector mode (`-vec`): analysis and port plan, with clock domains

Moved to the companion document
[vector-mode-analysis-port-plan-2026-06-10-en.md](vector-mode-analysis-port-plan-2026-06-10-en.md)
(referenced as "vector doc §N"). Short version: C++ `-vec` restructures
`compute` into a chunked DAG of small loops, implemented twice upstream
(Klass strings + FIR instructions) and disabled entirely on the research
branch — loop partitioning and nested guarded blocks are structurally
incompatible there. faust-rs ports it once, in `signal_fir`, as a
deterministic `LoopGraph` (steps V1-V6, independent of this plan), then
composes it with clock domains via **scalar islands** (D1): each OD/US/DS
block becomes one serial loop node whose buffer interface is exactly the
`TempVar`/`PermVar` boundary glue — bit-exact vs scalar, no option
rejection needed (supersedes §8 item 4). Vectorizing constant-factor
US/DS interiors (D2) is a later optimization.

## 11. Summary

The on-demand concept is: *one new box family that prepends a clock input; a
clock-environment tree that gives every signal a time reference; an inference
pass that assigns each signal to its domain; a hierarchical scheduler that
turns each domain into a guarded block; and code generation that materializes
domain boundaries as temp vars (in), perm vars (out, sample-and-hold), and
per-domain time counters.*

faust-rs already has the **front half** (syntax → boxes → eval → propagation →
typed clocked signal graph) at parity. The **back half** (clock inference,
hierarchical scheduling, block-structured FIR, backend control flow) does not
exist yet and constitutes the port: Steps 1-8 above, with the FIR block
extension (Step 4) as the architectural centerpiece.

On the FAD/RAD side (§9): differentiation **commutes with every clock-domain
boundary operator** as long as the clock is seed-independent, so exact
forward AD across a boundary is a structural block-augmentation rewrite under
the *same* clock env — and `fad` strictly inside a domain works with no new
AD code at all. The two immediate obligations are a loud FAD diagnostic on
boundary glue (landing with Step 1, to prevent silently-zero gradients) and a
named RAD rejection message; the clock-aware reverse tape comes last. The
combination is the enabler for control-rate in-graph learning
(`ondemand(ad.fit_adam …)`), which is the practical payoff of the whole
port.

On the vector-mode side (§10): `-vec` restructures `compute` into a chunked
DAG of small loops; C++ implements it twice (Klass strings + FIR
instructions) and disables it entirely on the clocked research branch
because loop partitioning and nested guarded blocks are structurally
incompatible there. faust-rs ports it once, in `signal_fir`, as a
`LoopGraph` (steps V1–V6, independent of the base plan), and composes it
with clock domains via **scalar islands** (D1): each OD/US/DS block becomes
one serial loop node whose buffer interface is exactly the existing
`TempVar`/`PermVar` boundary glue, with bit-exact semantics and no need to
reject the option; vectorizing constant-factor US/DS interiors (D2) is a
later optimization.
