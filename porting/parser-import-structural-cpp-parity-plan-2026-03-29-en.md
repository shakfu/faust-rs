# Plan — Structural C++ Parity for Parser Import Handling

Date: 2026-03-29

## 1. Goal

Restore true C++ compiler parity for Faust import handling by matching the C++
semantic boundary:

- parse source into a definition tree that still contains explicit import nodes,
- run import expansion structurally on that tree,
- feed `formatDefinitions(...)`-equivalent normalization with preserved import
  boundaries,
- reuse the same expansion model for file-backed evaluation paths such as
  `component(...)` and `library(...)`.

This plan explicitly replaces text-level import workarounds. The target is not
"support more import syntaxes in the pre-reader". The target is to port the
same architectural contract used by the C++ compiler.

## 2. Triggering Mismatch

Representative failure:

```faust
GEN = environment { import("karplus.dsp"); }.process;
FX = environment { import("freeverb.dsp"); }.process;

process = GEN<:(FX,FX);
```

Observed Rust behavior:

- `faust-rs` reports `recursive evaluation loop on node ...`

Observed C++ behavior:

- `faust -lang cpp` succeeds

Immediate diagnosis:

- Rust source loading only expands `import("...");` when it occupies a full
  source line in `crates/parser/src/source_reader.rs`,
- the parser records `import(...)` and returns `nil`,
- therefore inline imports inside `environment { ... }` disappear before eval.

Deeper diagnosis:

- this is not only an inline-import bug,
- it exposes a parity gap in the Rust parser/eval architecture,
- Rust currently makes import expansion a raw-source preprocessing concern,
- C++ makes import expansion a parsed-tree concern.

## 3. C++ Reference Model

Primary C++ files:

- `/Users/letz/Developpements/RUST/faust/compiler/parser/faustparser.y`
- `/Users/letz/Developpements/RUST/faust/compiler/parser/sourcereader.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/parser/sourcereader.hh`
- `/Users/letz/Developpements/RUST/faust/compiler/evaluate/eval.cpp`

Reference behavior:

1. The grammar accepts `import("...")` as a normal statement in `stmtlist`.
2. `environment { stmtlist }` becomes `boxWithLocalDef(boxEnvironment(), formatDefinitions(stmtlist))`.
3. Import statements survive parsing as import-file nodes.
4. `SourceReader::expandList(...)` / `expandRec(...)` later expands those
   nodes structurally from the parsed definition list.
5. `component(...)` and `library(...)` also load a file, then call
   `gReader.expandList(gReader.getList(fname))`.

The critical invariant is:

- C++ does not depend on source-line layout for import semantics.

Inline, multiline, nested, and reordered imports all go through the same parsed
representation.

## 4. Current Rust Divergence

Current Rust path:

1. `SourceReader` expands imports from raw text before parse.
2. `parse_file_with_imports(...)` consumes that flattened text blob.
3. Parser `import_statement(...)` records metadata and returns `nil`.
4. No explicit import node survives normalization.
5. Eval/file-loading paths operate on already-flattened definition trees.

Files involved:

- `crates/parser/src/source_reader.rs`
- `crates/parser/src/lib.rs`
- `crates/eval/src/lib.rs`
- any parser tests assuming fully flattened pre-parse source

This creates multiple parity failures:

- inline `environment { import(...) }` depends on source formatting,
- `format_definitions(...)` sees a different class of inputs than C++
  `formatDefinitions(...)`,
- imported-file boundaries are erased too early,
- eval-loaded sources (`component`, `library`) do not follow the same boundary
  contract as the C++ compiler,
- diagnostics provenance is coupled to rewritten source text rather than to
  structural expansion.

## 5. Required End State

Rust must satisfy all of the following:

1. Import statements are explicit parser/box nodes after parse.
2. `format_definitions(...)` preserves import-file nodes instead of erasing
   them.
3. Rust has a structural import expansion pass equivalent to C++
   `SourceReader::expandList(...)`.
4. File-backed parse and eval use that same expansion pass.
5. Raw-source readers handle resolution and origin tracking, not import
   semantics.
6. Differential tests confirm parity on import placement, import ordering,
   duplicate transitive imports, and local-environment imports.

## 6. Non-Goals

- Broad parser cleanup unrelated to import parity.
- Extending unsupported network/sourcefetcher behavior unless C++ reference
  behavior is required by the chosen test corpus.
- Cosmetic AST rewrites that do not reduce parity risk.

