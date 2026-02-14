# Faust-Rust — Recursion Model Design Note

> **Status**: exploratory architecture note  
> **Scope**: signal-level recursion representation and interaction with evaluation/propagation

---

## 1. Context

Current planning uses the classic signal recursion encoding:
- `sigRec(...)` to define a recursive group
- `sigProj(i, rec_group)` to reference projected recursive outputs

This mirrors C++ behavior and is already aligned with the Phase 4 plan (`signals`/`eval`/`propagate`).

At the same time, we are considering a new intermediate routing representation (`RouteIR`) to simplify propagation by lowering structural box composition (`boxSeq`, `boxPar`, `boxSplit`, `boxMerge`) into explicit routing.

The critical question is whether recursion should stay in `sigRec/sigProj`, or move to a new signal recursion model.

---

## 2. Assessment of the Current `sigRec/sigProj` Model

### 2.1 Strengths

- **Parity**: matches existing Faust/C++ semantics and expectations.
- **Canonical tree form**: works naturally with `TreeArena` hash-consing (`TreeId` identity).
- **Compatibility**: minimizes disruption for current APIs and downstream passes expecting signal trees.
- **Compact representation**: recursion fits in normal signal constructors.

### 2.2 Weaknesses

- **Operational complexity**: recursion structure is implicit (projection indices + binder scope).
- **Pass complexity**: analyses and rewrites need careful binder/index handling.
- **Diagnostics**: cycle-level error messages are harder than with explicit recursion groups.
- **Maintenance risk**: multiple passes may re-encode recursion logic in slightly different ways.

Conclusion: `sigRec/sigProj` is a strong canonical representation, but not always the easiest operational representation.

---

## 3. Alternative Recursion Models

| Model | Core idea | Pros | Cons |
|---|---|---|---|
| `sigRec/sigProj` (current) | Recursive group + projections in signal tree | Highest parity, compact, hash-cons friendly | Harder for analysis/rewrite ergonomics |
| `LetRec` equations | Explicit vector of recursive equations + outputs | Very clear semantics, easier fixpoint/type passes | Requires broad signal-pass adaptation |
| Graph/SCC recursion | Recursion as explicit cycles in graph IR | Natural for scheduling/dependency diagnostics | Needs robust tree <-> graph conversion |
| `RouteIR` RecGroup | Routing graph with explicit recursion boundaries/feedback ports | Simplifies propagation internals, explicit cycles | Extra IR layer and conversion logic |

---

## 4. Recommended Long-Run Direction

Use a **dual-model strategy**:

1. Keep `sigRec/sigProj` as the **canonical external signal form** (parity + API stability).
2. Introduce an explicit recursion model in an **internal IR** (`RouteIR` with `RecGroup`) for propagation and heavy analyses.
3. Convert internal recursion form back to canonical `sigRec/sigProj` at the propagation boundary.

This yields better internal ergonomics without forcing an immediate full rewrite of signal consumers.

---

## 5. RouteIR Coexistence Contract

### 5.1 Pipeline placement

Recommended pipeline:

`parse -> eval -> lower_to_route_ir -> propagate_route -> normalize -> FIR`

- `eval` still produces evaluated box trees.
- `lower_to_route_ir` removes structural composition into nodes/edges.
- `propagate_route` emits canonical signal trees (`TreeId`) and preserves recursion semantics.

### 5.2 External compatibility

No required public API break:
- `boxesToSignals` can remain box-tree based externally.
- Internally, `boxesToSignals` may call `lower_to_route_ir` then `propagate_route`.

---

## 6. Required Invariants

For safe coexistence, enforce:

1. **Acyclic outside recursion groups**: cycles only inside explicit `RecGroup`.
2. **Explicit recursion boundaries**: feedback channels are first-class, not inferred.
3. **Arity correctness**: all port connections preserve input/output cardinalities.
4. **Deterministic ordering**: stable node/edge/group order for reproducible output.
5. **Semantic parity**: converted signals must match legacy propagation on reference corpus.

---

## 7. Should Canonical Signal Recursion Be Replaced Later?

Potentially yes, but only with evidence.

