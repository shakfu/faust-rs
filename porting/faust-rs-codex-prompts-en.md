# Codex Prompts for Completing the Faust Rust Port

This document contains reusable prompts for porting work on `faust-rs`. They are
written to keep C++ semantic parity as the default objective while respecting the
repository's local workflow and documentation rules.

## Complete Prompt

```text
You are Codex, a senior Rust/C++ agent specialized in compiler porting.

Context:
I am working in RUST/faust-rs.
Long-term objective: complete the Rust port of the Faust C++ compiler while preserving semantic parity with the C++ compiler.
C++ reference: RUST/faust, branch master-dev-ocpp-od-fir-2-FIR19, commit 8eebea429.
Strictly follow the repository's AGENTS.md instructions.

Mission:
[Describe the exact task here: for example, finish phase X, compare a C++/Rust module pair, implement a missing feature, reduce a golden-output gap, etc.]

Constraints:
- Preserve C++ parity before optimizing.
- Read the existing code and relevant documents under porting/ before changing anything.
- Do not invent a new architecture when the repository already has a matching pattern.
- Respect crate boundaries.
- Add or update relevant tests.
- Document C++ source provenance in rustdoc when porting behavior.
- Update porting/journal/YYYY-MM-DD.md for notable changes.
- Do not touch unrelated local files.
- Do not use destructive commands.
- If a decision affects external APIs, ABI, UI/meta callbacks, lifecycle semantics, or ambiguous parity behavior, ask me before implementing that part.

Expected validation:
- cargo fmt --all
- cargo clippy --workspace --all-targets -- -D warnings when the scope is broad
- cargo test --workspace --all-targets when the change affects multiple crates
- Otherwise, at minimum run targeted tests for the touched crate(s)
- cargo run -p xtask -- golden-check when compiler output may change
- Rustdoc with RUSTDOCFLAGS='-D warnings' for crates whose public documentation changes

Response format:
1. Briefly summarize what you understood.
2. List the files/docs you will inspect.
3. Give a short plan before changes if the task is non-trivial.
4. Implement the task completely in the workspace.
5. Finish with:
   - changes made,
   - validation commands and results,
   - remaining gaps or risks,
   - commit hash if I asked you to commit.

Concrete task:
[FILL IN]
```

## Short Prompt

```text
In RUST/faust-rs, continue the Rust port of Faust C++ with strict parity.
Read AGENTS.md and the relevant porting/ documents before acting.
C++ reference: RUST/faust, branch master-dev-ocpp-od-fir-2-FIR19, commit 8eebea429.

Task: [FILL IN]

Constraints: respect crate boundaries, preserve parity, add tests/rustdocs/journal updates when needed, do not touch unrelated files, and ask before making ambiguous compatibility decisions.
Validation: fmt + targeted tests at minimum; clippy/workspace/golden-check when the scope justifies it.
Final response: summary, validation, remaining risks, commit if requested.
```

## Parity Audit Variant

```text
In RUST/faust-rs, perform a Rust/C++ parity audit for [MODULE/PHASE].
C++ reference: RUST/faust, branch master-dev-ocpp-od-fir-2-FIR19, commit 8eebea429.

Compare:
- the real C++ flow and relevant source functions,
- the existing Rust implementation,
- available tests/golden outputs,
- known gaps documented in porting/.

Do not modify code unless I explicitly ask for implementation.
Expected response:
- C++ files inspected,
- Rust files inspected,
- parity already covered,
- priority gaps,
- missing tests,
- correction plan split into coherent commits.
```

## Golden Gap Fix Variant

```text
In RUST/faust-rs, fix the following golden-output gap: [DESCRIBE CASE].
C++ reference: RUST/faust, branch master-dev-ocpp-od-fir-2-FIR19, commit 8eebea429.

Constraints:
- First identify the divergence source in the pipeline parse -> boxes -> eval -> propagate -> normalize -> type/interval -> transform -> fir -> backend.
- Preserve C++ parity rather than applying a local simplification.
- Add a targeted test or refresh golden output only when the reference change is justified.
- Document C++ provenance in rustdoc or the journal if behavior is ported.

Minimum validation:
- cargo fmt --all
- targeted tests for the touched crate
- cargo run -p xtask -- golden-check

Final response:
- root cause,
- change applied,
- validation,
- remaining risk.
```

## Roadmap Execution Prompt (clock domains × FAD/RAD × `-vec`)

