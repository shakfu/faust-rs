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
