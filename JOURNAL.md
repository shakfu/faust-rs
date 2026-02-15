# JOURNAL

## 2026-02-14

- Applied the structure defined in `porting/faust-rust-porting-plan-en.md`, section `4. Cargo Workspace Architecture`.
- Created Cargo workspace members for all crates listed in the plan.
- Added scaffold source files for each crate.
- Added `crates/compiler` as both library facade and binary entry point (`faust-rs`).
- Created `cffi/` and `tests/` placeholders.
- Extended CI to include `windows-latest` and split a dedicated `cargo check --workspace --all-targets` job from lint/test jobs.
- Updated `README.md` with a dedicated "How to compile" section (`cargo build --workspace`, release build, package build, and run command).
- Added a GitHub Actions CI badge in `README.md` for visual build status feedback.
- Added `AGENTS.md` at repository root with contribution and coding-agent guidelines (workspace rules, CI gates, porting discipline, and documentation hygiene).
- Enriched `AGENTS.md` with key constraints from `porting/` documents: frozen scope exclusions, Phase 0 validation gate, critical risk checklist, canonical pipeline target, and recursion/RouteIR coexistence guidance.
- Added a `xtask` crate implementing golden workflow commands:
  - `golden-check`
  - `golden-gen-rust`
  - `golden-gen-cpp` (using `FAUST_CPP_BIN`)
- Added initial corpus/golden scaffolding:
  - `tests/corpus/pass_through.dsp`
  - `tests/golden/cpp/pass_through/compiler_stdout.txt`
  - `tests/golden/METADATA.toml` with pinned C++ baseline metadata.
- Added CI golden validation step (`cargo run -p xtask -- golden-check`).
- Updated `README.md` and `AGENTS.md` with golden workflow documentation.
- Fixed cross-platform golden stability by normalizing source newlines before snapshot hashing/counting; added `.gitattributes` and a unit test to prevent LF/CRLF divergence.
- Added dual golden-reference mode in `xtask`:
  - `golden-check` (default Rust reference, used by CI),
  - `golden-check-cpp` (strict C++ parity target),
  - separate storage under `tests/golden/rust/` and `tests/golden/cpp/`.
- Refreshed C++ goldens with local `faust` and corrected invalid corpus case `rep_03_stereo_mix.dsp`.
- Consolidated all backend scaffolds into the `codegen` crate under `crates/codegen/src/backends/<backend>/mod.rs` (one folder per backend).
- Removed standalone workspace members `crates/backend-*` and updated the workspace manifest accordingly.
- Updated `codegen` public surface to expose `codegen::backends::*`.
- Aligned porting documentation with the new backend layout:
  - `porting/faust-rust-porting-plan-en.md`
  - `porting/phases/phase-6-fir-backends-en.md`
  - `porting/phases/phase-7-backends-supp-en.md`
  - `porting/phases/phase-9-integration-en.md`

## 2026-02-15

### Parser migration prototype plan (`faustparser.y`/`faustlexer.l` -> `lrpar`/`lrlex`) — reworked (Global-first, then Tree-first)

Decision: before parser migration, prioritize a `gGlobal` decomposition plan (`global.hh/.cpp`) to define crate boundaries and ownership. Parser and `TreeArena` work follows this map.
Principle: avoid temporary stubs whenever possible; prototype gates should be exercised with real APIs and real data paths.

Source of truth (C++):

- Global state:
  - `/Users/letz/Developpements/RUST/faust/compiler/global.hh`
  - `/Users/letz/Developpements/RUST/faust/compiler/global.cpp`
- Parser/lexer:
  - `/Users/letz/Developpements/RUST/faust/compiler/parser/faustparser.y`
  - `/Users/letz/Developpements/RUST/faust/compiler/parser/faustlexer.l`
- Tree/list/property core used by parser actions:
  - `/Users/letz/Developpements/RUST/faust/compiler/tlib/tree.hh`
  - `/Users/letz/Developpements/RUST/faust/compiler/tlib/tree.cpp`
  - `/Users/letz/Developpements/RUST/faust/compiler/tlib/list.hh`
  - `/Users/letz/Developpements/RUST/faust/compiler/tlib/list.cpp`
  - `/Users/letz/Developpements/RUST/faust/compiler/tlib/node.hh`
  - `/Users/letz/Developpements/RUST/faust/compiler/tlib/property.hh`
  - `/Users/letz/Developpements/RUST/faust/compiler/tlib/symbol.hh`

