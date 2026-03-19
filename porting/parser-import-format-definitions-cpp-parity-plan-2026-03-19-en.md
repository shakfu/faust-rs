# Plan — Exacter C++ Parser/Import Parity for `formatDefinitions`, `library(...)`, and `component(...)`

Date: 2026-03-19

## Goal

Replace the current Rust import-handling divergence with a parser/eval flow that
matches the C++ compiler as closely as practical for imported Faust sources,
especially around:

- `formatDefinitions(...)`
- `import("...")`
- `library("...")`
- `component("...")`

The immediate parity target is to eliminate the entire class of failures caused
by Rust feeding `format_definitions(...)` a fully flattened imported definition
list that C++ never constructs at that stage.

## Triggering Failure

Representative case:

- `faust-rs -pn operator_test tests/dx7_tests.dsp`

Observed Rust error:

- undefined symbol `ba` while evaluating `dx7/operator.lib`

Diagnosed root cause:

- Rust `parse_file_with_imports(...)` eagerly expands imported source text
  before parse normalization,
- C++ preserves import file boundaries until `formatDefinitions(...)`,
- therefore Rust can construct one flat group containing repeated imported
  helper aliases (`ba`, `ma`, `si`, ...), while C++ never normalizes that exact
  shape.

## C++ Reference

Primary files:

- `/Users/letz/Developpements/RUST/faust/compiler/parser/sourcereader.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/parser/sourcereader.hh`
- `/Users/letz/Developpements/RUST/faust/compiler/parser/faustparser.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/evaluate/eval.cpp`

Relevant C++ functions and concepts:

- `formatDefinitions(Tree rldef)`
- `makeDefinition(Tree symbol, list<Tree>& variants)`
- `isImportFile(def, file)`
- source reader / import list handling
- `component(...)` / `library(...)` evaluation through the reader

Key facts:

- C++ `formatDefinitions(...)` preserves import-file nodes and does **not**
  flatten imported source text into one single parser definition list at this
  stage.
- C++ `makeDefinition(...)` is therefore allowed to keep the strict rule
  “multiple zero-argument definitions are an error” without misclassifying
  repeated imported aliases as redefinitions.

## Problem Statement

Current Rust behavior is structurally different from C++ in a parity-sensitive
way:

1. `parser::parse_file_with_imports(...)` expands imported text eagerly.
2. The parser then normalizes one already-flattened definition list.
3. `format_definitions(...)` sees repeated zero-arg aliases that C++ would not
   see in the same representation.
4. Downstream `library(...)` / `component(...)` evaluation inherits this
   structural mismatch.

This leads to two bad outcomes:

- either Rust rejects/loses repeated helper aliases that should remain visible,
- or Rust needs an adaptation in `format_definitions(...)` that is not a
  structural C++ port.

## Desired End State

Rust should follow the C++ structure more closely:

- parser normalization for file-backed semantics should not consume a fully
  flattened import-expanded text blob,
- imported files should remain explicit semantic boundaries until the same
  point where C++ merges or evaluates them,
- `format_definitions(...)` should operate on the same class of inputs as C++
  `formatDefinitions(...)`,
- `library(...)` / `component(...)` should evaluate definitions loaded through
  that same boundary-preserving path.

## Non-Goal

This plan is **not** about broad parser cleanup or speculative redesign of the
whole parser crate.

It is specifically about restoring C++-shaped boundaries between:

- import reading,
- definition normalization,
- evaluation-time source loading.

## Current State

The temporary duplicate-zero-arg adaptation is explicitly **not** the target
fix and should not be reintroduced as the final solution.

The path forward should act directly on the import/definition boundary instead
of weakening `format_definitions(...)`.

## Target Architecture

### A1. Separate “file loading” from “definition flattening”

For parity-sensitive file-backed parsing used by `eval_loaded_source_value(...)`,
Rust should stop treating imported files as already inlined source text for the
normalization boundary that feeds `format_definitions(...)`.

Instead, imported files should stay explicit nodes or explicit load units until
the equivalent C++ point.

### A2. Keep import boundaries visible to normalization

Rust should gain a representation equivalent in spirit to C++ `isImportFile`:

- either preserve explicit import nodes in the parser output,
- or preserve a higher-level structured load list that still distinguishes
  imported definitions from local definitions before grouping.

