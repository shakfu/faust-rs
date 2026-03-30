# WebAssembly Foreign Symbol Support Plan

**Date:** 2026-03-30
**Status:** `ffunction(...)` implemented in wasm backend; `fconst(...)` / `fvar(...)` deferred
**Target crates:** `transform`, `codegen`, `compiler`, `wasm-ffi`
**Primary backend module:** `codegen::backends::wasm`
**C++ provenance:** `compiler/global.cpp`, `compiler/generator/wasm/`, `compiler/generator/instructions_compiler.cpp`
**Scope:** WebAssembly backend support for Faust foreign symbols:
`ffunction(...)`, `fconst(...)`, and `fvar(...)`

---

## 1. Purpose

This document defines a concrete plan for supporting Faust foreign symbols in
the Rust WebAssembly backend.

The key constraint is semantic, not syntactic:

- foreign symbols are already parsed and mostly lowered in the Rust pipeline,
- but the WebAssembly backend currently supports only a narrow whitelist of
  imported helper functions,
- while native backends can rely on C/C++ include/library integration that does
  not exist in a pure WASM target.

So the real question is not "can wasm parse `ffunction`, `fconst`, or `fvar`?"
but:

> how should a WebAssembly backend represent and expose Faust foreign symbols
> in a runtime-safe, host-provided way that remains compatible with Faust's
> product model?

---

## 2. Current State

### 2.1 Front-end and FIR lowering already cover most of the path

The Rust pipeline already carries foreign symbols through the front-end and
signal/FIR stages:

- `ffunction(...)` is decoded and lowered to `FunCall(...)` plus one recorded
  foreign prototype,
- `fconst(...)` is recognized, but only the `fSamplingFreq` /
  `fSamplingRate` fast lane is currently lowered,
- `fvar(...)` is lowered, with the Faust runtime symbol `count` treated as a
  special compute argument rather than as a normal extern.

Relevant implementation:

- `crates/transform/src/signal_fir/module.rs`
  - `lower_ffun(...)`
  - `decode_foreign_fun_proto(...)`
  - `lower_fconst(...)`
  - `lower_fvar(...)`

So the front-end path is already in place. The active backend work has now been
completed for generic `ffunction(...)`, while the broader foreign-symbol ABI
questions remain for `fconst(...)` and `fvar(...)`.

### 2.2 Current behavior by foreign symbol kind

#### `ffunction(...)`

The Rust WASM backend currently accepts:

- built-in FIR math intrinsics,
- a small hard-coded set of foreign helper names mapped to host imports.

Relevant implementation:

- `crates/codegen/src/backends/wasm/mod.rs`
  - `FirMatch::FunCall { ... }` lowering
  - `imported_foreign_signature(...)`

Observed behavior:

- `ffunction(float sinhf(float), <math.h>, "")` compiles with `-lang wasm`
- `ffunction(float myhost(float), <dummy.h>, "")` now compiles in the WASM
  backend when the FIR module carries the corresponding extern prototype
- the wasm backend still preserves the historical helper remapping path for
  names such as `sinhf`, `acoshf`, `isnanf`, and `copysignf`

#### `fconst(...)`

Current FIR lowering supports only:

- `fSamplingFreq`
- `fSamplingRate`

Both are lowered to the DSP struct field `fSampleRate`, not to a foreign import.

All other foreign constants currently fail during FIR lowering.

This matches the active parity slice in Rust and the long-standing C++ special
case for sample rate.

#### `fvar(...)`

Current FIR lowering supports:

- `count` as a special runtime value lowered to `AccessType::FunArgs`
- any other foreign variable as an extern global load in FIR

However, the current WASM backend still rejects foreign variable access. There
is already a regression test asserting that rejection.

So `fvar(...)` is lowered far enough to express the intent, but not yet far
enough to execute in wasm.

### 2.3 C++ reference behavior

The C++ compiler distinguishes:

- helper foreign math functions that are explicitly supported by `wasm/wast`,
- special runtime symbols such as `fSampleRate` and `fFullCount`,
- general foreign constants/variables, which remain governed by compilation
  mode and backend capabilities.

This suggests that C++ parity for WASM is already based on the idea of
host-provided imported helpers and backend-specific foreign-symbol policies, not
on arbitrary C/C++ linking at compile time.

---

## 3. Problem Statement

Supporting foreign symbols in a native backend and supporting them in a WASM
backend are not the same task.

For native backends:

- `incfile` and `libfile` can influence generated source and native link steps,
- foreign globals can map naturally to native extern variables.

For WebAssembly:

- there is no direct native link phase in the produced DSP module,
- the portable mechanism for foreign functions is host imports,
- imported globals exist in wasm, but introduce more ABI and runtime
  constraints than plain imported functions,
- `incfile` / `libfile` cannot keep their full native meaning.

Therefore, the Rust WASM backend needs a defined policy for:

1. which `ffunction(...)`, `fconst(...)`, and `fvar(...)` shapes are accepted,
2. how each kind is mapped to a WASM-level contract,
3. what ABI and naming convention the host must provide,
4. what to do with `incfile` / `libfile`,
5. what remains special-cased as internal runtime/compiler state,
6. how this is surfaced to `faustwasm` and other runtimes.

---

## 4. Recommended Target Model

### 4.1 Unify the semantics as "host-provided foreign symbols"

For the WebAssembly backend, Faust foreign symbols should be modeled as
host-provided entities rather than as native link targets.

That means:

- `ffunction(...)` maps to imported host functions,
- most portable `fconst(...)` support should map to host-provided immutable
  values, exposed either as imports or getter functions,
- most portable `fvar(...)` support should map to host-provided readable
  variables, preferably exposed through accessor functions.

This is the only robust cross-runtime interpretation.

### 4.2 Keep `fSamplingRate` and `count` special

Two existing special cases should remain special in wasm:

- `fSamplingFreq` / `fSamplingRate`
- `count`

Reason:

- they already represent stable compiler/runtime concepts,
- they do not need the general foreign-symbol ABI,
- keeping them special reduces host burden and preserves current parity.

So the target model is not "everything becomes an import". It is:

- keep internal runtime symbols internal,
- expose only genuinely foreign symbols through the host ABI.

### 4.3 Preserve the current hard-coded math subset as compatibility layer zero

The existing imported-helper whitelist should remain valid as the compatibility
baseline.

Reason:

- it already matches known C++/wrapper behavior,
- it is validated,
- it avoids breaking current `faustwasm` expectations.

The new work should generalize from that subset, not replace it abruptly.

### 4.4 Generalize to arbitrary foreign prototypes with a stable ABI

General `ffunction(...)` support should use one explicit ABI contract:

- import module: `"env"` by default
- import field name: derived from the selected foreign symbol name
- parameter/result types: derived from the existing FIR-level
  `ForeignFunProto`

This keeps the Rust backend aligned with the current WASM import style already
used for math helpers.

### 4.5 Deliberately narrow semantics of `incfile` / `libfile` in WASM mode

For WASM output, `incfile` and `libfile` should be treated as:

- metadata and provenance only, or
- validated-but-non-binding fields

They should **not** be treated as native link instructions in the emitted WASM
artifact.

Recommended policy:

- continue parsing and preserving them in the descriptor,
- do not require them for import generation,
- optionally expose them in diagnostics or metadata,
- document clearly that they are ignored for actual WASM linking semantics.

---

## 5. Symbol-Specific Strategy

### 5.1 `ffunction(...)`

Target model:

- general scalar `ffunction(...)` lowers to one typed imported function

Required information already exists at FIR level:

- symbol name
- argument types
- result type

Implemented backend work:

- carry generic foreign prototypes from FIR extern declarations into WASM
  import planning,