Execution plan (Phase 0 prototype, revised):

0. Gate 0: `gGlobal` decomposition map (mandatory first step).

- Inventory `gGlobal` fields and classify by responsibility: parser, eval/pattern, propagate, normalize/type, transform, fir/codegen, orchestration/API.
- Trace read/write usage in critical files (`faustparser.y`, `faustlexer.l`, `libcode.cpp`, `instructions_compiler*.cpp`, `dag_instructions_compiler.cpp`).
- Produce a target Rust context matrix:
  - `ParserCtx`, `EvalCtx`, `CompileSession`, `CodegenCtx`, API/session handles.
- Define ownership/lifecycle rules per context (creator, mutability, teardown point).
- Deliverables:
  - field-to-context mapping table,
  - unresolved coupling list,
  - first crate-boundary contract draft.
- Gate 0 pass criterion: every dependency currently using `gGlobal` in touched flows is mapped to an explicit target context (`ParserCtx`, `EvalCtx`, `CompileSession`, `CodegenCtx`, API/session handle) or to an explicit deferred dependency with owner/date.

1. Gate definitions after `gGlobal` mapping.

- Gate A (`TreeArena/tlib-core`) validates data model and performance for parser-required Tree semantics.
- Gate A.5 (`boxes` parser-driven subset) validates that parser semantic actions can target stable `boxes` constructors without temporary stubs.
- Gate B (`lrlex/lrpar`) validates grammar/lexer viability with real Tree-backed semantic actions.

2. Build `tlib-core` prototype (parser-driven subset).

- Implement minimal API needed by parser actions:
  - `TreeId` interned node handle,
  - `nil`,
  - `tree(node[, children...])`,
  - `cons`, `hd`, `tl`, `is_nil`, `is_list`,
  - symbol/string/int/float node constructors,
  - property store keyed by node id.
- Keep scope tight: only primitives required by Gate A.5 and Gate B.

3. Build C++ compatibility matrix for `tlib-core`.

- For each parser-used primitive, document:
  - C++ contract,
  - Rust equivalent signature,
  - ordering/interning/property behavior.
- Explicitly track semantic traps:
  - list order from repeated `cons`,
  - structural identity through hash-consing,
  - parser-local vs cross-pass property scope.

4. Run TreeArena micro-benchmarks vs C++ baseline.

- Benchmarks:
  - high-volume intern create,
  - repeated lookup on existing nodes,
  - deep traversal,
  - property set/get stress.
- Deliverable: benchmark report and Gate A decision (go/conditional/no-go with mitigation).
- Quantitative targets for Gate A:
  - no correctness drift on interning identity tests (exact match),
  - creation/lookup/traversal/property benchmarks <= 2x C++ baseline on identical workloads,
  - memory growth profile documented and bounded for the benchmark corpus.

5. Gate A.5: build a `boxes` minimal layer immediately after `tlib-core`.

- Implement the parser-driven subset first:
  - structural composition (`boxSeq`, `boxPar`, `boxSplit`, `boxMerge`, `boxRec`),
  - core primitives and identifiers (`boxWire`, `boxCut`, numeric boxes, `boxIdent`),
  - local definitions and environments (`boxWithLocalDef`, `boxWithRecDef`, `boxEnvironment`),
  - iterative constructors required by parser corpus (`boxIPar`, and if needed by selected corpus: `boxISeq`, `boxISum`, `boxIProd`),
  - basic UI primitives used in prototype corpus (`boxHSlider` and related basic widgets).
- Ensure signatures are stable and directly consumable by parser actions.
- Add dedicated `boxes` unit tests (independent from parser) and one deterministic structural dump helper for future parser differential checks.
- Deliverable: Gate A.5 decision proving parser can target real `boxes` APIs without parser-local placeholders.

6. Freeze parser context model from Gate 0 outputs.

