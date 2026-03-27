# `faustwasm` Dual-Mode Rust Interface Plan

**Date:** 2026-03-26
**Status:** In progress
**Target repos:** `faust-rs`, `faustwasm`
**Target crates:** `compiler`, `codegen`, `wasm-ffi` (new, proposed)
**C++ provenance:** `compiler/generator/wasm/`, `compiler/generator/wasm/bindings/`

---

## 1. Purpose

This document defines the porting plan for the interface layer needed to use
the Rust Faust compiler from the `faustwasm` project.

The target model must preserve the historical **dual mode** that already exists
in `faustwasm`:

1. **Embedded compiler mode**
   `faustwasm` can compile Faust source to WASM/JSON on demand.

2. **Precompiled artifact mode**
   `faustwasm` can consume an already compiled `{ wasm, json }` artifact pair
   without embedding the compiler.

The goal is to keep that model, but replace the current C++/Emscripten-facing
API with a cleaner interface that matches the Rust architecture better.

---

## 2. Why a New Interface Is Needed

The current C++ path used by `faustwasm` is centered on `libFaustWasm` and its
bindings:

- `createDSPFactory(name, dsp_content, args, internal_memory)`
- `deleteDSPFactory(cfactory)`
- `expandDSP(...)`
- `generateAuxFiles(...)`
- `getInfos(...)`
- exception cleanup helpers

This design was practical for the historical libfaust + Emscripten stack, but
it mixes several responsibilities:

- compile-service requests
- factory lifetime management
- JS binding constraints
- byte-vector transfer details
- error transport
- metadata/artifact serialization

For the Rust port, we should avoid reproducing the C++ object-lifetime model
(`cfactory`, explicit factory deletion, exception cleanup machinery) unless it
is still required by a concrete runtime constraint.

---

## 3. Constraints

### 3.1 Functional constraints

The new Rust-facing interface must support:

- compiling DSP source to WASM + companion JSON
- passing compiler arguments from JS
- selecting internal vs external memory for the generated WASM
- exposing diagnostics/errors in a JS-friendly way
- exposing helper actions currently used by `faustwasm`:
  - `expandDSP`
  - `generateAuxFiles`
  - `getInfos`

### 3.2 Product constraints

The `faustwasm` product model must remain intact:

- browser/worker environments may embed the compiler
- production deployments may skip the compiler and load precompiled artifacts
- the same downstream DSP/runtime classes should work in both cases once a
  `FaustDspFactory`-like object has been obtained

### 3.3 Embedded-library constraints

The Rust embedded-compiler module should be self-contained for the standard
Faust library set used by `import("stdfaust.lib")` and related imports.

That means:

- the shipped compiler-module `.wasm` should embed the standard Faust library
  sources as read-only assets
- import resolution inside the compiler-module should not depend on a host
  filesystem being present
- browser, worker, and Node use cases should all resolve the embedded library
  set the same way
- optional user-provided import roots may still be layered on top, but should
  not be required for the standard library set

### 3.4 Compatibility constraints

Compatibility is required at the **behavioral level**, not necessarily as a
1:1 copy of the current C++ binding API.

That means:

- preserving the dual “compile or load artifacts” product workflow
- preserving the downstream runtime expectations of `faustwasm`
- allowing a transition layer in `faustwasm`

It does **not** require preserving:

- `cfactory` as a public concept
- C++ object ownership semantics
- Emscripten-specific vector wrappers as the primary transport

---

## 4. Current C++ Model Summary

Today, `faustwasm` interacts with the compiler roughly like this:

1. JS calls `LibFaust.createDSPFactory(...)`
2. C++ compiles Faust source to:
   - binary WASM bytes
   - JSON companion
3. JS converts the returned byte vector to `Uint8Array`
4. JS compiles the module with `WebAssembly.compile(...)`
5. JS immediately deletes the C++ factory pointer
6. JS caches the resulting artifact set in its own `FaustDspFactory` object

So although the C++ API is factory-centric, the effective JS product is already
much closer to a pure artifact object:

- `code: Uint8Array`
- `module: WebAssembly.Module`
- `json: string`
- `poly: boolean`
- `shaKey`

This is the key opportunity for the Rust redesign.

---

## 5. Target Rust Design

### 5.1 High-level direction

The Rust-side interface should be artifact-centric, not factory-pointer-centric.

The primary contract should be:

- take a compilation request
- return a compilation result object
- let JS own the caching and WASM module instantiation lifecycle

### 5.2 Two first-class modes

The interface should explicitly support two entry points in `faustwasm`:

1. **Compiler-backed factory creation**
   - Faust source + args in
   - compiled artifact bundle out

2. **Artifact-backed factory loading**
   - precompiled `{ wasm, json }` in
   - normalized runtime factory out

