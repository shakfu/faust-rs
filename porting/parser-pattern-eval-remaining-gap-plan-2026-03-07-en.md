# Remaining Parser / Pattern / Eval Gap Plan

> **Date**: 2026-03-07
> **Scope**: `crates/parser`, `crates/boxes`, parser/eval integration points
> **Reference C++ baseline**: `master-dev-ocpp-od-fir-2-FIR19` (`8eebea429`)
> **Status**: implementation record; the remaining-gap plan is complete

This document follows:

- `porting/parser-pattern-eval-cpp-parity-gap-analysis-2026-03-06-en.md`
- `porting/parser-pattern-eval-remaining-gaps-2026-03-07-en.md`

It intentionally excludes the closure-model work already completed in
`crates/eval`.

## 1. Scope

The remaining parser/pattern/eval parity work was reduced to one item before
completion:

1. metadata / `declare` end-to-end parity through C++-equivalent box semantics

`prepare_pattern()` opacity parity was completed on 2026-03-07 and is retained
below only as implementation record.

## 2. Completed On 2026-03-07: `prepare_pattern()` Opacity Parity

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

## 3. Completed On 2026-03-07: Metadata / `declare` End-to-End Parity

### C++ target

Relevant references:

- `/Users/letz/Developpements/RUST/faust/compiler/parser/sourcereader.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/boxes/boxes.cpp`

Rust now carries definition-scoped metadata through the same semantic layer as
the C++ compiler for the parser/pattern/eval surface.

### Delivered

1. Added `BoxMatch::Metadata` / `BoxBuilder::metadata(...)`
2. Reinjects `declare <def> <key> <value>;` during `format_definitions(...)`
3. Added parser/eval regressions for metadata structural survival and eval
   transparency

### Implementation summary

The Rust representation is semantically aligned but documented as partially
adapted:

- definition metadata uses an explicit `BOXMETADATA` wrapper like C++
- top-level `declare key value;` stays parser-context metadata for now

### Exit criteria

- definition-scoped `declare` data is no longer parser-context-only,
- the selected metadata semantics are represented in `boxes`,
- regressions prove reinjection survives into the intended downstream
  representation.

## 4. Final Status

This plan is complete. The parser/pattern/eval scope can now be treated as
closed relative to the gaps isolated after the closure-model port.