- Define `ParserCtx` fields and APIs:
  - source location/diagnostics,
  - temporary waveform accumulator (`WAVEFORM`),
  - parse result root,
  - parser-local counters and property hooks (`setDefProp`/`setUseProp` equivalents).
- Exclude non-parser state not required by parser semantics.

7. Create `crates/parser-proto` (isolated from production parser crate).

- Add `lrlex`, `lrpar`, `cfgrammar`, and local deps (`tlib`, `boxes` as needed).
- Add `build.rs` generation.
- Keep `crates/parser` unchanged until Gate B decision.

8. Port lexer (`faustlexer.l` -> `lrlex`) with parity tests.

- Preserve tokenization priority and operator distinctions (`:`, `,`, `<:`, `:>`, `+>`, `~`, `@`, `'`, `->`, `=>`, etc.).
- Recreate lexer states for comment/doc/listing.
- Add tests for numbers, identifiers/keywords, strings/fstrings/doc tokens.

9. Port grammar (`faustparser.y` -> `lrpar`) incrementally.

- Slice 1: program/statement/definition/recovery (`error ENDDEF`).
- Slice 2: expression/infix/argument core with C++ precedence.
- Slice 3: subset of primitives needed by prototype corpus (UI/iter; imports kept as optional post-Gate-B integration check).
- Track conflicts after each slice and keep a conflict log.

10. Port semantic actions with Tree parity.

- Implement actions against `tlib-core` + `ParserCtx`.
- Preserve C++ list/order behavior first; avoid normalization until parity tests are stable.
- Route expression/primitive constructions through Gate A.5 `boxes` APIs (no parser-only construction layer).
- Keep side effects explicit and confined to `ParserCtx`.

11. Differential parser validation against C++.

- Prototype corpus:
  - `process = _;`
  - `process = + ~ _;`
  - `process = hslider("freq", 440, 20, 20000, 1);`
  - `process = _ <: _, _;`
  - `process = par(i, 4, _);`
- Secondary corpus: parse-only pass on `tests/corpus/rep_*.dsp`.
- Optional post-Gate-B integration check (separate from parser viability gate): `import("stdfaust.lib"); process = os.osc(440);` with pinned library path/environment.
- Compare:
  - parse success/failure class,
  - recovery behavior after malformed statements,
  - structural tree dump stability (shape/labels, not pointer addresses).

12. Gate B decision (`lrlex/lrpar` viability).

- Go: Gate A + Gate A.5 accepted, parser subset runs with real Tree/boxes semantics, core conflicts bounded/resolved.
- Conditional Go: small isolated grammar gaps with explicit mitigation and estimate.
- No-Go: precedence/conflict behavior diverges on core expression grammar.
- Quantitative targets for Gate B:
  - prototype corpus parse pass: 100%,
  - secondary corpus parse pass: >=95% (remaining failures triaged and categorized),
  - unresolved grammar conflicts in core expression path: 0,
  - malformed-input recovery tests (`error ENDDEF` class): pass on all defined recovery fixtures.

13. Post-Go integration path.

- Merge validated subset into `crates/tlib` and `crates/parser`.
- Expand grammar/action coverage toward full Faust grammar.
- Add parser regression tests to CI (corpus-based).
- Update `porting/phases/phase-0-validation-en.md` + `JOURNAL.md` with final decisions and residual gaps.

### Gate 0 progress update (`gGlobal` decomposition)

- Added critical-flow decomposition deliverables in:
  - `porting/phases/phase-0-gglobal-decomposition-map-en.md`
- Document includes:
  - field-to-context mapping table (target Rust contexts and owning crates),
  - unresolved coupling list for active flow,
  - first crate-boundary contract draft (`compiler`/`tlib`/`boxes`/`parser`/`codegen`/`errors`).
- Linked the decomposition deliverable into the Phase 0 validation checklist in:
  - `porting/phases/phase-0-validation-en.md`

### Gate A step 1 (`tlib-core` arena foundation)

