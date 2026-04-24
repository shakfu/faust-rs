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

# Raw Rust compiler module for faustwasm embedded-compiler mode
cargo run -p xtask -- build-faustwasm-compiler-module
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

# Generate interpreter bytecode (.fbc)
faust-rs -lang interp foo.dsp
# alias: -lang interp-fbc
# shorthand flag: --dump-interp

# Dump FIR text IR
faust-rs -lang fir foo.dsp

# Write output to a file
faust-rs -lang cpp foo.dsp -o foo.cpp
faust-rs -lang interp foo.dsp -o foo.fbc
```

Built-in FIR backend fixtures (for backend debugging / bring-up):

```bash
# List internal FIR fixtures
faust-rs --list-fir-fixtures

# Dump a built-in FIR fixture
faust-rs --fir-fixture sine_phasor -lang fir

# Generate backend output directly from a built-in FIR fixture
faust-rs --fir-fixture control_flow -lang c
faust-rs --fir-fixture gain_bias_ui_meta -lang cpp
faust-rs --fir-fixture sine_phasor -lang interp
faust-rs --fir-fixture sine_phasor -lang cranelift
```

Notes:

- `--fir-fixture` bypasses the Faust front-end pipeline and feeds a hand-written
  FIR module from `codegen::fixtures` directly into the selected backend.
- It is intended for backend debugging and parity bring-up, not end-user DSP
  compilation workflows.

If your installed command is named `faust` (for example via a symlink/wrapper),
the same model applies:

```bash
faust -lang c foo.dsp
faust -lang cpp foo.dsp
faust -lang interp foo.dsp
faust -lang fir foo.dsp
```

Without installation (equivalent):

```bash
cargo run -p compiler -- -lang c foo.dsp
cargo run -p compiler -- -lang cpp foo.dsp
cargo run -p compiler -- -lang interp foo.dsp
cargo run -p compiler -- -lang fir foo.dsp
```

## Documentation

- User CLI reference: `docs/user-cli-guide-en.md`
- User diagnostics guide: `docs/user-diagnostics-guide-en.md`
- Technical/developer workflows: `docs/developer-workflows-en.md`
- Raw `faustwasm` compiler-module build notes: `crates/wasm-ffi/README.md`

## Workspace crates

| Crate | Role |
|---|---|
| `tlib` | Hash-consed tree arena, symbols, lists, recursive tree helpers |
| `errors` | Structured diagnostics model |
| `interval` | Interval arithmetic |
| `algebra` | Shared algebra/rewrite scaffold |
| `graph` | Shared graph algorithms scaffold |
| `boxes` | Faust box IR builders and matchers |
| `parser` | Faust source parser and import handling |
| `signals` | Faust signal IR builders, matchers, and extended math nodes |
| `ui` | Grouped UI IR |
| `eval` | Box-level evaluator and pattern matcher |
| `propagate` | Box-to-signal propagation |
| `normalize` | Signal normalization and preparation helpers |
| `sigtype` | Signal type lattice and inference |
| `transform` | Signal preparation and signal-to-FIR lowering |
| `fir` | Faust Intermediate Representation |
| `foreign-call` | Raw C ABI foreign-function invocation bridge |
| `codegen` | C, C++, interpreter, Cranelift, and WASM backend generation |
| `draw` | Diagram rendering scaffold |
| `doc` | Documentation/reporting scaffold |
| `utils` | Shared FFI utilities |
| `compiler` | Top-level compiler facade and CLI |
| `xtask` | Developer and CI automation |
| `interp-ffi` | Interpreter backend C/C++ API |
| `cranelift-ffi` | Experimental Cranelift backend C/C++ API |
| `box-ffi` | Box manipulation C/C++ API |
| `faust-ffi` | Unified `libfaust` distribution crate |
| `wasm-ffi` | Raw WASM ABI for `faustwasm` embedded compiler mode |

## Generate API docs

Generate Rustdoc for workspace crates only (recommended):

```bash
cargo doc --workspace --no-deps
```

Generate Rustdoc including dependencies:

```bash
cargo doc --workspace
```

Open the generated HTML entry point:

```bash
open target/doc/index.html
```

Crate-specific entry point example:

- `target/doc/compiler/index.html`

## Porting references

- Porting plan: `porting/faust-rust-porting-plan-en.md`
- Critical points: `porting/faust-rust-points-critiques-en.md`
- Phases: `porting/phases/`
- Supported Faust subset: `porting/faust-rs-supported-faust-subset-en.md`
- Porting journal index: `JOURNAL.md`
