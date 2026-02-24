# xtask

Developer and CI automation for the `faust-rs` workspace.

`xtask` is not part of the compiler runtime.  It hosts workflows that compare
Rust output against the C++ reference compiler, regenerate golden snapshots, and
produce parity/diff reports.

## Usage

```
cargo run -p xtask -- <command>
```

## Commands

| Command | Description |
|---|---|
| `golden-check` | Verify Rust golden snapshots are up to date |
| `golden-check-cpp` | Verify Rust output matches C++ reference goldens |
| `golden-gen-rust` | Regenerate Rust golden snapshots from current output |
| `golden-gen-cpp` | Regenerate C++ reference goldens (requires `FAUST_CPP_BIN`) |
| `interp-trace-dump` | Phase 1 runtime trace harness: execute one DSP through `interp` and dump JSON trace |
| `parser-parity-report` | Write parser parity report vs C++ |
| `corpus-status-report` | Write corpus status diff report |
| `cpp-backend-diff-report` | Write C++ backend diff report |
| `c-fastlane-diff-report` | Write C fast-lane diff report |
| `backend-full-corpus-diff-report` | Write full corpus diff for all backends |
| `table-fastlane-diff-report` | Write table fast-lane diff report |

## Environment variables

| Variable | Used by | Description |
|---|---|---|
| `FAUST_CPP_BIN` | `golden-gen-cpp` | Path to reference C++ `faust` binary |
| `GOLDEN_REF` | `golden-check` | `rust` (default) or `cpp` |

## Design invariants

- Deterministic corpus file ordering (sorted).
- Normalized output text before snapshot comparison (CRLF → LF).
- Fail-fast: first diverging case aborts the run to keep CI signal clean.

## `interp-trace-dump` (Phase 1)

Minimal runtime trace harness prototype for continuous validation planning.

Example:

```bash
cargo run -p xtask -- interp-trace-dump \
  --case tests/corpus/rep_31_extended_primitives.dsp \
  --scenario impulse \
  --lane fast
```

Current scope (Phase 1):
- compiles one DSP through the Rust compiler pipeline to FIR
- builds an `interp` factory via Rust APIs (no CLI output parsing)
- runs deterministic inputs (`zeros`, `impulse`, `ramp`, `sine`)
- prints a JSON trace (stdout or `--out <path>`)

This command is intentionally a prototype and does not yet implement snapshot
generation/checking (`interp-trace-gen` / `interp-trace-check` planned in later phases).
