# faust-rs

Rust workspace for the Faust compiler port.

[![CI](https://github.com/sletz/faust-rs/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/sletz/faust-rs/actions/workflows/ci.yml)

## Repository contents

- `porting/faust-rust-porting-plan-en.md`: full porting plan
- `porting/faust-rust-points-critiques-en.md`: critical technical points and risks
- `porting/faust-rust-recursion-model-note-en.md`: recursion model analysis (`sigRec/sigProj` vs RouteIR rec groups)
- `porting/faust-rust-bilan-effort-en.md`: effort assessment
- `porting/faust-rust-bilan-global-en.md`: overall status summary
- `porting/phases/`: detailed phase-by-phase execution notes (`phase-0` to `phase-9`)

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

## C++ golden outputs

Corpus and golden layout:

- `tests/corpus/*.dsp`: input DSP corpus
- `tests/golden/cpp/<case>/compiler_stdout.txt`: expected reference output per case
- `tests/golden/METADATA.toml`: pinned reference metadata and command policy

Commands:

```bash
# Check Rust output against stored golden references
cargo run -p xtask -- golden-check

# Bootstrap/update golden files from current Rust scaffold output
cargo run -p xtask -- golden-gen-rust

# Update golden files from C++ Faust reference binary
FAUST_CPP_BIN=/path/to/faust cargo run -p xtask -- golden-gen-cpp -- <extra-args>
```

Note: CI runs `cargo run -p xtask -- golden-check` on every platform.