These two paths should converge as early as possible on one common JS-side
factory representation.

### 5.3 Proposed Rust-facing artifact model

The Rust compile layer should expose a typed result structurally equivalent to:

```text
CompileResult {
  wasm_bytes: Vec<u8>,
  dsp_json: String,
  compile_options: String,
  warnings: Vec<String>,
  aux_files: Option<Vec<AuxFile>>,
}
```

Where `AuxFile` is conceptually:

```text
AuxFile {
  path: String,
  content: Vec<u8> | String,
  binary: bool,
}
```

The exact JS transport format can differ, but this is the semantic contract.

### 5.4 Proposed `faustwasm` convergence model

Inside `faustwasm`, both modes should converge to a single internal factory
shape:

```text
FaustDspFactory {
  code: Uint8Array,
  module: WebAssembly.Module,
  json: string,
  poly: boolean,
  shaKey: string,
  soundfiles: ...
}
```

That means:

- embedded compile mode produces this object from a Rust compile request
- artifact mode produces this object from already available files/buffers

This is already close to the current TypeScript-side shape, so the main change
is in how the object is produced, not in what the runtime consumes.

---

## 6. Proposed Interface Layers

### 6.1 Layer A: pure Rust compile service

Add a Rust API that is independent of JS/WASM bindings:

- compile DSP source to artifact bundle
- expand DSP
- generate auxiliary files
- query compiler info
- resolve standard Faust-library imports through an embedded read-only bundle

This layer should live in Rust-native crates and be fully testable without any
binding/runtime packaging.

Proposed ownership:

- `compiler`: orchestration and typed requests/results
- `codegen`: backend artifacts
- `wasm-ffi` (new): binding-oriented transport wrappers, if needed

### 6.2 Layer B: JS/WASM binding surface

Expose the compile service to `faustwasm` through a small transport layer.

This layer may be implemented through:

- `wasm-bindgen`
- custom exports from a Rust-compiled WASM module
- another binding mechanism chosen later

The key point is that this layer should remain a thin adapter over Layer A, not
the owner of compiler semantics.

### 6.4 Embedded-library resolution model

For the embedded Rust compiler path, standard Faust libraries should be bundled
as source assets inside the compiler-module rather than exposed through a
general-purpose virtual filesystem.

Recommended model:

- build a generated read-only bundle mapping logical import names to source
  text, for example `stdfaust.lib`, `music.lib`, and related files
- resolve `import(...)` requests against that embedded bundle first
- allow optional user-provided import roots or explicit source assets to
  override or extend the embedded bundle when `faustwasm` needs additional
  libraries
- keep this mechanism request/response oriented and independent of Emscripten
  filesystem semantics

Rejected as the primary design:

- a general-purpose Emscripten-style virtual filesystem clone
- dependence on host paths from inside the compiler-module
- network fetching of library sources at compile time

### 6.3 Layer C: `faustwasm` integration adapter

Adapt `faustwasm/src/LibFaust.ts` and `faustwasm/src/FaustCompiler.ts` so they
consume the new Rust binding surface.

This adapter should:

- preserve the public product workflow
- normalize Rust compile outputs into the existing JS-side factory shape
- avoid exposing Rust binding details to the rest of `faustwasm`

---

## 7. API Direction

### 7.1 Embedded compiler mode API

Preferred semantic shape:

```text
compileDSP(request) -> CompileResult
```

Where `request` includes:

- `name`
- `source`
- `args`
- `target = wasm`
- `internal_memory`
- optional future flags:
  - `emit_wast`
  - `emit_aux_files`

This is cleaner than:

- `createDSPFactory(...)`
- separate object lifetime calls
- side-channel error cleanup

### 7.2 Artifact mode API

Preferred semantic shape in `faustwasm`:

```text
loadDSPFactory({ wasm, json, poly? }) -> FaustDspFactory
```

This mode should not depend on the compiler package being present.

### 7.3 Auxiliary actions

The following should stay request/response based:

- `expandDSP(request) -> ExpandedDspResult`
- `generateAuxFiles(request) -> AuxFilesResult`
- `getInfos(what) -> string`

Avoid side effects or hidden filesystem ownership in the binding interface
unless explicitly required.

---

## 8. Explicit Non-Goals

The first iteration should **not** attempt to port:

- the full C++ `wasm_dsp_factory` object model
- Wasmtime-based native C++ runtime support from `wasm_dsp_aux`
- C++ smart-pointer / factory-table lifetime mechanics
- a byte-for-byte clone of the Emscripten binding surface

Those are implementation details of the historical C++ stack, not required
product features for `faustwasm`.

---

## 9. Implementation Phases

### Phase 0: contract freeze