A canonical replacement (for example, `LetRec`-style signal nodes) should be considered only if:
- internal IR materially reduces bugs/complexity across multiple passes,
- performance gains persist on realistic DSP workloads,
- migration costs for normalize/transform/FIR/API remain acceptable.

Until then, keep canonical `sigRec/sigProj`.

---

## 8. Migration Plan (Low Risk)

### Stage A — Infrastructure
- Add `RouteIR` data model with `RecGroup`.
- Add lowering pass for non-rec structural composition (`seq/par/split/merge`).
- Keep legacy recursion path temporarily.

### Stage B — Recursion in RouteIR
- Lower `boxRec` to explicit `RecGroup`.
- Implement rec-aware propagation on RouteIR.
- Emit canonical `sigRec/sigProj` at output.

### Stage C — Validation and hardening
- Differential tests against legacy propagation on representative corpus.
- Add recursion-focused stress tests (`letrec`, feedback loops, pathological routing).
- Measure compile-time and memory behavior.

### Stage D — Decision gate
- If wins are clear, keep RouteIR as default internal pipeline.
- If not, retain legacy box propagation and keep RouteIR optional/experimental.

---

## 9. Validation Metrics

Track at least:

1. **Semantic parity**: zero unacceptable diffs on curated corpus.
2. **Propagation complexity**: code size/branch complexity reduction.
3. **Performance**: propagation time and memory on medium/large DSPs.
4. **Stability**: regression rate in recursion-heavy programs.
5. **Maintainability**: number of recursion-specific code paths across passes.

---

## 10. Practical Decision

At current project stage:

- Do **not** replace canonical `sigRec/sigProj` globally yet.
- Do introduce `RouteIR` recursion groups internally where they simplify propagation and analysis.
- Revisit canonical recursion redesign only after empirical evidence from Phase 4/5 implementation.

---

## 11. References

### 11.1 LetRec equations and cyclic lambda-calculus

1. Z. M. Ariola, J. W. Klop, *Lambda calculus with explicit recursion* (Information and Computation, 1997).  
   CWI page (with full text): <https://ir.cwi.nl/pub/1293>
2. Z. M. Ariola, M. Felleisen, *The call-by-need lambda calculus* (JFP, 1997).  
   DOI: <https://doi.org/10.1017/S0956796897002724>  
   Cambridge Core page: <https://www.cambridge.org/core/journals/journal-of-functional-programming/article/callbyneed-lambda-calculus/F4FC3C34E9CAE3F4326503E254FCF6F2>
3. J. Maraist, M. Odersky, P. Wadler, *The call-by-need lambda calculus* (JFP, 1998).  
   DOI: <https://doi.org/10.1017/S0956796898003037>  
   Cambridge Core page: <https://www.cambridge.org/core/journals/journal-of-functional-programming/article/callbyneed-lambda-calculus/7EDF4164D2F6EFBB5D36544D5390151A>
4. M. Schmidt-Schauss, D. Sabel, E. Machkasova, *Simulation in the Call-by-Need Lambda-Calculus with letrec* (RTA/LIPIcs, 2010).  
   DOI: <https://doi.org/10.4230/LIPIcs.RTA.2010.295>  
   Dagstuhl page: <https://drops.dagstuhl.de/entities/document/10.4230/LIPIcs.RTA.2010.295>

### 11.2 Graph/SCC recursion decomposition

1. R. E. Tarjan, *Depth-First Search and Linear Graph Algorithms* (SIAM Journal on Computing, 1972).  
   DOI: <https://doi.org/10.1137/0201010>  
   DBLP entry: <https://dblp.org/rec/journals/siamcomp/Tarjan72>
2. M. Sharir, *A strong-connectivity algorithm and its applications in data flow analysis* (Computers & Mathematics with Applications, 1981).  
   DOI: <https://doi.org/10.1016/0898-1221(81)90008-0>  
   ScienceDirect page: <https://www.sciencedirect.com/science/article/pii/0898122181900080>
3. Practical compiler example (recursive binding groups via SCC): GHC `OccurAnal` note (historical source view).  
   <https://downloads.haskell.org/ghc/7.0.2/docs/html/libraries/ghc-7.0.2/src/OccurAnal.html>
