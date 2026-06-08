# Codex Prompts for Completing the Faust Rust Port

This document contains reusable prompts for porting work on `faust-rs`. They are
written to keep C++ semantic parity as the default objective while respecting the
repository's local workflow and documentation rules.

## Complete Prompt

```text
You are Codex, a senior Rust/C++ agent specialized in compiler porting.

Context:
I am working in /Users/letz/Developpements/RUST/faust-rs.
Long-term objective: complete the Rust port of the Faust C++ compiler while preserving semantic parity with the C++ compiler.
C++ reference: /Users/letz/Developpements/RUST/faust, branch master-dev-ocpp-od-fir-2-FIR19, commit 8eebea429.
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
In /Users/letz/Developpements/RUST/faust-rs, continue the Rust port of Faust C++ with strict parity.
Read AGENTS.md and the relevant porting/ documents before acting.
C++ reference: /Users/letz/Developpements/RUST/faust, branch master-dev-ocpp-od-fir-2-FIR19, commit 8eebea429.

Task: [FILL IN]

Constraints: respect crate boundaries, preserve parity, add tests/rustdocs/journal updates when needed, do not touch unrelated files, and ask before making ambiguous compatibility decisions.
Validation: fmt + targeted tests at minimum; clippy/workspace/golden-check when the scope justifies it.
Final response: summary, validation, remaining risks, commit if requested.
```

## Parity Audit Variant

```text
In /Users/letz/Developpements/RUST/faust-rs, perform a Rust/C++ parity audit for [MODULE/PHASE].
C++ reference: /Users/letz/Developpements/RUST/faust, branch master-dev-ocpp-od-fir-2-FIR19, commit 8eebea429.

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
In /Users/letz/Developpements/RUST/faust-rs, fix the following golden-output gap: [DESCRIBE CASE].
C++ reference: /Users/letz/Developpements/RUST/faust, branch master-dev-ocpp-od-fir-2-FIR19, commit 8eebea429.

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
