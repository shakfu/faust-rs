# Parser / Pattern / Eval Remaining Gaps (Post-Closure Port)

> **Date**: 2026-03-07
> **Scope**: `crates/parser`, `crates/boxes`, parser/eval integration points
> **Reference C++ baseline**: `master-dev-ocpp-od-fir-2-FIR19` (`8eebea429`)
> **Status**: narrowed current-state gap list after the closure-model port

This document intentionally excludes gaps that were already closed by the
parser/eval/closure work completed on 2026-03-06. It captures only the
remaining items that still prevent the parser/pattern/eval area from being
called fully finished relative to the C++ reference.

The correction plan for these two items is tracked separately in
`porting/parser-pattern-eval-remaining-gap-plan-2026-03-07-en.md`.

## 1. Remaining Gap Summary

Two real gaps remain in this area:

1. `prepare_pattern()` is still broader than C++ `preparePattern()`.
2. parser-side `declare` / metadata recording is not yet carried through the
   same end-to-end box/eval semantics as C++ `boxMetadata`.

Everything else from the original 2026-03-06 gap analysis is now implemented:

- grouped and patterned definitions,
- evaluated `case` patterns,
- barrier semantics for repeated pattern variables,
- captured closure environment model,
- `a2sb` lowering through evaluator values,
- `slot` / `symbolic` support,
- `boxModifLocalDef`.

## 2. Remaining Gap A: `prepare_pattern()` Opacity Parity

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

### Why it still matters

The current corpus does not strongly constrain this area. That means the Rust
parser can still diverge on complex or future pattern-heavy sources without the
existing differential harness noticing.

This is not known to break the current production corpus, but it remains a
semantic parity risk because pattern preparation sits on the parser-side
language surface.

### Minimum closure condition

This gap is closed only when:

- Rust `prepare_pattern()` follows the same opacity boundaries as C++,
- parser differentials include fixtures that would regress if those boundaries
  drifted again.

## 3. Remaining Gap B: Metadata / `declare` End-to-End Parity

### Current Rust behavior

The parser records metadata declarations in parser context:

- `crates/parser/src/context.rs`
  - `declared_metadata`
  - `declared_definition_metadata`

but that information is not yet reified through the same box/eval pipeline
semantics as the C++ compiler.

### C++ reference behavior

The C++ parser/evaluator stack carries metadata through dedicated box-level
semantics, notably `boxMetadata`, so the information is preserved as part of
the language pipeline rather than staying parser-side bookkeeping only.

Relevant reference code:

- `/Users/letz/Developpements/RUST/faust/compiler/parser/sourcereader.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/boxes/boxes.cpp`

### Why it still matters

`declare` is part of the Faust source language. As long as Rust only records
the declarations without reifying them through equivalent box semantics, the
pipeline is not fully equivalent even if the production signal path often
ignores that information.

This is therefore a parity gap, not merely a documentation omission.

### Minimum closure condition

This gap is closed only when:

- the relevant metadata semantics are represented in `boxes`,
- parse-to-box construction reinjects metadata like the C++ parser does,
- compiler-level tests prove the information survives the intended pipeline
  boundaries.

## 4. Recommended Next Split

The next correction work should remain split in two:

1. `prepare_pattern()` parity hardening,
2. metadata / `declare` semantic reinjection.

They are adjacent in scope, but they are not the same problem:

- the first is a parser-side pattern-shape parity issue,
- the second is a parser-to-box semantic transport issue.

They should therefore not be hidden under the already-completed closure-port
umbrella anymore.