- Implemented initial `TreeArena` foundation in `crates/tlib/src/arena.rs`:
  - interned node storage (`TreeId`, `TreeNode`, `NodeKind`),
  - hash-consing interner for structural identity,
  - base constructors (`symbol`, `string_lit`, `int`, `float`, `tag`),
  - predefined `nil` node initialization,
  - basic accessors (`node`, `kind`, `children`, `len`).
- Updated `crates/tlib/src/lib.rs` to expose the new arena API.

### Gate A step 2 (`tlib-core` list and properties)

- Added parser-driven list operations to `TreeArena` in `crates/tlib/src/arena.rs`:
  - `cons`, `hd`, `tl`, `is_nil`, `is_list`.
- Added generic node-keyed property storage in `crates/tlib/src/property.rs`:
  - `PropertyStore<T>` with `set/get/get_mut/remove/clear/len`.
- Updated `crates/tlib/src/lib.rs` to expose `PropertyStore`.

### Gate A step 3 (`tlib-core` semantics tests)

- Added integration tests in `crates/tlib/tests/core_semantics.rs` covering:
  - hash-consing identity reuse for structurally identical nodes,
  - list ordering semantics for `cons/hd/tl`,
  - node-keyed property behavior (`set/get/get_mut/remove`).

### Gate A step 4 (`tlib-core` micro-bench harness)

- Added a dedicated micro-bench executable:
  - `crates/tlib/src/bin/treearena_bench.rs`
- Harness measures parser-driven `TreeArena` operations:
  - intern/create pass,
  - repeated intern lookup pass (cache hit behavior),
  - list traversal pass (`cons`/`tl` chain),
  - property set/get passes.
- Usage:
  - `cargo run -p tlib --bin treearena_bench -- <n>`
  - default `n=200000`.

### Gate A step 5 (`tlib-core` benchmark report deliverable)

- Added initial Gate A benchmark report:
  - `porting/phases/phase-0-treearena-benchmark-report-en.md`
- Recorded reproducible Rust measurements for `n=200000`:
  - `create_ms=674.245`
  - `lookup_ms=331.478`
  - `traversal_ms=376.075`
  - `property_set_ms=149.930`
  - `property_get_ms=85.656`
  - `arena_nodes=600002`
- Linked the report in `porting/phases/phase-0-validation-en.md`.
- Gate A marked as conditional pending C++ baseline ratio table (`<= 2x` target).

### Gate A step 6 (`tlib-core` C++ baseline + ratio table)

- Added reproducible C++ benchmark harness:
  - `porting/tools/treearena_cpp_bench.cpp`
- Harness intentionally links directly against current C++ tlib sources from:
  - `/Users/letz/Developpements/RUST/faust/compiler/tlib`
- Updated benchmark report with Rust (`--release`) vs C++ (`-O3`) numbers and ratio table:
  - `porting/phases/phase-0-treearena-benchmark-report-en.md`
- Recorded Gate A decision as **Conditional Go**:
  - create/lookup/traversal/property-set are within threshold or faster than C++,
  - `property_get` remains a hotspot (`12.126x`) and must be addressed before final Gate A closure.

### Gate A step 7 (`tlib-core` property hot-path optimization and closure)

- Refactored `PropertyStore` in `crates/tlib/src/property.rs`:
  - added interned `PropertyKey`,
  - added explicit key API (`key`, `set_with_key`, `get_with_key`, `get_mut_with_key`, `remove_with_key`),
  - switched storage to key-indexed slot vectors (`TreeId` direct indexing) to remove repeated get-path string allocation/hashing.
- Updated `crates/tlib/src/bin/treearena_bench.rs` to benchmark the parser-like hot path with pre-interned key.
- Added non-regression coverage in `crates/tlib/tests/core_semantics.rs` for interned-key API parity.
- Re-ran Rust/C++ benchmark and updated report:
  - `porting/phases/phase-0-treearena-benchmark-report-en.md`
- New ratios (`n=200000`):
  - `create=1.331x`
  - `lookup=1.524x`
  - `traversal=0.867x`
  - `property_set=0.075x`
  - `property_get=1.079x`
- Gate A status updated to **Go**.

### Gate A step 8 (`tlib-core` NodeKind string sharing / lookup parity pass)

