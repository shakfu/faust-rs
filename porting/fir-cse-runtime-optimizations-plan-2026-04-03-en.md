# FIR Runtime Optimization Plan: Variability Placement & CSE

**Date**: 2026-04-03
**Scope**: `crates/transform/src/signal_fir/`, `crates/fir/`, `crates/codegen/`
**Goal**: Improve runtime performance of generated audio code by (1) placing
expressions in the correct execution tier based on their rate of change, and
(2) eliminating redundant computations within each tier.

---

## 1. Current Architecture Analysis

### 1.1 Signal-to-FIR Pipeline (`signal_prepare.rs`)

`prepare_signals_for_fir_unverified` runs:

```
clone_forest
  -> de_bruijn_to_sym
  -> canonicalize_unary_rec_projections
  -> infer_full_types           [1]
  -> promote_signals_fastlane
  -> infer_full_types           [2]
  -> simplify_signals_fastlane
  -> canonicalize_one_sample_delays
  -> infer_full_types           [3]
  -> promote_signals_fastlane
  -> infer_full_types           [4]
  -> derive_simple_types
```

4 type-inference passes, 2 promotions, 1 simplification, then verification.
This is compile-time cost. The runtime cost is determined by the **quality of
the FIR emitted** by `SignalToFirLower`.

### 1.2 Three Levels of Sharing (Current State)

| Level | Mechanism | What it does | Limitation |
|-------|-----------|-------------|------------|
| **Signal** | `cache: HashMap<SigId, FirId>` in `lower_signal()` | Same `SigId` -> same `FirId` | Different `SigId`s with identical semantics are lowered separately |
| **FIR nodes** | `TreeArena` hash-consing via `intern_tag()` | Structurally identical nodes share the same `FirId` | Identity only -- no temp variable emitted |
| **Emission** | `emit_value()` in C++/C/WASM backends | Recursive descent on FIR tree | **Re-expands every `FirId` inline at each use site** |

The gap: hash-consing detects that two expression trees are identical (same
`FirId`), but the emission backends re-expand the subtree at every occurrence.
A `FirId` used N times in `sample_statements` produces N copies of the same
computation in the output code.

The only existing materialization mechanism is `TeeVar` (emits `(name = expr)`
in C++), but it is only used for explicit delay/recursion state, never for
automatic CSE.

### 1.3 Missing Variability-Based Placement

Currently, `lower_signal()` places **all** lowered expressions into
`sample_statements`, which ends up inside the per-sample `for` loop. This
includes expressions that do not change at sample rate (UI controls, init-time
constants). The `SigType` system already computes `Variability` per node
(`Konst` / `Block` / `Samp`) but it is not used during FIR lowering.

### 1.4 Concrete Examples

**Example A — missing variability placement**:
```faust
process = hslider("gain", 0.5, 0, 1, 0.01) * _;
```
Current output (slider cast recomputed every sample):
```cpp
for (int i0 = 0; i0 < count; i0++) {
    outputs[0][i0] = FAUSTFLOAT(float(fHslider0) * float(inputs[0][i0]));
}
```
Target output (slider hoisted before the loop):
```cpp
float fSlow0 = float(fHslider0);
for (int i0 = 0; i0 < count; i0++) {
    outputs[0][i0] = FAUSTFLOAT(fSlow0 * float(inputs[0][i0]));
}
```

**Example B — missing CSE**:
```faust
process = _ <: (*(0.5) + 0.1), (*(0.5) + 0.2);
```
Current output (`input * 0.5f` computed twice):
```cpp
for (int i0 = 0; i0 < count; i0++) {
    float fInput = float(inputs[0][i0]);
    outputs[0][i0] = FAUSTFLOAT(fInput * 0.5f + 0.1f);
    outputs[1][i0] = FAUSTFLOAT(fInput * 0.5f + 0.2f);
}
```
Target output (shared subexpression materialized once):
```cpp
for (int i0 = 0; i0 < count; i0++) {
    float fInput = float(inputs[0][i0]);
    float fTemp0 = fInput * 0.5f;
    outputs[0][i0] = FAUSTFLOAT(fTemp0 + 0.1f);
    outputs[1][i0] = FAUSTFLOAT(fTemp0 + 0.2f);
}
```

---

## 2. Phase 1 — Variability-Driven Statement Placement

### 2.1 Available Information

