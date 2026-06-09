# `libfaust` Box/Signal API Parity Plan — 2026-06-09

## 1. Purpose

This plan aligns `faust-rs` with the public Box and Signal APIs exported by the
Faust C++ project through:

- `architecture/faust/dsp/libfaust-box.h`
- `architecture/faust/dsp/libfaust-box-c.h`
- `architecture/faust/dsp/libfaust-signal.h`
- `architecture/faust/dsp/libfaust-signal-c.h`

The target is not only to expose more Rust functions. The goal is to make the
Rust port present the same programmatic surface as libfaust for external tools:
construct boxes/signals, inspect them structurally, convert Box to Signal, query
box arity, generate source from Box or Signal roots, and free C-owned memory with
the same ownership contract as C++ libfaust.

This matters for the AI-DSP/agentic workflow: an agent should eventually build
and mutate Faust programs through a structured Box/Signal API rather than only
through text.

## 2. Reference Model

The C++ reference API has two mirrored surfaces.

### Box API

The Box API exposes:

- common opaque tree types: `CTree`, `Tree`, `Box`, `Signal`;
- scalar enums: `SType`, `SOperator`;
- context lifecycle: `createLibContext`, `destroyLibContext`;
- printing and atom conversion: `printBox`, `CprintBox`, `tree2str`,
  `tree2int`, `getUserData`;
- constructors for constants, wiring, composition, primitive functions, tables,
  foreign functions, UI controls, groups, soundfiles, metadata, routes;
- structural predicates/matchers: `isBox*` / `CisBox*`;
- compilation/conversion helpers:
  `DSPToBoxes`, `getBoxType`, `boxesToSignals`, `boxesToSignals2`,
  `createSourceFromBoxes`;
- C memory ownership: returned `char*` and arrays are freed with `freeCMemory`.

The C surface uses C++-compatible names with a `C` prefix and `Aux` suffixes for
fully-applied forms where C++ overloads cannot be represented:

- C++: `boxAdd()` and `boxAdd(Box, Box)`
- C: `CboxAdd()` and `CboxAddAux(Box, Box)`

### Signal API

The Signal API exposes:

- the same common opaque tree types and scalar enums;
- interval/type metadata: `Interval`, `getSigInterval`, `setSigInterval`,
  `getSigNature`;
- extended node metadata: `xtendedArity`, `xtendedName`, `sigBranches`;
- constructors for constants, inputs, delays, casts, tables, waveform,
  soundfile accessors, select, foreign calls/constants/vars, binops, math
  primitives, recursion, UI controls, bargraphs, attach;
- structural predicates/matchers: `isSig*` / `CisSig*`;
- normal-form and source helpers:
  `simplifyToNormalForm`, `simplifyToNormalForm2`, `createSourceFromSignals`.

The Signal C API is not currently represented as a separate Rust FFI crate.
Today most exported functions live in `crates/box-ffi`, whose context can also
encode signal handles produced by `CboxesToSignals`.

## 3. Current `faust-rs` State

Relevant Rust crates:

- `crates/boxes`: canonical Rust `BoxBuilder`, `BoxMatch`, `match_box`,
  `dump_box`.
- `crates/signals`: canonical Rust `SigBuilder`, `SigMatch`, `match_sig`,
  `dump_sig`, FAD/RAD carriers, structured FIR/IIR carriers.
- `crates/box-ffi`: process-global C context, many `Cbox*` and `CisBox*`
  exports, `CDSPToBoxes`, `CgetBoxType`, `CboxesToSignals*`,
  `CcreateSourceFromBoxes`, `freeCMemory`.
- `crates/faust-ffi`: aggregator crate exporting `box-ffi`, `interp-ffi`, and
  `cranelift-ffi` symbols as the distribution library.

The existing direction is good: internal Rust builders/matchers are already
cleaner than a raw FFI port, and `box-ffi` intentionally mirrors libfaust names.
The missing piece is a systematic parity matrix and a complete Signal C API
surface.

## 4. Design Principles

