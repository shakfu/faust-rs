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