## 7. Design Decision

The recommended parity direction is:

- preserve explicit import nodes in parser output.

This is the C++-shaped route. A higher-level Rust-only container is possible,
but it would still be an adaptation and would preserve the current risk that
parser consumers continue to reason over an already-lowered representation.

For this area, direct parser/box parity is preferable.

Mapping status target:

- parser import node representation: `1:1`
- import expansion algorithm: `1:1`
- parser public APIs: `adapted` where needed for staged migration, but the
  internal semantic boundary should remain C++-shaped

## 8. Implementation Plan

### Step 1: Inventory the Current Rust Import Lifecycle

Scope:

- document every place where Rust currently expands, records, removes, or
  reinterprets import statements.

Files:

- `crates/parser/src/source_reader.rs`
- `crates/parser/src/lib.rs`
- `crates/eval/src/lib.rs`

Deliverables:

- one concrete call graph for file-backed parse,
- one concrete call graph for `component(...)` / `library(...)`,
- one delta table against C++ `getList(...)` + `expandList(...)`.

Pass criteria:

- no unresolved ambiguity remains about where imports are consumed today.

### Step 2: Preserve Import Statements in Rust Parser Output

Scope:

- stop lowering parser `import_statement(...)` to `nil`,
- introduce or expose an explicit box/definition form for import-file nodes.

Likely files:

- `crates/parser/src/lib.rs`
- `crates/boxes/src/builder.rs`
- `crates/boxes/src/internals.rs`
- `crates/boxes/src/matcher.rs`
- any parser semantic tests expecting `nil`

Expected behavior:

- `environment { import("child.dsp"); }.process` must retain an import node
  inside the local definition list after parse.

Deliverables:

- parser/box representation for import-file nodes,
- matcher support for detection similar in spirit to C++ `isImportFile(...)`,
- provenance comments referencing the C++ source.

Pass criteria:

- dump-box for inline and multiline import variants differs only in formatting,
  not semantics,
- parser tests confirm import nodes are preserved both top-level and local.

### Step 3: Make `format_definitions(...)` Preserve Import Nodes

Scope:

- align Rust definition normalization with C++ `formatDefinitions(...)`.

Likely files:

- `crates/parser/src/lib.rs`
- parser normalization tests

Requirements:

- import nodes pass through normalization untouched,
- duplicate-definition grouping ignores import nodes except for ordered
  preservation,
- normalization order matches C++ expectations for later expansion.

Deliverables:

- explicit test coverage for mixed local definitions and import statements,
- updated mapping comments documenting `1:1` status.

Pass criteria:

- normalized local-environment deflists preserve import nodes,
- no import is silently erased before structural expansion.

### Step 4: Introduce Structural Import Expansion Pass

Scope:

- port C++ `SourceReader::expandList(...)` / `expandRec(...)` semantics to Rust.

Likely files:

- `crates/parser/src/lib.rs`
- `crates/parser/src/source_reader.rs`
- possibly a new helper module if separation improves clarity

Requirements:

- walk parsed definition lists,
- detect import-file nodes,
- resolve/import target files through the source reader,
- recursively expand imported definition lists,
- maintain duplicate-visit suppression matching C++ visited-set semantics,
- preserve deterministic order.

Deliverables:

- Rust helper equivalent to `expandList`,
- direct test coverage for:
  - top-level import,
  - transitive import,
  - duplicate re-import,
  - local `environment { import(...) }`,
  - inline and multiline source layout equivalence.

Pass criteria:

- expanding a parsed tree produces the same semantic tree class as C++ on the
  representative corpus,
- no source-line-layout dependency remains.

### Step 5: Narrow `SourceReader` Back to Resolution Duties

Scope:

- remove semantic dependence on raw-text import flattening for file-backed
  parsing.

Likely files:

- `crates/parser/src/source_reader.rs`
- parser API entry points currently named `parse_file_with_imports*`

Requirements:

- `SourceReader` still resolves files, search paths, cycles, and used-file
  bookkeeping,
- import semantics move out of the line-by-line source rewriting path,
- transitional APIs may exist temporarily, but the production parity path must
  no longer depend on pre-expanded text.

Deliverables:

- production parse path that parses source files without semantic loss of import
  statements,
- documented API lifecycle if legacy flattened APIs are temporarily retained.

Pass criteria:

- source layout no longer affects semantic import behavior,
- import handling is driven by parsed structure only.