1. Preserve the C++ public naming contract at the C ABI boundary.
   External C/C++ callers should recognize `Cbox*`, `CisBox*`, `Csig*`,
   `CisSig*`, `CcreateSourceFrom*`, and `freeCMemory`.

2. Keep Rust-native APIs idiomatic and typed.
   `BoxBuilder`, `BoxMatch`, `SigBuilder`, and `SigMatch` remain the internal
   canonical APIs. The FFI layer adapts them; it must not leak raw tag strings to
   external callers.

3. Use one coherent opaque handle model.
   `Box` and `Signal` are both opaque tree handles in the C++ API. In Rust this
   can be backed by one shared context/arena, but the FFI code should track
   handle provenance enough to detect obvious misuse and return null/false
   rather than panic.

4. Separate API parity from compiler semantic parity.
   An exported function may exist before all downstream semantics are complete,
   but gaps must be explicit and tested. Stubbed parity is acceptable only when
   documented and behavior is deterministic.

5. Match C memory ownership exactly.
   Any returned `char*` or null-terminated handle array must be owned by the
   FFI context or by heap allocation that `freeCMemory` can release.

## 5. Proposed Architecture

### 5.1 Introduce a shared `tree-ffi` support layer

Create a small internal crate or module, tentatively `crates/tree-ffi`, shared by
Box and Signal FFI.

Responsibilities:

- process-global `TreeArena` context and handle encoder/decoder;
- string allocation pool and `freeCMemory` integration;
- null-terminated arrays of opaque handles;
- C string decoding helpers;
- safe write helpers for `*mut *mut c_void`, `*mut c_int`, `*mut f64`;
- common enum definitions: `SType`, `SOperator`, and later `CInterval`;
- shared `CprintBox`, `CprintSignal`, `Ctree2str`, `Ctree2int`,
  `CgetUserData`, `CisNil`.

This avoids duplicating the current `BoxContext` when Signal API parity is
added. If extracting a crate is too much churn initially, factor the current
context into a private module inside `box-ffi` first, then split it once tests
stabilize.

Status 2026-06-09: extracted as `crates/tree-ffi`. The clean architecture choice
is a shared crate-level context support layer rather than a new top-level
`faust-ffi` owner. `box-ffi` keeps the Box API symbols and uses the shared
handle/string/array helpers; `signal-ffi` should use the same crate when added.

### 5.2 Keep `box-ffi` as the Box API owner

`box-ffi` should continue to export:

- all `Cbox*` constructors;
- all `CisBox*` predicates;
- `CDSPToBoxes`, `CgetBoxType`, `CboxesToSignals`, `CboxesToSignals2`,
  `CcreateSourceFromBoxes`;
- `CprintBox`.

The implementation should be audited against `libfaust-box-c.h` and a generated
matrix should classify each symbol as:

- `implemented-exact`;
- `implemented-nearest-rust-ir`;
- `stubbed-deterministic`;
- `missing`;
- `not-applicable`.

### 5.3 Add a dedicated Signal C API owner

Add a crate or module, tentatively `crates/signal-ffi`, exported by
`crates/faust-ffi`.

It should provide:

- `CsigInt`, `CsigInt64`, `CsigReal`, `CsigInput`;
- delay/cast/table/waveform/soundfile constructors;
- select/foreign/binop/math constructors;
- recursion constructors: `CsigSelf`, `CsigRecursion`, `CsigSelfN`,
  `CsigRecursionN`;
- UI signal constructors: button, checkbox, sliders, numentry, bargraphs;
- structural predicates: all `CisSig*` functions in `libfaust-signal-c.h`;
- source helpers: `CsimplifyToNormalForm`, `CsimplifyToNormalForm2`,
  `CcreateSourceFromSignals`;
- `CprintSignal`.

The Signal FFI should use `signals::SigBuilder` and `signals::match_sig` rather
than constructing raw tags directly.

### 5.4 Provide C++ header wrappers last

The Rust port should first expose the C ABI. Once stable, add generated or
hand-maintained headers matching:

- `libfaust-box-c.h`
- `libfaust-signal-c.h`
- optionally thin C++ overload wrappers matching `libfaust-box.h` and
  `libfaust-signal.h`.