The `SigType` system (`crates/sigtype`) already computes `Variability` for every
signal node:

```rust
pub enum Variability {
    Konst = 0,   // compile-time or init-time constant
    Block = 1,   // changes once per compute() call (UI controls)
    Samp  = 3,   // changes every sample
}
```

The `sig_types: HashMap<SigId, SigType>` map is already passed to
`SignalToFirLower` (field `sig_types`, line 571 of `module.rs`). The variability
of any signal is accessible via `sig_types.get(&sig).map(|t| t.variability())`.

### 2.2 Target Architecture

Three execution tiers, matching the C++ Faust compiler:

```
instanceConstants(sample_rate):     // called once on init
  fConst0 = 2.0f * float(fSampleRate);
  fConst1 = 3.14159f / fConst0;

compute(count, inputs, outputs):
  // --- block-rate tier (before the loop) ---
  float fSlow0 = float(fHslider0);
  float fSlow1 = fSlow0 * fConst1;
  FAUSTFLOAT* output0 = outputs[0];

  // --- sample-rate tier (inside the loop) ---
  for (int i0 = 0; i0 < count; i0++) {
      output0[i0] = FAUSTFLOAT(float(inputs[0][i0]) * fSlow1);
  }
```

| Variability | FIR bucket | Execution frequency |
|-------------|-----------|---------------------|
| `Konst` | `constants_statements` | Once at init (`instanceConstants`) |
| `Block` | `control_statements` | Once per `compute()` call (before the loop) |
| `Samp` | `sample_statements` | Every sample (inside the loop) |

### 2.3 Design: Variability-Aware Lowering

Modify `lower_signal()` to check each node's variability and, for `Konst` and
`Block` nodes, materialize the result into a named variable in the appropriate
bucket instead of returning an inline value expression.

```rust
fn lower_signal(&mut self, sig: SigId) -> Result<FirId, SignalFirError> {
    if let Some(id) = self.cache.get(&sig).copied() {
        return Ok(id);
    }

    let lowered = self.lower_signal_inner(sig)?;

    // --- NEW: variability-driven placement ---
    let result = match self.variability_of(sig) {
        Some(Variability::Konst) if !is_trivial_fir(&self.store, lowered) => {
            self.materialize_in_bucket(lowered, Bucket::Constants)
        }
        Some(Variability::Block) if !is_trivial_fir(&self.store, lowered) => {
            self.materialize_in_bucket(lowered, Bucket::Control)
        }
        _ => lowered,  // Samp or trivial -> stays inline in sample loop
    };

    self.cache.insert(sig, result);
    Ok(result)
}

fn variability_of(&self, sig: SigId) -> Option<Variability> {
    self.sig_types.get(&sig).map(|t| t.variability())
}

fn materialize_in_bucket(&mut self, value: FirId, bucket: Bucket) -> FirId {
    let (name, access) = match bucket {
        Bucket::Constants => {
            let n = self.const_counter;
            self.const_counter += 1;
            (format!("fConst{n}"), AccessType::Struct)
        }
        Bucket::Control => {
            let n = self.slow_counter;
            self.slow_counter += 1;
            (format!("fSlow{n}"), AccessType::Stack)
        }
    };
    let typ = /* infer from value */;

    let mut b = FirBuilder::new(&mut self.store);
    let decl = b.declare_var(&name, typ.clone(), access, Some(value));
    match bucket {
        Bucket::Constants => self.constants_statements.push(decl),
        Bucket::Control   => self.control_statements.push(decl),
    };

    let mut b = FirBuilder::new(&mut self.store);
    b.load_var(&name, access, typ)
}
```

### 2.4 Naming Convention (C++ Parity)

| Bucket | Prefix | Storage | Example |
|--------|--------|---------|---------|
| `constants_statements` | `fConst` | `AccessType::Struct` (persistent across calls) | `fConst0 = 2.0f * float(fSampleRate)` |
| `control_statements` | `fSlow` | `AccessType::Stack` (local to `compute()`) | `fSlow0 = float(fHslider0)` |
| `sample_statements` | `fTemp` | `AccessType::Stack` (local to loop body) | `fTemp0 = fInput * fSlow0` |

`fConst` variables need `AccessType::Struct` because they are initialized in
`instanceConstants()` and read in `compute()`. `fSlow` variables can be
`AccessType::Stack` since they live within the `compute()` function body.

