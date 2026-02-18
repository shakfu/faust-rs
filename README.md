# faust-rs

Rust workspace for the Faust compiler port.

[![CI](https://github.com/sletz/faust-rs/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/sletz/faust-rs/actions/workflows/ci.yml)

## Build

```bash
# Entire workspace
cargo build --workspace

# Entire workspace (release)
cargo build --workspace --release

# Compiler binary crate only
cargo build -p compiler

# Compiler binary crate only (release)
cargo build -p compiler --release
```

## Install

```bash
# Install the `faust-rs` binary into Cargo's bin directory
cargo install --path crates/compiler
```

## Use faust-rs

```bash
# Run without installation (from the repository)
cargo run -p compiler

# Run the installed binary
faust-rs
```

DSP compilation examples:

```bash
# Generate C
faust-rs -lang c foo.dsp

# Generate C++
faust-rs -lang cpp foo.dsp

# Dump FIR text IR
faust-rs -lang fir foo.dsp

# Write output to a file
faust-rs -lang cpp foo.dsp -o foo.cpp
```

If your installed command is named `faust` (for example via a symlink/wrapper),
the same model applies:

```bash
faust -lang c foo.dsp
faust -lang cpp foo.dsp
faust -lang fir foo.dsp
```

Without installation (equivalent):

```bash
cargo run -p compiler -- -lang c foo.dsp
cargo run -p compiler -- -lang cpp foo.dsp
cargo run -p compiler -- -lang fir foo.dsp
```

## Documentation

- User CLI reference: `docs/user-cli-guide-en.md`
- User diagnostics guide: `docs/user-diagnostics-guide-en.md`
- Technical/developer workflows: `docs/developer-workflows-en.md`

## Porting references

- Porting plan: `porting/faust-rust-porting-plan-en.md`
- Critical points: `porting/faust-rust-points-critiques-en.md`
- Phases: `porting/phases/`