Do not start with C++ wrappers. They hide ABI problems and make it harder to
test exact exported symbols.

## 6. Box API Work Items

### B1. Generate a Box symbol matrix

Write an `xtask` command or script that parses `libfaust-box-c.h` and compares
declared `LIBFAUST_API` C symbols with exported Rust symbols from `box-ffi`.

Initial output:

- `porting/generated/libfaust-box-c-api-matrix.md`
- columns: C symbol, C++ equivalent, Rust implementation, status, notes.

Manual notes to capture from the current code:

- advanced predicates like `CisBoxPrim*` are nearest-shape approximations until
  primitive function pointer identity exists.
- `CboxSoundfile` currently covers the two-argument C form; C++ also has a
  fully-applied read form with `part` and `ridx`.

### B2. Fill constructor gaps

Audit `BoxBuilder` against C++ `box*` constructors and add missing exact forms
before expanding FFI:

- logical vs arithmetic right shift, if currently conflated;
- real `exp10`;
- fully-applied `boxSoundfile(label, chan, part, ridx)`;
- any missing `boxPrimN` forms required by C++ parity;
- public constructors for metadata/definition nodes where C API exposes
  matching predicates but no current constructor exists.

Status 2026-06-09: logical right shift is no longer conflated with arithmetic
right shift at the Box FFI boundary. `BoxBuilder::lrsh` and `BoxMatch::LRsh`
now carry `CboxLRightShift*`, while the existing `rsh`/`BoxMatch::Rsh` path
keeps arithmetic-right-shift behavior for compatibility.

Status 2026-06-09: `CboxExp10*` no longer falls back to `exp`. Dedicated
`BOXEXP10`, `SIGEXP10`, and `FirMathOp::Exp10` nodes carry the math operation
through Box construction, signal propagation, typing/normalization, FIR
lowering, and backend code generation. The interpreter backend implements the
runtime call as `pow(10, x)` internally while preserving the external `exp10`
FIR/source surface.

Status 2026-06-09: the C++ `boxSoundfile(label, chan, part, ridx)` overload is
covered by the local wrapper header using the same reference composition as
C++ Faust: `boxSeq(boxPar(part, ridx), boxSoundfile(label, chan))`. No extra C
ABI symbol is introduced because the reference C header only exposes the
two-argument `CboxSoundfile` form.

### B3. Fill matcher gaps

Audit `BoxMatch` and `CisBox*` against `libfaust-box-c.h`.

Priorities:

- make every C predicate exported, even if it returns false for not-yet-modeled
  C++ node families;
- add exact match shapes for nodes already represented internally;
- avoid panics on invalid handles or null out-pointers;
- document nearest-equivalent mappings for Rust-only nodes such as FAD/RAD
  wrappers and closure/pattern side-table nodes.

### B4. Stabilize Box -> Signal conversion contracts

`CboxesToSignals` and `CboxesToSignals2` should match C++ ownership and error
behavior:

- returned arrays are null terminated;
- arrays are released by `freeCMemory`;
- error buffer behavior is explicit and bounded;
- signal handles remain valid after the call as long as the global context
  lives;
- propagated UI/control side effects are either preserved or explicitly
  dropped.

Status 2026-06-09: `CboxesToSignals` and `CboxesToSignals2` now have direct
FFI contract tests for null-terminated result arrays, release through
`freeCMemory`, and signal handle validity after the array allocation is freed.
The handles remain context-owned and valid until `destroyLibContext`, matching
the shared `tree-ffi` handle model.

### B5. Source generation parity

`CcreateSourceFromBoxes` should support the same high-level contract:

- `name_app`;
- target language string;
- argc/argv options;
- generated string returned with `freeCMemory`;
- diagnostics encoded in a stable way.

Start with Rust-supported backends (`c`, `cpp`, `interp`, possibly `fir`) and
return null plus a clear diagnostic for unsupported languages.

Status 2026-06-09: `CcreateSourceFromBoxes` has direct FFI tests covering the
currently supported `c`, `cpp`, `fir`, and `interp` language strings, returned
string ownership through `freeCMemory`, and the unsupported-language diagnostic
shape.

