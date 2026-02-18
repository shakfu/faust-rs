# Phase 5 Addendum - Recursive Trees and `deBruijn2Sym` Porting Plan

> Status: implemented (initial phase scope delivered on February 18, 2026)  
> Scope: clean Rust port of recursive-tree machinery and `deBruijn2Sym` parity behavior.

---

## 1. Problem Statement

The C++ compiler has two recursion encodings:

- de Bruijn form (`DEBRUIJN`, `DEBRUIJNREF`)
- symbolic form (`SYMREC` + recursive-definition binding)

and performs explicit conversion with `deBruijn2Sym(...)` during normalization:

- `compiler/tlib/recursive-tree.cpp` (`deBruijn2Sym`, `substitute`, `liftn`)
- `compiler/normalize/normalform.cpp` (`simplifyToNormalForm` calls `deBruijn2Sym`)

Current Rust status:

- `propagate` emits de Bruijn recursion placeholders.
- `normalize` is still scaffolded and has no dedicated recursion-conversion pass.
- fast-lane currently consumes de Bruijn form directly.

Decision for now: **do not require `deBruijn2Sym` in the fast-lane chain**.

---

## 2. Goals

1. Port a Rust equivalent of C++ recursive-tree conversion with explicit invariants and tests.
2. Document and stabilize recursion representations used across Rust passes.
3. Preserve parity behavior for recursive trees without introducing hidden global state patterns.
4. Keep fast-lane behavior unchanged for now (de Bruijn input form accepted directly).

---

## 3. Scope / Non-Goals

In scope:

- `tlib` recursion primitives and conversion kernel.
- API contract for recursion representations in Rust (`de Bruijn` and symbolic forms).
- differential/corpus validation for recursion behavior.

Out of scope:

- forcing `deBruijn2Sym` into fast-lane now,
- full RouteIR recursion redesign,
- unrelated scheduler/vectorization/parallelization changes.

---

## 4. C++ to Rust Mapping Targets

| C++ symbol | Rust target | Status target | Notes |
|---|---|---|---|
| `rec(Tree body)` / `ref(int level)` | existing de Bruijn tags in `TreeArena` | `1:1` | canonical internal recursion form already used today |
| `deBruijn2Sym(Tree)` | `tlib::de_bruijn_to_sym(TreeId)` | `adapted` | same semantics, Rust API adapted |
| `substitute(Tree, level, id)` | internal helper in `tlib` | `adapted` | session-scoped memoization; no global mutable property keys |
| symbolic recursion (`SYMREC` + `RECDEF`) | explicit Rust representation | `adapted` | prefer explicit node shape over hidden side-properties |
| `sym2deBruijn(Tree)` | `tlib::sym_to_de_bruijn(TreeId)` | `deferred` | not required for first milestone |

---

## 5. Representation Decision (Required Before Coding)

Freeze one symbolic representation contract before implementation.

Option A (recommended):

- explicit symbolic recursive nodes with children only:
  - `SYMREC(var, body)`
  - `SYMREF(var)`

Option B:

- legacy-like shape with side-property recursion binding.

Recommendation: **Option A**, to keep matching deterministic and avoid hidden mutable-property coupling.

---

## 6. Implementation Plan

### Step 0 - Baseline and acceptance matrix

Deliverables:

- recursion-focused fixture inventory,
- explicit acceptance table (`OK/OK`, `ERR/ERR`) for Rust vs C++ classification.

Pass criteria:

- baseline report added under `porting/phases/` with pinned C++ commit and flags.

### Step 1 - Port core recursive-tree utilities in `tlib`

Deliverables:

- `de_bruijn_to_sym` implementation with memoization,
- helper equivalent to C++ `substitute`,
- aperture/lifting behavior parity notes and Rustdoc provenance comments.

Pass criteria:

- deterministic unit tests for representative recursion trees,
- no `unsafe`, no global singleton coupling.

### Step 2 - Add symbolic recursion model in Rust tree/matcher surface

Deliverables:

- explicit symbolic recursion constructors/matchers,
- tests covering de Bruijn and symbolic representations.

Pass criteria:

- stable tree-shape snapshots,
- parity tests for recursive expressions used in C++ references.

### Step 3 - Define pass contracts (without forcing fast-lane usage)

Deliverables:

- written contract per pass:
  - which recursion form is accepted,
  - whether conversion is mandatory/optional/forbidden at that boundary.

Initial policy:

- `propagate` output remains de Bruijn,
- fast-lane continues to consume de Bruijn directly.

Pass criteria:

- contract documented in `porting` and reflected by tests.

### Step 4 - Differential and corpus gates

Deliverables:

- recursion-focused differential checks against C++ behavior,
- golden updates only when justified and documented.

Pass criteria:

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo run -p xtask -- golden-check`

### Current implementation status

- Step 0: implemented (baseline matrix report added in `porting/phases/phase-5-recursive-baseline-matrix-en.md`).
- Step 1: implemented in `crates/tlib/src/recursion.rs` (`de_bruijn_to_sym`, `substitute` parity helper, aperture/lift helpers, memoized conversion context).
- Step 2: implemented in `crates/tlib/src/recursion.rs` with explicit symbolic tags:
  - `SYMREC(var, body)`
  - `SYMREF(var)`
  plus integration coverage in `crates/tlib/tests/recursive_trees.rs`.
- Step 3: documented below (pass-level recursion contract) and aligned with current code (`propagate` + fast-lane keep de Bruijn).
- Step 4: implemented for current scope (`cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --all-targets`, `cargo run -p xtask -- golden-check` all passing locally).

---

## 7. Fast-Lane Position (Current)

Current decision:

- `deBruijn2Sym` is **not required** in the fast-lane chain for now.
- fast-lane recursion lowering remains de Bruijn-based until normalization phase porting is mature.

Future revisit trigger:

- when a full normalization pipeline is active and parity evidence shows clear benefit/risk reduction.

## 7.1 Pass-level recursion contract (current)

| Pass / boundary | Accepted recursion form | Produced recursion form | `de_bruijn_to_sym` policy |
|---|---|---|---|
| `propagate` output | n/a (input is box graph) | `DEBRUIJN` + `DEBRUIJNREF` placeholders | forbidden |
| `transform::signal_fir` fast-lane input | `DEBRUIJN` + `DEBRUIJNREF` (plus `SIGREC/SIGPROJ`) | FIR recursion/state form | forbidden |
| `normalize` (future dedicated pass) | de Bruijn closed trees | symbolic recursion (`SYMREC/SYMREF`) | mandatory when normalization parity is enabled |
| generic `tlib` utilities | both forms may exist | caller-defined | optional (explicit API call) |

---

## 8. Risks and Mitigations

Risk: semantic drift in `substitute`/aperture behavior vs C++.

- Mitigation: targeted parity fixtures and deterministic tree-shape assertions.

Risk: representation ambiguity between de Bruijn and symbolic forms.

- Mitigation: explicit pass-level contract and tests per boundary.

Risk: hidden complexity from side-property recursion bindings.

- Mitigation: explicit symbolic node encoding and matcher API.

---

## 9. Exit Criteria

This addendum is complete when:

1. `de_bruijn_to_sym` Rust implementation is merged with provenance and tests.
2. recursion representation contracts are explicit across passes.
3. fast-lane de Bruijn policy is documented and validated by tests (no forced symbolic conversion).
4. workspace quality gate and golden gate are green with recursion coverage.
