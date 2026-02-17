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

### Gate A step 12 (`TreeNode` compact children representation)

- Refactored `crates/tlib/src/arena.rs` to remove `Vec<TreeId>` allocation for common arities in node storage:
  - introduced `ChildList` (`Empty`, `One`, `Two`, `Many`),
  - `TreeNode.children` now uses `ChildList`,
  - `children()` API remains exposed as slice (`&[TreeId]`) for read-side compatibility.
- Exported `ChildList` from `crates/tlib/src/lib.rs`.
- Goal: reduce per-node allocation pressure for 0/1/2-child nodes (dominant parser shape).
- Re-measured with interleaved medians (`n=1_000_000`, 3 runs each):
  - Rust: `create_ms=439.164`, `lookup_ms=363.015`, `traversal_ms=162.204`, `property_set_ms=5.545`, `property_get_ms=2.239`
  - C++: `create_ms=881.556`, `lookup_ms=708.131`, `traversal_ms=915.364`, `property_set_ms=469.337`, `property_get_ms=7.467`
  - Ratios: `create=0.498x`, `lookup=0.513x`, `traversal=0.177x`, `property_set=0.012x`, `property_get=0.300x`
- Conclusion:
  - measurable gain on `lookup`/`traversal`,
  - `create` and `property_get` stayed in the same range (noise-level variation), no regression signal at this scale.

### Gate A step 13 (`ahash` fast hasher pass)

- Added `ahash` dependency in `crates/tlib/Cargo.toml`.
- Switched performance-critical hash maps to `AHashMap`:
  - `TreeArena` interners in `crates/tlib/src/arena.rs`,
  - `PropertyStore.key_intern` in `crates/tlib/src/property.rs`.
- Rationale: remove default SipHash overhead from compiler-internal hash-consing and key interning paths.
- Re-measured with interleaved medians (`n=1_000_000`, 3 runs each):
  - Rust: `create_ms=226.897`, `lookup_ms=210.167`, `traversal_ms=99.829`, `property_set_ms=5.794`, `property_get_ms=2.121`
  - C++: `create_ms=864.897`, `lookup_ms=719.490`, `traversal_ms=984.207`, `property_set_ms=468.464`, `property_get_ms=7.578`
  - Ratios: `create=0.262x`, `lookup=0.292x`, `traversal=0.101x`, `property_set=0.012x`, `property_get=0.280x`
- Conclusion:
  - clear additional gain on `create`/`lookup` with no regression signal on other metrics.

### Gate A step 14 (`pre-allocation` A/B validation pass)

- Added explicit pre-allocation APIs:
  - `TreeArena::with_capacity`, `TreeArena::with_capacities`, `TreeArena::reserve` in `crates/tlib/src/arena.rs`,
  - `PropertyStore::with_key_capacity`, `PropertyStore::reserve_slots` in `crates/tlib/src/property.rs`.
- Extended bench in `crates/tlib/src/bin/treearena_bench.rs`:
  - new `--prealloc` mode,
  - two-phase reserve strategy for arity-2 interner (pairs first, then cons).
- Added non-regression tests in `crates/tlib/tests/core_semantics.rs`:
  - `tree_arena_with_capacities_preserves_interning_semantics`,
  - `tree_arena_reserve_preserves_interning_semantics`,
  - `property_store_reserve_slots_does_not_set_values`.
- A/B median comparison (`n=1_000_000`, 6 runs, alternating order):
  - baseline medians:
    - `create_ms=195.569`, `lookup_ms=185.300`, `traversal_ms=97.185`, `property_set_ms=5.954`, `property_get_ms=2.175`
  - `--prealloc` medians:
    - `create_ms=178.464`, `lookup_ms=191.432`, `traversal_ms=70.114`, `property_set_ms=2.296`, `property_get_ms=0.845`
  - `prealloc / baseline`:
    - `create=0.913x`, `lookup=1.033x`, `traversal=0.721x`, `property_set=0.386x`, `property_get=0.388x`
- Conclusion:
  - pre-allocation gives clear wins on `create`, `traversal`, and property passes,
  - slight `lookup` regression (~3.3%) remains on this protocol, so keep pre-allocation as opt-in API for now (not default path).

### Gate A step 15 (`tlib` coverage status checkpoint)

- Recorded current validation status for `tlib` only (not full compiler parity).
- Considered as covered in current `tlib` scope:
  - hash-consing identity reuse (`intern` structural sharing),
  - list primitives (`cons`/`hd`/`tl`, `is_nil`, `is_list`),
  - property API semantics (string and interned-key paths, clear/remove behavior),
  - Rust vs C++ micro-benchmark parity envelope and optimization history.
- Identified remaining gaps before calling `tlib` validation "exhaustive":
  - broader `NodeKind` semantic matrix (`float`, `string_lit`, mixed symbol/tag edge cases),
  - explicit coverage for arity `>=3` interning fallback paths under high cardinality,
  - adversarial hash/collision-style stress cases,
  - determinism checks (stable structure/IDs across repeated builds for identical construction order),
  - memory growth and peak-allocation tracking alongside timing metrics,
  - reserve/pre-allocation invariants on very large, sparse `TreeId` distributions.
- Decision:
  - current `tlib` validation level is sufficient for Phase 0 Go,
  - not yet marked as "exhaustive"; above gap list remains the backlog for hardening.

### Gate A step 16 (process rule sync: unit tests during porting)

- Updated `AGENTS.md` to make the testing rule explicit:
  - each porting change must add/update unit tests in touched crate(s),
  - if immediate tests are not possible, the exception must be documented in `JOURNAL.md` with owner and follow-up plan.
- Purpose:
  - align `AGENTS.md` wording with the existing rule already present in `porting/faust-rust-porting-plan-en.md`.

### Gate A step 17 (process rule sync: source provenance in Rustdoc)

- Added an explicit documentation rule in `AGENTS.md`:
  - migrated code must carry source-provenance in Rustdoc (`///`/`//!`) with C++ source references and parity-relevant invariants/behavior notes.
- Updated `porting/faust-rust-porting-plan-en.md`:
  - elevated source-provenance Rustdoc to a global migration objective,
  - clarified expected Rustdoc provenance content during porting (`source path + invariants`).
- Updated `porting/phases/phase-0-validation-en.md`:
  - added a dedicated "source-provenance documentation discipline" validation item,
  - added corresponding deliverable, go/no-go criteria, and exit-checklist entry.
- Updated `porting/faust-rust-points-critiques-en.md`:
  - added source-provenance Rustdoc requirement in the top-level prototype execution rules.

### Gate A step 18 (`tlib` Rustdoc provenance pass)

- Documented `crates/tlib` public API in Rustdoc with explicit source provenance and parity invariants:
  - `crates/tlib/src/lib.rs`:
    - crate-level overview and C++ source mapping (`tree/list/property/node/symbol` files),
    - core parity invariants summary.
  - `crates/tlib/src/arena.rs`:
    - module-level provenance (`tree.hh/.cpp`, `list.hh/.cpp`, `node.hh`),
    - invariants for hash-consing, `TreeId`, canonical `nil`, list semantics,
    - public type/method documentation (`TreeArena`, `NodeKind`, `ChildList`, constructors/accessors).
  - `crates/tlib/src/property.rs`:
    - module-level provenance (`property.hh`, `tree.hh` property API),
    - invariants for node-keyed properties and interned key fast path,
    - public API documentation (`PropertyStore`, `PropertyKey`, keyed/string methods, reserve behavior).
- Validation:
  - `cargo fmt --all`
  - `cargo test -p tlib`

### Gate A step 19 (public API migration policy clarification)

- Clarified API migration policy across governance docs:
  - APIs are not blindly ported signature-by-signature (`1:1`) in all cases,
  - internal Rust APIs may be adapted when needed for idiomatic ownership/types/error handling,
  - external compatibility surfaces (CLI + C/C++ API tiers) remain parity targets.
- Added explicit status convention for touched public APIs:
  - `1:1`, `adapted`, `deferred`.
- Added traceability requirements (for touched APIs):
  - C++ symbol/file reference,
  - Rust symbol/module,
  - rationale + compatibility impact,
  - validation tests.
- Updated files:
  - `AGENTS.md`
  - `porting/faust-rust-porting-plan-en.md`
  - `porting/phases/phase-0-validation-en.md`
  - `porting/phases/phase-9-integration-en.md`

### Gate A.5 step 1 (`boxes` minimal parser-driven subset, no stubs)

- Replaced `crates/boxes` scaffold with a real Tree-backed API subset intended for parser semantic actions.
- Added `tlib` dependency to `crates/boxes/Cargo.toml`.
- Implemented tagged `BoxId` constructors/predicates in `crates/boxes/src/lib.rs`:
  - identifiers/numerics: `box_ident`, `box_ident_name`, `box_int`, `box_real`, `is_box_int`, `is_box_real`,
  - composition: `box_seq`, `box_par`, `box_rec`, `box_split`, `box_merge` + `is_*`,
  - primitives/environment: `box_wire`, `box_cut`, `box_environment` + `is_*`,
  - iterative/local-rec subset: `box_ipar`, `box_with_local_def`, `box_with_rec_def` + `is_*`,
  - UI subset: `box_hslider` + `is_box_hslider` with preserved C++ `list4(cur,min,max,step)` payload layout.
- Added explicit Rustdoc provenance and API mapping status (`1:1` vs `adapted`) for this first subset:
  - C++ sources of truth: `compiler/boxes/boxes.hh`, `compiler/boxes/boxes.cpp`.
- Added dedicated crate tests in `crates/boxes/tests/core_api.rs` covering:
  - structural roundtrip for constructors/predicates,
  - hash-consing stability for primitives,
  - `box_hslider` list payload ordering,
  - local/recursive def node preservation.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p boxes --all-targets`
  - `cargo test -p tlib --all-targets`

### Gate A.5 step 2 (`boxes` iterative/UI completion + structural dump)

- Extended `crates/boxes/src/lib.rs` with parser-needed constructors from C++ `boxes.hh/.cpp`:
  - iterative composition: `box_iseq`, `box_isum`, `box_iprod` + `is_*`,
  - UI inputs: `box_button`, `box_checkbox`, `box_vslider`, `box_num_entry` + `is_*`,
  - UI outputs: `box_vbargraph`, `box_hbargraph` + `is_*`.
- Preserved C++ UI payload shape for slider-like widgets:
  - `tree(TAG, label, list4(cur,min,max,step))` for `hslider`/`vslider`/`numentry`.
- Added deterministic structural dump helper:
  - `dump_box(&TreeArena, BoxId) -> String`,
  - output is shape/labels/value based and excludes pointer/address data,
  - intended for upcoming parser differential checks (Rust vs C++).
- Extended `crates/boxes/tests/core_api.rs`:
  - iterative constructor roundtrips (`ipar`/`iseq`/`isum`/`iprod`),
  - UI constructor/predicate roundtrips (button/checkbox/sliders/numentry/bargraphs),
  - structural dump determinism check with stable expected string.

### Gate A.5 step 3 (process docs sync: explicit `clippy` gate in porting docs)

- Completed process documentation so `clippy` usage is explicit beyond AGENTS:
  - `porting/faust-rust-porting-plan-en.md`:
    - added a global "mandatory quality gate for each porting step" section:
      - `cargo fmt --all`
      - `cargo clippy --workspace --all-targets -- -D warnings`
      - `cargo test --workspace --all-targets`
    - added exception tracking rule in `JOURNAL.md` when one command cannot run.
  - `porting/phases/phase-0-validation-en.md`:
    - added a dedicated "quality gate discipline" validation item,
    - added corresponding Phase 0 exit-checklist criterion.
  - `porting/phases/phase-9-integration-en.md`:
    - expanded final integration "Done" criteria from tests-only to full
      `fmt` + `clippy -D warnings` + workspace tests on Linux/macOS/Windows.
  - `AGENTS.md`:
    - mirrored the same mandatory per-step quality-gate rule in Porting Discipline.

### Gate B step 1 (`ParserCtx` + `parser-proto` crate bootstrap)

- Added a new isolated crate:
  - `crates/parser-proto` (workspace member),
  - keeps production `crates/parser` untouched while validating parser migration foundations.
- Added `lrlex`/`lrpar`/`cfgrammar` wiring with compile-time generation:
  - `crates/parser-proto/build.rs`,
  - minimal grammar files under `crates/parser-proto/src/grammar/`:
    - `faustlexer.l`
    - `faustparser.y`
  - smoke helper `parse_minimal("process = _;")` in `crates/parser-proto/src/lib.rs`.
- Implemented `ParserCtx` in `crates/parser-proto/src/context.rs` with explicit C++ provenance mapping:
  - parser cursor (`FAUSTfilename`/`FAUSTlineno` equivalent),
  - waveform accumulator (`gWaveForm` equivalent),
  - parse result root (`gResult` equivalent),
  - property hooks equivalent to `setDefProp`/`setUseProp` with typed location payload,
  - parser-local diagnostics + error/recovery counters.
- Added dedicated tests:
  - `crates/parser-proto/tests/parser_ctx.rs`:
    - def/use property mapping semantics,
    - cursor hooks,
    - waveform drain behavior,
    - parse root storage,
    - diagnostics/counters behavior.
  - `crates/parser-proto/tests/parser_smoke.rs`:
    - accept/reject checks for minimal generated lexer/parser pipeline.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline`
- Quality-gate execution note:
  - online `cargo clippy` / `cargo test` could not run because crates.io DNS resolution failed in this environment (`Could not resolve host: index.crates.io`);
  - equivalent offline validation was executed and passed.

### Gate B step 2 (`faustlexer.l` -> `lrlex` prototype subset + position bridge)

- Extended `crates/parser-proto/src/grammar/faustlexer.l` with a first C++-aligned subset:
  - numerics: `INT`, `FLOAT` forms (`42`, `42f`, `3.14`, `.5`, `1e-3`, etc.),
  - identifiers/strings: `IDENT`, `STRING`, `FSTRING`,
  - operator/layout tokens used by parser slices (`SEQ/PAR/SPLIT/MIX/REC`, comparisons, shifts, `LAPPLY`, `ARROW`),
  - keywords/primitives/UI iterators (`with`, `letrec`, `par`, `seq`, `sum`, `prod`, widgets, etc.),
  - comment and whitespace skipping (`//...`, `/*...*/`, blanks/newlines).
- Updated `crates/parser-proto/src/grammar/faustparser.y` token declarations and added `TokenCatalog` catch-all rule so lexer token map stays fully linked during this bootstrap phase.
- Added lexer utilities in `crates/parser-proto/src/lib.rs`:
  - `lex_tokens(input) -> Vec<LexedToken>` with stable token names/text/spans/line-column,
  - `set_use_prop_from_token(...)` to bridge lexer source positions into `ParserCtx` use-site properties.
- Added dedicated lexer tests in `crates/parser-proto/tests/lexer_tokens.rs`:
  - keyword-vs-identifier priority,
  - operator priority ordering (`<:`, `<=`, `<`, `:>`, `+>`, `->`, `=>`, `>>`, `>`),
  - numeric and string token class coverage,
  - comment/whitespace skipping,
  - position bridge from lexed token to `ParserCtx::set_use_prop`.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline`

### Gate B step 3 (Slice 1 parser/actions: `program/statement/definition/recovery`)

- Implemented Slice 1 grammar in `crates/parser-proto/src/grammar/faustparser.y`:
  - `Program -> StmtList`,
  - `StmtList` cons-list accumulation,
  - `Definition` forms:
    - `defname = expression;`
    - `defname(arglist) = expression;`
    - recovery forms for malformed definitions before `;`.
  - Note: bison-style `error` symbol is not used directly in `lrpar`; Slice 1 recovery is encoded with explicit malformed-definition alternatives ending on `ENDDEF`.
  - expression subset wired to `boxes` APIs:
    - `seq/par/split/mix/rec`,
    - atoms: wire/cut/int/float/ident/parentheses,
    - iterative prototype form: `par(i, 4, expr)`.
- Added `%parse-param` parser state integration through `RefCell<ParseState>`:
  - introduced `ParseState` + `with_state(...)` helper in `crates/parser-proto/src/lib.rs`,
  - grammar actions now mutate real `TreeArena` + `ParserCtx` (no stubs).
- Added parser runtime API and state return path:
  - `parse_program(input, source_file) -> ParseOutput`,
  - keeps parse root, diagnostics/errors, arena, and parser context for structural checks.
- Source-location / property hooks wired in semantic actions:
  - `setDefProp` equivalent on definition names,
  - `setUseProp` equivalent on identifier uses.
- Added Slice 1 dedicated tests in `crates/parser-proto/tests/parser_slice1.rs`:
  - nominal `process = _;`,
  - malformed definition recovery ending at `;`,
  - iterative `par(i, 4, _)`,
  - identifier use-property tracking.
- Updated smoke behavior in `crates/parser-proto/tests/parser_smoke.rs`:
  - invalid minimal sentence now covered as recovered parse (Slice 1 recovery semantics).
- Build-generation adjustment for prototype stage:
  - `crates/parser-proto/build.rs` now uses:
    - `.warnings_are_errors(false)`
    - `.show_warnings(false)`
  - rationale: keep full lexer token set declared while Slice 1 grammar intentionally uses only a subset.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline`

