# faust-rs

Rust workspace for the Faust compiler port.

[![CI](https://github.com/sletz/faust-rs/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/sletz/faust-rs/actions/workflows/ci.yml)

## Repository contents

- `porting/faust-rust-porting-plan-en.md`: full porting plan
- `porting/faust-rust-points-critiques-en.md`: critical technical points and risks
- `porting/faust-rust-recursion-model-note-en.md`: recursion model analysis (`sigRec/sigProj` vs RouteIR rec groups)
- `porting/faust-rust-bilan-effort-en.md`: effort assessment
- `porting/faust-rust-bilan-global-en.md`: overall status summary
- `porting/faust-rust-error-flow-en.md`: concise parser -> eval -> propagate error flow
- `porting/phases/`: detailed phase-by-phase execution notes (`phase-0` to `phase-9`)
- `docs/user-diagnostics-guide-en.md`: user-oriented diagnostics guide

## Suggested reading order

1. `porting/faust-rust-porting-plan-en.md`
2. `porting/faust-rust-points-critiques-en.md`
3. `porting/phases/phase-0-validation-en.md`
4. Remaining files in `porting/phases/` in numeric order

## How to compile

Compile all crates in debug mode:

```bash
cargo build --workspace
```

Compile all crates in release mode:

```bash
cargo build --workspace --release
```

Compile only the compiler binary crate:

```bash
cargo build -p compiler
```

Run the scaffold CLI binary:

```bash
cargo run -p compiler
```

Run parser/signal diagnostics with explicit output controls:

```bash
# Human diagnostics (default), concise note stream
cargo run -p compiler -- --dump-sig tests/corpus/err_03_propagate_split_mismatch.dsp --error-format human --error-verbosity standard

# Human diagnostics with internal debug notes
cargo run -p compiler -- --dump-sig tests/corpus/err_03_propagate_split_mismatch.dsp --error-format human --error-verbosity debug

# JSON diagnostics (stable contract)
cargo run -p compiler -- --dump-sig tests/corpus/err_03_propagate_split_mismatch.dsp --error-format json

# JSON diagnostics with debug enrichment (`diagnostics[*].debug`)
cargo run -p compiler -- --dump-sig tests/corpus/err_03_propagate_split_mismatch.dsp --error-format json --error-verbosity debug
```

See `docs/user-diagnostics-guide-en.md` for a quick reading guide of `cause/rule/computed/help`.

## C++ golden outputs

Corpus and golden layout:

- `tests/corpus/*.dsp`: input DSP corpus
- `tests/golden/rust/<case>/compiler_stdout.txt`: current Rust scaffold reference used by CI
- `tests/golden/cpp/<case>/compiler_stdout.txt`: C++ Faust reference outputs (parity target)
- `tests/golden/METADATA.toml`: pinned reference metadata and command policy

Commands:

```bash
# Check Rust output against stored golden references
cargo run -p xtask -- golden-check

# Check Rust output against C++ reference goldens (expected to fail until parity)
cargo run -p xtask -- golden-check-cpp

# Generate corpus-wide C++ vs Rust status differential report
cargo run -p xtask -- corpus-status-report

# Bootstrap/update golden files from current Rust scaffold output
cargo run -p xtask -- golden-gen-rust

# Update golden files from C++ Faust reference binary
FAUST_CPP_BIN=/path/to/faust cargo run -p xtask -- golden-gen-cpp -- <extra-args>
```

Note: CI runs `cargo run -p xtask -- golden-check` (Rust reference mode) on every platform.
