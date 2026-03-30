# WebAssembly Foreign Symbol Support Plan

**Date:** 2026-03-30
**Status:** Planning
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

So the missing work is not basic parsing. It is backend-side realization and
ABI policy.

### 2.2 Current behavior by foreign symbol kind

#### `ffunction(...)`

The Rust WASM backend currently accepts:

- built-in FIR math intrinsics,
- a small hard-coded set of foreign helper names mapped to host imports.

Relevant implementation:

- `crates/codegen/src/backends/wasm/mod.rs`
  - `FirMatch::FunCall { ... }` lowering
  - `imported_foreign_signature(...)`

If a function call is neither:

- a recognized `FirMathOp`, nor
- one of the hard-coded imported foreign helpers,

code generation fails with:

```text
unsupported function call in WASM subset: `<name>`
```

Observed behavior:

- `ffunction(float sinhf(float), <math.h>, "")` compiles with `-lang wasm`
- `ffunction(float myhost(float), <dummy.h>, "")` currently fails in the WASM
  backend

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

### 4.4 Prefer function-shaped ABI surfaces, even for values

Although wasm imported globals are possible, the recommended default ABI is:

- imported functions for `ffunction(...)`,
- imported zero-argument getter functions for portable `fconst(...)`,
- imported getter functions for portable `fvar(...)`.

Examples:

- `ffunction(float myhost(float), ...)` -> import `env.myhost`
- `fconst(float, "extsr", ...)` -> import `env.extsr`
  as `() -> f32` or `() -> f64`
- `fvar(float, "cutoff", ...)` -> import `env.cutoff`
  as `() -> f32` or `() -> f64`

Why prefer getter functions:

- simpler import planning in the current backend,
- fewer host/runtime differences than imported mutable globals,
- easier validation and fallback diagnostics,
- better control over update timing if hosts want dynamic values.

Imported globals can remain a later optimization or alternate ABI mode, but
they should not be the first implementation target.

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

Main missing work:

- carry generic foreign prototypes into WASM import planning,
- emit import descriptors beyond the current hard-coded whitelist,
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

---

## 6. Proposed Implementation Slices

### Slice 1 — Formalize current support and rename the scope

Goal:

- stabilize the existing `ffunction` subset,
- document `fconst` and `fvar` current limits under one plan.

Changes:

- update backend docs to describe current behavior for all three symbol kinds,
- keep the existing `ffunction` whitelist tests,
- keep the existing foreign-variable rejection test,
- add a regression test for the current `fconst` fast lane:
  `fSamplingRate` remains internal.

Pass criteria:

- the current state is explicit, intentional, and covered by tests.

### Slice 2 — Preserve foreign-symbol provenance into WASM planning

Goal:

- stop treating foreign symbols as backend-anonymous nodes once they reach wasm
  lowering,
- give the backend enough information to distinguish:
  - ordinary FIR helpers,
  - `ffunction(...)`-originated calls,
  - `fconst(...)` getter candidates,
  - `fvar(...)` getter candidates.

Changes:

- enrich FIR-side metadata or module-level side tables so the wasm backend can
  recover foreign-symbol provenance,
- keep the existing lowering for `fSamplingRate` and `count` untouched.

Pass criteria:

- the backend can build a typed import plan from foreign-symbol metadata rather
  than from name heuristics alone.

### Slice 3 — Generalize imported function emission for `ffunction(...)`

Goal:

- generate valid WASM imports for generic scalar `ffunction(...)`.

Changes:

- extend the import section builder in `codegen::backends::wasm`,
- assign type indices for generic imported foreign functions,
- ensure function indices remain ABI-correct:
  - imported functions first,
  - built-in exported functions after imports,
- keep the existing hard-coded helper remapping path working.

Pass criteria:

- generated modules validate with `wasmparser`,
- generic host-imported `ffunction(...)` compiles,
- existing helper behavior remains intact.

### Slice 4 — Add `fconst(...)` support through getter imports

Goal:

- support portable non-special foreign constants in wasm without introducing
  imported-global complexity first.

Changes:

- extend FIR/wasm lowering so supported foreign constants become imported
  zero-argument calls or equivalent wasm import descriptors,
- reserve direct imported-global support for a later optional slice,
- keep unsupported shapes failing clearly.

Pass criteria:

- a DSP using one non-special scalar `fconst(...)` compiles to wasm,
- `fSamplingRate` still uses the internal fast lane,
- diagnostics distinguish internal runtime constants from real foreign
  constants.

### Slice 5 — Add `fvar(...)` read support through getter imports

Goal:

- support readable foreign variables in wasm.

Changes:

- extend wasm lowering so foreign variable reads become imported getter calls or
  equivalent import descriptors,
- preserve `count` as an internal compute argument,
- keep writable-foreign-variable semantics out of scope for this slice.

Pass criteria:

- the existing rejection test is replaced with positive coverage for supported
  read-only foreign variables,
- `count` keeps its current lowering semantics,
- missing host imports fail at instantiation, not at backend encoding time.

### Slice 6 — Define host ABI and runtime contract

Goal:

- make the feature usable, not just encodable.

Changes:

- document the host import contract:
  - default module name
  - import field naming
  - scalar type mapping
  - float32/float64 name selection
  - getter-based conventions for `fconst` and `fvar`