### 2.5 Edge Cases

- **Recursive projections**: `Proj(i, SYMREC)` may have `Konst` variability in
  the type system but carry feedback state. These must stay in
  `sample_statements`. Guard: skip materialization when the signal is a `Proj`
  targeting a symbolic recursion group.
- **Delay operands**: The delay amount may be `Konst` but the delay read itself
  is `Samp`. Only the amount subexpression benefits from hoisting.
- **Soundfile/table reads**: The table content may be `Konst` but the read
  index is `Samp`. Table init is already in `constants_statements`.
- **Bargraph stores**: The store statement remains in `sample_statements`, but
  the value subexpression can be hoisted if it is `Block`.

### 2.6 Implementation Steps

| Step | File | Work |
|------|------|------|
| **V1** | `signal_fir/module.rs` | Add `const_counter: u32`, `slow_counter: u32` fields. Add `materialize_in_bucket()` helper. |
| **V2** | `signal_fir/module.rs` | Add variability check in `lower_signal()` after `lower_signal_inner()`. Skip trivial and recursive-projection nodes. |
| **V3** | `signal_fir/module.rs` | Handle edge cases: recursion, delays, bargraphs, soundfiles. |
| **V4** | tests | Slider-only -> `fSlow` in control. `2.0*SR` -> `fConst` in constants. Recursive feedback stays in sample loop. Compare against C++ reference. |

---

## 3. Phase 2 — CSE Materialization

### 3.1 Overview

A post-lowering, pre-emission pass that:
1. counts how many times each `FirId` value node appears as a child,
2. wraps multi-referenced non-trivial expressions in `DeclareVar` + `LoadVar`,
3. operates on the `FirStore` so all backends benefit,
4. runs independently on **each** of the three buckets (`constants_statements`,
   `control_statements`, `sample_statements`).

### 3.2 Insertion Point

```
signal_fir/module.rs::build_module()
  |-- lower_signal() with variability placement   (Phase 1)
  |-- ** materialize_shared_expressions() **      (Phase 2) <-- NEW
  |       applied to constants_statements
  |       applied to control_statements
  |       applied to sample_statements
  |-- assemble FIR Module
```

### 3.3 Reference Counting

Walk each bucket. For each `FirId` that is a **value node** (not a statement),
count how many distinct parent nodes reference it as a child.

```rust
fn count_fir_value_uses(
    store: &FirStore,
    roots: &[FirId],
) -> HashMap<FirId, usize> {
    let mut ref_counts: HashMap<FirId, usize> = HashMap::new();
    let mut descended: HashSet<FirId> = HashSet::new();

    for &root in roots {
        count_refs(store, root, &mut ref_counts, &mut descended);
    }
    ref_counts
}

fn count_refs(
    store: &FirStore,
    node: FirId,
    ref_counts: &mut HashMap<FirId, usize>,
    descended: &mut HashSet<FirId>,
) {
    *ref_counts.entry(node).or_insert(0) += 1;

    if !descended.insert(node) {
        return;
    }
    for child in fir_value_children(store, node) {
        count_refs(store, child, ref_counts, descended);
    }
}
```

Key: `ref_counts` increments on every **reference**, but children are only
visited once per unique `FirId`. This correctly measures fan-out.

### 3.4 Trivial-Node Filter

Nodes that should never be materialized into a temp variable because they are
already free:

```rust
fn is_trivial_value(store: &FirStore, node: FirId) -> bool {
    matches!(
        match_fir(store, node),
        FirMatch::Int32 { .. }
        | FirMatch::Int64 { .. }
        | FirMatch::Float32 { .. }
        | FirMatch::Float64 { .. }
        | FirMatch::Bool { .. }
        | FirMatch::LoadVar { .. }
        | FirMatch::LoadVarAddress { .. }
        | FirMatch::NullValue { .. }
    )
}
```

### 3.5 Bottom-Up Rewrite

When a non-trivial value node has `ref_count >= 2`, emit a `DeclareVar` at
first encounter and replace all references with `LoadVar`.