## 7. Signal API Work Items

### S1. Generate a Signal symbol matrix

Add the same matrix workflow for `libfaust-signal-c.h`:

- `porting/generated/libfaust-signal-c-api-matrix.md`
- C symbol, Rust `SigBuilder`/`SigMatch` mapping, status, notes.

### S2. Add `signal-ffi`

Implement a first `signal-ffi` slice:

- constants and input: `CsigInt`, `CsigInt64`, `CsigReal`, `CsigInput`;
- arithmetic/math: `CsigBinOp`, `CsigAdd`, `CsigSub`, `CsigMul`, `CsigDiv`,
  `CsigPow`, `CsigMin`, `CsigMax`, unary math;
- delays/casts: `CsigDelay`, `CsigDelay1`, `CsigIntCast`, `CsigFloatCast`;
- printing: `CprintSignal`;
- basic predicates: `CisSigInt`, `CisSigReal`, `CisSigInput`,
  `CisSigDelay`, `CisSigDelay1`, `CisSigBinOp`;
- null-safe behavior for every entry point.

Then export it from `faust-ffi`.

Status 2026-06-09: the process-global tree context moved from `box-ffi` into
`tree-ffi` before adding `signal-ffi`, so Box and Signal handles can share the
same opaque handle arena and existing shared symbols such as `CprintSignal` and
`freeCMemory` can decode handles from both API surfaces.

Status 2026-06-09: first slice implemented in `crates/signal-ffi` and exported
through `faust-ffi`. The slice covers constants, inputs, binary operators,
binary/unary math, shifts, comparisons, delays, casts, and the basic predicates
listed above. `CprintSignal` remains owned by `box-ffi` but now works for
`signal-ffi` handles through the shared `tree-ffi` context. The regenerated
Signal matrix now reports 60 exact implementation candidates and 63 missing
symbols; remaining missing rows are deferred to S3-S5.

### S3. Add table, waveform, soundfile, and UI signal constructors

Implement:

- `CsigReadOnlyTable`, `CsigWriteReadTable`, `CsigWaveform`;
- `CsigSoundfile`, `CsigSoundfileLength`, `CsigSoundfileRate`,
  `CsigSoundfileBuffer`;
- `CsigButton`, `CsigCheckbox`, `CsigVSlider`, `CsigHSlider`,
  `CsigNumEntry`, `CsigVBargraph`, `CsigHBargraph`, `CsigAttach`.

The main design question is UI identity. C++ signal constructors carry labels
directly; Rust `SigBuilder` carries `ui::ControlId`. The FFI layer should build
or retrieve corresponding UI entries in the shared context, so repeated labels
produce stable signal nodes and source generation can recover usable UI code.

Status 2026-06-09: implemented. `tree-ffi` now owns a Signal FFI control
registry keyed by control kind, label, and range expressions. `signal-ffi`
constructors register or reuse `ControlId` values for UI and soundfile leaves,
and `box-ffi` translates that registry into a synthesized `UiProgram` when
lowering Signal handles to FIR/source. The S3 implementation also added
`CsigSelect2` and `CsigSelect3` because they map directly to existing
`SigBuilder` constructors. The regenerated Signal matrix now reports 77 exact
implementation candidates and 46 missing symbols.

Status 2026-06-09: completed the remaining constructor subset by adding
`CsigFConst`, `CsigFVar`, and `CsigFFun`. The foreign-function constructor now
builds the C++-shaped `ffunction(signature, incfile, libfile)` descriptor in the
shared arena, preserves the full null-terminated `names` and `largs` arrays, and
uses the C++ zero-terminated `SType*` convention for argument types. `SType` and
`SOperator` are now plain copyable C tags in `tree-ffi`, matching their ABI use.
The regenerated Signal matrix now reports 120 exact implementation candidates
and 3 missing symbols: `CsimplifyToNormalForm`, `CsimplifyToNormalForm2`, and
`CcreateSourceFromSignals`.

### S4. Add recursion API parity

Implement:

- `CsigSelf`;
- `CsigRecursion`;
- `CsigSelfN`;
- `CsigRecursionN`.