- update `faustwasm` integration notes if needed,
- decide whether unsupported foreign-symbol signatures fail at:
  - compile time, or
  - host instantiation time.

Recommendation:

- compile time should succeed if the foreign symbol shape is representable in
  the wasm ABI,
- host/runtime instantiation should fail if the required import is missing.

Pass criteria:

- one small end-to-end runtime test can instantiate a generated module with
  user-provided imports for function, constant, and variable cases.

### Slice 7 — Metadata and diagnostics polish

Goal:

- make the feature understandable to users.

Changes:

- add backend diagnostics clarifying that:
  - native include/lib semantics do not apply in WASM output,
  - `ffunction(...)` becomes a host function import,
  - non-special `fconst(...)` becomes a host-provided value import,
  - non-special `fvar(...)` becomes a host-provided variable read import,
- optionally surface foreign import requirements in companion JSON or debug
  output.

Pass criteria:

- failure modes are explicit and actionable.

---

## 7. Validation Plan

### 7.1 Unit/backend tests

Add targeted tests in `crates/codegen/src/backends/wasm/tests.rs` for:

- import emission for one generic unary float foreign function,
- import emission for one generic binary double foreign function,
- retained support for the hard-coded helper whitelist,
- `fSamplingRate` staying internal,
- one non-special foreign constant lowered to a getter-style import,
- one readable foreign variable lowered to a getter-style import,
- function-index and type-index stability with mixed:
  - built-in math imports,
  - generic foreign function imports,
  - getter-style foreign symbol imports.

### 7.2 Compiler integration tests

Add compiler-facing tests for:

- `-lang wasm` on a DSP using supported whitelist helpers,
- `-lang wasm` on a DSP using a generic host-imported `ffunction(...)`,
- `-lang wasm` on a DSP using a non-special `fconst(...)`,
- `-lang wasm` on a DSP using a non-special `fvar(...)`,
- stable artifact production in all supported cases.

### 7.3 Runtime validation

At least one end-to-end runtime check should instantiate the generated module
with a host import object supplying the expected function(s).

Minimum target:

- one Node/WebAssembly host harness,
- one DSP using `ffunction(float myhost(float), ...)`,
- one DSP using one non-special foreign constant,
- one DSP using one readable foreign variable,
- successful instantiation and `compute`.

---

## 8. Risks and Design Tensions

### 8.1 FIR provenance loss

Risk:

- by the time the backend sees a node, the distinction between:
  - ordinary helper,
  - `ffunction(...)`,
  - `fconst(...)`,
  - `fvar(...)`
  may be too weak.

Mitigation:

- enrich FIR extern metadata or keep one side-table of foreign-symbol
  descriptors reachable from the module.

### 8.2 ABI drift against historical JS wrappers

Risk:

- generic import naming may diverge from historical C++/JS wrapper behavior.

Mitigation:

- keep the existing hard-coded helper mappings intact,
- use the same `"env"` import module by default,
- validate against `faustwasm` expectations before broadening naming rules.

### 8.3 Overusing imported globals too early

Risk:

- imported globals may look natural for `fconst` / `fvar`, but complicate the
  first implementation and reduce portability.

Mitigation:

- ship getter-style imports first,
- add direct imported-global support only if profiling or host integration
  clearly justifies it.

### 8.4 User confusion around `incfile` / `libfile`

Risk:

- users may expect C/C++ linking semantics in a WASM target.

Mitigation:

- document explicitly that, in WASM mode, these fields do not imply native
  linkage and are not sufficient by themselves.

### 8.5 Runtime update semantics for `fvar(...)`

Risk:

- hosts may expect `fvar(...)` to behave like one stable captured value, while
  others may expect it to be refreshed on each access.

Mitigation:

- document the first supported semantics explicitly:
  each lowered getter call reads the current host-provided value at the point of
  DSP execution where the FIR node appears.

### 8.6 Cross-runtime portability

Risk:

- a design that works in one JS runtime may not generalize cleanly to all wasm
  hosts.

Mitigation:

- keep the contract to plain wasm imports with scalar signatures only,
- avoid JS-specific glue in the backend ABI itself.

---

## 9. Deliverables

The work is complete when the following are true:

1. `ffunction(...)` with arbitrary scalar signatures can be represented as typed
   WASM imports.
2. Non-special scalar `fconst(...)` can be represented in wasm with a stable
   host ABI.
3. Readable non-special scalar `fvar(...)` can be represented in wasm with a
   stable host ABI.
4. `fSamplingRate` and `count` remain special internal runtime/compiler paths.
5. The generated module validates and preserves stable import/function index
   ordering.
6. Existing imported math helper behavior remains intact.
7. At least one end-to-end host-instantiated example works for each supported
   foreign-symbol category.
8. Documentation clearly states the semantic difference between native foreign
   symbols and WASM host-provided symbols.

---

## 10. Recommended First Increment

The best first implementation increment is:

1. keep the existing hard-coded helper path unchanged,
2. generalize import planning for `ffunction(...)` first,
3. add getter-based `fconst(...)` support next,
4. add getter-based `fvar(...)` read support after that,
5. leave imported globals and writable-variable semantics for a later,
   explicitly justified increment.

This keeps risk low while forcing the important ABI decisions early.
