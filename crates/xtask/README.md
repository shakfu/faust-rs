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
| `interp-trace-gen` | Phase 2 scaffold: generate runtime trace snapshots for `tests/runtime_corpus/` |
| `interp-trace-check` | Phase 2 scaffold: compare runtime traces against generated snapshots (tolerant float compare) |
| `interp-trace-diff-lanes` | Phase 3 scaffold: compare `legacy` vs `fast-lane` runtime traces |
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

Optional guardrail:
- `--strict-fir-types` re-runs the FIR verifier and rejects traces when
  type-related FIR diagnostics are present (including warnings such as
  `FIR-B03`), preventing misleading runtime results on under-typed FIR.

## `interp-trace-gen` / `interp-trace-check` (Phase 2 scaffold)

Phase 2 has started with a simple snapshot workflow built on top of
`interp-trace-dump`.

Examples:

```bash
cargo run -p xtask -- interp-trace-gen
cargo run -p xtask -- interp-trace-check
```

Current Phase 2 scaffold behavior:
- iterates `tests/runtime_corpus/*.dsp`
- uses a built-in scenario mapping (documented in `tests/runtime_corpus/README.md`)
- writes snapshots under `tests/runtime_traces/rust/<case>/<scenario>.json`
- checks by regenerating traces and comparing parsed traces with tolerance-based
  float comparison (metadata/shape must still match exactly)
- supports `--strict-fir-types` to enforce a clean FIR typing subset during
  generation/check runs

## `interp-trace-diff-lanes` (Phase 3 scaffold)

Compares runtime traces produced by the `legacy` and `fast-lane` signal->FIR
lowerings on the snapshot-enabled runtime corpus subset.

Current scaffold behavior:
- reuses the same fixture/scenario mapping as Phase 2
- compares traces with the same tolerance-based float comparator
- skips cases when one lane currently panics/errors (prints a skip reason)
- supports `--strict-fir-types` to reject lanes that only verify with
  type-related warnings/errors