### A3. Make `library(...)` / `component(...)` consume the same structure

`eval_loaded_source_value(...)` should load parsed definitions in a way that
replays the C++ import/file boundary semantics, rather than consuming a purely
flattened parser result.

### A4. Preserve existing diagnostics quality

Any parity refactor must preserve:

- deterministic `used_files` ordering,
- imported-file source spans/origins for parser diagnostics,
- metadata continuity across loaded files,
- current search-path semantics unless an explicit C++ mismatch is found.

## Recommended Design Direction

The most C++-faithful direction is:

1. keep `SourceReader` as a file resolver and import enumerator,
2. stop using it to build one parser input string for evaluation semantics,
3. introduce a new loaded-file structure that preserves:
   - one file’s own raw definitions,
   - explicit imported-file references in order,
   - associated metadata/origin information,
4. normalize definitions after that structure is assembled in a way that still
   recognizes import-file boundaries like C++ `formatDefinitions(...)`.

This means the likely end state is **not** “remove `SourceReader`”, but
“narrow `SourceReader` back to resolution/origin duties and stop letting its
text expansion define semantic normalization boundaries”.

## Design Options

### Option 1 — Preserve explicit import nodes in parser output

Pros:

- closest in spirit to C++ `isImportFile`,
- simplest conceptual mapping to `formatDefinitions(...)`.

Cons:

- touches grammar/AST shape,
- may require broader parser fixture updates.

### Option 2 — Add a higher-level loaded-file semantic container

Pros:

- less invasive for the core grammar,
- keeps parser AST mostly stable,
- may let the current flat parse API continue to exist for non-parity callers.

Cons:

- introduces an adapted Rust representation instead of a literal AST match,
- requires careful documentation of the mapping status.

### Recommendation

Prefer **Option 2** first if it can preserve the same semantic boundary as C++
without destabilizing the parser grammar. If that proves too leaky, move to
Option 1.

The success criterion is semantic parity at the `formatDefinitions(...)` /
`library(...)` boundary, not AST purity for its own sake.

## Deliverables

### D1. Differential inventory of current Rust vs C++ import boundaries

Document precisely:

- where Rust expands imported text,
- where C++ preserves import nodes,
- which public parser APIs currently expose the flattened form,
- which consumers depend on it,
- which consumers actually need C++-faithful import boundaries.

Primary files:

- `crates/parser/src/source_reader.rs`
- `crates/parser/src/lib.rs`
- `crates/eval/src/lib.rs`

### D2. Introduce a C++-shaped import-preserving parse path

Create a parser/file-loading path for evaluation that preserves import
boundaries closely enough for `format_definitions(...)` parity.

This may be:

- a new parser entry point,
- or a refactor of `parse_file_with_imports(...)`,
- or a split between “expanded for diagnostics” and “preserved for semantics”.

The exact API may remain adapted, but the semantic boundary must match C++ and
its mapping status must be documented.

### D3. Realign `format_definitions(...)`

Ensure Rust `format_definitions(...)` is fed with the same class of inputs as
the C++ `formatDefinitions(...)`, so imported-library alias duplication is
eliminated at the structural source instead of patched in the rule.

### D4. Re-validate `library(...)` and `component(...)`

Re-check that:

- file-rooted source contexts still resolve nested relative imports correctly,
- top-level metadata collection still matches C++ expectations,
- closure/environment capture at the loaded-source boundary remains correct.

### D5. Remove the temporary duplicate-zero-arg adaptation if present

Once D2/D3 are in place:

- remove the Rust-specific allowance for identical repeated zero-arg defs,
- restore the stricter C++ rule for true multiply-defined constants in the
  remaining cases.

### D6. Add differential coverage on imported library aliases

Add tests covering:

- imported libraries that each define the same helper alias (`ba`, `ma`, etc.),
- `library("operator.lib")`-style nested loads,
- at least one real external-corpus reproducer (`dx7_tests.dsp` or a minimized
  fixture),
- one test proving genuine conflicting zero-arg redefinitions still fail.

### D7. Document API mapping status

For every touched API/path:

- mark `1:1`, `adapted`, or `deferred`,
- record compatibility impact,
- record whether the flattened import API remains available for non-parity
  callers.