### Gate B step 4 (Slice 2 parser core: `expression/infix/argument` + C++ precedence)

- Extended `crates/parser-proto/src/grammar/faustparser.y` with Slice 2 core:
  - C++-aligned precedence tiers for `PAR/SEQ/SPLIT/MIX/REC`, infix arithmetic/logical/comparison operators, postfix delay (`'`), and dot-access.
  - Added `ParamList` (definition parameters) and `ArgList`/`Argument` (application arguments) split to preserve C++ parser behavior around comma-vs-expression ambiguity.
  - Added `InfixExp` lowering rules matching C++ parser actions:
    - binary ops lower to `boxSeq(boxPar(lhs,rhs), boxOp())`,
    - postfix `DELAY1` lowers to `boxSeq(expr, boxDelay1())`,
    - dot-access lowers to `boxAccess(expr, ident)`,
    - application lowers to `boxAppl(fun, revarglist)` (same prototype behavior as C++ `buildBoxAppl` path used today).
- Extended `crates/parser-proto/src/lib.rs` (`ParseState`) for Slice 2 action support:
  - added `PrimitiveOp` enum and lowering helpers (`binary_prim`, `postfix_prim`),
  - added signed literal parsing helpers for `+/- INT/FLOAT`,
  - added `apply_box` and `access_box` action helpers.
- Extended `crates/boxes/src/lib.rs` with parser-needed APIs (Tree-backed, no stubs):
  - application/access: `box_appl`, `is_box_appl`, `box_access`, `is_box_access`,
  - primitive operators: `box_add/sub/mul/div/rem/and/or/xor/lsh/rsh/lt/le/gt/ge/eq/ne/pow/delay/delay1` + `is_*` predicates.
- Added/updated tests:
  - `crates/boxes/tests/core_api.rs`:
    - `primitive_appl_and_access_boxes_roundtrip`.
  - `crates/parser-proto/tests/parser_slice2.rs`:
    - infix precedence (`1 + 2 * 3`),
    - postfix delay and dot-access (`_';`, `foo.bar`),
    - application argument-list shape (`foo(1,2)` reversed list contract),
    - unary minus identifier lowering (`-foo`).
- Parser generation notes:
  - Slice 2 grammar compiles without parser conflicts under current subset (`error_on_conflicts` gate remains active in build pipeline).
  - Full token-set warnings remain intentionally non-blocking/hidden at this stage (`warnings_are_errors(false)`, `show_warnings(false)`) while only a subset of tokens is consumed.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline`

### Gate B step 5 (Slice 3 parser subset: UI + iterative + standalone primitives)

- Extended `crates/parser-proto/src/grammar/faustparser.y` with Slice 3 rules targeting prototype corpus coverage:
  - standalone primitive forms used as callable functions:
    - arithmetic/logic/compare core (`+ - * / % @ '`, `and/or/xor`, shifts, comparisons, `pow`),
    - `mem` mapped to `boxDelay1`,
    - `min`/`max` primitive tokens.
  - UI primitives:
    - `button`, `checkbox`,
    - `hslider`, `vslider`, `nentry`,
    - `hbargraph`, `vbargraph`.
  - iterative primitives:
    - `ipar`, `iseq`, `isum`, `iprod`.
  - string-label parsing nonterminal:
    - `UQString` from `STRING`/`FSTRING`.
- Extended `crates/parser-proto/src/lib.rs` (`ParseState`):
  - added `uqstring_from_token(...)` helper (quoted-string unquote bridge),
  - retained action path through real `boxes` + `tlib` APIs (no stubs).
- Extended `crates/boxes/src/lib.rs` with parser-needed primitive constructors:
  - `box_min` / `box_max` + `is_box_min` / `is_box_max`.
- Extended tests:
  - `crates/boxes/tests/core_api.rs`:
    - primitive roundtrip now includes `min/max`.
  - `crates/parser-proto/tests/parser_slice3.rs`:
    - UI constructor parse check (`hslider`),
    - iterative parse checks (`seq/sum/prod` iterator forms),
    - recursion form check (`process = + ~ _;`),
    - parse-only acceptance on corpus subset:
      - `tests/corpus/rep_01_passthrough.dsp`
      - `tests/corpus/rep_02_gain_bias.dsp`
      - `tests/corpus/rep_03_stereo_mix.dsp`
      - `tests/corpus/rep_04_delay_echo.dsp`
      - `tests/corpus/rep_05_one_pole_lowpass.dsp`
      - `tests/corpus/rep_06_comb_feedback.dsp`
      - `tests/corpus/rep_07_nonlinear_clip.dsp`
      - `tests/corpus/rep_08_branch_and_sum.dsp`
      - `tests/corpus/rep_09_ui_slider.dsp`
      - `tests/corpus/rep_10_two_in_two_out_ui.dsp`
