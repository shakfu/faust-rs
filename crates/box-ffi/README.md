# box-ffi

Faust-style C/C++ export for box construction, matching, and conversion to
signals/source, backed by Rust `boxes::BoxBuilder` and `boxes::match_box`.

## Scope

- Exposes a broad `Cbox*` constructor family (`libfaust-box-c.h` style):
  primitives, arithmetic and comparison operators, math functions, routing
  (`CboxPar*`, `CboxSeq`, `CboxSplit`, `CboxMerge`, `CboxRec`, `CboxRoute`),
  tables, selectors, UI widgets, soundfiles, waveforms, and foreign
  primitives (`CboxFFun`).
- Exposes the broad matcher family declared by the maintained C header,
  including `CisBoxPrim0..5`, `CisBoxAppl`, `CisBoxAccess`, and
  `CisBoxMetadata`. Some advanced predicates are mapped to the nearest Rust IR
  equivalent when an exact node kind does not yet exist in this port.
- Exposes the pipeline-conversion helpers:
  - `CDSPToBoxes` — compile DSP source text to a box graph,
  - `CboxesToSignals` / `CboxesToSignals2` — propagate a box graph to signals,
  - `CcreateSourceFromBoxes` — generate backend source from a box graph.
- Exposes a thin C++ convenience wrapper (`libfaust-box.h`).
- Uses `tree-ffi` for shared tree handles, context-owned C strings,
  null-terminated handle arrays, and common Box/Signal enum definitions
  (`SType`, `SOperator`).

## Context model

A process-global context (`createLibContext` / `destroyLibContext`), owned by
`tree-ffi`, holds one mutable `TreeArena` and maps opaque C handles
(`Box` / `Signal`) to arena node ids. `box-ffi` and `signal-ffi` share this
single context, so handles created by either crate are interchangeable in the
common helpers.

## Notes

- `Box` handles are opaque and stable within one active context.
- `Ctree2str` and `CprintBox` return heap C strings allocated by Rust;
  release them with `freeCMemory`.
- Matcher out-parameters and error buffers are written only on success;
  invalid pointers produce `null` / `false` / `0` results.
- This is an incremental parity layer; rows marked as exact candidates in the
  generated Box API matrix still need focused semantic parity tests.

## Source provenance

The exported symbol names and signatures mirror the C++ reference headers
`architecture/faust/dsp/libfaust-box-c.h` and `libfaust-box.h`.

## Relationship to other crates

- `boxes` — owns `BoxBuilder` / `match_box`; this crate only adapts them to the
  C ABI.
- `tree-ffi` — shared handle model, global context, and enum definitions.
- `signal-ffi` — sibling C ABI crate sharing the same context.
- `compiler`, `propagate`, `codegen`, `transform` — drive the conversion
  helpers (`CDSPToBoxes`, `CboxesToSignals*`, `CcreateSourceFromBoxes`).
- `faust-ffi` — re-exports these symbols (`box_api`) in the unified `libfaust`
  distribution artifact.