This must be tested against current Rust recursion carriers and C++ printed
signal/source forms. RAD-specific carriers such as `BlockReverseAD` remain
internal Rust IR unless a future public API requires them.

Status 2026-06-09: implemented on Rust's canonical external recursion shape.
`CsigSelfN(i)` builds `delay1(proj(i, DEBRUIJNREF(1)))`, `CsigSelf()` aliases
slot 0, `CsigRecursion` wraps a single-body `SIGREC` and returns projection 0,
and `CsigRecursionN` returns per-slot projections while preserving closed
branches outside the recursion group. The regenerated Signal matrix now reports
81 exact implementation candidates and 42 missing symbols.

### S5. Add signal structural predicates

Map every `CisSig*` function to `SigMatch`.

Where Rust lacks a C++ node family:

- return false deterministically;
- record the gap in the matrix;
- add a focused parity test so the behavior is intentional.

Status 2026-06-09: implemented for all `Cis*` rows. Predicates backed by
`SigMatch` now decode their children through shared handle out-pointers; UI and
soundfile predicates recover labels/ranges from the `tree-ffi` Signal control
registry. The C++ doc-table predicate families return `false` deterministically
because faust-rs has no public doc-table Signal node family. `CisRec` is mapped
adaptively to Rust's `SIGREC(body)` shape by returning `nil` for the C++ symbolic
`var` slot and the Rust recursion body in `body`. The regenerated Signal matrix
now reports 117 exact implementation candidates and 6 missing symbols.

### S6. Add normal-form/source helpers

Implement:

- `CsimplifyToNormalForm`;
- `CsimplifyToNormalForm2`;
- `CcreateSourceFromSignals`.

Use the existing `normalize`/`transform`/`codegen` path where possible. The
first acceptable version can support C/C++ source generation only, then expand
to other backends.

Status 2026-06-09: implemented. `CsimplifyToNormalForm` and
`CsimplifyToNormalForm2` call the current Rust normal-form preparation subset
(`normalize::normalform`) with a synthesized Signal-only `UiProgram` recovered
from the shared FFI context. `CcreateSourceFromSignals` reuses the `box-ffi`
Signal-array FIR export path and the shared backend renderer, so Signal source
generation now supports the same `c`, `cpp`, `fir`, and `interp` language set
as `CcreateSourceFromBoxes`. The regenerated Signal matrix now reports 123
exact implementation candidates and no missing symbols.

## 8. Header And Packaging Work Items

### H1. C headers

Add generated or maintained C headers under `cffi/include/faust/dsp/` or an
equivalent distribution directory:

- `libfaust-box-c.h`
- `libfaust-signal-c.h`

They should compile as C and C++ and be usable with the `faust-ffi` staticlib or
cdylib.

Status 2026-06-09: added `crates/signal-ffi/include/libfaust-signal-c.h` as a
maintained C header for the implemented Signal API and updated
`crates/box-ffi/include/libfaust-box-c.h` so Box and Signal headers share one
guarded `CTree`/`SType`/`SOperator` definition. A local syntax check verifies
that including both headers compiles as C11 and C++17.

### H2. C++ overload wrappers

After the C ABI is stable, add C++ convenience headers:

- `libfaust-box.h`
- `libfaust-signal.h`

These wrappers can be thin overload shims over the C ABI rather than a full C++
object model. Preserve the C++ names where possible, but avoid exposing Rust
implementation details.

### H3. Export verification

Add a CI/local check that:

- builds `faust-ffi`;
- extracts exported dynamic symbols;
- compares them to the header matrix;
- compiles a tiny C example and a tiny C++ example using the headers.

## 9. Tests

### Unit tests

- Box FFI constructor/matcher round trips.
- Signal FFI constructor/matcher round trips.
- Null and invalid handle behavior.
- String and array ownership via `freeCMemory`.
- Enum value parity for `SType` and `SOperator`.

### Differential tests against C++ libfaust

For each supported family:

- construct the same Box/Signal through C++ libfaust and Rust FFI;
- print it with `CprintBox`/`CprintSignal`;
- compare normalized text where exact pointer names differ;
- for source helpers, compile generated C++ and run small impulse tests.

