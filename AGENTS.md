# AGENTS

Guidelines for contributors and coding agents working on `faust-rs`.

## 1. Project Goal

- Port the Faust compiler from C++ to Rust.
- Keep semantic parity with the C++ compiler as the default objective.
- Prefer explicit, testable behavior over speculative refactors.
- C++ reference branch used for baseline analysis: `master-dev-ocpp-od-fir-2-FIR19` (`8eebea429`) in /Users/letz/Developpements/RUST/faust folder.

## 2. Workspace Rules

- This repository is a Cargo workspace with many crates under `crates/`.
- Respect crate boundaries; avoid circular dependencies.
- Add new code in the most specific crate first, then expose upward through public APIs.
- Keep `crates/compiler` as the top-level orchestration crate (lib + CLI entry point).
- Keep crate responsibilities aligned with `porting/faust-rust-porting-plan-en.md` section 2/4.
- Preserve key integrations recommended by the plan:
  - `patternmatcher` logic merged into `eval`.
  - `extended` math nodes integrated into `signals`.
  - `parallelize` integrated into `transform`.

## 3. Code Quality

- Rust edition and toolchain are controlled by workspace files.
- Before committing, run:
  - `cargo fmt --all`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test --workspace --all-targets`
- Avoid introducing `unsafe` unless strictly required and documented.

## 4. CI Expectations

- CI runs on Linux, macOS, and Windows.
- CI stages include `cargo check`, formatting, clippy, and tests.
- CI also runs golden parity guardrails via `cargo run -p xtask -- golden-check`.
- A change is not considered ready unless CI is green.

## 5. Porting Discipline

- Use the `porting/` documents as source of truth for scope and phases.
- Preserve behavior first, optimize later.
- Add or update unit tests in the touched crate(s) as part of each porting change; if tests cannot be added immediately, record the reason, owner, and planned follow-up in `JOURNAL.md`.
- Document migrated source provenance as you port: add Rustdoc comments (`///` or `//!`) that reference the corresponding C++ source files/functions and capture key invariants/semantic notes needed to maintain parity.
- Prefer real end-to-end integrations over temporary stubs; if a stub is unavoidable, it must be explicitly justified, owner-assigned, time-boxed, and removed within the same phase gate.
- Define explicit deliverables and pass criteria for each phase/prototype before implementation; do not start deep work on tasks with implicit success conditions.
- For critical compiler behavior, prefer differential tests against C++ reference outputs.
- Document known gaps and temporary scaffolding in `JOURNAL.md`.
- Follow the canonical pipeline described in the plan:
  - `parse -> boxes -> eval -> propagate -> normalize -> type/interval -> transform -> fir -> backend`

## 6. Scope and Non-Goals (Frozen)

- Keep these exclusions unless explicitly revised in planning docs:
  - `backend-java` is out of Rust port target scope.
  - legacy `-lang ocpp` mode is out of Rust port target scope.
- Treat maintained but non-primary paths as secondary until parity is reached on the production flow.

## 7. Phase 0 Gate (Mandatory Before Deep Implementation)

Before substantial implementation in a subsystem, confirm Phase 0 validation items are addressed for that scope (`porting/phases/phase-0-validation-en.md`):

- Effective compile pipeline confirmation (production path first).
- Differential baseline corpus and acceptance rules.
- `gGlobal` decomposition plan for touched flow.
- TreeArena hash-consing performance validation.
- API lifecycle and ownership model clarity for exposed entry points.

Do not lock large architectural decisions before these checks.

## 8. Critical Technical Risks to Re-check

From `porting/faust-rust-points-critiques-en.md`, keep these risks visible when designing changes:

- Parser migration parity (`bison/flex` semantics vs Rust parser stack).
- TreeArena performance regressions.
- Choosing the wrong initial signal->FIR path.
- Hidden `gGlobal` coupling in active compile flow.
- LLVM backend constraints (versioning/JIT/platform).
- Compiler-to-Wasm constraints.
- C API surface prioritization and lifecycle consistency.

When touching one of these areas, add focused tests/benchmarks in the same PR.

## 9. Golden Output Workflow

- Golden corpus inputs live in `tests/corpus/`.
- Golden reference outputs live in:
  - `tests/golden/rust/` (CI default gate)
  - `tests/golden/cpp/` (long-run parity target)
- Metadata and reference pinning live in `tests/golden/METADATA.toml`.
- Use:
  - `cargo run -p xtask -- golden-check` to validate against Rust reference snapshots.
  - `cargo run -p xtask -- golden-check-cpp` to validate against C++ reference snapshots.
  - `cargo run -p xtask -- golden-gen-rust` only for local bootstrap/scaffold updates.
  - `FAUST_CPP_BIN=/path/to/faust cargo run -p xtask -- golden-gen-cpp` for true C++ reference refresh.
- Any golden refresh must be documented in `JOURNAL.md` and mention reference commit/flags in PR description.

## 10. Recursion and RouteIR Guidance

From `porting/faust-rust-recursion-model-note-en.md`:

- Keep `sigRec/sigProj` as canonical external signal form for parity/API stability.
- RouteIR-style recursion groups are acceptable as internal optimization/analysis IR.
- Maintain conversion boundaries and invariants:
  - explicit recursion boundaries,
  - arity correctness,
  - deterministic ordering,
  - semantic parity against legacy output.

## 11. Commit and Documentation Hygiene

- Make small, coherent commits.
- Update `README.md` when user-facing build/run instructions change.
- Update `JOURNAL.md` for notable architecture, CI, or process changes.
- Keep comments and docs concise, factual, and implementation-oriented.