## Migration Plan

### Phase A — Audit current parser/eval import boundaries

1. Record how `parse_file_with_imports(...)` currently expands source text.
2. Record where C++ keeps `isImportFile(...)` nodes.
3. Identify which existing Rust callers require the flattened form and which do
   not.
4. Record current metadata/origin behavior that must be preserved.

Exit condition:

- one concrete Rust/C++ boundary map exists,
- the affected call sites are enumerated,
- invariants to preserve are listed.

### Phase B — Add a parity-oriented import-preserving path

1. Implement a parser/file-loading path that preserves import boundaries for
   evaluation-sensitive consumers.
2. Keep diagnostics and file-origin tracking intact.
3. Route `eval_loaded_source_value(...)` to that path.
4. Keep the existing flat parse path only where explicitly needed.

Exit condition:

- loaded libraries/components no longer depend on fully flattened imported
  definition lists,
- the new boundary is exercised by targeted tests.

### Phase C — Realign normalization and evaluation on the new boundary

1. Update `format_definitions(...)` callers to consume the new structure.
2. Re-check `library(...)` / `component(...)` environment construction.
3. Confirm that the duplicate-alias class is gone structurally.

Exit condition:

- the DX7 reproducer passes for structural reasons,
- no new adaptation is needed in `format_definitions(...)`.

### Phase D — Remove temporary adaptations and lock regressions

1. Delete any temporary parser-side acceptance of identical repeated zero-arg
   definitions if present.
2. Add/refresh the imported-alias and `operator_test` regressions.
3. Confirm that genuine conflicting zero-arg redefinitions still fail.

Exit condition:

- workaround removed,
- good and bad cases are both covered.

### Phase E — Documentation and parity notes

1. Add Rustdoc comments pointing to the relevant C++ parser/eval files.
2. Update the parser porting notes and journal.
3. Document any remaining adapted API surface explicitly as `adapted`, not
   `1:1`.

Exit condition:

- provenance and mapping status are explicit,
- no silent parser/import divergence remains undocumented.

## Risks and Watchpoints

### R1. Metadata continuity

The current parser path shares a metadata store across imported files. The new
path must preserve that behavior or document any intentional divergence.

### R2. Origin fidelity

Current diagnostics can point into imported files because the expanded text path
tracks line origins. A more C++-faithful semantic boundary must not degrade
error spans.

### R3. Public API churn

`parse_file_with_imports(...)` may be used by tests or helper tools expecting
flattened imported source. If we split APIs, the compatibility contract must be
documented.

### R4. Duplicate work between parser and eval

Do not solve this by adding a second ad hoc normalization layer in `eval`.
Parity should come from restoring the correct parser/import boundary, not by
re-implementing parser grouping logic downstream.

### R5. Search-path semantics

The current Rust `SourceReader` search order was already tuned to C++ import
behavior. This refactor must preserve that unless a proven mismatch is found.

## Validation Requirements

Mandatory before declaring this plan complete:

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`

Targeted parity checks:

- `faust-rs -pn operator_test tests/dx7_tests.dsp`
- parser regression for repeated imported helper aliases
- at least one `library(...)` / `component(...)` regression test using a temp
  import hierarchy
- at least one parser diagnostic-origin regression on an imported file
- one negative regression for genuine conflicting zero-arg defs

## Success Criteria

This work is successful when:

1. `operator_test` passes without any duplicate-zero-arg parser adaptation.
2. Rust `format_definitions(...)` sees imported definitions with boundaries
   matching the C++ pipeline closely enough to eliminate the current alias
   collision class.
3. `library(...)` / `component(...)` source loading has documented provenance,
   mapping status, and regression coverage.
4. Genuine conflicting zero-arg redefinitions still fail as in C++.
5. The temporary adapted parser rule is absent.

## Open Questions

- Should Rust preserve import nodes directly in the parser tree, or expose a
  separate higher-level load structure for evaluation?
- Can one API serve both current parser diagnostics needs and the stricter C++
  semantic boundary, or do we need two entry points?
- Which current tests or crates implicitly rely on flattened imported source
  text and must be migrated?
- Is the minimal acceptable parity point “new eval-facing import-preserving API”
  or “full parser AST import-node parity”?
