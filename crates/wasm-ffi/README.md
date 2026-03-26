# wasm-ffi

Raw WASM export surface for the Rust-backed `faustwasm` embedded-compiler path.

## Role

`wasm-ffi` is the thin binding crate that sits on top of the pure Rust compile
service in [`compiler`](../compiler/). It is intended to be compiled as a
standalone `wasm32-unknown-unknown` module and loaded from `faustwasm`.

The exported ABI is intentionally small:

- one compile request in
- one owned result handle out
- raw pointer/length accessors for `{ wasm, json, compile_options }`
- helper text-result calls for `getInfos(...)` and the current explicit stubs

## Build the compiler module

From the workspace root:

```bash
cargo run -p xtask -- build-faustwasm-compiler-module
```

This command:

- builds `wasm-ffi` for `wasm32-unknown-unknown`
- verifies that the emitted module exports the raw ABI expected by the
  `faustwasm` Rust adapter
- prints the output path under `target/wasm32-unknown-unknown/`

Use `--debug` to build the non-release artifact:

```bash
cargo run -p xtask -- build-faustwasm-compiler-module --debug
```

## Current scope

- implemented:
  - compile DSP source to `{ wasm, json }`
  - `getInfos("version" | "help")`
- explicit stubs:
  - `expandDSP(...)`
  - `generateAuxFiles(...)`
  - path-oriented `getInfos(...)` keys not yet backed by a Rust parity source

## Expected artifact

The default release build emits:

```text
target/wasm32-unknown-unknown/release/faust_wasm_ffi.wasm
```

That `.wasm` file is the compiler-module artifact that the `faustwasm`
embedded-compiler path is expected to load.