Deliverables:

- one reviewed TypeScript/Rust boundary contract document
- one explicit decision on the binding technology
- one decision on whether to keep a temporary compatibility shim named
  `libFaustWasm` in `faustwasm`

Pass criteria:

- no unresolved ambiguity on public API shape
- no unresolved ambiguity on error transport model

### Phase 1: pure Rust compile service

Deliverables:

- Rust request/response API for:
  - compile DSP to WASM artifact bundle
  - expand DSP
  - generate aux files
  - info queries
  - embedded standard-library import resolution contract
- unit/integration coverage at the Rust layer

Pass criteria:

- can compile `osc.dsp` to `{ wasm, json }`
- can compile a representative `import("stdfaust.lib")` DSP through the
  self-contained embedded-library path
- can return structured errors without JS binding involvement

### Phase 2: binding layer

Deliverables:

- minimal Rust->JS binding exposing the compile service
- a real Rust compiler WASM module built from `wasm-ffi` for
  `wasm32-unknown-unknown`
- documented raw exports for memory allocation, compile requests, and
  text-result helpers
- binary payload transfer for WASM bytes
- string payload transfer for JSON/diagnostics
- embedded standard Faust libraries packaged into the shipped compiler-module
  asset as read-only sources

Pass criteria:

- JS can request compilation and receive a usable artifact bundle
- no explicit factory pointer lifetime is required in the public API
- the Rust compiler module can be instantiated directly with
  `WebAssembly.instantiate(...)`
- JSON/WASM artifact requests default to the transform fast lane so the
  returned FIR module keeps `metadata` and `buildUserInterface`
- the compiler-module resolves `import("stdfaust.lib")` without requiring a
  host filesystem

### Phase 3: `faustwasm` integration

Deliverables:

- adapted `LibFaust.ts`
- adapted `FaustCompiler.ts`
- one dedicated loader for the raw Rust compiler module
- retained artifact-loading path
- compiler-module asset packaging/distribution plan
- shared convergence to one JS factory representation
- documented override mechanism for optional user-supplied import roots/assets
- packaged internal mixer assets for the Rust polyphonic path when no compiler
  filesystem is available

Pass criteria:

- embedded compile mode works through the Rust interface
- `faustwasm` can load the Rust compiler module as a first-class alternative to
  the historical C++/Emscripten compiler package
- precompiled artifact mode still works unchanged at the product level
- validated end-to-end with a Rust-produced compiler module loaded from
  `faustwasm`, including visible UI controls in the returned companion JSON
- validated end-to-end on at least one DSP importing `stdfaust.lib`
- validated end-to-end on at least one polyphonic DSP using the packaged mixer
  fallback instead of the historical compiler filesystem

Current helper-surface status snapshot:

| Surface | Status |
| --- | --- |
| `createDSPFactory(...)` | implemented through the Rust artifact compile service |
| `getInfos("version")` | implemented |
| `getInfos("help")` | implemented |
| `getInfos("libdir"\\|"includedir"\\|"archdir"\\|"dspdir"\\|"pathslist")` | explicit `unsupported` |
| `expandDSP(...)` | API present, still not parity-complete |
| `generateAuxFiles(...)` | API present, still not parity-complete |
| polyphonic internal mixer | implemented through packaged mixer fallback, not compiler `FS` |

### Phase 4: compatibility hardening

Deliverables:

- differential validation against current `faustwasm` behavior
- docs for migration from C++ libfaust-wasm to Rust compile service
- performance and caching review

Pass criteria:

- validated on representative mono/poly DSP examples
- no regression in `faustwasm` runtime behavior on the supported WASM subset

---

## 10. Error Model

The Rust-facing interface should prefer explicit result payloads over exception
cleanup side channels.

Preferred shape:

```text
Result<CompileResult, CompileErrorPayload>
```

Where `CompileErrorPayload` contains:

- user-facing message
- optional structured diagnostics
- optional phase/stage tag

If the chosen binding technology forces exceptions at the JS boundary, the Rust
service layer should still keep the internal contract typed and exception-free.

---

## 11. Caching Model

The current `faustwasm` cache is already JS-side and artifact-oriented.

That should remain the primary cache layer:

- hash key built from `name + source + args + mono/poly`
- cache stores `{ wasm bytes, module, json }`

The Rust compile service should not initially own a long-lived factory cache.
If a Rust-side cache is later justified, it should be an internal optimization,
not a public API concept.

---

## 12. Migration Strategy

### Option A: hard switch

Replace the current `libFaustWasm` integration in `faustwasm` with the Rust
binding surface in one step.

Pros:

- cleaner final architecture immediately

Cons:

- harder to validate incrementally

### Option B: compatibility adapter