- Optimized string-carrying node kinds in `crates/tlib/src/arena.rs`:
  - `NodeKind::Symbol`, `NodeKind::StringLiteral`, `NodeKind::Tag` now store `Arc<str>` instead of `String`.
- Updated constructors to build shared string payloads (`Arc::<str>::from(...)`) to reduce clone/allocate pressure in intern hot paths.
- Updated bench workload in `crates/tlib/src/bin/treearena_bench.rs`:
  - pre-build and reuse `pair_kind` (`NodeKind::Tag`) instead of rebuilding owned strings in each loop.
- Updated tests in `crates/tlib/tests/core_semantics.rs` to match the new node payload type.
- Re-measured (`n=200000`, warm run):
  - Rust: `create_ms=84.593`, `lookup_ms=67.887`, `traversal_ms=55.365`, `property_set_ms=2.708`, `property_get_ms=1.631`
  - C++: `create_ms=69.058`, `lookup_ms=66.816`, `traversal_ms=77.092`, `property_set_ms=38.055`, `property_get_ms=1.515`
  - Ratios: `create=1.225x`, `lookup=1.016x`, `traversal=0.718x`, `property_set=0.071x`, `property_get=1.076x`

### Gate A step 9 (`tlib-core` arity-specialized interning maps)

- Refactored `TreeArena::intern` in `crates/tlib/src/arena.rs` to avoid generic key allocations for common arities:
  - `interner0`: `NodeKind` keys (arity 0),
  - `interner1`: `(NodeKind, TreeId)` keys (arity 1),
  - `interner2`: `(NodeKind, TreeId, TreeId)` keys (arity 2),
  - `interner_n`: fallback `NodeKey` (`Vec<TreeId>`) for arity `>= 3`.
- Goal: remove transient `Vec` allocation and key construction overhead on parser-hot paths (`int`, `cons`, binary tags).
- Re-measured (`n=200000`, warm run):
  - Rust: `create_ms=58.701`, `lookup_ms=45.905`, `traversal_ms=33.444`, `property_set_ms=2.469`, `property_get_ms=1.829`
  - C++: `create_ms=78.483`, `lookup_ms=60.262`, `traversal_ms=77.944`, `property_set_ms=35.679`, `property_get_ms=1.436`
  - Ratios: `create=0.748x`, `lookup=0.762x`, `traversal=0.429x`, `property_set=0.069x`, `property_get=1.274x`

### Gate A step 10 (benchmark report refresh after TreeArena optimizations)

- Updated `porting/phases/phase-0-treearena-benchmark-report-en.md` with optimized post-step-8/9 measurements and ratios.
- Kept Gate A status as **Go** with updated evidence:
  - `create` and `lookup` are now faster than the C++ baseline on this workload,
  - `property_get` remains under the acceptance threshold (`<= 2x`).

### Gate A step 11 (`property_get` targeted optimization without cross-metric regression)

- Refactored `PropertyStore` in `crates/tlib/src/property.rs`:
  - replaced keyed storage `HashMap<PropertyKey, Vec<Option<T>>>` with direct key-indexed `Vec<Vec<Option<T>>>`,
  - kept `PropertyKey` API and string-key compatibility semantics (`key`, `set/get/remove`, `set_with_key/get_with_key/...`).
- Added non-regression test in `crates/tlib/tests/core_semantics.rs`:
  - `property_store_clear_preserves_key_reuse`.
- Validation strategy:
  - due high jitter at `n=200000`, ran interleaved Rust/C++ medians at `n=1_000_000` (3 runs each).
- Median results (`n=1_000_000`):
  - Rust: `create_ms=431.228`, `lookup_ms=378.125`, `traversal_ms=213.172`, `property_set_ms=5.279`, `property_get_ms=2.139`
  - C++: `create_ms=837.103`, `lookup_ms=686.872`, `traversal_ms=908.958`, `property_set_ms=459.997`, `property_get_ms=7.552`
  - Ratios: `create=0.515x`, `lookup=0.551x`, `traversal=0.235x`, `property_set=0.011x`, `property_get=0.283x`
- Conclusion:
  - `property_get` improved strongly and no regression signal observed on `create/lookup` in large-`n` median comparison.