### Agentic API smoke tests

Add one small end-to-end test that builds a DSP through Box API only:

```text
button/slider or input -> onepole/filter graph -> getBoxType
-> boxesToSignals -> createSourceFromBoxes -> compile generated C++
```

Add one Signal-only source generation smoke test:

```text
sigInput(0), sigHSlider("g", ...), sigMul/add/delay
-> createSourceFromSignals -> compile generated C++
```

These tests are important because they exercise the future AI-DSP use case:
structured construction without source text.

## 10. Implementation Phases

### Phase 0 — Inventory and matrices

Deliverables:

- Box C API matrix.
- Signal C API matrix.
- list of exact/nearest/missing/stubbed symbols.

Exit criteria:

- every symbol in the four reference headers is classified.

### Phase 1 — Shared FFI context hardening

Deliverables:

- factored common handle/string/array/free helpers;
- no regressions in current `box-ffi` tests;
- documented ownership model.

Exit criteria:

- `freeCMemory` handles all Rust-allocated C strings and handle arrays used by
  Box/Signal APIs.

### Phase 2 — Box API completion

Deliverables:

- missing Box constructors/predicates filled or explicitly stubbed;
- corrected operator mismatches (`exp10`, right shifts, soundfile read form);
- stronger `CboxesToSignals*` and `CcreateSourceFromBoxes` tests.

Exit criteria:

- Box matrix has no unclassified symbols and no accidental missing exports.

### Phase 3 — Signal API first slice

Deliverables:

- new `signal-ffi` crate/module;
- constants, inputs, arithmetic, math, delay, casts, basic predicates;
- exported through `faust-ffi`.

Exit criteria:

- C client can build and print a simple signal graph using only `Csig*`.

Status 2026-06-09: satisfied for the first slice. `signal-ffi` tests build
constant/input/math/delay graphs with `Csig*`, inspect them through basic
`CisSig*` predicates, and print them through the shared `CprintSignal` helper.

### Phase 4 — Signal API full construction surface

Deliverables:

- tables, waveform, soundfile, UI, recursion constructors;
- corresponding predicates.

Exit criteria:

- Signal matrix construction/predicate rows are implemented or documented as
  deterministic gaps.

### Phase 5 — Source generation and normal form

Deliverables:

- `CsimplifyToNormalForm*`;
- `CcreateSourceFromSignals`;
- C/C++ source smoke tests.

Exit criteria:

- Signal-only C API can produce compilable C++ for a small DSP.

### Phase 6 — Headers and distribution

Deliverables:

- C headers;
- optional C++ overload headers;
- exported-symbol checks;
- C and C++ client examples.

Exit criteria:

- external client code can include Rust-provided headers and link against the
  Rust `faust-ffi` artifact without depending on Rust crates.

## 11. Open Questions

1. Should Box and Signal FFI share the current `box-ffi` global context, or
   should `faust-ffi` own a new top-level context and delegate to submodules?

2. How much C++ header compatibility is required in the first milestone:
   C ABI only, or also overload wrappers?

3. Should Rust-only AD carriers (`ForwardAD`, `ReverseAD`, `BlockReverseAD`) be
   exposed through public extension APIs, or kept internal behind ordinary
   `boxFAD`/`boxRAD` source-level compatibility?

4. What is the expected behavior for C++ APIs that rely on function pointer
   identity (`prim0`...`prim5`) when Rust has tag-based primitive nodes?

5. Should `getSigInterval` / `setSigInterval` be true mutable metadata on signal
   nodes, or should they be deferred until interval analysis parity is stronger?

## 12. Recommended First Task

Start with the inventory matrices. They are low-risk and will prevent the rest
of the work from drifting.

Concrete command target:

```bash
cargo xtask libfaust-api-matrix \
  --cpp-root /Users/letz/Developpements/RUST/faust \
  --out porting/generated
```

The first implementation can be a simple parser over `LIBFAUST_API` lines plus
`nm`/`cargo metadata` inspection. It does not need to understand C++ overloads
perfectly; it only needs to make missing C ABI symbols visible.
