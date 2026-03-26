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

## Embedded Faust libraries

At build time, `wasm-ffi` can discover and embed a read-only bundle of Faust
`.lib` sources directly into the compiler-module. The current root discovery
order is:

- `FAUST_RS_EMBEDDED_LIB_ROOT`
- `FAUST_RS_FAUSTLIBRARIES_ROOT`
- first valid entry from `FAUST_LIB_PATH`
- `/usr/local/share/faust`
- `/usr/share/faust`

The embedded bundle is used for the Rust raw compiler path only. It allows:

- parser-side `import("stdfaust.lib")` from an in-memory DSP source
- evaluator-side `library("...")` / `component("...")` resolution against the
  same bundled logical sources

This keeps the `faustwasm` compiler-module self-contained for the standard
Faust libraries without recreating an Emscripten-style virtual filesystem.

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