```rust
fn materialize_shared_values(
    store: &mut FirStore,
    statements: &mut Vec<FirId>,
    ref_counts: &HashMap<FirId, usize>,
    prefix: &str,                        // "fConst", "fSlow", or "fTemp"
) {
    let mut materialized: HashMap<FirId, String> = HashMap::new();
    let mut temp_decls: Vec<FirId> = Vec::new();
    let mut counter = 0u32;

    for stmt in statements.iter_mut() {
        *stmt = rewrite_node(
            store, *stmt, ref_counts,
            &mut materialized, &mut temp_decls,
            prefix, &mut counter,
        );
    }

    // Prepend temp declarations before the rewritten statements.
    temp_decls.append(statements);
    *statements = temp_decls;
}

fn rewrite_node(
    store: &mut FirStore,
    node: FirId,
    ref_counts: &HashMap<FirId, usize>,
    materialized: &mut HashMap<FirId, String>,
    temp_decls: &mut Vec<FirId>,
    prefix: &str,
    counter: &mut u32,
) -> FirId {
    // Already materialized -> LoadVar.
    if let Some(name) = materialized.get(&node) {
        return emit_load_var(store, name, node);
    }

    // Rewrite children first (bottom-up).
    let rewritten = rewrite_children(
        store, node, ref_counts, materialized, temp_decls, prefix, counter,
    );

    // Candidate for materialization?
    if ref_counts.get(&node).copied().unwrap_or(0) >= 2
        && !is_trivial_value(store, node)
    {
        let name = format!("{prefix}{counter}");
        *counter += 1;

        let typ = infer_fir_type(store, rewritten);
        let decl = emit_declare_var(store, &name, typ.clone(), rewritten);
        temp_decls.push(decl);

        materialized.insert(node, name.clone());
        return emit_load_var(store, &name, rewritten);
    }

    rewritten
}
```

### 3.6 Backend Impact

| Backend | Benefit |
|---------|---------|
| **C / C++** | `emit_value()` sees `LoadVar` instead of deep subtree |
| **WASM** | Shared values become `local.get` instead of duplicated expression trees |
| **Cranelift** | JIT sees `load` from stack slot; Cranelift's register allocator may further optimize |
| **Interpreter (FBC)** | Single heap load instead of re-evaluating bytecode sequence; especially valuable since FBC has no CSE pass |

### 3.7 Ordering and Side-Effect Safety

- **Only value nodes** are candidates. Statement nodes (`StoreVar`, `StoreTable`,
  `If`, `ForLoop`, etc.) are never considered.
- **Bottom-up rewrite** ensures children are materialized before parents.
- **Declaration ordering**: `DeclareVar` nodes are prepended in first-encounter
  order, which respects data dependencies.
- **Per-bucket isolation**: each bucket is processed independently with its own
  counter namespace.

### 3.8 Implementation Steps

| Step | File | Work |
|------|------|------|
| **C1** | `crates/fir/src/lib.rs` | Add `fir_value_children(store, node) -> Vec<FirId>` using `match_fir` dispatch. |
| **C2** | `crates/fir/src/lib.rs` | Add `infer_fir_type(store, node) -> FirType` reading the encoded type child. |
| **C3** | `signal_fir/cse.rs` (new) | Implement `count_fir_value_uses()`. |
| **C4** | `signal_fir/cse.rs` | Implement `materialize_shared_values()`, `rewrite_node()`, `is_trivial_value()`. |
| **C5** | `signal_fir/module.rs` | Call `materialize_shared_values()` on each bucket after lowering. |
| **C6** | tests | Shared subtree -> `DeclareVar` + `LoadVar`. Trivial nodes untouched. Single-use stays inline. Differential validation on test corpus. |

---

## 4. Interaction Between Phase 1 and Phase 2

The two passes are complementary and orthogonal:

- **Variability placement** (Phase 1) resolves **inter-tier** sharing by
  hoisting `Konst`/`Block` expressions out of the sample loop.
- **CSE materialization** (Phase 2) resolves **intra-tier** sharing by
  deduplicating multi-referenced expressions within each bucket.

**Why they don't interfere**:

1. When variability placement hoists a node, it stores a `LoadVar("fSlow0")`
   in the lowering cache. All subsequent references receive that `LoadVar`.
   Since `LoadVar` is trivial, CSE never tries to materialize it.

2. CSE only sees the FIR that remains in each bucket after hoisting. The
   sample loop no longer contains the hoisted expressions.

3. The only case where CSE acts on hoisted code is when **two different
   `SigId`s** (no DAG sharing at signal level) produce the **same FIR
   expression** in the same bucket. Hash-consing gives them the same `FirId`,
   and CSE detects the multiple references.