### Step 6: Realign Eval Loaded-Source Semantics

Scope:

- make Rust `component(...)` / `library(...)` loading follow the same boundary
  as C++.

Likely files:

- `crates/eval/src/lib.rs`

Requirements:

- file-backed loads should use parsed definitions plus structural import
  expansion,
- `component` returns a closure over `process` from the expanded list,
- `library` returns a closure over `environment` from the expanded list,
- no special-case flattening path distinct from parser parity flow.

Deliverables:

- shared helper between parser file-backed flows and eval-loaded source flows,
- regression tests covering the WAC `chain.dsp` pattern and reduced fixtures.

Pass criteria:

- `chain.dsp` class failures disappear without source-layout workarounds,
- `component(...)` / `library(...)` use the same import-boundary contract as C++.

### Step 7: Rework Provenance and Diagnostics Boundaries

Scope:

- ensure diagnostics remain stable once imports are expanded structurally rather
  than by rewriting source text.

Likely files:

- `crates/parser/src/lib.rs`
- `crates/parser/src/context.rs`
- `crates/parser/src/source_reader.rs`
- compiler diagnostic integration where file/line origins are consumed

Requirements:

- imported-file origins remain attached to the right parsed nodes/definitions,
- error spans stay deterministic across expanded imports,
- used-file ordering remains portable.

Deliverables:

- provenance model note for the adapted API boundary if needed,
- regression tests for unresolved import, cycle, and imported-file parse errors.

Pass criteria:

- no diagnostics quality regression versus current parser output,
- no fallback to absolute-path-only or flattened-line-origin assumptions.

### Step 8: Differential Validation Against C++

Scope:

- prove the new path matches the C++ compiler on the import semantics that
  triggered this work.

Corpus minimum:

- reduced inline `environment { import(...) }`,
- multiline equivalent,
- `chain.dsp`,
- import-heavy fixtures already covered by the earlier `formatDefinitions`
  parity gap,
- duplicate transitive import cases,
- `component(...)` / `library(...)` loaders.

Pass criteria:

- Rust/C++ accept and reject the same targeted fixtures,
- dump-box shape is structurally aligned where comparable,
- no untriaged mismatches remain in this import-parity slice.

## 9. Migration Strategy

Recommended sequence:

1. preserve import nodes in parser output,
2. update normalization to keep them,
3. add structural expansion pass,
4. switch eval-loaded source paths to the new pass,
5. remove production dependence on raw-text expansion,
6. delete any temporary compatibility shims.

Do not start by teaching the raw-text reader more import layouts. That would
re-entrench the wrong boundary.

## 10. Risks To Re-check

- parser public API churn for callers that assume fully expanded source,
- diagnostics provenance once source text is no longer pre-expanded,
- accidental duplication of visited-set behavior between parser and eval,
- import ordering drift versus C++ if normalization/expansion order is changed,
- hidden consumers of `ParseState::imports()` that currently assume imports are
  metadata-only rather than semantic nodes.

Each risk needs at least one focused regression test in the same implementation
series.

## 11. Validation Commands

Mandatory local gates for the implementation series:

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo test -p parser --no-fail-fast`
- `cargo test -p compiler --no-fail-fast`
- targeted Rust/C++ differential runs for the import corpus

Recommended targeted commands during development:

- `cargo run -p compiler -- --dump-box <case.dsp>`
- `faust -lang cpp <case.dsp> -o /tmp/ref.cpp`

## 12. Definition of Done

This parity slice is complete only when all of the following hold:

- inline and multiline imports are semantically identical in Rust,
- import statements survive parse as structural nodes,
- Rust has a tree-level import expansion pass equivalent to C++,
- `format_definitions(...)` consumes the same class of inputs as C++
  `formatDefinitions(...)`,
- `component(...)` / `library(...)` use the same import-boundary semantics as
  the C++ compiler,
- `chain.dsp`-class failures are closed by structural parity, not by source
  preprocessing,
- all temporary text-level compatibility workarounds are absent.

## 13. Relationship To Existing Porting Docs

This plan refines and supersedes the import-boundary parts of:

- `porting/parser-import-format-definitions-cpp-parity-plan-2026-03-19-en.md`

It should also be used to tighten Step 5 of:

- `porting/phases/phase-3-parser-full-parity-plan-en.md`

The principle is unchanged, but the implementation decision is now explicit:

- real C++ parity here requires structural import nodes and structural expansion,
- not a more permissive text pre-expander.