- emit generic import descriptors beyond the hard-coded helper whitelist,
- preserve compatibility mappings for historical helper names.

### 5.2 `fconst(...)`

Target model:

- keep `fSamplingRate` internal,
- support other foreign constants through getter imports first

Recommended wasm ABI:

- `env.<name>: () -> scalar`

Rationale:

- imported immutable globals are possible, but getter imports fit the current
  function-centric backend architecture better,
- getter imports avoid requiring special global-import plumbing on day one,
- constants can still be semantically constant at the host level even if they
  are fetched through a function.

Open optimization path:

- once the generalized ABI is stable, optionally add direct imported-global
  support for immutable scalar `fconst(...)`.

### 5.3 `fvar(...)`

Target model:

- keep `count` internal,
- support other foreign variables through getter imports first

Recommended wasm ABI:

- `env.<name>: () -> scalar`

Rationale:

- the current FIR model treats foreign variables as readable extern state,
- imported mutable globals are less portable and more awkward to version,
- getter imports make update timing explicit and fit existing call lowering.

Important semantic note:

- this plan covers readable foreign variables,
- if future parity work needs writable foreign variables in wasm, that should
  be a separate design slice with explicit setter or memory-sharing policy.

### 5.4 Implementation status for now

Only the `ffunction(...)` track is active and implemented for now.

`fconst(...)` and `fvar(...)` remain documented here as follow-up design
directions, but they are deferred and are not part of the active
implementation slices below.

Reason:

- the `ffunction(...)` path already has the necessary FIR prototype structure,
- it is the smallest wasm foreign-import milestone,
- it lets the backend ABI be exercised before adding value/global semantics.

---

## 6. Proposed Implementation Slices

### Slice 1 — Formalize current support

Goal:

- document and stabilize the currently supported imported foreign helper subset

Changes:

- add explicit backend docs for current `ffunction` support in WASM
- add regression tests for:
  - supported imported helper (`sinhf`, `isnan`, etc.)
  - supported generic host function (`myhost`)

Pass criteria:

- behavior is intentional and covered by tests

### Slice 2 — Carry generic foreign prototypes into WASM import planning

Goal:

- stop treating general foreign calls as anonymous unsupported `FunCall`
- make them available to the import planner

Changes:

- extend the WASM backend import collection pass to discover generic
  `FunCall`s that originate from foreign prototypes
- build one import descriptor from name + arg types + return type

Notes:

- this may require preserving or exposing more provenance from FIR lowering if
  plain `FunCall(name, ...)` is currently insufficient to distinguish ordinary
  functions from `ffunction(...)`-originated calls

Pass criteria:

- backend can build a typed import plan for arbitrary foreign prototypes

Status:

- implemented

### Slice 3 — Emit generic imported functions in the WASM module

Goal:

- generate valid WASM imports for generic `ffunction(...)`

Changes:

- extend the import section builder in `codegen::backends::wasm`
- assign type indices for generic imported foreign functions
- ensure function indices remain ABI-correct:
  - imported functions first
  - built-in exported functions after imports

Pass criteria:

- generated module validates with `wasmparser`
- function calls target the imported index, not a fallback error path

Status:

- implemented

### Slice 4 — Define host ABI and runtime contract

Goal:

- make the feature usable, not just encodable

Changes:

- document the host import contract:
  - module name
  - import field naming
  - scalar type mapping
  - float32/float64 symbol selection
- update `faustwasm` integration plan if needed
- decide whether unsupported imports fail at:
  - compile time, or
  - instantiation time in the host

Recommendation:

- compile time should succeed if the import signature is representable
- host/runtime instantiation should fail if the import is missing

Pass criteria:

- one small end-to-end runtime test can instantiate a generated module with
  user-provided imports

### Slice 5 — Metadata and diagnostics polish

Goal:

- make the feature understandable to users

Changes:

- add backend diagnostics clarifying that:
  - native include/lib semantics do not apply in WASM output
  - `ffunction(...)` becomes a host import
  - `fconst(...)` / `fvar(...)` stay out of the active implementation scope for
    now
- optionally surface foreign import requirements in companion JSON or debug
  output

Pass criteria:

- failure modes are explicit and actionable

---

## 7. Validation Plan

### 7.1 Unit/backend tests

Add targeted tests in `crates/codegen/src/backends/wasm/tests.rs` for:

- import emission for one generic unary float foreign function
- import emission for one generic binary double foreign function
- retained support for the hard-coded helper whitelist
- retained deferral of `fconst(...)` / `fvar(...)` in the active wasm support
  plan
- function-index and type-index stability with mixed:
  - built-in math imports
  - generic foreign function imports

### 7.2 Compiler integration tests

Add compiler-facing tests for:

- `-lang wasm` on a DSP using supported whitelist helpers
- `-lang wasm` on a DSP using a generic host-imported `ffunction(...)`
- no change in current support status for `fconst(...)` / `fvar(...)`
- stable artifact production in supported cases

### 7.3 Runtime validation

At least one end-to-end runtime check should instantiate the generated module
with a host import object supplying the expected function(s), ideally in the
same style used by `faustwasm`.

Minimum target:

- one Node/WebAssembly host harness
- one DSP using `ffunction(float myhost(float), ...)`
- successful instantiation and `compute`

---

## 8. Risks and Design Tensions

### 8.1 FIR provenance loss

Risk:

- by the time the backend sees a node, the distinction between:
  - ordinary helper
  - `ffunction(...)`
  - `fconst(...)`
  - `fvar(...)`
  may be too weak

Mitigation:

- enrich FIR extern metadata or keep one side-table of foreign-symbol
  descriptors reachable from the module

### 8.2 ABI drift against historical JS wrappers

Risk:

- generic import naming may diverge from historical C++/JS wrapper behavior

Mitigation:

- keep the existing hard-coded helper mappings intact
- use the same `"env"` import module by default
- validate against `faustwasm` expectations before broadening naming rules

### 8.3 Over-scoping the first milestone

Risk:

- folding `fconst(...)` / `fvar(...)` into the first implementation pass would
  mix value/global semantics into what is currently a pure function-import
  problem

Mitigation:

- keep the roadmap visible in this document
- but restrict the active implementation slices to `ffunction(...)` only for
  now

### 8.4 User confusion around `incfile` / `libfile`

Risk:

- users may expect C/C++ linking semantics in a WASM target

Mitigation:

- document explicitly that, in WASM mode, these fields do not imply native
  linkage and are not sufficient by themselves

### 8.5 Cross-runtime portability

Risk:

- a design that works in one JS runtime may not generalize cleanly to all WASM
  hosts

Mitigation:

- keep the contract to plain WASM imports with scalar signatures only
- avoid JS-specific glue in the backend ABI itself

---

## 9. Deliverables

The work is complete when the following are true:

1. `ffunction(...)` with arbitrary scalar signatures can be represented as typed
   WASM imports.
2. `fconst(...)` and `fvar(...)` remain documented, but deferred.
3. The generated module validates and preserves stable import/function index
   ordering.
4. Existing imported math helper behavior remains intact.
5. At least one end-to-end host-instantiated example works with a user-defined
   imported function.
6. Documentation clearly states the semantic difference between native foreign
   functions and WASM host-provided functions, and marks the non-function
   foreign forms as deferred for now.

---

## 10. Recommended First Increment

The best first implementation increment is:

1. keep the existing hard-coded helper path unchanged
2. generalize import planning for `ffunction(...)` first
3. prove the end-to-end host import model with a minimal runtime harness
4. leave `fconst(...)` and `fvar(...)` documented but deferred for a later,
   explicitly justified plan

This keeps risk low while forcing the important ABI decisions early.