**Concrete interaction scenarios**:

| Scenario | Phase 1 handles | Phase 2 handles |
|----------|----------------|----------------|
| Same `SigId` used 5x, `Block` rate | Hoists once to `fSlow*`; cache returns `LoadVar` to all 5 sites | Nothing (`LoadVar` is trivial) |
| Same `SigId` used 3x, `Samp` rate | Nothing (stays in sample loop) | Materializes to `fTemp*`; 3 sites become `LoadVar` |
| Two different `SigId`s, same `Block` expr | Hoists each to `fSlow0`, `fSlow1` (same init `FirId`) | Deduplicates in control bucket: merges `fSlow1` into `fSlow0` |
| `Block` subexpr inside `Samp` expr | Hoists subexpr to `fSlow*`; parent stays in sample loop with `LoadVar` | May still materialize the `Samp` parent if multi-ref |
| `Konst` subexpr inside `Block` expr | Hoists subexpr to `fConst*`; `Block` expr in control tier uses `LoadVar` | May deduplicate in control bucket if same `Block` expr duplicated |

---

## 5. Implementation Order

**Phase 1 first (Variability, §2), then Phase 2 (CSE, §3).**

Rationale:

1. **Technical dependency**: CSE counts references in each bucket and must see
   the final bucket contents. Variability placement changes which bucket each
   expression lands in. Running CSE first would produce `fTemp` variables in
   `sample_statements` that later need to migrate — an unnecessary complication.

2. **Impact ordering**: variability placement eliminates O(buffer_size)
   redundant evaluations per hoisted expression (typically 64-1024x). CSE
   eliminates O(fan_out) redundant evaluations (typically 2-5x).

3. **Testing isolation**: each phase can be validated independently. Phase 1
   produces `fConst`/`fSlow` variables comparable against C++ Faust reference
   output. Phase 2 then adds `fTemp` variables within each tier.

```
Phase 1 — Variability (§2)           Phase 2 — CSE (§3)
─────────────────────────────         ──────────────────────────
V1: counters + helper method          C1: fir_value_children()
V2: variability check in              C2: infer_fir_type()
    lower_signal()                    C3: count_fir_value_uses()
V3: edge cases (recursion,            C4: materialize_shared_values()
    delays, bargraphs)                C5: integrate in build_module()
V4: tests + diff validation           C6: tests + diff validation
         │                                      │
         ▼                                      ▼
    Checkpoint: verify                 Checkpoint: verify
    fConst/fSlow placement             fTemp deduplication
    matches C++ reference              within each bucket
```

---

## 6. Combined Pipeline Summary

```
signal_prepare.rs
  ├── de_bruijn_to_sym
  ├── type inference (4 passes)
  ├── promote + simplify
  └── PreparedSignals { arena, outputs, types, sig_types }

signal_fir/module.rs::build_module()
  ├── lower_signal() with variability placement       ← Phase 1 (§2)
  │     Konst nodes → constants_statements (fConst*)
  │     Block nodes → control_statements   (fSlow*)
  │     Samp nodes  → sample_statements    (inline)
  ├── materialize_shared_expressions() per bucket     ← Phase 2 (§3)
  │     multi-ref non-trivial → DeclareVar + LoadVar
  │     constants_statements: prefix fConst
  │     control_statements:   prefix fSlow
  │     sample_statements:    prefix fTemp
  └── assemble FIR Module
        instanceConstants = block(constants_statements)
        compute           = block(control_statements + for_loop(sample_statements))
```

---

## 7. Future Refinements (Out of Scope for v1)

| Refinement | Description |
|-----------|-------------|
| **Cost-weighted CSE threshold** | Only materialize when `uses * op_cost > threshold` (e.g., `sin` used 2x -> always; `add` used 2x -> leave inline) |
| **Redundant cast elimination** | Peephole to fold `FloatCast(IntCast(FloatCast(x)))` -> `FloatCast(x)` after the second promotion |
| **FIR-level constant folding** | Fold `FloatCast(IntConst(0))` -> `RealConst(0.0)` after lowering |
| **Scheduling / reordering** | Reorder FIR statements within `sample_statements` for register locality |
| **Short-delay shift registers** | Scalar shift registers for delays of 2-4 samples instead of masked circular buffers |
