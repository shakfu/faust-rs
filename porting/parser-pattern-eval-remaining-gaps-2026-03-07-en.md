# Parser / Pattern / Eval Remaining Gaps (Post-Closure Port)

> **Date**: 2026-03-07
> **Scope**: `crates/parser`, `crates/boxes`, parser/eval integration points
> **Reference C++ baseline**: `master-dev-ocpp-od-fir-2-FIR19` (`8eebea429`)
> **Status**: no known parser/pattern/eval parity gaps after the final metadata step

This document intentionally excludes gaps that were already closed by the
parser/eval/closure work completed on 2026-03-06. It captures only the
remaining items that still prevent the parser/pattern/eval area from being
called fully finished relative to the C++ reference.

The correction plan for the former two items was tracked separately in
`porting/parser-pattern-eval-remaining-gap-plan-2026-03-07-en.md`.

## 1. Remaining Gap Summary

No known parser/pattern/eval parity gap remains in this area.

Everything else from the original 2026-03-06 gap analysis is now implemented:

- grouped and patterned definitions,
- evaluated `case` patterns,
- barrier semantics for repeated pattern variables,
- captured closure environment model,
- `a2sb` lowering through evaluator values,
- `slot` / `symbolic` support,
- `boxModifLocalDef`.

## 2. Closed On 2026-03-07: `prepare_pattern()` Opacity Parity

Status: implemented.

### Current Rust behavior

Rust `prepare_pattern()` in
`crates/parser/src/lib.rs` recursively descends through generic tree structure
and rewrites children broadly.

Relevant implementation:

- `crates/parser/src/lib.rs`
  - `prepare_pattern(...)`

### C++ reference behavior

C++ `preparePattern()` in
`/Users/letz/Developpements/RUST/faust/compiler/boxes/boxes.cpp`
preserves a narrower set of forms as opaque and only descends through the box
families explicitly supported by the original function.

Notably, the C++ function treats forms such as:

- `abstr`
- `access`
- `component`
- `environment`
- `slot`
- `symbolic`
- `case`

as explicit shape boundaries.

### Why it mattered

The current corpus does not strongly constrain this area. That means the Rust
parser can still diverge on complex or future pattern-heavy sources without the
existing differential harness noticing.

This was not known to break the current production corpus, but it remained a
semantic parity risk because pattern preparation sits on the parser-side
language surface.

Closure completed on 2026-03-07:

- Rust `prepare_pattern()` now follows the same opacity boundary strategy as
  C++ `preparePattern()`
- parser structural guardrails now cover opaque `access`, `component`, and
  `environment` forms

## 3. Closed On 2026-03-07: Metadata / `declare` End-to-End Parity

### Current Rust behavior

Definition-scoped metadata is now reified through the parser/boxes/eval path:

- `crates/boxes/src/lib.rs`
  - `BoxMatch::Metadata`
  - `BoxBuilder::metadata(...)`
- `crates/parser/src/lib.rs`
  - `format_definitions(...)` reinjects `declare <def> <key> <value>;`
    through nested `BOXMETADATA` wrappers
- `crates/eval/src/lib.rs`
  - `BOXMETADATA` is evaluation-transparent, matching the C++ result semantics

Top-level `declare key value;` entries remain recorded in parser context:

- `crates/parser/src/context.rs`
  - `declared_metadata`

### C++ reference behavior

The C++ parser/evaluator stack carries metadata through dedicated box-level
semantics, notably `boxMetadata`, so the information is preserved as part of
the language pipeline rather than staying parser-side bookkeeping only.

Relevant reference code:

- `/Users/letz/Developpements/RUST/faust/compiler/parser/sourcereader.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/boxes/boxes.cpp`

### Why it mattered

`declare` is part of the Faust source language. Until definition metadata
survived parse-to-box transport, Rust remained observably different from the
C++ compiler in the parser/eval surface.

### Closure completed on 2026-03-07

- `BOXMETADATA` now exists in `crates/boxes`
- parser formatting reinjects definition metadata like C++
  `addFunctionMetadata`
- parser and eval regressions prove structural survival and evaluation
  transparency

## 4. Current Conclusion

The parser/pattern/eval scope described by the 2026-03-06 gap analysis is now
closed.