Add a thin adapter in `faustwasm` that presents the old conceptual methods
(`createDSPFactory`, `expandDSP`, `generateAuxFiles`, `getInfos`) but is backed
by the new Rust compile service internally.

Pros:

- easier migration
- smaller blast radius for downstream code

Cons:

- temporarily keeps some historical naming

Recommended direction:

- **Option B first**
- then simplify/rename once the Rust path is stable

---

## 13. Key Risks

### 13.1 Binding technology risk

The cleanest semantic API may still be constrained by the actual JS/WASM
binding stack chosen for the Rust compiler packaging.

Mitigation:

- freeze the semantic contract first
- let the binding layer adapt to it, not redefine it

### 13.2 Payload transfer cost

Transferring large generated WASM binaries or auxiliary file sets across the
binding boundary may become expensive.

Mitigation:

- start with simple correctness-first transport
- measure before optimizing

### 13.3 Drift between embedded-compile mode and artifact mode

If the two paths normalize artifacts differently, `faustwasm` behavior will
diverge subtly.

Mitigation:

- converge both paths on one internal `FaustDspFactory` builder
- add explicit tests that compile mode and load mode yield equivalent runtime
  behavior

### 13.4 Over-porting the C++ object model

Recreating `cfactory`/factory tables/smart pointers in Rust would add
complexity with little product value.

Mitigation:

- keep the port artifact-centric
- only reintroduce explicit persistent factory identities if a concrete runtime
  need appears

### 13.5 Packaging drift between compiler and product integration

The compile-service API can be correct while the shipped compiler-module asset
and the `faustwasm` loader still fail to line up in practice.

Mitigation:

- treat the build of the `wasm-ffi` compiler module as an explicit project
  deliverable, not an implicit local developer step
- define one documented loader path in `faustwasm`
- validate the embedded-compiler mode end to end with the actual distributed
  compiler-module asset

### 13.6 Embedded-library packaging drift

The Rust compiler-module can be self-contained in principle while still
shipping an incomplete or inconsistent embedded library set in practice.

Mitigation:

- generate the embedded library bundle from one explicit source of truth
- test `import("stdfaust.lib")` end to end against the shipped compiler-module
- keep the logical import names stable and documented
- make user-provided overrides explicit instead of relying on hidden path
  conventions

---

## 14. Recommended First Slice

The first implementation slice should be deliberately small:

1. Rust compile service:
   - compile source -> `{ wasm, json }`
2. JS binding:
   - one compile request/response path
3. `faustwasm` adapter:
   - implement `createDSPFactory` on top of the new Rust compile service
4. Keep artifact-loading mode unchanged

This gives immediate value while preserving the dual product model.

The next milestone after that first slice should make the embedded-compiler
path actually shippable:

1. Compiler-module build:
   - build `crates/wasm-ffi` as a real `wasm32-unknown-unknown` compiler
     module
   - document the expected exports and build command
2. Compiler-module loader:
   - add a dedicated `faustwasm` loader for the raw Rust compiler module
   - return the typed raw-export surface expected by the Rust adapter
3. Compiler-module asset distribution:
   - decide how the compiler module is packaged for `faustwasm`
   - ensure the embedded-compiler mode can obtain the matching `.wasm` asset
4. End-to-end embedded validation:
   - instantiate the Rust compiler module from `faustwasm`
   - run `createDSPFactory(...)` through to a usable `FaustDspFactory`
   - verify that precompiled artifact mode remains unchanged

The next milestone after that should make the embedded compiler self-contained
for Faust libraries:

1. Embedded library bundle:
   - generate a read-only bundle of standard Faust library sources for the
     compiler-module
   - keep logical import names aligned with Faust source imports
2. Import resolution:
   - resolve `import(...)` requests against the embedded bundle first
   - define how optional user-provided import roots/assets override or extend
     the embedded bundle
3. Product packaging:
   - ship the compiler-module with the matching embedded library set
   - avoid dependence on host filesystem layout in browser/worker mode
4. End-to-end validation:
   - compile a representative `stdfaust.lib`-based DSP in embedded mode
   - confirm artifact mode remains unchanged

---

## 15. Success Criteria

This plan is considered successful when all of the following are true:

- `faustwasm` can still operate in two modes:
  - embedded compile
  - precompiled artifact loading
- the embedded compile path is backed by Rust, not the historical C++
  libfaust-wasm path
- the shipped Rust compiler-module is self-contained for the standard Faust
  library set used by `faustwasm`
- the public/product workflow remains stable for users
- the Rust integration no longer depends on public `cfactory`-style object
  ownership
- the resulting interface is simpler to reason about than the current
  C++/Emscripten binding stack
- the shipped Rust compiler-module asset and the `faustwasm` loader are tested
  together end to end
