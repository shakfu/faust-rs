# FIR Architecture Contract (Rust)

**Status**: active design contract for Phase 6 implementation.

## 1. Goal

Define one canonical FIR architecture for the Rust port, so all backends (`c`, `cpp`, `rust`, `wasm`, `llvm`, `interp`, ...) consume the same IR model and dispatch API.

This document is intentionally aligned with:

- `porting/phases/phase-4-signaux-en.md` (signals side of the boundary),
- `porting/phases/phase-6-fir-backends-en.md` (FIR/backends migration plan),
- C++ source-of-truth in `/Users/letz/Developpements/RUST/faust`.

## 2. Canonical Public API

FIR must expose exactly one canonical construction/matching surface:

- `FirBuilder`: construction API.
- `FirMatch` + `match_fir`: inspection/dispatch API.
- `FirStore` + typed IDs (`FirId`): stable storage and references (implemented over
  `tlib::TreeArena` interning).

No backend-local constructor ladders and no duplicated matcher ladders are allowed in production paths.

## 3. Mapping from C++

Primary C++ anchors:

- `compiler/generator/instructions.hh`: `ValueInst`/`StatementInst`, visitors, `IB::gen*`.
- `compiler/generator/instructions_type.hh`: FIR type model (`Typed::VarType` and variants).
- `compiler/generator/instructions_compiler.hh/.cpp`: currently effective production signal->FIR path.
- `compiler/transform/signalFIRCompiler.hh/.cpp`: secondary/experimental direct signal->FIR path.
- `compiler/generator/code_container.hh/.cpp`: sectioned FIR ownership and lifecycle.

Required Rust mapping:

- `IB::gen*` -> `FirBuilder::*` methods.
- Visitor/RTTI dispatch (`accept`, `DispatchVisitor`, `dynamic_cast`) -> `match_fir` + exhaustive `match`.
- Pointer-owned instruction graphs -> `FirStore` + `FirId`.
- Value typing reconstruction (`typing_instructions.hh`) -> explicit `typ` carried in Rust FIR value nodes.

## 4. Pipeline Boundary Contract

Pipeline contract remains:

`parse -> boxes -> eval -> propagate -> normalize -> transform -> fir -> codegen::backends::*`

Boundary constraints:

- `transform` is the producer of FIR nodes.
- `fir` crate owns FIR node definitions, IDs, builders, matchers, and FIR-local transforms/checkers.
- `codegen` and every backend are consumers of `fir` crate APIs; they do not redefine FIR semantics.
- FIR node storage relies on `tlib` so structurally identical FIR nodes are shared automatically.

## 5. Architectural Invariants

- Deterministic node semantics and child ordering.
- Explicitly typed memory-access classes (`stack`, `struct`, `funargs`, `loop`, ...).
- Explicit value-node result typing (`FirValue.typ`) available at IR construction time.
- Explicitly typed UI FIR nodes (open/close group, button, slider, bargraph, metadata).
- Exhaustive dispatch coverage for canonical nodes in `FirMatch`.
- No hidden global state (`gGlobal`-like) in FIR builders/checkers.

## 5.1 Module Entrypoint Contract for Text Backends

For text backends that consume FIR directly (starting with C++ backend migration), the canonical
entrypoint is a FIR **module** node:

- input to backend API must be a `FirMatch::Module` node,
- module children (`dsp_struct`, `globals`, `declarations/functions`) are emitted in deterministic
  order,
- non-module roots are rejected with typed backend diagnostics (no silent fallback).

This mirrors C++ `ModuleInst`-based backend visitors while preserving the Rust invariant:
construction through `FirBuilder`, dispatch through `match_fir`.

## 5.2 FIR DSP I/O Arity Contract (General, Backend-Independent)

The FIR module contract must carry DSP audio channel arity explicitly, instead
of relying on backend-local heuristics.

### Problem statement

Today, several backends can infer `getNumInputs` / `getNumOutputs` from
`compute` body patterns (aliases like `input0`, `output0`, or table accesses),
but this is fragile and can diverge across backends.

### Target contract

- FIR module metadata must include:
  - `num_inputs: u32`
  - `num_outputs: u32`
- These values are the canonical source for DSP audio arity in all backends
  (`c`, `cpp`, `interp`, `cranelift`, and future backends).
- Backend-local arity inference is forbidden in production code paths.

### Implementation strategy

1. Extend FIR module representation and builder API to store explicit arity.
2. Update FIR producers (`transform` fast-lane, fixtures, inliner-preserving
   paths) to set arity at module construction time.
3. Update FIR checker with module-level validation:
   - arity fields are present and non-negative,
   - `compute` declaration shape remains compatible with canonical DSP API,
   - explicit errors when declared arity disagrees with detectable `compute`
     accesses.
4. Update backends to consume arity from FIR module metadata first.
5. Remove runtime inference paths; missing/invalid arity must fail fast.

### Migration and compatibility policy

- Missing module arity is a verifier error for production pipelines.
- The `parse -> ... -> transform -> fir -> backend` contract is then: arity is
  produced in FIR, not reconstructed in backends.

### Required tests

- FIR unit tests: module builder/matcher round-trip for arity fields.
- FIR checker tests: missing/invalid/inconsistent arity diagnostics.
- Backend tests (C/C++/Interp/Cranelift): `getNumInputs/getNumOutputs` values
  must match FIR module arity on shared fixtures.
- Differential tests: arity consistency between C++ reference outputs and Rust
  backends on selected corpus cases.

## 6. Implementation Pattern

Recommended internal layering in `crates/fir`:

1. `types`: FIR types and operation enums.
2. `nodes`: canonical FIR node enum + typed IDs.
3. `builder`: `FirBuilder` (ergonomic and deterministic constructors).
4. `matching`: `FirMatch` + `match_fir`.
5. `passes`: FIR->FIR transforms/checkers.

The first migration slice can be partial on node coverage, but it must keep API shape stable and documented.

## 7. Test Contract

Minimum tests per slice:

- unit tests for every newly added `FirBuilder` constructor.
- unit tests for corresponding `match_fir` variant decoding.
- negative tests for unknown/invalid IDs.
- differential tests for emitted FIR dumps once text FIR output is connected.

## 8. Rustdoc Requirements

For each public FIR API item:

- include C++ provenance (`instructions.hh`, `instructions_type.hh`, etc.),
- state parity invariants,
- state adaptation policy if not a strict 1:1 signature mapping.