Reusable, resumable prompt to land the consolidated roadmap
`porting/ondemand-vec-fad-implementation-roadmap-2026-06-10-en.md`
one work package at a time. Run it session after session; it finds the
next unchecked item by itself.

```text
You are working in RUST/faust-rs. Read AGENTS.md first and follow it
strictly.

Mission: execute the NEXT work package of the consolidated implementation
roadmap porting/ondemand-vec-fad-implementation-roadmap-2026-06-10-en.md
(phases P0-P9).

Procedure:
1. Open the roadmap, find the first work package (Px.y) with unchecked
   checkboxes, respecting the §2 dependency table and phase order (P0
   before everything). If everything is checked, say so and stop.
2. Before writing any code, read:
   - that phase's roadmap section (deliverables and exit criteria),
   - the source sections it cites in the three analysis documents:
     porting/ondemand-clock-domains-analysis-port-plan-2026-06-10-en.md
     ("plan §N"),
     porting/ondemand-fad-rad-cohabitation-2026-06-10-en.md
     ("cohabitation §N"),
     porting/vector-mode-analysis-port-plan-2026-06-10-en.md
     ("vector doc §N"),
   - the C++ reference when the step ports behavior:
     RUST/faust, branch master-dev-ocpp-od-fir-2-FIR19, commit 8eebea429
     (pinned; do not re-sync),
   - the existing Rust code in the crates the step names, before
     proposing any new structure.
3. Implement exactly ONE work package, or one coherent sub-slice of it if
   it is large. Hard rules carried by the roadmap:
   - P0.1-P0.4 are one indivisible change set: never land the
     signal_prepare fix without the loud FAD boundary-glue diagnostic.
   - P2.2 is a behavior-preserving refactor: all existing golden
     snapshots and tests must come out identical; any diff is a bug to
     fix, not a golden to refresh.
   - Diagnostics discipline: every unsupported path fails with a
     structured FRS- code naming the construct - never a panic, never a
     silent fallback, never a silent zero tangent.
   - Preserve C++ parity before optimizing; keep the C++ rule names
     (R_PROJ, R_CLOCKED, R_CD, R_SEQ) in comments for parity audits.
   - No stubs: if a dependency of the step is missing, stop and report
     instead of stubbing it.
4. Add unit tests with the work (never after), in the harness styles the
   roadmap names (signal_pipeline.rs, diagnostic_errors.rs,
   cpp_signal_differential.rs, fad_recursive_runtime.rs, rad_runtime.rs,
   block_reverse_ad.rs). Write rustdoc with C++ source provenance for
   ported behavior.
5. Validate:
   - cargo fmt --all
   - cargo clippy on the touched crates (workspace-wide with
     -D warnings if the scope is broad)
   - targeted tests for the touched crates; cargo test --workspace
     --all-targets when several crates are affected
   - cargo run -p xtask -- golden-check whenever compiler output may
     change
6. When (and only when) the work package's exit criteria are met:
   - tick its checkboxes in the roadmap (only those),
   - if the implementation deviated from the analysis documents, amend
     the relevant doc section and say so explicitly,
   - add an entry to porting/journal/YYYY-MM-DD.md (in English, with a
     Validation section),
   - commit with a descriptive message; do not push.
7. End your reply with: what landed (Px.y and ticked boxes), validation
   commands and results, deviations from the docs, known risks, and
   which roadmap item is next.

Ask me before implementing if a decision affects public APIs/ABI, UI or
lifecycle semantics, or an ambiguous parity behavior. If a roadmap step
turns out to be wrong or impossible as written, stop and propose a
roadmap amendment instead of improvising. Do not touch unrelated files.
Do not use destructive commands.
```

### Short variant

```text
In RUST/faust-rs: read AGENTS.md, then execute the next unchecked work
package of porting/ondemand-vec-fad-implementation-roadmap-2026-06-10-en.md
(phase order, §2 dependencies; P0 is indivisible). Read the cited "plan
§N" / "cohabitation §N" / "vector doc §N" sections and the pinned C++
reference (RUST/faust @ 8eebea429) before coding. One work package only;
tests with the work; structured FRS- diagnostics on every unsupported
path; P2.2 must leave all goldens identical. Validate (fmt, clippy,
targeted tests, golden-check if output may change), tick the roadmap
checkboxes, journal in English, commit without pushing. Report: landed
Px.y, validation, deviations, next item. Ask before ambiguous
parity/API decisions.
```
