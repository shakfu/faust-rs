# Phase 0 — Validation Sprint (Go/No-Go)

> **Scope**: pre-port validation and scope freeze before implementation
> **Estimate**: 5–10 person days
> **Prerequisites**: none (must happen before Phase 1)

---

## 1. Objectives

Phase 0 validates high-risk assumptions on the current C++ branch so the Rust port starts from a stable baseline.

Primary goals:
- Confirm the effective production pipeline to port first.
- Freeze migration scope (included and excluded backends/modes).
- Validate API lifecycle model and integration constraints.
- Lock differential testing baseline and acceptance thresholds.
- Surface blockers early with explicit go/no-go criteria.

---

## 2. Mandatory validation work

### 2.1 Effective pipeline confirmation
- Confirm that major end-to-end paths are still driven by `libcode.cpp` dispatch + `InstructionsCompiler` / `DAGInstructionsCompiler`.
- Keep `signalFIRCompiler` and `signalRenderer` as maintained but non-critical/experimental tracks.
- Record exact call paths used by `-lang c`, `-lang cpp`, `-lang rust`, `-lang wasm`, `-lang llvm`, `-lang interp`.

### 2.2 Scope freeze
- Confirm that `backend-java` is out of Rust port target scope.
- Confirm that old C++ mode `-lang ocpp` is out of Rust port target scope.
- Keep any legacy references only as historical context, not as deliverable requirements.

### 2.3 CLI/backend option model
- Inventory backend/option compatibility rules currently spread across imperative checks.
- Define a declarative capability matrix target for Rust.
- Add contradiction tests (for example impossible or duplicated conditions).

### 2.4 API lifecycle and ownership model
- Inventory all C/C++ entry points and their context init/teardown behavior.
- Identify divergent lifecycle paths and normalize to one Rust contract.
- Lock explicit ownership rules for compile session handles and returned artifacts.
- For `box_signal_api.cpp`, limit Phase 0 to usage/surface inventory; defer full export prioritization to Phase 1 scope planning.

### 2.5 Orchestration safety checks
- Validate that no fixed-size temporary argument staging is kept in Rust design.
- Validate deterministic per-request state reset in orchestration paths.
- Validate that output mode handling is typed (text vs binary capabilities), not stream-downcast-based.

### 2.6 Stack/recursion strategy
- Audit recursive hotspots and current stack workaround behavior.
- Define iterative or bounded-recursion strategy where needed.
- Record maximum supported recursion/depth behavior for parity and diagnostics.

### 2.7 Differential baseline
- Build representative DSP corpus (small, medium, large, pathological).
- Capture baseline outputs on selected backends.
- Define tolerated differences (formatting/cosmetic) and non-tolerated differences (semantic/structural/runtime).

### 2.8 TreeArena hash-consing performance validation
- Implement a minimal Rust `TreeArena` prototype (`make`, intern lookup, traversal, property access).
- Run micro-benchmarks for creation, lookup on existing nodes, traversal, and property set/get.
- Compare against equivalent C++ `CTree` benchmark on the same workload profile.
- Record thresholds and optimization levers (hash function choice, pre-allocation, map implementation).

---

## 3. Deliverables

- `phase-0-validation-en.md` updated with:
  - confirmed pipeline map
  - scope decisions
  - capability matrix draft
  - API lifecycle contract
  - baseline differential protocol
  - TreeArena performance report (results, thresholds, decision)
- Go/No-Go decision with explicit blockers list.
- If blockers exist: mitigation plan with owner and target phase.

---

## 4. Go/No-Go criteria

Go:
- Effective production pipeline is confirmed and documented.
- Scope exclusions are frozen (`backend-java`, `-lang ocpp`).
- API lifecycle model is unified and accepted.
- Capability matrix approach is defined and testable.
- Differential baseline corpus and procedure are ready.
- TreeArena hash-consing performance is validated against agreed thresholds.

No-Go:
- Pipeline ownership is ambiguous between competing compile paths.
- Scope is not frozen or remains contradictory across docs.
- API lifecycle remains divergent with no agreed target contract.
- Differential baseline cannot be reproduced reliably.
- TreeArena performance is outside agreed limits with no credible mitigation path.

---

## 5. Exit checklist

- [ ] Pipeline target confirmed (`InstructionsCompiler` path first)
- [ ] Experimental tracks documented (`signalFIRCompiler`, `signalRenderer`)
- [ ] Java backend excluded from target scope
- [ ] `-lang ocpp` excluded from target scope
- [ ] Capability matrix model defined for CLI/backend validation
- [ ] API lifecycle unified across entry points
- [ ] No fixed-size temporary argument staging in target design
- [ ] Deterministic per-request orchestration state model documented
- [ ] Typed output sink model documented
- [ ] Recursion/stack strategy documented
- [ ] Differential corpus and acceptance rules finalized
- [ ] TreeArena hash-consing benchmarks completed and reviewed
- [ ] Final Go/No-Go decision recorded
