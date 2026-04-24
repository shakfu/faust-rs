# tests

Workspace-level integration tests, fixtures, and differential harnesses.

## Main fixture areas

| Path | Purpose |
|---|---|
| `corpus/` | Default compile/parity corpus used by golden checks |
| `golden/rust/` | Rust reference snapshots for CI `golden-check` |
| `golden/cpp/` | C++ reference snapshots for long-run parity checks |
| `runtime_corpus/` | Curated DSPs for interpreter runtime trace validation |
| `runtime_traces/` | Persisted runtime trace snapshots |
| `eval_micro_fixtures/` | Focused eval/propagate differential reproducers |
| `cpp_parity_known_gaps/` | Historical focused C++ parity reproducers |
| `runtime_corpus_known_failures/` | Documented runtime repros excluded from default trace discovery |

## Common commands

```bash
cargo run -p xtask -- golden-check
cargo run -p xtask -- golden-check-cpp
cargo run -p xtask -- interp-trace-check
```

Use `FAUST_CPP_BIN=/path/to/faust` for commands that need the C++ reference
compiler.
