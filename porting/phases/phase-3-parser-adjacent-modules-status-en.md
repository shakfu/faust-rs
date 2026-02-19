# Phase 3 Parser-Adjacent Modules Status (`SourceFetcher`, `Enrobage`)

## 1. Purpose

This document closes **Gate B remaining step 7** by making lifecycle/API status explicit for parser-adjacent C++ modules that are not part of core grammar migration.

Source of truth (C++):
- `/Users/letz/Developpements/RUST/faust/compiler/parser/sourcefetcher.hh`
- `/Users/letz/Developpements/RUST/faust/compiler/parser/sourcefetcher.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/parser/enrobage.hh`
- `/Users/letz/Developpements/RUST/faust/compiler/parser/enrobage.cpp`

## 2. Status Matrix (`1:1` / `adapted` / `deferred`)

| C++ module | Main C++ API / role | Rust target scope | Status | Rationale | Owner + milestone | Validation |
|---|---|---|---|---|---|---|
| `sourcefetcher` | low-level `http_fetch(...)` and HTTP helpers used by import/file handling | optional parser-adjacent capability (`parser` feature-gated path) | `deferred` | Not required for parser migration viability gate; introduces network/dependency policy questions; avoid stubs in Phase 3. Core parser/import functionality remains local-file based via `SourceReader`. | Parser integration track, target **Phase 9 integration** | `parser-proto` `SourceReader` tests pass for local/cycle flows; explicit URL import behavior is asserted as unresolved in scope tests. |
| `enrobage` | architecture-template/file helper set (`openArchStream`, `fopenSearch`, stream copy utilities, output naming) used by `libcode.cpp` and documentator | better aligned with `compiler`/`codegen`/`doc` integration layer, not parser-core grammar migration | `deferred` (with crate-boundary adaptation) | This module is operationally adjacent to backend/output orchestration, not syntax parsing. Porting it inside parser migration would blur crate boundaries and increase coupling to global lifecycle state. | Compiler/codegen integration track, target **Phase 9 integration** (execution plan: `phase-9-enrobage-porting-plan-en.md`) | Existing parser migration tests (`parser-proto`) remain green without this module; responsibilities are now explicitly tracked here. |

## 3. Scope Contract for Phase 3

- `SourceReader` in `parser-proto` is intentionally **local-file only**.
- URL/network imports are intentionally not fetched in this phase.
- No placeholder network/wrapper implementation is introduced in Phase 3.
- This is an explicit **defer decision**, not an omission.

## 4. Integration Preconditions for Phase 9

Before moving these modules out of `deferred`:
1. Define feature policy for remote fetch (default-off, reproducible/offline-safe behavior).
2. Place APIs at the right boundary (`compiler`/`codegen`/`doc` orchestration layer) instead of parser-core.
3. Add lifecycle mapping per API (`1:1` or `adapted`) with compatibility impact notes.
4. Add focused tests for:
   - successful/failed URL fetch cases (if enabled),
   - wrapper/architecture file insertion behavior parity,
   - deterministic behavior when network is disabled.

## 5. Step-5 Coverage Update (Import Envelope)

Additional `parser-proto` `SourceReader` tests now cover:
- local-directory import precedence over global search paths when both provide the same import name,
- parent-relative import resolution (`../...`) through nested source trees,
- uniqueness of `used_files` tracking under repeated imports through different paths.

These checks extend the local-file import parity envelope while keeping network fetch out of scope for Phase 3.