- Validation:
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline`

### Gate B step 6 (Rust vs C++ differential validation: parse/recovery classes)

- Added dedicated differential harness:
  - `crates/parser-proto/tests/cpp_differential.rs`
- Harness compares Rust `parser-proto` and C++ `faust` on:
  - `tests/corpus/rep_01_passthrough.dsp` ... `rep_10_two_in_two_out_ui.dsp`
  - malformed fixtures:
    - `malformed_empty_rhs`: `process = ;`
    - `malformed_missing_rpar`: `process = hslider("g", 0.5, 0.0, 1.0, 0.01;`
- Classification used:
  - Rust: `Ok` / `Recovered` / `Error` (from parse root + `ParserCtx` error/recovery counters),
  - C++: `Ok` / `ParseError` / `OtherError` (process status + diagnostics text).
- C++ reference used for this run:
  - source-of-truth root: `/Users/letz/Developpements/RUST/faust`
  - source commit: `8eebea429`
  - executable used by harness: `/usr/local/bin/faust`
- Observed results (all matched expectations):
  - valid corpus cases (`rep_01..rep_10`): `Rust=Ok`, `C++=Ok`
  - malformed fixtures:
    - `malformed_empty_rhs`: `Rust=Recovered` (recovery path triggered), `C++=ParseError`
    - `malformed_missing_rpar`: `Rust=Recovered` (parser error path), `C++=ParseError`
- Differential status for Step 6:
  - parse class parity on valid corpus: pass
  - malformed-input detection parity: pass
  - no class mismatches in current Slice 3 scope.
- Validation:
  - `cargo test -p parser-proto --test cpp_differential --offline -- --nocapture`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline`

### Gate B step 7 (Go/No-Go decision: `lrlex/lrpar` parser-proto viability)

- Evaluated Gate B against the acceptance criteria defined in the plan:
  - Gate A status (`tlib-core`): **Go**.
  - Gate A.5 status (`boxes` parser subset): **Go**.
  - prototype/secondary corpus parse pass:
    - `tests/corpus/rep_01` ... `rep_10`: `10/10` (`100%`) accepted by Rust parser-proto.
  - unresolved grammar conflicts in core expression path:
    - `0` (current Slice 1/2/3 grammar compiles under `error_on_conflicts`).
  - malformed-input recovery fixtures:
    - `2/2` pass (`Recovered` on Rust, parse error class on C++).
- Decision for Gate B:
  - **Go** for parser migration prototype viability (`faustparser.y`/`faustlexer.l` -> `lrpar`/`lrlex`) on the validated Slice 1/2/3 scope.
- Non-blocking residual scope (explicitly out of this gate):
  - full grammar coverage beyond Slice 3 (imports, pattern matching, route, signatures, metadata full matrix),
  - stdlib-wide and large-corpus parse coverage,
  - structural tree-shape differential beyond parse/recovery class checks.
- Consequence:
  - proceed from prototype gate validation to incremental integration path (parser coverage expansion + eventual merge plan from `parser-proto` into production `parser` when target slices are stabilized).

### Parser porting docs update (post Gate B remaining steps)

- Updated parser porting documentation to include an explicit, ordered "remaining steps" roadmap from Gate B prototype to full parser completion.
- Added concrete deliverable + pass criterion for each remaining step:
  - strict parser-proto baseline gate,
  - full lexer parity,
  - full grammar parity,
  - semantic action parity,
  - diagnostics/recovery parity,
  - `SourceReader` integration,
  - optional `SourceFetcher`/`Enrobage` scope resolution,
  - expanded Rust vs C++ differential suite,
  - merge into production `crates/parser`,
  - final quality/documentation closure.
- Updated files:
  - `porting/phases/phase-3-parser-en.md`
  - `porting/faust-rust-porting-plan-en.md`

### Gate B remaining step 1 (strict parser-proto baseline gate)

- Locked parser-proto build generation to strict mode:
  - `crates/parser-proto/build.rs` now uses:
    - `.warnings_are_errors(true)`
    - `.show_warnings(true)`
- Removed hidden warning debt on currently touched Slice 1/2/3 areas:
  - reduced `%token` declarations in `crates/parser-proto/src/grammar/faustparser.y` to the active subset,
  - added explicit `LexProbeToken` recovery branch so lexer-priority probe tokens (`WITH`, `LETREC`, `WHERE`, `ARROW`, `LAPPLY`) stay covered without parser-local stubs.
- Synchronized lexer subset with the strict grammar gate:
  - simplified `crates/parser-proto/src/grammar/faustlexer.l` to the currently supported token surface,
  - kept C++ operator-priority probes and keyword-priority probes used by tests.
- Validation:
  - `cargo test -p parser-proto --offline --no-fail-fast`
  - `cargo clippy -p parser-proto --all-targets --offline -- -D warnings`

### Gate B remaining step 2 (lexer parity expansion: `faustlexer.l` -> `lrlex`)

- Expanded `crates/parser-proto/src/grammar/faustlexer.l` toward C++ lexer parity using `lrlex` start states:
  - `%x comment doc lst`
  - state transitions for doc/listing/equation/diagram/metadata sections.
- Extended parser token declarations in `crates/parser-proto/src/grammar/faustparser.y` to the broader lexer surface.
- Added `LexProbeToken` coverage branch for currently unparsed token families so strict parser generation remains warning-clean while grammar migration is still Slice 1/2/3.
- Added lexer parity documentation artifact:
  - `porting/phases/phase-3-lexer-token-mapping-en.md`
- Linked lexer mapping artifact from:
  - `porting/phases/phase-3-parser-en.md` (step 2 deliverable path).
- Extended lexer test coverage in `crates/parser-proto/tests/lexer_tokens.rs`:
  - doc/listing/equation state transition tests,
  - extended keyword/token matrix aligned with C++ lexer surface.
- Validation:
  - `cargo test -p parser-proto --offline --no-fail-fast`
  - `cargo clippy -p parser-proto --all-targets --offline -- -D warnings`

### Gate B remaining step 3 (grammar parity progress: statements `import` / `declare`)

- Extended `Statement` grammar in `crates/parser-proto/src/grammar/faustparser.y` with C++-aligned forms:
  - `import("...");`
  - `declare key "value";`
  - `declare def key "value";`
- Added parser-side metadata/import recording in `ParserCtx`:
  - import list (`imports()`),
  - metadata list (`declared_metadata()`),
  - definition-metadata list (`declared_definition_metadata()`).
- Added corresponding semantic-action helpers in `ParseState` (`crates/parser-proto/src/lib.rs`):
  - `import_statement`,
  - `declare_metadata_from_token`,
  - `declare_definition_metadata_from_tokens`.
- Added dedicated Slice 4 tests:
  - `crates/parser-proto/tests/parser_slice4.rs`
  - validates parse acceptance and recorded import/declare payloads.
- Validation:
  - `cargo test -p parser-proto --offline --no-fail-fast`
  - `cargo clippy -p parser-proto --all-targets --offline -- -D warnings`

### Gate B remaining step 8 (differential suite expansion: `declare` cases)

- Extended Rust vs C++ differential harness in:
  - `crates/parser-proto/tests/cpp_differential.rs`
- Added new cases:
  - `declare_metadata` (valid),
  - `declare_definition_metadata` (valid),
  - `malformed_declare_missing_value` (invalid/recovery class).
- Differential run (source of truth C++ root `/Users/letz/Developpements/RUST/faust`, commit `8eebea429`, binary `/usr/local/bin/faust`) shows no class mismatches on the expanded set.
- Validation:
  - `cargo test -p parser-proto --test cpp_differential --offline -- --nocapture`
  - `cargo test -p parser-proto --offline --no-fail-fast`
  - `cargo clippy -p parser-proto --all-targets --offline -- -D warnings`

### Gate B remaining step 6 (SourceReader prototype: import expansion + cycle detection)

- Added `SourceReader` prototype in:
  - `crates/parser-proto/src/source_reader.rs`
- Implemented:
  - import resolution with search paths,
  - recursive `import("...");` expansion,
  - read cache,
  - used-files tracking,
  - import-cycle detection.
- Exported reader API through `crates/parser-proto/src/lib.rs`:
  - `SourceReader`,
  - `SourceReaderError`,
  - `parse_file_with_imports(...)`.
- Added dedicated tests:
  - `crates/parser-proto/tests/source_reader.rs`
  - resolves imports through search paths,
  - nested import expansion + used-file tracking,
  - cycle detection behavior.
- Validation:
  - `cargo test -p parser-proto --offline --no-fail-fast`
  - `cargo clippy -p parser-proto --all-targets --offline -- -D warnings`

### Gate B remaining step 3 (grammar parity progress: documentation statements/tags)

- Extended parser grammar (`crates/parser-proto/src/grammar/faustparser.y`) with doc statement support:
  - `BDOC ... EDOC` statement form,
  - doc elements: `DOCCHAR`, `NOTICE`, `BEQN/EEQN`, `BDGM/EDGM`, `BLST/ELST`, `BMETADATA/EMETADATA`,
  - listing attributes: `dependencies`, `mdoctags`, `distributed` (`LST*` token family).
- Extended `ParserCtx` (`crates/parser-proto/src/context.rs`) with doc/listing state tracking:
  - doc block/notice/listing counters,
  - doc-char counter,
  - metadata tag capture,
  - listing switches (`dependencies`, `mdoctags`, `distributed`).
- Added parser action helpers in `ParseState` (`crates/parser-proto/src/lib.rs`) to route doc/listing effects through `ParserCtx`.
- Added dedicated tests:
  - `crates/parser-proto/tests/parser_slice5_doc.rs`
  - validates doc parse acceptance and recorded doc/listing metadata state.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p parser-proto --offline --no-fail-fast`
  - `cargo clippy -p parser-proto --all-targets --offline -- -D warnings`

### Gate B remaining step 8 (differential suite expansion: documentation case)

- Extended differential harness (`crates/parser-proto/tests/cpp_differential.rs`) with:
  - `doc_notice_listing_metadata` (valid doc statement case).
- Differential result: Rust/C++ class parity preserved (`Rust=Ok`, `C++=Ok`) on this new case.
- Note:
  - an exploratory malformed doc-unclosed case was not retained in the stable harness because it can cause long-running behavior on the C++ parser binary; timeout-hardening of the harness remains a follow-up task.
- Validation:
  - `cargo test -p parser-proto --test cpp_differential --offline -- --nocapture`
  - `cargo test -p parser-proto --offline --no-fail-fast`
  - `cargo clippy -p parser-proto --all-targets --offline -- -D warnings`

### Gate B remaining step 3 (grammar parity progress: local scopes + module/route/waveform primitives)

- Extended parser grammar (`crates/parser-proto/src/grammar/faustparser.y`) with additional C++ families:
  - local scopes:
    - `expression WITH { deflist }`
    - `expression LETREC { reclist }`
    - `expression LETREC { reclist WHERE deflist }`
  - recursive definition forms:
    - `recname = DELAY1 ident` (C++-aligned `'x` shape in `letrec`)
  - module and structural primitives:
    - `component("...")`
    - `library("...")`
    - `environment { stmtlist }`
    - `waveform { number, ... }`
    - `route(a, b)` and `route(a, b, expr)`
- Extended parser semantic actions (`crates/parser-proto/src/lib.rs`):
  - waveform accumulation bridge:
    - `push_waveform_value(...)`
    - `waveform_box_from_ctx(...)`
  - fake-route compatibility helper:
    - `route_box_default_spec(...)` producing `boxRoute(a,b,boxPar(boxInt(0),boxInt(0)))` for the 2-argument route form.
- Extended `boxes` API (`crates/boxes/src/lib.rs`) with C++-aligned constructors/predicates:
  - `box_component` / `is_box_component`
  - `box_library` / `is_box_library`
  - `box_waveform` / `is_box_waveform`
  - `box_route` / `is_box_route`
- Added/extended tests:
  - `crates/parser-proto/tests/parser_slice6_scope_modules.rs`
    - validates `with`, `letrec`, `environment`, `component`, `library`, `waveform`, `route` parse shapes.
  - `crates/boxes/tests/core_api.rs`
    - added roundtrip checks for component/library/waveform/route constructors.

### Gate B remaining step 8 (differential suite expansion: local-scope/waveform cases)

- Extended differential harness (`crates/parser-proto/tests/cpp_differential.rs`) with stable C++ parity cases:
  - `with_local_def`
  - `letrec_basic`
  - `waveform_numbers`
- Differential reference used:
  - source-of-truth root: `/Users/letz/Developpements/RUST/faust`
  - source commit: `8eebea429`
  - binary: `/usr/local/bin/faust`
- Note:
  - exploratory `environment` and `route` differential fixtures were not kept in the stable harness because the current C++ binary returns non-zero status on those standalone snippets in this harness setup (error class not suitable for a strict "valid-case" gate). They remain covered by Rust structural parser tests.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`
  - `cargo test -p parser-proto --test cpp_differential --offline -- --nocapture`

### Gate B remaining step 3 (grammar parity progress: foreign forms `ffunction/fconstant/fvariable`)

- Extended `boxes` foreign API surface in `crates/boxes/src/lib.rs` (C++ aligned):
  - `ffunction(signature, incfile, libfile)` + `is_ffunction`
  - `box_ffun` + `is_box_ffun`
  - `box_fconst` + `is_box_fconst`
  - `box_fvar` + `is_box_fvar`
- Extended parser semantic helpers in `crates/parser-proto/src/lib.rs`:
  - raw symbol extraction for `STRING`/`FSTRING` foreign payloads,
  - foreign type-code builders (`int=0`, `float=1`, `any=2`),
  - C++-shaped signature building:
    - 4-slot function-name list (`float/double/quad/fixed` dispatch slots),
    - `cons(ret_type, cons(names4, arg_types))` layout,
  - `box_foreign_function(...)` bridge (`ffunction` -> `boxFFun`).
- Extended parser grammar (`crates/parser-proto/src/grammar/faustparser.y`) with foreign families:
  - `ffunction(type fun(|fun){0..3} (typelist?), fstring, string)`
  - `fconstant(type name, fstring)`
  - `fvariable(type name, fstring)`
  - plus `type`, `argtype`, `typelist`, `fun`, `name`, `fstring`, `string` support rules.
- Added/extended tests:
  - `crates/parser-proto/tests/parser_slice7_foreign.rs`
    - validates `ffunction` signature structure and `fconstant/fvariable` node forms.
  - `crates/boxes/tests/core_api.rs`
    - foreign box constructor/predicate roundtrip coverage.

### Gate B remaining step 8 (differential suite expansion: foreign forms)

- Extended differential harness (`crates/parser-proto/tests/cpp_differential.rs`) with:
  - `foreign_fconstant`
  - `foreign_fvariable`
  - `foreign_ffunction`
- Differential run (C++ source-of-truth root `/Users/letz/Developpements/RUST/faust`, commit `8eebea429`, binary `/usr/local/bin/faust`) passed with no class mismatch on new foreign cases.
- Validation:
  - `cargo test -p parser-proto --test cpp_differential --offline -- --nocapture`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`

### Gate B remaining step 3 (grammar parity progress: `CASE` / `rulelist`)

- Extended `boxes` API (`crates/boxes/src/lib.rs`) with pattern-matching nodes:
  - `box_case` / `is_box_case`
  - `box_pattern_var` / `is_box_pattern_var`
- Extended parser grammar (`crates/parser-proto/src/grammar/faustparser.y`) with:
  - `CASE { rulelist }` primitive form,
  - `rulelist` and `rule` productions:
    - `rule: (arglist) => expression;`
- Added parser-side C++-aligned rule checks and pattern preparation in `ParseState` (`crates/parser-proto/src/lib.rs`):
  - `box_case_checked(...)`:
    - rejects empty case rule list,
    - checks arity consistency across all rules,
    - records parser diagnostics on mismatch,
    - returns `nil` on invalid case shape (recovery path).
  - recursive `prepare_pattern(...)` pass on rule lhs:
    - converts `BOXIDENT` to `BOXPATVAR`,
    - preserves `BOXAPPL` function-ident head behavior (`x(e)` keeps `x`, maps args),
    - recursively maps lhs pattern trees/lists before wrapping in `BOXCASE`.
- Added/extended tests:
  - `crates/parser-proto/tests/parser_slice8_case.rs`
    - valid case parsing + presence of `BOXPATVAR` in prepared lhs,
    - malformed arity mismatch diagnostic path.
  - `crates/boxes/tests/core_api.rs`
    - case/pattern-var constructor roundtrip.

### Gate B remaining step 8 (differential suite expansion: `CASE` forms)

- Extended differential harness (`crates/parser-proto/tests/cpp_differential.rs`) with:
  - `case_single_rule` (valid)
  - `case_arity_mismatch` (malformed)
- Differential run (C++ source-of-truth root `/Users/letz/Developpements/RUST/faust`, commit `8eebea429`, binary `/usr/local/bin/faust`) passed:
  - valid case classified `Rust=Ok`, `C++=Ok`,
  - mismatched-arity case classified invalid on both sides (`Rust=Recovered`, `C++=ParseError`).
- Validation:
  - `cargo test -p boxes --offline --no-fail-fast`
  - `cargo test -p parser-proto --test parser_slice8_case --offline --no-fail-fast`
  - `cargo test -p parser-proto --test cpp_differential --offline -- --nocapture`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`

### Gate B remaining step 3 (grammar parity progress: `lambda` + UI groups + stream wrappers)

- Extended `boxes` API (`crates/boxes/src/lib.rs`) with C++-aligned constructors/predicates:
  - lambda:
    - `box_abstr` / `is_box_abstr`
    - `build_box_abstr` (equivalent to C++ `buildBoxAbstr`)
  - stream wrappers:
    - `box_inputs` / `is_box_inputs`
    - `box_outputs` / `is_box_outputs`
    - `box_ondemand` / `is_box_ondemand`
    - `box_upsampling` / `is_box_upsampling`
    - `box_downsampling` / `is_box_downsampling`
  - UI grouping and soundfile:
    - `box_vgroup` / `is_box_vgroup`
    - `box_hgroup` / `is_box_hgroup`
    - `box_tgroup` / `is_box_tgroup`
    - `box_soundfile` / `is_box_soundfile`
- Extended parser grammar (`crates/parser-proto/src/grammar/faustparser.y`) with:
  - `LAMBDA (params) . (expression)`
  - `vgroup(...)`, `hgroup(...)`, `tgroup(...)`
  - `soundfile(label, chan)`
  - `inputs(expr)`, `outputs(expr)`, `ondemand(expr)`, `upsampling(expr)`, `downsampling(expr)`
- Extended parser actions (`crates/parser-proto/src/lib.rs`):
  - added `box_lambda(...)` helper delegating to `boxes::build_box_abstr(...)`.
- Added/extended tests:
  - `crates/parser-proto/tests/parser_slice9_lambda_groups.rs`
    - lambda nesting shape,
    - UI group + soundfile forms,
    - stream wrapper forms.
  - `crates/boxes/tests/core_api.rs`
    - lambda/group/soundfile/wrapper constructor roundtrip.

### Gate B remaining step 8 (differential suite expansion: lambda/groups/wrappers)

- Extended differential harness (`crates/parser-proto/tests/cpp_differential.rs`) with:
  - `lambda_identity`
  - `vgroup_basic`
  - `stream_wrappers`
- Differential run (C++ source-of-truth root `/Users/letz/Developpements/RUST/faust`, commit `8eebea429`, binary `/usr/local/bin/faust`) passed with no new class mismatch.
- Note:
  - `soundfile(...)` was kept in parser structural tests but not added to stable differential valid cases because standalone `soundfile` examples can fail later semantic/type checks in full C++ compilation depending on channel/range context, which would make a parse-class gate unstable.
- Validation:
  - `cargo test -p boxes --offline --no-fail-fast`
  - `cargo test -p parser-proto --test parser_slice9_lambda_groups --offline --no-fail-fast`
  - `cargo test -p parser-proto --test cpp_differential --offline -- --nocapture`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`

### Gate B remaining step 3 (grammar parity progress: primitive families `prefix/table/select/bounds/control`)

- Extended `boxes` primitive API (`crates/boxes/src/lib.rs`) with C++-aligned constructors/predicates:
  - `box_prefix`
  - `box_read_only_table` (`rdtable`)
  - `box_write_read_table` (`rwtable`)
  - `box_select2`
  - `box_select3`
  - `box_assert_bounds`
  - `box_lowest`
  - `box_highest`
  - `box_attach`
  - `box_enable`
  - `box_control`
  - also added API-level support in `boxes` for cast primitives (`box_int_cast`, `box_float_cast`) for later parser integration.
- Extended parser grammar (`crates/parser-proto/src/grammar/faustparser.y`) with primitive tokens:
  - `prefix`, `rdtable`, `rwtable`, `select2`, `select3`, `assertbounds`, `lowest`, `highest`, `attach`, `enable`, `control`, `pow`.
- Added targeted parser test:
  - `crates/parser-proto/tests/parser_slice10_primitives.rs`
  - validates structural parse support for the extended primitive token family.
- Added/updated `boxes` tests:
  - `crates/boxes/tests/core_api.rs`
  - primitive roundtrip checks now include these constructors and predicates.
- Conflict-resolution note:
  - enabling these zero-argument primitives initially introduced `lrpar` reduce/reduce conflicts due overlap with `LexProbeToken` recovery entries.
  - resolved by removing now-supported zero-argument tokens from `LexProbeToken` so recovery no longer competes with valid expression reductions.
  - parser strict conflict gate restored to `0` unresolved conflicts.

### Gate B remaining step 8 (differential suite expansion: primitive family cases)

- Extended differential harness (`crates/parser-proto/tests/cpp_differential.rs`) with stable C++-accepted primitive cases:
  - `prefix_primitive`
  - `rdtable_primitive`
  - `rwtable_primitive`
  - `select2_primitive`
  - `select3_primitive`
  - `lowest_primitive`
  - `highest_primitive`
  - `attach_primitive`
  - `enable_primitive`
  - `control_primitive`
- Differential run (C++ source-of-truth root `/Users/letz/Developpements/RUST/faust`, commit `8eebea429`, binary `/usr/local/bin/faust`) passed with no class mismatches.
- Note:
  - `assertbounds` is covered in parser structural tests but not included in stable "valid" differential cases because C++ runtime/transform behavior can assert/fail depending on downstream constraints, which makes it unsuitable for a parse-class stability gate.
  - parser-level cast primitives (`int`/`float`) are left for a dedicated follow-up slice because they overlap with foreign-signature type tokens and require a conflict-free grammar refactor under strict parser gates.
- Validation:
  - `cargo test -p boxes --offline --no-fail-fast`
  - `cargo test -p parser-proto --test parser_slice10_primitives --offline --no-fail-fast`
  - `cargo test -p parser-proto --test cpp_differential --offline -- --nocapture`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`

### Gate B remaining step 3 (grammar parity progress: parser-level cast primitives `int`/`float`)

- Reintroduced parser-level cast primitives in `Primitive`:
  - `INTCAST` -> `boxes::box_int_cast(...)`
  - `FLOATCAST` -> `boxes::box_float_cast(...)`
  - file: `crates/parser-proto/src/grammar/faustparser.y`
- Kept foreign-signature type parsing unchanged (`Type`/`ArgType`), but resolved strict parser conflicts by removing now-supported cast tokens from `LexProbeToken` recovery alternatives:
  - removed `INTCAST` and `FLOATCAST` from `LexProbeToken`.
- Result:
  - strict parser generation remains conflict-free under Gate B strict settings (`0` unresolved shift/reduce or reduce/reduce conflicts).
- Extended tests:
  - `crates/parser-proto/tests/parser_slice10_primitives.rs`
    - primitive matrix now includes `int` and `float` cast tokens in parsed expression coverage.

### Gate B remaining step 8 (differential suite expansion: cast primitive cases)

- Extended differential harness (`crates/parser-proto/tests/cpp_differential.rs`) with:
  - `int_cast_primitive`: `process = _ : int;`
  - `float_cast_primitive`: `process = _ : float;`
- Differential run (C++ source-of-truth root `/Users/letz/Developpements/RUST/faust`, commit `8eebea429`, binary `/usr/local/bin/faust`) passed:
  - both new cast cases classify as valid on Rust and C++ (`Ok/Ok`).
- Validation:
  - `cargo test -p parser-proto --test parser_slice10_primitives --offline --no-fail-fast`
  - `cargo test -p parser-proto --test cpp_differential --offline -- --nocapture`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`

### Gate B remaining step 5 (malformed diagnostics/recovery parity envelope)

- Added dedicated malformed diagnostics suite:
  - `crates/parser-proto/tests/parser_diagnostics.rs`
  - validates, for malformed fixtures:
    - Rust parse class is not `Ok`,
    - parser error/recovery path is reached,
    - parser diagnostic location is present and tied to expected source file + line.
- Added optional C++ envelope cross-check in the same suite:
  - compares malformed class only (`not Ok` on both sides),
  - using C++ source-of-truth binary (`FAUST_CPP_BIN` or `/usr/local/bin/faust`).
- Fixed one diagnostics location parity gap in parser runtime:
  - `crates/parser-proto/src/lib.rs` (`parse_program`):
    - when recording `lrpar` errors into `ParserCtx`, cursor is now updated from the failing lexeme span before emitting the diagnostic.
  - impact:
    - malformed `declare` and other `lrpar`-driven errors now carry the correct file/line in `ParserCtx` diagnostics instead of fallback cursor state.
- Validation:
  - `cargo test -p parser-proto --test parser_diagnostics --offline --no-fail-fast`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`

### Gate B remaining step 4 (semantic action parity mapping + structural corpus)

- Added semantic action mapping artifact for the migrated parser scope:
  - `porting/phases/phase-3-semantic-action-mapping-en.md`
  - includes touched grammar-family mapping: C++ action -> Rust action, mapping status (`1:1`/`adapted`), and linked structural checks.
- Updated parser phase plan to reference the mapping artifact path:
  - `porting/phases/phase-3-parser-en.md`
- Added consolidated semantic parity test corpus:
  - `crates/parser-proto/tests/parser_semantic_parity.rs`
  - covers C++ action-shape formulas and constructor-family mapping across:
    - infix/postfix/unary lowering,
    - application/access and fake-route default shape,
    - scope families (`with`/`letrec`),
    - primitive families (`rdtable`, `int`/`float` cast, `attach`, `control`),
    - module/waveform families,
    - foreign + case/pattern preparation families.
- Added C++ acceptance envelope on the stable semantic corpus in the same test:
  - validates selected structural-corpus fixtures against `/usr/local/bin/faust` (or `FAUST_CPP_BIN`) as source-of-truth compiler behavior.
  - cases known to be structurally valid but unstable for full C++ compilation-stage acceptance are kept in structural tests and excluded from strict acceptance envelope checks.
- Validation:
  - `cargo test -p parser-proto --test parser_semantic_parity --offline --no-fail-fast`
  - `cargo fmt --all`
  - `cargo clippy -p parser-proto --all-targets --offline -- -D warnings`
  - `cargo test -p parser-proto --offline --no-fail-fast`

### Gate B remaining step 7 (optional parser-adjacent modules status: `SourceFetcher` / `Enrobage`)

- Added explicit lifecycle/API status artifact:
  - `porting/phases/phase-3-parser-adjacent-modules-status-en.md`
  - status is now explicit for both modules with rationale, owner, milestone, and validation:
    - `sourcefetcher`: `deferred` to Phase 9 integration (feature-policy + reproducibility constraints),
    - `enrobage`: `deferred` to Phase 9 integration with crate-boundary adaptation toward `compiler`/`codegen`/`doc` orchestration (not parser-core).
- Linked this artifact in parser phase plan:
  - `porting/phases/phase-3-parser-en.md`:
    - sections `3.3` and `3.4`,
    - remaining step `7` mapping artifact path.
- Locked current prototype behavior in tests (no hidden network stub):
  - `crates/parser-proto/tests/source_reader.rs`:
    - added `url_imports_are_unresolved_in_parser_proto_scope`,
    - asserts URL imports are reported as `UnresolvedImport` in current Phase 3 scope.
- Added explicit scope provenance in `SourceReader` Rustdoc:
  - `crates/parser-proto/src/source_reader.rs`.
- Validation:
  - `cargo test -p parser-proto --test source_reader --offline --no-fail-fast`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`

### Gate B remaining step 8 (differential suite expansion: import-heavy file fixtures)

- Extended Rust-vs-C++ differential harness to cover two input modes:
  - inline source cases (`parse_program`),
  - file fixture cases with imports (`parse_file_with_imports`) and explicit search paths.
- Updated `crates/parser-proto/tests/cpp_differential.rs`:
  - introduced fixture-based case model (`CaseInput::FileFixture`) with per-case temp workspace generation,
  - added C++ execution path for file fixtures with `-I` search path propagation.
- Added stable import-heavy differential cases:
  - `import_nested_search_path` (valid):
    - multi-file import chain with `-I` directory and nested local import.
  - `import_missing_search_path` (invalid):
    - unresolved import file path in import-heavy context.
- Differential run remains green against C++ source-of-truth root `/Users/letz/Developpements/RUST/faust` (commit `8eebea429`) and binary `/usr/local/bin/faust`.
- Scope note:
  - full stdlib-wide differential parsing is still pending a later grammar-completeness stage; this step extends import-heavy coverage with stable parser-prototype-compatible fixtures.
- Validation:
  - `cargo test -p parser-proto --test cpp_differential --offline -- --nocapture`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`

### Gate B remaining step 9 (production `parser` integration start: API bridge)

- Replaced `crates/parser` scaffold with production-facing parser API bridge over `parser-proto`:
  - `crates/parser/src/lib.rs`
  - exported API includes:
    - `parse_program`,
    - `parse_file_with_imports`,
    - `parse_minimal`,
    - parser diagnostics/token/source-reader types re-exported for upper-layer integration.
- Added `parser-proto` dependency in:
  - `crates/parser/Cargo.toml`
- Added production parser bridge tests:
  - `crates/parser/tests/api_bridge.rs`
  - validates:
    - minimal parse helper path,
    - direct string parse path,
    - file+import parse path through production `parser` crate.
- Scope note:
  - this is Step 9 integration phase 1 (API replacement of scaffold);
  - compiler orchestration wiring to the production parser crate remains a follow-up sub-step.
- Validation:
  - `cargo test -p parser --offline --no-fail-fast`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`

### Gate B remaining step 9 (production integration phase 2: `compiler` -> `parser` wiring)

- Wired compiler facade parse orchestration to production `parser` APIs:
  - `crates/compiler/src/lib.rs`
  - added:
    - `Compiler::compile_source(...)` -> `parser::parse_program(...)`,
    - `Compiler::compile_file(...)` -> `parser::parse_file_with_imports(...)`.
- Added compiler-stage parser error classification:
  - `CompilerError::{Import, Parse}`,
  - parse failures now include parser recovery/error paths (`parse_error_count` and `recovery_count`) instead of treating recovered roots as success.
- Added compiler integration tests proving parser wiring:
  - `crates/compiler/src/lib.rs` tests:
    - valid source compile success,
    - malformed source compile failure,
    - file+import compile success,
    - missing import compile failure.
- Scope note:
  - this closes Step 9 phase 2 for parser entry-point wiring in `compiler` crate;
  - full end-to-end compiler pipeline integration (post-parse orchestration through later phases) remains out of this step scope.
- Validation:
  - `cargo test -p compiler --offline --no-fail-fast`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`

### Gate B remaining step 9 (production integration phase 3: `compiler` binary parse path)

- Extended production compiler API with default file parse search path behavior:
  - `crates/compiler/src/lib.rs`
  - added `Compiler::compile_file_default(&Path)`:
    - uses input file parent directory as default import search path,
    - delegates to production parser-backed `compile_file(...)`.
- Added dedicated compiler test for this default import behavior:
  - `crates/compiler/src/lib.rs` tests:
    - `compiler_compile_file_default_uses_parent_dir_for_imports`.
- Extended compiler CLI to exercise production parser path directly:
  - `crates/compiler/src/main.rs`
  - added `--parse <input.dsp> [-I <dir> ...]` command:
    - routes to `Compiler::compile_file_default(...)` when no `-I` is provided,
    - routes to `Compiler::compile_file(...)` when import dirs are provided,
    - reports parse summary (`root`, parse error count, recovery count),
    - exits non-zero on parse failure or usage errors.
- Scope note:
  - this closes Step 9 production integration at compiler entry points (library + CLI parse mode) for parser consumption;
  - full end-to-end post-parse compile pipeline integration remains tracked in later phases.
- Validation:
  - `cargo run -p compiler --offline -- --parse tests/corpus/rep_01_passthrough.dsp`
  - `cargo test -p compiler --offline --no-fail-fast`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`

### Gate B remaining step 9 (production integration phase 4: CLI structural box dump)

- Added a first user-facing CLI tool to inspect parser output box structure:
  - `crates/compiler/src/main.rs`
  - new command:
    - `--dump-box <input.dsp> [-I <dir> ...]`
  - behavior:
    - parses through production parser APIs (`Compiler::compile_file_default` / `compile_file`),
    - prints deterministic structural dump via `boxes::dump_box(...)`,
    - returns non-zero on parse failure or invalid usage.
- Refactored parse-related CLI argument handling:
  - introduced shared helper `parse_input_with_import_dirs(...)` for `--parse` and `--dump-box`.
- Added `boxes` dependency to compiler crate:
  - `crates/compiler/Cargo.toml`.
- Scope note:
  - this provides a direct operator tool to inspect produced box trees while parser migration continues.
- Validation:
  - `cargo run -p compiler --offline -- --dump-box tests/corpus/rep_08_branch_and_sum.dsp`
  - `cargo test -p compiler --offline --no-fail-fast`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`

### Parser corpus expansion (`tests/corpus` `rep_11`..`rep_20`)

- Added 10 new parser-focused corpus files:
  - `tests/corpus/rep_11_declare_metadata.dsp`
  - `tests/corpus/rep_12_import_statement.dsp`
  - `tests/corpus/rep_13_case_expression.dsp`
  - `tests/corpus/rep_14_with_local_scope.dsp`
  - `tests/corpus/rep_15_letrec_scope.dsp`
  - `tests/corpus/rep_16_lambda_abstraction.dsp`
  - `tests/corpus/rep_17_ui_groups.dsp`
  - `tests/corpus/rep_18_stream_wrappers.dsp`
  - `tests/corpus/rep_19_primitive_family.dsp`
  - `tests/corpus/rep_20_environment_waveform.dsp`
- Updated parser corpus acceptance test:
  - `crates/parser-proto/tests/parser_slice3.rs`
  - replaced hard-coded `rep_01..rep_10` list with dynamic `rep_*.dsp` discovery (sorted) to keep coverage growing without test rewrites.
- Refreshed golden artifacts for the expanded corpus:
  - Rust reference snapshots:
    - `cargo run -p xtask --offline -- golden-gen-rust`
  - C++ reference snapshots (source of truth binary):
    - `FAUST_CPP_BIN=/usr/local/bin/faust cargo run -p xtask --offline -- golden-gen-cpp`
- Validation:
  - `cargo test -p parser-proto --test parser_slice3 --offline --no-fail-fast`
  - `cargo run -p xtask --offline -- golden-check`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`

### Parser corpus expansion (`tests/corpus` `rep_21`..`rep_30`)

- Added 10 additional valid corpus files (parser + C++ compilable envelope):
  - `tests/corpus/rep_21_operator_precedence.dsp`
  - `tests/corpus/rep_22_parallel_mix.dsp`
  - `tests/corpus/rep_23_feedback_simple.dsp`
  - `tests/corpus/rep_24_case_three_rules.dsp`
  - `tests/corpus/rep_25_with_local_defs.dsp`
  - `tests/corpus/rep_26_letrec_defs.dsp`
  - `tests/corpus/rep_27_lambda_two_args.dsp`
  - `tests/corpus/rep_28_nested_ui_groups.dsp`
  - `tests/corpus/rep_29_stream_wrapper_pair.dsp`
  - `tests/corpus/rep_30_environment_access_pair.dsp`
- Coverage intent:
  - operator precedence and mixed arithmetic composition,
  - split/parallel composition + feedback form,
  - extended `case` shape,
  - local/recursive scopes (`with`/`letrec`),
  - lambda + nested UI grouping,
  - stream wrappers + environment access + waveform.
- Refreshed golden artifacts:
  - `cargo run -p xtask --offline -- golden-gen-rust`
  - `FAUST_CPP_BIN=/usr/local/bin/faust cargo run -p xtask --offline -- golden-gen-cpp`
- Validation:
  - `cargo test -p parser-proto --test parser_slice3 --offline --no-fail-fast`
  - `cargo run -p xtask --offline -- golden-check`

### Gate B remaining step 1 (parity baseline automation: lexer/grammar coverage report)

- Added a new `xtask` command to generate a reproducible parser/lexer parity baseline:
  - `cargo run -p xtask -- parser-parity-report`
  - implementation in `crates/xtask/src/main.rs`.
- New artifact generated from C++ source-of-truth and Rust parser-proto grammar/lexer:
  - `porting/phases/phase-3-parser-parity-report-en.md`
  - compares:
    - parser token declarations (`%token` + precedence token directives),
    - lexer state declarations (`%x`/`%s`),
    - grammar nonterminal coverage (name-based, with explicit alias mapping),
    - parser/lexer internal consistency (declared vs emitted token sets).
- Added explicit reference to this artifact in parser phase plan:
  - `porting/phases/phase-3-parser-en.md` (remaining step 1 coverage artifact path).
- Current generated baseline highlights:
  - parser tokens: unresolved missing `0` after alias mapping (`LISTING`->`BLST`, `VIRG`->`PAR`),
  - lexer states: unresolved missing `0`,
  - nonterminals: unresolved missing `4` (`modentry`, `modlist`, `variant`, `variantlist`), now explicitly tracked.
- Validation:
  - `cargo run -p xtask --offline -- parser-parity-report`
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test --workspace --all-targets --offline --no-fail-fast`

### Parity closure step 1 (`variant`/`variantlist` precision filters)

- Ported C++ `variant`/`variantlist` grammar behavior (`FLOATMODE/DOUBLEMODE/QUADMODE/FIXEDPOINTMODE`) in parser-proto grammar:
  - `crates/parser-proto/src/grammar/faustparser.y`
  - `StmtList` and `DefList` now gate statement/definition insertion through `VariantList`.
- Added C++-aligned precision acceptance logic in parser context:
  - `crates/parser-proto/src/context.rs`
  - `ParserCtx::{set_float_size,float_size,accept_definition}` with default single precision (`gFloatSize=1` equivalent).
- Added parser-state helper:
  - `crates/parser-proto/src/lib.rs`
  - `ParseState::prepend_statement_with_variant(...)`.
- Added focused tests:
  - `crates/parser-proto/tests/parser_ctx.rs`:
    - variant prefix acceptance contract across precision modes.
  - `crates/parser-proto/tests/parser_slice11_variants.rs`:
    - filtering of `doubleprecision`-prefixed definitions in default single mode,
    - acceptance of `singleprecision`-prefixed definitions,
    - filtering behavior inside local definition lists (`with { ... }`).
- Updated parity report baseline:
  - `porting/phases/phase-3-parser-parity-report-en.md`
  - unresolved nonterminals reduced from `4` to `2` (`modentry`, `modlist`).
- Validation:
  - `cargo test -p parser-proto --offline --no-fail-fast`
  - `cargo run -p xtask --offline -- parser-parity-report`
  - `cargo fmt --all`

### Parity closure step 2 (`modentry`/`modlist` + bracket modulation form)

- Ported C++ modulation grammar rules from source of truth:
  - `/Users/letz/Developpements/RUST/faust/compiler/parser/faustparser.y`
  - `modentry`, `modlist`, and `LCROC modlist LAPPLY expression RCROC`.
- Added equivalent Rust grammar coverage:
  - `crates/parser-proto/src/grammar/faustparser.y`
  - new nonterminals `ModEntry` and `ModList`
  - primitive form `[modlist -> expression]`.
- Added parser semantic actions matching C++ `boxModulation` and `buildBoxModulation` behavior:
  - `crates/parser-proto/src/lib.rs`
  - `ParseState::{box_modulation,build_box_modulation}`.
- Added focused parser tests:
  - `crates/parser-proto/tests/parser_slice12_modulation.rs`
  - verifies bracket modulation acceptance and nested-entry order parity (`a` outer, then `b`).
- Updated parity report baseline:
  - `porting/phases/phase-3-parser-parity-report-en.md`
  - unresolved nonterminals reduced from `2` to `0`.
- Validation:
  - `cargo test -p parser-proto --test parser_slice12_modulation --offline --no-fail-fast`
  - `cargo test -p parser-proto --offline --no-fail-fast`
  - `cargo run -p xtask --offline -- parser-parity-report`
  - `cargo fmt --all`

### Parity closure step 3 (move modulation constructors to `boxes`)

- Moved modulation constructors from parser-local implementation to shared `boxes` APIs:
  - `crates/boxes/src/lib.rs`
  - added `box_modulation`, `is_box_modulation`, and `build_box_modulation`.
- Updated parser-proto semantic action to use `boxes` directly:
  - `crates/parser-proto/src/grammar/faustparser.y`
  - modulation form now calls `boxes::build_box_modulation(&mut state.arena, ...)`.
- Removed parser-local modulation constructors:
  - `crates/parser-proto/src/lib.rs`
  - deleted `ParseState::{box_modulation,build_box_modulation}`.
- Added `boxes` unit coverage for modulation parity and nesting order:
  - `crates/boxes/tests/core_api.rs`.
- Validation:
  - `cargo test -p boxes --offline --no-fail-fast`
  - `cargo test -p parser-proto --test parser_slice12_modulation --offline --no-fail-fast`
  - `cargo fmt --all`

### Parity closure step 4 (differential validation extension for modulation/recovery)

- Extended Rust vs C++ differential parser suite with modulation forms:
  - `crates/parser-proto/tests/cpp_differential.rs`
  - added cases:
    - `modulation_single`,
    - `modulation_chain`,
    - `malformed_modulation_missing_rcroc`.
- Extended malformed diagnostics suite with modulation recovery coverage:
  - `crates/parser-proto/tests/parser_diagnostics.rs`
  - added malformed case `modulation_missing_rcroc` (line-1 location check + C++ error envelope class).
- Goal: keep parser parity checks tied to C++ source-of-truth while closing the newly ported modulation path.
- Validation:
  - `cargo test -p parser-proto --test parser_diagnostics --offline --no-fail-fast`
  - `cargo test -p parser-proto --test cpp_differential --offline --no-fail-fast`
  - `cargo fmt --all`

### Parity closure step 5 (report closure wording when unresolved gaps are zero)

- Updated parity-report generator to emit closure-specific next actions when unresolved gaps are fully closed:
  - `crates/xtask/src/main.rs`
  - `parser-parity-report` now reports:
    - explicit zero-gap closure message when unresolved parser-token/lexer-state/nonterminal gaps are `0`,
    - consistency triage action only when parser/lexer declared-vs-emitted mismatches remain.
- Regenerated phase report artifact:
  - `porting/phases/phase-3-parser-parity-report-en.md`.
- Validation:
  - `cargo run -p xtask --offline -- parser-parity-report`
  - `cargo fmt --all`

### Parser corpus expansion (`tests/corpus` `rep_31`..`rep_33`)

- Added 3 parser-focused corpus files for newly ported grammar/actions:
  - `tests/corpus/rep_31_variant_filters.dsp` (`variantlist` precision filtering in top-level and local defs)
  - `tests/corpus/rep_32_modulation_single.dsp` (single-entry bracket modulation)
  - `tests/corpus/rep_33_modulation_chain.dsp` (multi-entry bracket modulation nesting)
- Refreshed Rust golden snapshots to include the new corpus files:
  - `cargo run -p xtask --offline -- golden-gen-rust`
- Validation:
  - `cargo test -p parser-proto --test parser_slice3 --offline --no-fail-fast`
  - `cargo run -p xtask --offline -- golden-check`

### Architecture decision update (canonical IR API style: builder + matcher)

- Recorded and aligned the canonical API direction across porting documents:
  - `boxes`: `BoxBuilder` + `match_box`
  - `signals`: `SigBuilder` + `match_sig`
- Updated documentation files:
  - `porting/phases/phase-2-block-diagrams-en.md`
  - `porting/phases/phase-4-signaux-en.md`
  - `porting/faust-rust-porting-plan-en.md`
  - `AGENTS.md`
- Added explicit migration guidance for `boxes`:
  - keep `box_*` / `is_box_*` as compatibility wrappers during transition,
  - move new read-side dispatch users to canonical `match_box`,
  - require wrapper equivalence tests and standard quality gates.
- Goal:
  - reduce duplicated `is*` ladders across future passes (`eval`/`propagate`/typing/printing),
  - keep dispatch logic centralized and explicit before deep Phase 4 implementation.

### Boxes canonical API step 1 (`BoxBuilder` + `match_box` core tranche)

- Implemented first production tranche in `crates/boxes/src/lib.rs`:
  - added `BoxBuilder<'a>` write-side facade,
  - added `BoxMatch` + `match_box(...)` read-side canonical dispatch.
- Scope covered in this tranche:
  - values: `int`, `real`, `ident`,
  - core composition: `wire`, `cut`, `seq`, `par`, `rec`, `split`, `merge`,
  - functional forms: `appl`, `access`, `abstr`, `modulation`,
  - recursive builders: `build_abstr`, `build_modulation`.
- Kept compatibility API stable:
  - existing `box_*` / `is_box_*` functions remain public and unchanged at call sites,
  - core covered functions now delegate to canonical builder/matcher paths.
- Added dedicated tests in `crates/boxes/tests/core_api.rs`:
  - `canonical_builder_matches_free_function_ids_for_core_tranche`,
  - `match_box_decodes_core_tranche_and_falls_back_to_unknown`.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets --offline -- -D warnings`
  - `cargo test -p boxes --offline --no-fail-fast`
  - `cargo test -p parser-proto --offline --no-fail-fast`

### Boxes canonical API completion (remove public `box_*` / `is_box_*`)

- Completed migration from free-function API to canonical builder/matcher API:
  - `crates/boxes/src/lib.rs`
  - public surface now centered on:
    - `BoxBuilder` (construction),
    - `match_box` + `BoxMatch` (inspection),
    - `dump_box` (structural diagnostics).
- Removed public exports of legacy free functions:
  - `box_*`, `is_box_*`, `ffunction`, `is_ffunction`, `build_box_abstr`, `build_box_modulation`
  - these remain internal implementation details inside `boxes`.
- Migrated parser prototype construction paths to builder API:
  - `crates/parser-proto/src/lib.rs`
  - `crates/parser-proto/src/grammar/faustparser.y`
  - grammar semantic actions now use `state.box_builder().*` constructors.
- Migrated parser-proto tests away from `boxes::is_box_*` helpers:
  - added matcher-based test adapter module:
    - `crates/parser-proto/tests/support/box_match_helpers.rs`
  - updated parser slice/parity tests to consume this adapter.
- Reworked boxes integration tests to canonical API only:
  - `crates/boxes/tests/core_api.rs`
  - no dependency on legacy free-function exports.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy -p boxes -p parser-proto -p parser --all-targets --offline -- -D warnings`
  - `cargo test -p boxes --offline --no-fail-fast`
  - `cargo test -p parser-proto --offline --no-run`
  - `cargo test -p parser --offline --no-run`
  - `cargo test -p boxes -p parser -p parser-proto --offline --no-fail-fast`
    - expected pre-existing failure remains in `parser-proto` differential test:
      `cpp_differential` mismatch on stream-wrapper cases (`rep_18_stream_wrappers.dsp`, `stream_wrappers`)

### `match_box` hot-path benchmark + dispatch optimization

- Added dedicated release benchmark binary:
  - `crates/boxes/src/bin/match_box_bench.rs`
  - run with: `cargo run -p boxes --release --bin match_box_bench`
- Optimized `match_box` dispatch in `crates/boxes/src/lib.rs`:
  - switched from large tuple/slice pattern matching to arity-first dispatch (`children.len()` -> tag match),
  - added single-pass slider parameter decoder `slider_params4(...)` to avoid repeated list traversal for
    `vslider` / `hslider` / `nentry`.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy -p boxes --all-targets --offline -- -D warnings`
  - `cargo test -p boxes --offline --no-fail-fast`
- Benchmark results (same machine/workload):
  - before:
    - `primitives`: `15.09 ns/op` (`66.28 Mops/s`)
    - `sliders`: `22.39 ns/op` (`44.66 Mops/s`)
    - `mixed`: `14.15 ns/op` (`70.65 Mops/s`)
  - after:
    - `primitives`: `12.04 ns/op` (`83.09 Mops/s`) -> `~1.25x`
    - `sliders`: `21.95 ns/op` (`45.55 Mops/s`) -> `~1.02x`
    - `mixed`: `13.38 ns/op` (`74.76 Mops/s`) -> `~1.06x`

### `match_box` dispatch experiment (`tag_id/u32`) and decision

- Investigated a direct `tag_id` (`u32`) dispatch variant in `crates/boxes/src/lib.rs` to reduce
  dependence on string tag comparisons.
- Two variants were prototyped and benchmarked with
  `cargo run -p boxes --release --bin match_box_bench`:
  - `tag_id` decode + per-arena/per-tag cache:
    - `primitives`: `33.69 ns/op`
    - `sliders`: `30.01 ns/op`
    - `mixed`: `23.84 ns/op`
  - `tag_id` decode without cache:
    - `primitives`: `16.83 ns/op`
    - `sliders`: `20.99 ns/op`
    - `mixed`: `16.19 ns/op`
- Reference retained implementation (current):
  - `primitives`: `12.04 ns/op`
  - `sliders`: `21.95 ns/op`
  - `mixed`: `13.38 ns/op`
- Decision:
  - keep the current arity-first + tag-name matching implementation,
  - do not merge the `tag_id` dispatch prototype in this state because it regresses hot paths
    (`primitives`, `mixed`) despite slight slider gain.

### Phase 4 start (`signals` canonical API: `SigBuilder` / `SigMatch` / `match_sig`)

- Implemented first production tranche for `crates/signals` (previously scaffold-only):
  - `crates/signals/src/lib.rs`
  - `crates/signals/Cargo.toml` (adds dependency on `tlib`)
- Added Rustdoc provenance and invariants aligned with C++ source of truth:
  - `/Users/letz/Developpements/RUST/faust/compiler/signals/signals.hh`
  - `/Users/letz/Developpements/RUST/faust/compiler/signals/signals.cpp`
  - `/Users/letz/Developpements/RUST/faust/compiler/signals/binop.hh`
- Added canonical signal write/read APIs:
  - `SigBuilder` constructors for constants, I/O, delays, casts, tables, selectors, binops,
    foreign symbols, recursion (`rec`/`proj`), UI items, wrappers (`attach/enable/control`),
    waveform/soundfile, stream wrappers (`od/us/ds`), sequence/zeropad.
  - `BinOp` enum aligned to C++ `SOperator` integer mapping.
  - `SigMatch` + `match_sig(...)` exhaustive decoding for this tranche.
  - `dump_sig(...)` deterministic structural dump helper.
- Added integration tests:
  - `crates/signals/tests/core_api.rs`
  - coverage includes core shapes, binop decode, `select3` composition shape, slider list payload
    order, wrapper/recursion forms, dump determinism.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy -p signals --all-targets --offline -- -D warnings`
  - `cargo test -p signals --offline --no-fail-fast`

### Documentation policy update: `clap` as default CLI parser

- Updated governance docs so CLI parsing policy is explicit and consistent:
  - `AGENTS.md`: `clap` is the default parser for user-facing binaries.
  - `porting/faust-rust-porting-plan-en.md`: added a dedicated CLI parsing policy section.
  - `porting/phases/phase-9-integration-en.md`: dependency list now states `clap` as default, with documented-justification fallback for alternatives.

### Phase 4 / 2.2 eval first implementation tranche

- Replaced `crates/eval` scaffold with a first functional evaluator core:
  - `Environment` with lexical scope chaining (`empty`, `bind`, `lookup`, `push_scope`),
  - `LoopDetector` with cycle and max-depth guards,
  - `EvalError` typed error surface (`MissingProcessDefinition`, `UndefinedSymbol`, malformed defs, loop/depth).
- Added first production APIs:
  - `eval_process(arena, definitions)`:
    - decodes parser definition list shape `cons(name, cons(args, expr))`,
    - builds the top environment,
    - resolves and evaluates `process`.
  - `eval_box(arena, expr, env, loop_detector)`:
    - resolves `BOXIDENT` through environment bindings,
    - handles lexical scoping for `BOXWITHLOCALDEF`, `BOXWITHRECDEF`, and `BOXABSTR`,
    - recursively maps/evaluates children for all other box nodes.
- Added crate tests:
  - `crates/eval/tests/core_eval.rs`
  - coverage:
    - named-definition resolution (`process -> foo -> BOXWIRE`),
    - `with {}` local-scope resolution,
    - missing-`process` error,
    - recursive loop detection (`process <-> foo`).
- Validation:
  - `cargo fmt --all`
  - `cargo clippy -p eval --all-targets -- -D warnings`
  - `cargo test -p eval --all-targets`
  - `cargo test --workspace --all-targets`

### Phase 4 / 2.2 eval second implementation tranche (application + iterations)

- Extended `crates/eval/src/lib.rs` with C++-aligned evaluation paths:
  - `BOXAPPL` evaluation with:
    - evaluated argument list reversal (`revEvalList` behavior),
    - abstraction application (`applyList` behavior),
    - non-closure fallback lowering to `BOXSEQ(larg2par(args), fun)`.
  - Iterative forms:
    - `BOXIPAR` -> parallel expansion,
    - `BOXISEQ` -> sequential expansion,
    - `BOXISUM` -> chained `BOXADD` reductions,
    - `BOXIPROD` -> chained `BOXMUL` reductions.
  - Added evaluator helpers and stricter typed errors for malformed list/application/iteration cases.
- Kept abstraction building for parser-style reversed parameter lists aligned with C++ `buildBoxAbstr` semantics in eval path (`bind_definitions`).
- Added tests in `crates/eval/tests/core_eval.rs`:
  - function application argument order (C++ parity intent),
  - non-closure application fallback shape,
  - `ipar` index binding expansion,
  - `isum` additive chain construction.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy -p eval --all-targets -- -D warnings`
  - `cargo test -p eval --all-targets`
  - `cargo test --workspace --all-targets`

### Phase 4 / 2.2-2.3 eval third implementation tranche (`case` / pattern matching)

- Extended `crates/eval/src/lib.rs` with first pattern-matching execution path:
  - `apply_list` now handles `BoxMatch::Case(rules)` directly.
  - Case rules are interpreted with parser/C++ list-order parity:
    - rules and rule-pattern lists are reversed back to source order before matching.
  - Implemented structural matcher with `BoxMatch::PatternVar` bindings:
    - repeated pattern variables must match the same value,
    - recursive structural checks for non-variable subtrees.
  - Added explicit case errors:
    - malformed case/rule shapes,
    - arity mismatch (`PatternArityMismatch`),
    - no matching rule (`PatternMatchFailed`).
- Kept `BoxMatch::Case` and `BoxMatch::PatternVar` stable under evaluation (`eval_box`) so
  pattern nodes are not incorrectly resolved as plain identifiers.
- Added/extended eval tests in `crates/eval/tests/core_eval.rs`:
  - source-rule priority despite parser reverse list encoding,
  - pattern-variable binding (`(x) => x`),
  - arity mismatch diagnostics,
  - no-match diagnostics.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy -p eval --all-targets -- -D warnings`
  - `cargo test -p eval --all-targets`
  - `cargo test --workspace --all-targets`

## 2026-02-16

### Phase 4 / 2.4 propagate first implementation tranche (`boxes` -> `signals`)

- Replaced `crates/propagate` scaffold with a first functional propagation layer:
  - `crates/propagate/src/lib.rs`
  - `crates/propagate/Cargo.toml` (adds `boxes`/`signals`/`tlib` dependencies)
- Added Rustdoc provenance and scope notes aligned with C++ source-of-truth:
  - `/Users/letz/Developpements/RUST/faust/compiler/propagate/propagate.hh`
  - `/Users/letz/Developpements/RUST/faust/compiler/propagate/propagate.cpp`
  - `/Users/letz/Developpements/RUST/faust/compiler/boxes/boxtype.cpp`
- Added first production APIs:
  - `make_sig_input_list(arena, n)` -> canonical `sigInput(i)` vector.
  - `box_arity(arena, box_tree)` -> typed box arity inference (`BoxArity`) for supported families.
  - `propagate(arena, box_tree, inputs)` -> typed propagation with I/O arity validation.
- Implemented propagation support for:
  - constants and wire/cut (`int`, `real`, `_`, `!`),
  - primitive lowering subset (`add/sub/mul/div/rem/logic/shifts/comparisons`, `delay/delay1/prefix`,
    `int/float cast`, `table/select/assert/lowest/highest`, `attach/enable/control`),
  - UI subset (`button`, `checkbox`, sliders, bargraphs),
  - foreign constants/variables (`fconst`, `fvar`),
  - composition algebra subset (`seq`, `par`, `split`, `merge`),
  - arity-introspection wrappers (`inputs`, `outputs`), plus `environment`.
- Added explicit typed diagnostics in `PropagateError`:
  - unsupported box families,
  - input/output arity mismatches,
  - composition coherence mismatches (`seq`/`split`/`merge`/`rec`),
  - integer payload validation errors.
- Added crate tests:
  - `crates/propagate/tests/core_api.rs`
  - coverage:
    - input signal list generation,
    - primitive lowering (`+`),
    - `seq/par/split` composition behavior,
    - `merge` bus mixing behavior,
    - mismatch and unsupported diagnostics,
    - `inputs(...)` / `outputs(...)` lowering to signal integers.
- Current explicit limitation kept intentional:
  - `rec` propagation execution path is still rejected as `UnsupportedBox` (arity inference exists),
    pending dedicated recursion group semantics port (`sigRec/sigProj` group handling parity).
- Validation:
  - `cargo fmt --all`
  - `cargo clippy -p propagate --all-targets -- -D warnings`
  - `cargo test -p propagate --all-targets`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`

### Phase 4 / 2.4 propagate second implementation tranche (`rec` support with de-Bruijn refs)

- Extended `crates/propagate/src/lib.rs` to execute `BoxMatch::Rec(left, right)` instead of returning
  `UnsupportedBox`.
- Ported the C++ recursion propagation skeleton from:
  - `/Users/letz/Developpements/RUST/faust/compiler/propagate/propagate.cpp` (`isBoxRec` branch)
  - `/Users/letz/Developpements/RUST/faust/compiler/tlib/recursive-tree.cpp` (`rec/ref/liftn/aperture` model)
- Added internal helpers in `propagate` for recursive group plumbing:
  - de-Bruijn nodes:
    - `DEBRUIJN` (recursive group wrapper),
    - `DEBRUIJNREF(level)` (recursive reference placeholder),
  - `make_mem_sig_proj_list(...)` (`delay1(proj(i, DEBRUIJNREF(1)))` seeds),
  - `liftn(...)` (minimal free-ref lifting on propagated inputs),
  - `aperture(...)` (minimal free-ref depth analysis used to keep closed branches out of projected outputs).
- Rec propagation behavior now implemented:
  - compute recursive seed list for right branch inputs,
  - propagate right branch, then left branch with `right_outputs + lifted_inputs`,
  - build recursive group as `DEBRUIJN(list(left_outputs))`,
  - output `proj(i, group)` only for branches with positive aperture;
    closed branches remain as direct expressions (C++ parity intent).
- Updated tests in `crates/propagate/tests/core_api.rs`:
  - former `rec unsupported` assertion replaced by positive execution checks,
  - added `+ ~ _` structure test (`proj` output + expected `DEBRUIJNREF(1)` seed path),
  - added mixed recursion test verifying closed branch passthrough and projected recursive branch.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy -p propagate --all-targets -- -D warnings`
  - `cargo test -p propagate --all-targets`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`

### Phase 4 integration step 1 (`compiler` wires `parse -> eval -> propagate`)

- Extended `crates/compiler` to expose a first full signal pipeline API:
  - added `SignalCompileOutput` (`parse`, `process_box`, `process_arity`, `signals`)
  - added:
    - `compile_source_to_signals(...)`
    - `compile_file_to_signals(...)`
    - `compile_file_default_to_signals(...)`
  - internal flow:
    - parse through production `parser`,
    - evaluate `process` via `eval::eval_process`,
    - infer arity + create canonical inputs + propagate via `propagate`.
- Extended compiler error surface:
  - `MissingRoot`
  - `Eval(eval::EvalError)`
  - `Propagate(propagate::PropagateError)`
- Added CLI integration in `crates/compiler/src/main.rs`:
  - new command:
    - `cargo run -p compiler -- --dump-sig <input.dsp> [-I <dir> ...]`
  - output prints inferred process arity and one dumped signal per output.
- Added compiler-level tests (`crates/compiler/src/lib.rs`):
  - pass-through (`process = _;`) signal pipeline,
  - recursive process (`process = + ~ _;`) signal pipeline,
  - missing `process` evaluation error mapping.
- Updated crate dependencies:
  - `crates/compiler/Cargo.toml` now depends on `eval`, `propagate`, `signals`.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy -p compiler --all-targets -- -D warnings`
  - `cargo test -p compiler --all-targets`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`

### Phase 4 integration step 2 (compiler-level corpus signal integration tests)

- Added compiler integration tests exercising the full `parse -> eval -> propagate` pipeline on
  real corpus files:
  - `crates/compiler/tests/signal_pipeline.rs`
- Covered corpus cases (currently stable under implemented propagation subset):
  - `rep_01_passthrough.dsp` (direct `sigInput` passthrough),
  - `rep_02_gain_bias.dsp` (add/mul/constant lowering shape),
  - `rep_21_operator_precedence.dsp` (structural precedence lowering),
  - `rep_23_feedback_simple.dsp` (recursive projection output).
- Added compiler test-only dependency:
  - `crates/compiler/Cargo.toml` -> `[dev-dependencies] tlib` for arena-level structural assertions.
- Notes:
  - initial integration attempts on `rep_10_two_in_two_out_ui.dsp` and `rep_22_parallel_mix.dsp`
    exposed unsupported sequential arity forms in current propagation subset; these remain tracked
    for later propagation coverage expansion.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy -p compiler --all-targets -- -D warnings`
  - `cargo test -p compiler --all-targets`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`

### Phase 4 integration step 3 (Rust vs C++ differential status for signal pipeline)

- Added differential integration test:
  - `crates/compiler/tests/cpp_signal_differential.rs`
- Differential scope (current supported subset):
  - valid corpus files:
    - `rep_01_passthrough.dsp`
    - `rep_02_gain_bias.dsp`
    - `rep_21_operator_precedence.dsp`
    - `rep_23_feedback_simple.dsp`
  - malformed inline case:
    - `process = ;`
- Differential policy:
  - compare Rust signal pipeline status (`compile_*_to_signals`) vs C++ status using
    `faust -norm`,
  - robust C++ classification handles `-norm` non-zero exit codes when normal-form dump
    succeeds (`Dump normal form finished...`),
  - skip test when no C++ compiler is available (`FAUST_CPP_BIN` unset and no `/usr/local/bin/faust`).
- Last local differential run (with C++ source-of-truth `/Users/letz/Developpements/RUST/faust`
  at commit `8eebea429` and binary `/usr/local/bin/faust`):
  - `rep_01_passthrough`: `rust=Ok`, `cpp=Ok`
  - `rep_02_gain_bias`: `rust=Ok`, `cpp=Ok`
  - `rep_21_operator_precedence`: `rust=Ok`, `cpp=Ok`
  - `rep_23_feedback_simple`: `rust=Ok`, `cpp=Ok`
  - `malformed_missing_rhs`: `rust=Error`, `cpp=Error`
- Unresolved gap list (outside this differential subset, tracked for next propagation slices):
  - sequential arity forms currently rejected in Rust pipeline for:
    - `tests/corpus/rep_10_two_in_two_out_ui.dsp`
    - `tests/corpus/rep_22_parallel_mix.dsp`
  - this indicates remaining `propagate` coverage work for additional composition shapes
    used after eval lowering.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy -p compiler --all-targets -- -D warnings`
  - `cargo test -p compiler --all-targets`
  - `cargo test -p compiler --test cpp_signal_differential -- --nocapture`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`

### Phase 4 / 2.2 eval fourth implementation tranche (non-closure partial application parity)

- Ported C++ `applyList` non-closure behavior from source-of-truth:
  - `/Users/letz/Developpements/RUST/faust/compiler/evaluate/eval.cpp`
- Extended `crates/eval/src/lib.rs` non-closure application path:
  - infer function/input arity for fallback apply,
  - infer evaluated argument list output arity,
  - reject over-application with explicit error:
    - `EvalError::TooManyArguments { expected, got }`,
  - when arguments are missing, synthesize wire placeholders (`_`) and inject them with C++ parity:
    - binary primitive with one arg (`prim2`) except `prefix`: prepend missing wire,
    - other partial applications: append missing wire(s),
  - keep final lowering shape:
    - `BOXSEQ(larg2par(adjusted_args), fun)`.
- Added eval tests in `crates/eval/tests/core_eval.rs`:
  - partial binary primitive (`*(0.5)`) inserts leading wire,
  - partial `prefix(0)` inserts trailing wire,
  - over-application (`+(1,2,3)`) reports `TooManyArguments`.
- This unblocks previously failing pipeline forms that depend on partial primitive application in eval:
  - `tests/corpus/rep_10_two_in_two_out_ui.dsp`
  - `tests/corpus/rep_22_parallel_mix.dsp`
- Validation:
  - `cargo fmt --all`
  - `cargo clippy -p eval --all-targets -- -D warnings`
  - `cargo test -p eval --all-targets`
  - `cargo run -p compiler -- --dump-sig tests/corpus/rep_10_two_in_two_out_ui.dsp`
  - `cargo run -p compiler -- --dump-sig tests/corpus/rep_22_parallel_mix.dsp`

### Phase 4 integration step 4 (closure of `rep_10` / `rep_22` signal-pipeline validation)

- Extended compiler signal integration tests (`crates/compiler/tests/signal_pipeline.rs`) with:
  - `rep_10_two_in_two_out_ui.dsp`:
    - asserts arity (`2 -> 2`) and output shape (`mul(input, hslider)` on each channel),
  - `rep_22_parallel_mix.dsp`:
    - asserts arity (`1 -> 1`) and output shape (`add(mul(input,const), mul(input,const))`).
- Extended Rust vs C++ differential status test (`crates/compiler/tests/cpp_signal_differential.rs`) with:
  - `rep_10_two_in_two_out_ui.dsp`
  - `rep_22_parallel_mix.dsp`
- Result:
  - the previously tracked unresolved Phase 4 differential subset gap on `rep_10` / `rep_22` is now closed and CI-visible.
- Validation:
  - `cargo test -p compiler --test signal_pipeline`
  - `cargo test -p compiler --test cpp_signal_differential -- --nocapture`

### Phase 4 integration step 5 (`rep_20_environment_waveform` end-to-end closure)

- Extended `eval` environment-access behavior in `crates/eval/src/lib.rs`:
  - added `BoxMatch::Access(body, field)` handling that resolves environment local bindings for:
    - `with { ... }` environments (`BOXWITHLOCALDEF(BOXENVIRONMENT, defs)`),
    - `letrec` environments (`BOXWITHRECDEF(BOXENVIRONMENT, ...)`),
  - this ports the practical C++ `eval.cpp` access-to-closure environment lookup behavior for the
    currently used corpus shape.
- Added eval test:
  - `crates/eval/tests/core_eval.rs`:
    - `eval_box_access_reads_environment_local_binding`.
- Extended `propagate` waveform support in `crates/propagate/src/lib.rs`:
  - `BoxMatch::Waveform(list)` now has arity `(0 -> 2)` (size + waveform),
  - propagation lowers waveform to:
    - first output: `int(len(values))`,
    - second output: `SIGWAVEFORM(values...)`.
- Added propagate test:
  - `crates/propagate/tests/core_api.rs`:
    - `waveform_box_lowers_to_size_and_waveform_signal`.
- Extended compiler integration coverage:
  - `crates/compiler/tests/signal_pipeline.rs`:
    - added `rep_20_environment_waveform.dsp` shape/arity assertions (`1 -> 3`).
  - `crates/compiler/tests/cpp_signal_differential.rs`:
    - added `rep_20_environment_waveform.dsp` in Rust vs C++ status set.
- Differential status (C++ source-of-truth `/Users/letz/Developpements/RUST/faust` @ `8eebea429`):
  - `rep_20_environment_waveform`: `rust=Ok`, `cpp=Ok`.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p eval --all-targets`
  - `cargo test -p propagate --all-targets`
  - `cargo test -p compiler --test signal_pipeline`
  - `cargo test -p compiler --test cpp_signal_differential -- --nocapture`
  - `cargo run -p compiler -- --dump-sig tests/corpus/rep_20_environment_waveform.dsp`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`

### Phase 4 integration step 6 (`pow/min/max` signal support closure for `rep_07` and `rep_19`)

- Source of truth checked in C++:
  - `/Users/letz/Developpements/RUST/faust/compiler/signals/signals.hh`
  - `/Users/letz/Developpements/RUST/faust/compiler/signals/signals.cpp`
  - (`sigPow`, `sigMin`, `sigMax` exposed as signal constructors via extended math).
- Extended `crates/signals/src/lib.rs`:
  - added dedicated signal node families:
    - `SIGPOW`, `SIGMIN`, `SIGMAX`,
  - added `SigBuilder` constructors:
    - `pow(x, y)`, `min(x, y)`, `max(x, y)`,
  - extended `SigMatch` + `match_sig` with:
    - `Pow`, `Min`, `Max`.
- Extended `crates/propagate/src/lib.rs`:
  - `BoxMatch::Pow/Min/Max` now lower to signal nodes instead of `UnsupportedBox`.
- Added tests:
  - `crates/signals/tests/core_api.rs`:
    - builder/matcher coverage for `Pow/Min/Max`,
  - `crates/propagate/tests/core_api.rs`:
    - `propagate_pow_min_max_map_to_signal_nodes`,
  - `crates/compiler/tests/signal_pipeline.rs`:
    - `rep_07_nonlinear_clip.dsp` (`max(min(...))` shape),
    - `rep_19_primitive_family.dsp` (contains `Pow` output),
  - `crates/compiler/tests/cpp_signal_differential.rs`:
    - added `rep_07_nonlinear_clip.dsp`,
    - added `rep_19_primitive_family.dsp`.
- Differential status (C++ source-of-truth `/Users/letz/Developpements/RUST/faust` @ `8eebea429`):
  - `rep_07_nonlinear_clip`: `rust=Ok`, `cpp=Ok`,
  - `rep_19_primitive_family`: `rust=Ok`, `cpp=Ok`.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p signals --all-targets`
  - `cargo test -p propagate --all-targets`
  - `cargo test -p compiler --test signal_pipeline`
  - `cargo test -p compiler --test cpp_signal_differential -- --nocapture`
  - `cargo run -p compiler -- --dump-sig tests/corpus/rep_07_nonlinear_clip.dsp`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`

### Phase 4 integration step 7 (full parser/eval/propagate closure for extended math primitives)

- Source of truth checked in C++:
  - `/Users/letz/Developpements/RUST/faust/compiler/parser/faustparser.y`
  - `/Users/letz/Developpements/RUST/faust/compiler/parser/faustlexer.l`
  - `/Users/letz/Developpements/RUST/faust/compiler/signals/signals.hh`
  - `/Users/letz/Developpements/RUST/faust/compiler/signals/signals.cpp`
- Extended `crates/boxes/src/lib.rs` with the full parser-exposed extended primitive family:
  - unary: `acos`, `asin`, `atan`, `cos`, `sin`, `tan`, `exp`, `log`, `log10`, `sqrt`, `abs`, `floor`, `ceil`, `rint`, `round`,
  - binary: `atan2`, `fmod`, `remainder`,
  - plus existing `pow/min/max`.
- Extended `crates/parser-proto/src/grammar/faustparser.y` primitive lowering:
  - added semantic actions for `ACOS/ASIN/ATAN/ATAN2/COS/SIN/TAN/EXP/LOG/LOG10/SQRT/ABS/FMOD/REMAINDER/FLOOR/CEIL/RINT/ROUND`,
  - removed these now-supported tokens from `LexProbeToken` recovery branch to avoid parser conflicts.
- Extended `crates/signals/src/lib.rs`:
  - added dedicated signal node families and `SigBuilder`/`SigMatch` support for all extended primitives above.
- Extended `crates/propagate/src/lib.rs`:
  - full box-arity + lowering support to the new signal nodes for all unary/binary extended primitives.
- Extended `crates/eval/src/lib.rs` arity model:
  - added new extended primitive arities in non-closure application inference,
  - added binary extended primitives in implicit-wire partial application classification.
- Added/extended tests:
  - `crates/boxes/tests/core_api.rs`: primitive builder/matcher coverage now includes full extended family.
  - `crates/parser-proto/tests/parser_slice10_primitives.rs` + `tests/support/node_match_helpers.rs`:
    - parser primitive matrix now checks all extended tokens map to expected box nodes.
  - `crates/signals/tests/core_api.rs`: builder/matcher coverage for all extended signal nodes.
  - `crates/propagate/tests/core_api.rs`:
    - `propagate_extended_math_primitives_map_to_signal_nodes`.
  - `crates/compiler/tests/signal_pipeline.rs`:
    - new integration test `corpus_extended_primitives_cover_unary_and_binary_signal_nodes`.
  - `crates/compiler/tests/cpp_signal_differential.rs`:
    - added `rep_31_extended_primitives.dsp`.
  - new corpus fixture:
    - `tests/corpus/rep_31_extended_primitives.dsp`.
- Differential status (C++ source-of-truth `/Users/letz/Developpements/RUST/faust` @ `8eebea429`):
  - `rep_31_extended_primitives`: `rust=Ok`, `cpp=Ok`.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p boxes --all-targets`
  - `cargo test -p signals --all-targets`
  - `cargo test -p propagate --all-targets`
  - `cargo test -p parser-proto --all-targets`
  - `cargo test -p compiler --test signal_pipeline`
  - `cargo test -p compiler --test cpp_signal_differential -- --nocapture`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`

### Diagnostics model rollout (steps 1 to 8, structured error reporting)

- Objective:
  - start implementing the structured diagnostics model planned in porting docs,
  - integrate it incrementally in existing code (`errors` -> `parser` -> `compiler`) without breaking current pipeline behavior.

#### Step 1 — core diagnostics model in `errors` crate

- Commit: `e0a6488`
- Files:
  - `crates/errors/src/lib.rs`
- Implemented:
  - core types: `Severity`, `Stage`, `DiagnosticCode`, `SourceSpan`, `LabelStyle`, `Label`, `Diagnostic`, `DiagnosticBundle`,
  - conversion trait: `IntoDiagnostic`,
  - builder-style helpers on `Diagnostic` (`with_label`, `with_note`, `with_help`),
  - initial unit tests for payload integrity and `error_count`.
- Validation:
  - `cargo test -p errors`

#### Step 2 — stable diagnostics code taxonomy

- Commit: `44ac67b`
- Files:
  - `crates/errors/src/codes.rs` (new),
  - `crates/errors/src/lib.rs` (module export).
- Implemented:
  - stable code families for current scope:
    - `FRS-SRC-*`, `FRS-LEX-*`, `FRS-PARSE-*`, `FRS-EVAL-*`, `FRS-PROP-*`, `FRS-COMP-*`,
  - `all_codes()` listing for cross-checking,
  - tests for code format and uniqueness.
- Validation:
  - `cargo test -p errors`

#### Step 3 — parser structured diagnostics bundle

- Commit: `d834eee`
- Files:
  - `crates/parser-proto/Cargo.toml`,
  - `crates/parser-proto/src/lib.rs`,
  - `crates/parser-proto/tests/parser_diagnostics.rs`,
  - `Cargo.lock`.
- Implemented:
  - `ParseOutput` now carries `diagnostics: errors::DiagnosticBundle`,
  - parser diagnostics mapped to structured diagnostics with:
    - `Stage::Parser`,
    - stable `FRS-PARSE-*` code family (`unexpected`, `recovery`, `invalid literal`),
  - compatibility kept:
    - `ParseOutput.errors: Vec<String>` still present as temporary compatibility channel,
  - parser diagnostics tests now assert structured diagnostics/code-family presence.
- Validation:
  - `cargo test -p parser-proto`
  - `cargo test -p parser`
  - `cargo test -p compiler`

#### Step 4 — precise parser span propagation (line/column/range)

- Commit: `1594d48`
- Files:
  - `crates/parser-proto/src/context.rs`,
  - `crates/parser-proto/src/lib.rs`,
  - `crates/parser-proto/tests/parser_diagnostics.rs`.
- Implemented:
  - `SourceLocation` enriched with:
    - `col`, `end_line`, `end_col`,
  - parser cursor setters extended:
    - `set_cursor_with_col`, `set_cursor_span`,
  - `lrpar` spans now propagated to structured diagnostics labels (`SourceSpan`) with real ranges,
  - diagnostic tests now assert range consistency.
- Validation:
  - `cargo test -p parser-proto`
  - `cargo test -p parser`
  - `cargo test -p compiler`

#### Step 5 — compiler facade + CLI surfacing of structured parse diagnostics

- Commit: `2152747`
- Files:
  - `crates/compiler/src/lib.rs`,
  - `crates/compiler/src/main.rs`.
- Implemented:
  - `CompilerError::Parse` now carries `diagnostics: DiagnosticBundle`,
  - `CompilerError::diagnostics()` accessor added,
  - parse failure path preserves parser diagnostics (not counters only),
  - CLI parse-failure output now includes structured diagnostics lines:
    - `file:line:col severity [code] message`.
- Validation:
  - `cargo test -p compiler`

#### Step 6 — `eval` diagnostics conversion (`EvalError` -> structured diagnostics)

- Commit: `cb9e513`
- Files:
  - `crates/eval/Cargo.toml`,
  - `crates/eval/src/lib.rs`,
  - `crates/eval/tests/core_eval.rs`,
  - `crates/errors/src/codes.rs`.
- Implemented:
  - `eval` now depends on `errors` and implements:
    - `impl IntoDiagnostic for EvalError`,
  - stable code mapping in eval diagnostics:
    - `FRS-EVAL-0001` (`EVAL_MISSING_PROCESS`),
    - `FRS-EVAL-0002` (`EVAL_UNDEFINED_SYMBOL`),
    - `FRS-EVAL-0003` (`EVAL_ARITY_MISMATCH`),
    - `FRS-EVAL-0004` (`EVAL_ITERATION_INVALID`),
    - `FRS-EVAL-0099` (`EVAL_GENERIC_FAILURE`) for remaining variants,
  - contextual note/help entries added for common classes (missing process, symbol, arity, iteration),
  - Rustdoc added on conversion semantics in `eval` (`IntoDiagnostic` impl block),
  - new eval test coverage:
    - `eval_error_converts_to_structured_diagnostic_codes`.
- Validation:
  - `cargo test -p errors`
  - `cargo test -p eval`
  - `cargo test -p compiler`

#### Step 7 — `propagate` diagnostics conversion (`PropagateError` -> structured diagnostics)

- Commit: `17fc686`
- Files:
  - `crates/propagate/Cargo.toml`,
  - `crates/propagate/src/lib.rs`,
  - `crates/propagate/tests/core_api.rs`,
  - `crates/errors/src/codes.rs`.
- Implemented:
  - `propagate` now depends on `errors` and implements:
    - `impl IntoDiagnostic for PropagateError`,
  - stable code mapping in propagate diagnostics:
    - `FRS-PROP-0001` (`PROP_UNSUPPORTED_BOX`),
    - `FRS-PROP-0002` (`PROP_ARITY_MISMATCH`) for input/output/seq/split/merge mismatches,
    - `FRS-PROP-0003` (`PROP_RECURSION_MISMATCH`) for recursive composition constraints,
    - `FRS-PROP-0099` (`PROP_GENERIC_FAILURE`) for integer conversion/range classes,
  - contextual note/help entries added for arity and recursion mismatch classes,
  - Rustdoc added on conversion semantics in `propagate` (`IntoDiagnostic` impl block),
  - new propagate test coverage:
    - `propagate_error_converts_to_structured_diagnostic_codes`.
- Validation:
  - `cargo test -p errors`
  - `cargo test -p propagate`
  - `cargo test -p compiler`

#### Step 8 — compiler cross-phase diagnostics aggregation (`parse/eval/propagate`)

- Commit: `be79a86`
- Files:
  - `crates/compiler/src/lib.rs`.
- Implemented:
  - `Compiler::pipeline_to_signals` now wraps `EvalError` and `PropagateError` into
    `CompilerError` variants carrying:
    - `source`,
    - original typed error,
    - `DiagnosticBundle` created from `IntoDiagnostic`.
  - `CompilerError` variants enriched:
    - `Eval { source, error, diagnostics }`,
    - `Propagate { source, error, diagnostics }`,
    replacing payload-only tuple variants.
  - `CompilerError::diagnostics()` now exposes structured diagnostics for all three
    relevant phases:
    - parse,
    - eval,
    - propagate.
  - compiler tests strengthened:
    - eval failure now asserts `FRS-EVAL-*` presence in returned diagnostics,
    - new propagate failure test asserts `FRS-PROP-*` presence.
- Validation:
  - `cargo test -p compiler`

#### Step 9 — CLI diagnostics output model (`--error-format human|json`)

- Commit: `01b3fe6`
- Files:
  - `crates/compiler/Cargo.toml`,
  - `crates/compiler/src/main.rs`.
- Implemented:
  - added explicit CLI diagnostics output format selection for parse/signal flows:
    - `--error-format human` (default),
    - `--error-format json`,
  - human rendering kept stable (`file:line:col severity [code] message`),
  - added structured JSON rendering for diagnostics payload:
    - severity/stage/code/message,
    - labels (style + source span),
    - notes/help,
  - usage strings updated for `--parse`, `--dump-box`, and `--dump-sig`,
  - added renderer unit tests in compiler binary:
    - human output includes location and diagnostic code,
    - JSON output is valid and exposes expected structured keys for eval failures.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p compiler`

#### Step 9b — negative corpus fixtures for parser/eval/propagate diagnostics

- Commit: `b2cab3a`
- Files:
  - `tests/corpus/err_01_parse_missing_rhs.dsp`,
  - `tests/corpus/err_02_eval_missing_process.dsp`,
  - `tests/corpus/err_03_propagate_split_mismatch.dsp`,
  - `crates/compiler/tests/diagnostic_errors.rs`.
- Implemented:
  - added dedicated `.dsp` fixtures triggering one representative failure per stage:
    - parse failure (`process = ;`),
    - eval failure (missing `process` definition),
    - propagate failure (split arity mismatch),
  - added compiler integration tests validating stage-specific structured diagnostic code families:
    - `FRS-PARSE-*`,
    - `FRS-EVAL-*`,
    - `FRS-PROP-*`.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p compiler --test diagnostic_errors`

#### Documentation updates linked to this rollout

- Commit: `559af95`
- Added/updated porting docs:
  - `porting/faust-rust-diagnostics-model-en.md` (new global diagnostics architecture doc),
  - `porting/faust-rust-porting-plan-en.md`,
  - `porting/faust-rust-points-critiques-en.md`,
  - `porting/phases/phase-0-validation-en.md`,
  - `porting/phases/phase-0-gglobal-decomposition-map-en.md`,
  - `porting/phases/phase-1-fondations-en.md`,
  - `porting/phases/phase-3-parser-en.md`,
  - `porting/phases/phase-4-signaux-en.md`.

#### Documentation addendum — diagnostics UX explainability roadmap

- Commit: `b2cab3a`
- Files:
  - `porting/faust-rust-diagnostics-model-en.md`,
  - `porting/phases/phase-4-signaux-en.md`,
  - `porting/faust-rust-porting-plan-en.md`.
- Implemented:
  - documented a prioritized post-step-9 plan to make errors more explicit for users:
    - node-context enrichment (`node_id` + expression preview),
    - rule-aware actionable arity/composition explanations,
    - source-span propagation for Phase 4 diagnostics,
    - human renderer snippet/caret upgrade with JSON schema stability and snapshot locks.
  - added pass criteria for this UX tranche directly in diagnostics/phase documents.

#### Diagnostics UX rollout — Step 1 (node-context enrichment in compiler aggregation)

- Commit: `56122fb`
- Files:
  - `crates/compiler/src/lib.rs`,
  - `crates/compiler/Cargo.toml`.
- Implemented:
  - compiler diagnostics aggregation now enriches eval/propagate diagnostics with:
    - `node_id=<TreeId>` note when the error variant carries a node,
    - compact `box_expr=<dump_box(...)>` preview note for the offending node.
  - this context is injected in `pipeline_to_signals` before wrapping into `CompilerError`.
  - regression test strengthened:
    - propagate mismatch test now asserts presence of `node_id=` and `box_expr=` notes.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p compiler`

#### Diagnostics UX rollout — Step 2 (rule-aware actionable propagate diagnostics)

- Commit: `1623712`
- Files:
  - `crates/propagate/src/lib.rs`,
  - `crates/propagate/tests/core_api.rs`.
- Implemented:
  - enriched `PropagateError -> Diagnostic` conversion for composition/arity failures:
    - explicit rule notes (seq/split/merge/rec),
    - computed-condition notes (including divisibility remainders),
    - actionable help text for correction.
  - widened baseline arity mismatch diagnostics with direct help hints.
  - extended propagate diagnostics unit tests to lock the new notes/help payloads.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p propagate --all-targets`
  - `cargo test -p compiler`

#### Diagnostics UX rollout — Step 3 (source-label attachment when parser metadata is available)

- Commit: `711365d`
- Files:
  - `crates/compiler/src/lib.rs`.
- Implemented:
  - compiler diagnostics enrichment now attempts to attach a primary source label for
    eval/propagate node-based errors by consulting parser metadata:
    - direct node property (`use_prop` / `def_prop`),
    - fallback search on labeled descendants in the offending subtree.
  - this keeps labels opportunistic (added only when metadata exists) and does not
    regress diagnostics when source info is absent.
  - added unit tests covering:
    - direct node property lookup,
    - descendant property fallback lookup.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p compiler`

#### Diagnostics UX rollout — Step 4 (human renderer snippet/caret + snapshot lock)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/compiler/src/main.rs`,
  - `crates/compiler/src/lib.rs`.
- Implemented:
  - upgraded human diagnostics formatting:
    - source snippet line and caret span when labeled source file is readable,
    - explicit note/help lines in output.
  - kept JSON diagnostics schema stable while extending tests.
  - added snapshot-style tests in compiler CLI module:
    - human output snapshot with snippet/caret (path-normalized),
    - JSON shape stability assertions for eval diagnostics payload.
  - added Rustdoc comments on new diagnostics helpers (renderer and compiler enrichment).
- Validation:
  - `cargo fmt --all`
  - `cargo test -p compiler`

#### Diagnostics UX rollout — Step 4b (complex propagate error fixtures + alias-chain source mapping)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `tests/corpus/err_04_propagate_seq_mismatch_alias.dsp`,
  - `tests/corpus/err_05_propagate_merge_mismatch_alias.dsp`,
  - `tests/corpus/err_06_propagate_split_mismatch_chain.dsp`,
  - `tests/corpus/err_07_propagate_rec_mismatch_alias.dsp`,
  - `tests/corpus/err_08_propagate_seq_ui_mismatch.dsp`,
  - `crates/compiler/tests/diagnostic_errors.rs`,
  - `crates/compiler/src/lib.rs`.
- Implemented:
  - added a richer negative corpus of connection errors (seq/split/merge/rec + alias chains).
  - extended compiler diagnostics integration tests to assert:
    - `FRS-PROP-*` code family,
    - source-label presence and expected source line.
  - refined source-label fallback in compiler diagnostics:
    - prefers the definition owning the failing expression (`foo = ...`) over top-level alias lines,
    - handles alias-chain forms (`foo -> bar -> process`).
- Validation:
  - `cargo fmt --all`
  - `cargo test -p compiler --test diagnostic_errors`
  - `cargo test -p compiler --lib`

#### Documentation addendum — diagnostics UX next tranche planning

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `porting/faust-rust-diagnostics-model-en.md`,
  - `porting/phases/phase-4-signaux-en.md`,
  - `porting/faust-rust-porting-plan-en.md`.
- Implemented:
  - documented the next prioritized diagnostics-improvement tranche:
    - operator-level source precision (column-level spans),
    - alias-resolution context notes (`process -> ... -> owner`),
    - paired-side mismatch context (left/right arity notes),
    - richer human-readable expression context,
    - expanded complex negative snapshot corpus,
    - operator-specific correction hints.

#### Diagnostics UX next tranche — Step 1 (operator-level source precision)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/parser-proto/src/context.rs`,
  - `crates/parser-proto/src/lib.rs`,
  - `crates/parser-proto/src/grammar/faustparser.y`,
  - `crates/compiler/src/lib.rs`,
  - `crates/compiler/tests/diagnostic_errors.rs`.
- Implemented:
  - parser context now preserves full cursor span (`line/col/end_line/end_col`) when
    storing def/use properties from cursor hooks.
  - parser semantic actions for composition operators now tag produced expression nodes
    with operator-token source spans (`PAR`, `SEQ`, `SPLIT`, `MIX`, `REC`) so diagnostics
    can point to the operator column.
  - compiler label-resolution priority now prefers direct offending-node spans before
    definition fallback, keeping alias-chain ownership fallback intact.
  - added compiler integration test to lock operator-level label behavior on propagate
    split mismatch fixtures.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p parser-proto --test parser_slice1`
  - `cargo test -p compiler --test diagnostic_errors`

#### Diagnostics UX next tranche — Step 2 (alias-resolution context notes)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/compiler/src/lib.rs`,
  - `crates/compiler/tests/diagnostic_errors.rs`.
- Implemented:
  - compiler diagnostics enrichment now computes a deterministic binding/reference trace
    from `process` to the definition owning the failing node when available.
  - added `binding_trace=process -> ... -> owner` notes on eval/propagate diagnostics.
  - trace resolution handles non-direct aliases by following identifier references
    in top-level definition expressions (example: `process = baz,baz; baz = bar; bar = foo`).
  - added integration test lock for alias-chain trace on `err_06`.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p compiler --test diagnostic_errors`

#### Diagnostics UX next tranche — Step 3 (human-readable expression context)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/compiler/src/lib.rs`,
  - `crates/compiler/tests/diagnostic_errors.rs`.
- Implemented:
  - added a second expression-context note in diagnostics:
    - machine-oriented: `box_expr=...` (existing, unchanged),
    - human-oriented: `expr=...` (Faust-like rendering for common composition/infix forms).
  - introduced readable rendering helpers with bounded depth/size and stable fallback to
    `dump_box` for unsupported forms.
  - diagnostic tests now lock presence of the new `expr=` note and ensure composition
    operators remain visible in the readable context.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p compiler --test diagnostic_errors`
  - `cargo test -p compiler --lib`

#### Diagnostics UX next tranche — Step 4 (paired-side mismatch context)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/compiler/src/lib.rs`,
  - `crates/compiler/tests/diagnostic_errors.rs`.
- Implemented:
  - propagate mismatch diagnostics now include explicit paired context:
    - `A (<op> left) = <expr>`,
    - `B (<op> right) = <expr>`,
    - `A arity: inputs=... outputs=...`,
    - `B arity: inputs=... outputs=...`.
  - this aligns Rust diagnostics with the C++ style expectation of naming both sides
    of a composition error while keeping the structured note model.
  - added integration test lock on merge-mismatch alias fixture.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p compiler --test diagnostic_errors`
  - `cargo test -p compiler --lib`

#### Diagnostics UX next tranche — Step 5 (snapshot expansion on complex failures)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/compiler/src/main.rs`.
- Implemented:
  - expanded human/json diagnostics snapshot coverage to complex Phase 4 negative fixtures:
    - alias-chain split mismatch (`err_06`),
    - recursive mismatch alias (`err_07`),
    - UI-driven sequential mismatch (`err_08`).
  - new CLI renderer tests now assert presence/stability of:
    - trace notes,
    - paired-side A/B notes,
    - source-snippet inclusion in human output,
    - structured notes shape in JSON output.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p compiler --bin faust-rs`

#### Diagnostics UX next tranche — Step 6 (operator-specific correction hints)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/propagate/src/lib.rs`,
  - `crates/propagate/tests/core_api.rs`.
- Implemented:
  - refined propagate `help` payload per composition class:
    - `seq`: explicit `A : B` equality rule + concrete channel-width fix pattern,
    - `split`: explicit `A <: B` divisibility rule + concrete grouping/duplication fix pattern,
    - `merge`: explicit `A :> B` multiple rule + concrete arity adjustment fix pattern,
    - `rec`: explicit `A ~ B` inequalities + concrete feedback-bus fix pattern.
  - locked help-shape expectations with targeted propagate unit tests.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p propagate --all-targets`
  - `cargo test -p compiler --test diagnostic_errors`

#### Documentation addendum — diagnostics readability micro-tranche planning

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `porting/faust-rust-diagnostics-model-en.md`,
  - `porting/phases/phase-4-signaux-en.md`,
  - `porting/faust-rust-porting-plan-en.md`.
- Implemented:
  - documented a follow-up diagnostics UX micro-tranche focused on user readability:
    - C++-style paired `Here A / while B` rendering with arity lines,
    - readable pretty-print for UI/primitive expressions in notes,
    - explicit owner-definition note in alias-expanded failures,
    - computed numeric correction suggestions when deterministic,
    - dedicated human/json snapshot lock for these readability rules.

#### Diagnostics readability micro-tranche — Step 1 (`Here A / while B` human rendering)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/compiler/src/main.rs`.
- Implemented:
  - human renderer now collapses paired-side notes into explicit C++-style blocks:
    - `Here  A = ...`,
    - `has inputs=... outputs=...`,
    - `while B = ...`,
    - `has inputs=... outputs=...`.
  - low-level `A (...) = ...`/`B (...) = ...` notes remain in diagnostics payload (JSON),
    while human rendering presents the condensed readable form.
  - updated complex human snapshot expectations accordingly.
  - added dedicated renderer unit test to lock this formatting contract.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p compiler --bin faust-rs`

#### Diagnostics readability micro-tranche — Step 2 (UI/primitive pretty-print in `expr=`)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/compiler/src/lib.rs`,
  - `crates/compiler/tests/diagnostic_errors.rs`.
- Implemented:
  - extended `render_human_box_expr` to avoid internal tag/list forms on common UI and
    primitive nodes in diagnostics expression notes.
  - added readable forms for:
    - UI nodes (`button`, `checkbox`, `hslider`, `vslider`, `nentry`, `bargraph`, groups, soundfile),
    - primitive names/symbols (infix and named primitives).
  - string/symbol literal rendering now uses source-like forms (`"..."`, symbol names).
  - added integration test lock to ensure UI mismatch diagnostics no longer expose
    internal `float_bits(...)` / `cons(...)` payloads in `expr=`.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p compiler --test diagnostic_errors`
  - `cargo test -p compiler --bin faust-rs`

#### Diagnostics readability micro-tranche — Step 3 (explicit owner-definition note)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/compiler/src/lib.rs`,
  - `crates/compiler/tests/diagnostic_errors.rs`.
- Implemented:
  - diagnostics enrichment now emits an explicit ownership note when resolvable:
    - `error originates from definition 'foo'`.
  - owner note is emitted for both eval and propagate node-based failures and complements
    `binding_trace=process -> ... -> foo`.
  - added integration test lock on alias-chain mismatch fixture to assert owner note presence.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p compiler --test diagnostic_errors`
  - `cargo test -p compiler --bin faust-rs`

#### Diagnostics readability micro-tranche — Step 4 (numeric correction proposals)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/propagate/src/lib.rs`,
  - `crates/propagate/tests/core_api.rs`.
- Implemented:
  - added numeric target proposals in propagate mismatch diagnostics:
    - `seq`: common equality target for `outputs(A)` / `inputs(B)`,
    - `split`: next multiple proposal for `inputs(B)`,
    - `merge`: next multiple proposal for `outputs(A)`,
    - `rec`: minimum required targets for `outputs(A)` and `inputs(A)`.
  - proposals are emitted as structured notes (`suggested target: ...`) with safe
    fallback text for zero-divisor edge cases.
  - expanded propagate unit tests to lock proposal presence for representative mismatch types.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p propagate --all-targets`
  - `cargo test -p compiler --test diagnostic_errors`

#### Diagnostics readability micro-tranche — Step 5 (snapshot lock for readability contract)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/compiler/src/main.rs`,
  - `crates/compiler/src/lib.rs`.
- Implemented:
  - expanded human/json complex fixture snapshots to lock readability contract:
    - `Here A / while B` block expectations in human output,
    - owner-definition note presence,
    - numeric suggestion note presence,
    - readable UI pretty-print (`hslider(...)`) in human output.
  - added/updated Rustdoc on rendering helpers involved in this contract:
    - paired-context extraction/filtering in CLI renderer,
    - primitive readable-name mapping helper in compiler diagnostics enrichment.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p compiler --bin faust-rs`
  - `cargo test -p compiler --test diagnostic_errors`

#### Documentation addendum — eval diagnostics readability gap planning

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `porting/faust-rust-diagnostics-model-en.md`,
  - `porting/phases/phase-4-signaux-en.md`,
  - `porting/faust-rust-porting-plan-en.md`.
- Implemented:
  - documented a dedicated follow-up plan for eval diagnostics readability:
    - increase node-carrying coverage in `EvalError` where possible,
    - improve eval source-label attachment quality (or explicit fallback notes),
    - expand eval-focused human/json negative snapshots,
    - tune eval-specific actionable hints.

#### Eval diagnostics readability implementation (node context + fixtures + snapshots)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/eval/src/lib.rs`,
  - `crates/eval/tests/core_eval.rs`,
  - `crates/compiler/src/lib.rs`,
  - `crates/compiler/src/main.rs`,
  - `crates/compiler/tests/diagnostic_errors.rs`,
  - `tests/corpus/err_09_eval_undefined_symbol.dsp`,
  - `tests/corpus/err_10_eval_too_many_arguments.dsp`,
  - `tests/corpus/err_11_eval_case_arity_mismatch.dsp`,
  - `tests/corpus/err_12_eval_case_no_match.dsp`.
- Implemented:
  - enriched `EvalError` context payload for readability/source mapping:
    - `MissingProcessDefinition { definitions, available_defs }`,
    - `UndefinedSymbol { symbol, node }`,
    - `EmptyArgumentList { node }`,
    - `PatternArityMismatch { node, expected, got }`,
    - `PatternMatchFailed { node }`,
    - `TooManyArguments { node, expected, got }`.
  - improved `EvalError -> Diagnostic` conversion:
    - richer notes/help for missing process, undefined symbol, arity mismatch.
    - explicit top-level definition inventory on missing process.
  - added deterministic helper for top-level name extraction in eval diagnostics
    with Rustdoc (`top_level_definition_names`).
  - adjusted evaluator call-site tracking so `TooManyArguments` carries source-relevant node
    from application site (not only post-eval function value).
  - extended compiler eval-node extraction to attach source labels/notes for new eval variants.
  - added eval negative DSP fixtures:
    - undefined symbol,
    - too many arguments,
    - case arity mismatch,
    - case no-match.
  - expanded eval diagnostics tests:
    - compiler integration tests for `FRS-EVAL-*` + source labels + readable context + owner/binding notes,
    - CLI human/json snapshot tests for eval undefined symbol readability contract.
  - updated eval unit tests to lock new error payload shape and diagnostic conversion.
- Validation:
  - `cargo fmt --all`
  - `cargo test -p eval --all-targets`
  - `cargo test -p compiler --test diagnostic_errors`
  - `cargo test -p compiler --bin faust-rs`

#### Documentation addendum — eval diagnostics v2 planning

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `porting/faust-rust-diagnostics-model-en.md`,
  - `porting/phases/phase-4-signaux-en.md`,
  - `porting/faust-rust-porting-plan-en.md`.
- Implemented:
  - documented remaining eval diagnostics improvements after current rollout:
    - multi-label call-site/definition-site diagnostics,
    - explicit scope-resolution notes for unresolved symbols,
    - deterministic correction templates,
    - eval/propagate wording normalization,
    - nested realistic eval negative corpus expansion,
    - optional IDE-oriented structured JSON enrichment (`owner_definition`, binding-path vector, label roles).

#### Eval diagnostics v2 implementation — multi-label/source scopes/json context

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/eval/src/lib.rs`,
  - `crates/eval/tests/core_eval.rs`,
  - `crates/compiler/src/lib.rs`,
  - `crates/compiler/src/main.rs`,
  - `crates/compiler/tests/diagnostic_errors.rs`.
- Implemented:
  - enriched unresolved-symbol diagnostics with explicit lexical context:
    - `scope.local=...`,
    - `scope.visible=...`,
    - `scope.top_level=...`.
  - extended `EvalError::UndefinedSymbol` payload to carry scope vectors for deterministic reporting.
  - replaced generic eval source-label attachment with eval-specific call/definition pairing:
    - primary label: `call site`,
    - secondary label: `definition site` (when distinct).
  - enriched eval wording for missing process / pattern arity / extra arguments / case no-match.
  - added compiler integration coverage for:
    - scope-context notes,
    - multi-label call/definition contract on undefined-symbol fixture.
  - enriched JSON diagnostics with optional machine-readable context extraction:
    - label role mapping (`call_site`, `definition_site`),
    - `context.owner_definition`,
    - `context.binding_trace_path`,
    - `context.scope.{local,visible,top_level}`.
  - documented new helper behavior in Rustdoc (`label_role`, `diagnostic_context_from_notes`).
- Validation:
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`

#### Diagnostics quality gate implementation tranche — origin-site priority + templates + nested fixtures

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/compiler/src/lib.rs`,
  - `crates/compiler/src/main.rs`,
  - `crates/compiler/tests/diagnostic_errors.rs`,
  - `crates/eval/src/lib.rs`,
  - `crates/propagate/src/lib.rs`,
  - `tests/corpus/err_13_eval_undefined_symbol_alias_chain_nested.dsp`,
  - `tests/corpus/err_14_propagate_split_mismatch_nested_alias.dsp`.
- Implemented:
  - source-label strategy updated for alias chains:
    - when owner definition is known, diagnostics prioritize origin definition span,
    - process call-site is attached as secondary label when distinct.
  - explicit fallback note added when origin span cannot be resolved:
    - `origin span unavailable; pointing to nearest call/owner site`.
  - eval wording normalized to `rule -> computed -> context -> help` for undefined symbol and arity/case classes.
  - deterministic correction templates added:
    - eval undefined symbol / pattern arity / over-application,
    - propagate seq/split/merge/rec mismatch classes.
  - added realistic nested negative fixtures:
    - eval undefined symbol through alias chain (`process -> baz -> bar -> foo`),
    - propagate split mismatch with nested alias + local scope.
  - expanded integration/CLI snapshot coverage for new fixtures and label-role expectations.

#### Diagnostics polish tranche — cause-line + compound fixtures + fallback-path lock

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/eval/src/lib.rs`,
  - `crates/propagate/src/lib.rs`,
  - `crates/compiler/src/lib.rs`,
  - `crates/compiler/src/main.rs`,
  - `crates/compiler/tests/diagnostic_errors.rs`,
  - `tests/corpus/err_15_eval_compound_with_letrec_case_arity.dsp`,
  - `tests/corpus/err_16_propagate_compound_with_letrec_split.dsp`.
- Implemented:
  - added explicit `cause:` note lines for top frequent eval/propagate failures.
  - expanded compound negative corpus with stacked contexts:
    - eval: `with + letrec + case arity mismatch`,
    - propagate: `with + letrec + alias chain + split mismatch`.
  - extended human/json snapshot coverage for compound fixtures and `cause:` expectations.
  - added dedicated fallback-path tests for missing origin spans:
    - eval labeler fallback note,
    - propagate labeler fallback note.
  - kept deterministic correction-template helps and ordering contract intact.

#### Diagnostics polish tranche — secondary coverage + JSON ordering lock + human noise reduction

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/eval/src/lib.rs`,
  - `crates/eval/tests/core_eval.rs`,
  - `crates/propagate/src/lib.rs`,
  - `crates/propagate/tests/core_api.rs`,
  - `crates/compiler/src/lib.rs`,
  - `crates/compiler/src/main.rs`,
  - `tests/corpus/err_17_origin_fallback_missing_props_eval.dsp`.
- Implemented:
  - completed explicit `cause:` note coverage on secondary eval/propagate variants:
    - eval iteration-invalid and generic-eval fallback,
    - propagate unsupported-box, generic arity mismatch, and integer-field failures.
  - introduced human diagnostics verbosity modes in CLI:
    - `--error-verbosity standard` (default concise output),
    - `--error-verbosity debug` (keeps internal notes).
  - added Rustdoc-documented advanced compiler API:
    - `compile_parsed_to_signals(source_name, parse_output)` for test/tooling flows
      that mutate parser metadata before Phase 4.
  - added JSON snapshot ordering lock assertions:
    - eval + propagate (`split`, `merge`, `rec`) fixtures now assert note order
      contract `cause -> rule -> computed -> context`.
  - added pipeline-level fallback coverage for missing source properties:
    - new corpus fixture for origin fallback scenario,
    - compiler unit test parses fixture then clears parser context properties and verifies
      `origin span unavailable; pointing to nearest call/owner site`,
    - dedicated human-renderer snapshot for the same fallback wording.
  - reduced standard human output noise:
    - internal notes `node_id=` and `box_expr=` are filtered in human renderer,
    - readable `expr=` notes remain visible,
    - debug verbosity keeps full notes for troubleshooting.
  - added renderer unit lock for this human-noise contract.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test -p compiler --all-targets`
  - `cargo test -p eval --all-targets`
  - `cargo test -p propagate --all-targets`

#### Diagnostics polish tranche — verbosity modes + JSON debug enrichment + help concision

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/compiler/src/lib.rs`,
  - `crates/compiler/src/main.rs`,
  - `crates/eval/src/lib.rs`,
  - `crates/propagate/src/lib.rs`,
  - `README.md`,
  - `porting/faust-rust-diagnostics-model-en.md`,
  - `tests/corpus/err_17_origin_fallback_missing_props_eval.dsp`.
- Implemented:
  - added diagnostics verbosity contract to compiler CLI:
    - `--error-verbosity standard` (default concise human output),
    - `--error-verbosity debug` (keeps internal note stream).
  - extended JSON diagnostics with optional debug enrichment:
    - `diagnostics[*].debug = { node_id, box_expr }` in debug verbosity only.
  - added advanced compiler API entry point with Rustdoc:
    - `compile_parsed_to_signals(source_name, parse_output)`.
  - expanded JSON note-order coverage to additional families:
    - propagate split/merge/rec ordering checks,
    - eval undefined-symbol ordering check.
  - added pipeline-level human snapshot for origin-fallback wording by running
    eval/propagate on parsed output with parser source properties cleared.
  - performed help/template concision pass on eval/propagate diagnostics wording.
  - documented diagnostics CLI usage and quick reading guide in project docs.
- Validation:
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test -p compiler --all-targets`
  - `cargo test -p eval --all-targets`
  - `cargo test -p propagate --all-targets`

#### Diagnostics documentation synthesis (parser -> eval -> propagate)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `porting/faust-rust-error-flow-en.md` (new),
  - `docs/user-diagnostics-guide-en.md` (new),
  - `README.md`.
- Implemented:
  - added a concise technical reference for contributors describing:
    - the diagnostics data flow from parser context to eval/propagate and compiler aggregation,
    - the stable diagnostics code families,
    - source-label resolution/fallback strategy,
    - rendering contract (`human/json`, `standard/debug`).
  - added a user-facing diagnostics guide with:
    - practical command-line usage,
    - how to read `cause/rule/computed/help`,
    - quick mapping of error code families.
  - linked both documents from the repository README.

#### Golden refresh (Rust) — negative diagnostics corpus alignment

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `tests/golden/rust/err_01_parse_missing_rhs/compiler_stdout.txt`
  - `tests/golden/rust/err_02_eval_missing_process/compiler_stdout.txt`
  - `tests/golden/rust/err_03_propagate_split_mismatch/compiler_stdout.txt`
  - `tests/golden/rust/err_04_propagate_seq_mismatch_alias/compiler_stdout.txt`
  - `tests/golden/rust/err_05_propagate_merge_mismatch_alias/compiler_stdout.txt`
  - `tests/golden/rust/err_06_propagate_split_mismatch_chain/compiler_stdout.txt`
  - `tests/golden/rust/err_07_propagate_rec_mismatch_alias/compiler_stdout.txt`
  - `tests/golden/rust/err_08_propagate_seq_ui_mismatch/compiler_stdout.txt`
  - `tests/golden/rust/err_09_eval_undefined_symbol/compiler_stdout.txt`
  - `tests/golden/rust/err_10_eval_too_many_arguments/compiler_stdout.txt`
  - `tests/golden/rust/err_11_eval_case_arity_mismatch/compiler_stdout.txt`
  - `tests/golden/rust/err_12_eval_case_no_match/compiler_stdout.txt`
  - `tests/golden/rust/err_13_eval_undefined_symbol_alias_chain_nested/compiler_stdout.txt`
  - `tests/golden/rust/err_14_propagate_split_mismatch_nested_alias/compiler_stdout.txt`
  - `tests/golden/rust/err_15_eval_compound_with_letrec_case_arity/compiler_stdout.txt`
  - `tests/golden/rust/err_16_propagate_compound_with_letrec_split/compiler_stdout.txt`
  - `tests/golden/rust/err_17_origin_fallback_missing_props_eval/compiler_stdout.txt`
  - `tests/golden/rust/rep_31_extended_primitives/compiler_stdout.txt`
  - `JOURNAL.md`.
- Implemented:
  - generated missing Rust golden outputs for the `err_*` diagnostics fixtures.
  - regenerated `rep_31_extended_primitives` Rust golden output for corpus parity with current fixtures.
  - restored `xtask golden-check` pass on full corpus.
- Validation:
  - `cargo run -p xtask -- golden-gen-rust`
  - `cargo run -p xtask -- golden-check`

#### Corpus-wide C++ vs Rust status differential gate (Phase 4/9)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/xtask/src/main.rs`,
  - `porting/faust-rust-porting-plan-en.md`,
  - `porting/phases/phase-4-signaux-en.md`,
  - `porting/phases/phase-9-integration-en.md`,
  - `porting/phases/phase-4-corpus-status-diff-report-en.md`,
  - `README.md`.
- Implemented:
  - documented a mandatory corpus-wide parity gate in porting docs:
    - run every `tests/corpus/*.dsp` with C++ `faust` and Rust `--dump-sig`,
    - classify `OK/OK`, `ERR/ERR`, `OK/ERR`, `ERR/OK`,
    - treat mixed statuses as blocking parity tasks.
  - added new automation command:
    - `cargo run -p xtask -- corpus-status-report`.
  - command generates:
    - `porting/phases/phase-4-corpus-status-diff-report-en.md` with summary, mismatch table, and full matrix.
  - first full run result:
    - total `51`,
    - `OK/OK=26`,
    - `ERR/ERR=16`,
    - `OK/ERR=9`,
    - `ERR/OK=0`.
  - confirmed user-reported mismatch:
    - `err_11_eval_case_arity_mismatch`: `C++=OK`, `Rust=ERR` (`eval` stage).
- Validation:
  - `cargo run -p xtask -- corpus-status-report`

#### Eval parity fix — `case` under-application (C++ behavior)

- Commit: pending (working tree step, to be committed separately)
- Files:
  - `crates/eval/src/lib.rs`,
  - `crates/eval/tests/core_eval.rs`,
  - `porting/phases/phase-4-corpus-status-diff-report-en.md`.
- Implemented:
  - aligned `eval::apply_list` for `BoxMatch::Case` with C++ under-application behavior:
    - when provided args `<` case arity, keep `case` node and lower to
      `seq(par(args + implicit_wires), case)` instead of raising `PatternArityMismatch`.
  - added eval regression test locking this behavior.
  - retained no-match failure behavior for fully-applied case dispatch.
- Result:
  - `err_11_eval_case_arity_mismatch` and `err_15_eval_compound_with_letrec_case_arity`
    are no longer failing in `eval`; they now fail later in `propagate` due unsupported
    `case` box family (expected until propagate support is extended).
- Validation:
  - `cargo test -p eval --all-targets`
  - `cargo run -p xtask -- corpus-status-report`
