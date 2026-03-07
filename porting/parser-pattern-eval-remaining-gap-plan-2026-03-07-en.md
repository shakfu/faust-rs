# Remaining Parser / Pattern / Eval Gap Plan

> **Date**: 2026-03-07
> **Scope**: `crates/parser`, `crates/boxes`, parser/eval integration points
> **Reference C++ baseline**: `master-dev-ocpp-od-fir-2-FIR19` (`8eebea429`)
> **Status**: current correction plan for the two post-closure remaining gaps

This document follows:

- `porting/parser-pattern-eval-cpp-parity-gap-analysis-2026-03-06-en.md`
- `porting/parser-pattern-eval-remaining-gaps-2026-03-07-en.md`

It intentionally excludes the closure-model work already completed in
`crates/eval`.

## 1. Scope

The remaining parser/pattern/eval parity work is now limited to two items:

1. `prepare_pattern()` opacity parity with C++ `preparePattern()`
2. metadata / `declare` end-to-end parity through C++-equivalent box semantics

These should be treated as two adjacent but separate workstreams.

## 2. Gap A: `prepare_pattern()` Opacity Parity

### C++ target

Reference function:

- `/Users/letz/Developpements/RUST/faust/compiler/boxes/boxes.cpp`
  - `preparePattern(Tree box)`

Rust must match the same recursive/opaque shape boundaries rather than using a
generic "walk everything" pattern-preparation rule.

### Deliverables

1. Audit current Rust `prepare_pattern(...)` branch-by-branch against C++.
2. Narrow recursion so the same families stay opaque.
3. Add parser-level differential fixtures that would fail if:
   - `case`
   - `access`
   - `component`
   - `environment`
   - `slot`
   - `symbolic`
   are recursed into incorrectly.

### Suggested implementation order

1. Produce a small Rust/C++ branch mapping table in code comments or Rustdoc
   near `prepare_pattern(...)`.
2. Replace the generic recursive fallback with explicit shape handling where
   needed.
3. Add structural tests in `crates/parser/tests/structural_cpp_differential.rs`
   or a dedicated parser differential file.

### Exit criteria

- the Rust recursion/opacity boundary is explainable directly from C++
  `preparePattern()`,
- targeted differentials stay green against the C++ reference,
- no existing parser parity tests regress.

## 3. Gap B: Metadata / `declare` End-to-End Parity

### C++ target

Relevant references:

- `/Users/letz/Developpements/RUST/faust/compiler/parser/sourcereader.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/boxes/boxes.cpp`

Rust must move from parser-side metadata bookkeeping only to the same
language-pipeline semantics as the C++ compiler.

### Deliverables

1. Determine the minimal metadata semantics that must be represented in
   `crates/boxes`.
2. Add the missing box family or equivalent representation for metadata.
3. Reinject parser-recorded `declare` data when building the relevant box
   structures.
4. Add end-to-end tests proving the metadata survives the intended parse/box
   boundaries.

### Suggested implementation order

1. Inspect the exact C++ metadata reinjection points in `sourcereader.cpp`.
2. Decide the Rust representation in `boxes`:
   - preferably a direct `BoxMatch` family mirroring the C++ semantic role,
   - document mapping status as `1:1` or `adapted` with rationale.
3. Thread parser context metadata into box construction.
4. Add tests at two levels:
   - parser/boxes structural tests,
   - compiler-facing tests if metadata is expected to be externally visible.

### Exit criteria

- parser `declare` data is no longer parser-context-only,
- the selected metadata semantics are represented in `boxes`,
- at least one regression proves reinjection survives into the intended
  downstream representation.

## 4. Recommended Execution Order

The pragmatic order is:

1. finish `prepare_pattern()` parity first,
2. then implement metadata reinjection.

Reason:

- `prepare_pattern()` is narrower and lower-risk,
- metadata reinjection touches representation and likely crosses parser/boxes
  boundaries more broadly.

## 5. Test Strategy

Minimum additions expected in the same implementation series:

- parser differential cases for opacity boundaries in `prepare_pattern()`,
- one or more metadata structural tests at parser/boxes level,
- if metadata is surfaced publicly, one compiler-facing regression as well.

Existing closure/eval regressions should remain green throughout:

- `crates/eval/tests/core_eval.rs`
- `crates/compiler/tests/signal_pipeline.rs`
- `crates/compiler/tests/cpp_signal_differential.rs`

## 6. Done Definition

This area can be considered fully closed only when:

- `prepare_pattern()` is aligned with the C++ opacity boundary,
- metadata / `declare` semantics are carried through the Rust pipeline in a
  C++-equivalent way,
- the remaining-gap snapshot in
  `porting/parser-pattern-eval-remaining-gaps-2026-03-07-en.md`
  can be retired or reduced to “no known gaps in this area”.
