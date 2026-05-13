# Julia Backend Plan

Date: 2026-05-13

## Goal

Add a first Rust `julia` backend wired to Faust-style CLI selection:

```sh
faust-rs -lang julia foo.dsp -o foo.julia
```

The reference contract is the C++ Faust backend:

```sh
faust -lang julia foo.dsp -o foo.julia
```

The first target is structural and runtime-shape parity for the existing
module-first FIR fast lane, not byte-for-byte parity with the C++ generator.

## Reference Observations

For a pass-through DSP, the C++ Faust Julia backend emits:

- a top comment containing Faust version and compile options,
- `using StaticArrays`,
- `const REAL = Float32`,
- helper aliases (`pow`, `rint`, `remainder`),
- `mutable struct mydsp{T} <: dsp` with typed state fields,
- lifecycle functions:
  - `metadata!`,
  - `getSampleRate`,
  - `getNumInputs`,
  - `getNumOutputs`,
  - `classInit!`,
  - `instanceResetUserInterface!`,
  - `instanceClear!`,
  - `instanceConstants!`,
  - `instanceInit!`,
  - `init!`,
  - `getJSON`,
  - `buildUserInterface!`,
  - `compute!`,
- `compute!(..., inputs::Matrix{FAUSTFLOAT}, outputs::Matrix{FAUSTFLOAT})`
  with one-based Julia array indexing and zero-based Faust loop variables.

## Implementation Scope

### Phase J1: module-first emitter

Implement `crates/codegen/src/backends/julia/mod.rs` as a real emitter from FIR
`Module` roots.

Required J1 coverage:

- scalar and array state fields from FIR `dsp_struct` and `globals`,
- static table declarations,
- lifecycle bodies from canonical FIR functions when present,
- synthesized lifecycle fallbacks when a function is absent,
- metadata and UI calls,
- `compute!` with `Matrix{FAUSTFLOAT}` inputs/outputs,
- FIR expressions already supported by the C/C++ fast-lane slice:
  - constants,
  - loads/stores,
  - table loads/stores,
  - casts,
  - arithmetic/comparison/binop nodes,
  - `select2`,
  - math calls,
  - blocks,
  - if/control,
  - loops, including reverse loops used by BRA/RAD.

Unsupported FIR nodes must fail with a typed Julia backend diagnostic rather
than emitting partial Julia.

### Phase J2: compiler facade and CLI

Wire the backend through:

- `codegen::backends::julia`,
- `compiler::Compiler` file/source helpers,
- `faust-rs -lang julia`,
- legacy `-lang` normalization.

Default output remains stdout unless `-o` is provided.

### Phase J3: parity smoke tests

Add tests that:

- compile a small source with `-lang julia`,
- assert the expected Julia shell (`mutable struct mydsp`, `compute!`,
  `getNumInputs`, `getNumOutputs`),
- compare selected structural markers against `faust -lang julia` when the C++
  compiler is available,
- cover at least one recursive or delayed FIR case once J1 supports the needed
  nodes.

## Non-goals for the first pass

- byte-for-byte output matching,
- full Julia package/runtime scaffolding,
- generated Julia execution tests,
- all Faust auxiliary files,
- exact C++ backend JSON content and include path ordering.

These can be added after the emitter is present and structurally aligned.

## Acceptance Criteria

- `faust-rs -lang julia foo.dsp -o foo.julia` produces Julia source.
- Existing C/C++/WASM/interp CLI paths are unchanged.
- Unsupported FIR shapes report a stable `FRS-CGEN-JULIA-*` error.
- `cargo fmt --all` passes.
- Focused Julia backend/CLI tests pass.
