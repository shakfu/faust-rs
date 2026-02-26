# cranelift-ffi

C/C++ FFI export crate for the experimental Cranelift backend (`cranelift_dsp` family).

## Purpose

This crate hosts the Rust-side C ABI and C++ wrapper scaffolding for a Faust
backend implemented with Cranelift.

It mirrors the overall strategy used by `llvm_dsp` / `interpreter_dsp`:

- factory cache and factory lifecycle
- instance lifecycle and `compute`
- UI / metadata callback dispatch (`UIGlue` / `MetaGlue`)
- C API + C++ wrapper headers

## Current status (important)

This crate is **experimental** and still in scaffold/bring-up mode.

What is already implemented:

- executable C ABI scaffold
- factory/instance opaque types
- minimal factory cache
- source factory creation preflight through `compiler -> FIR -> codegen::cranelift`
- bitcode family scaffold (temporary text format, for API-path validation)
- header smoke tests (C and C++)

What is not final yet:

- full backend/runtime parity
- final Cranelift backend serialization format
- full C++ wrapper behavioral implementation parity
- full V1 export surface completeness (some families intentionally deferred)

## Crate outputs

`Cargo.toml` currently builds:

- `rlib` (needed for Rust examples/tests)
- `cdylib`
- `staticlib`

Library name:

- Rust lib target name: `faust_cranelift`

## Headers

Headers are in `include/`:

- `include/cranelift-dsp-c.h`
- `include/cranelift-dsp.h`

Related files:

- `cpp/cranelift-dsp.cpp` (C++ wrapper bridge scaffold)
- `tests/header-smoke/` (syntax smoke for C/C++ headers)

The headers document the currently exposed scaffold surface and explicitly list
V1-deferred families where relevant.

## Internal structure

- `src/types.rs`
  - opaque factory/instance wrappers
  - re-exports shared `UIGlue` / `MetaGlue`
- `src/cache.rs`
  - global factory cache wrappers over `utils::FactoryCache<T>`
- `src/factory.rs`
  - Cranelift factory `extern "C"` API
  - compiler/FIR preflight
  - scaffold bitcode family
- `src/instance.rs`
  - instance lifecycle / UI / metadata / compute scaffold exports
- `src/ui.rs`
  - UI/meta callback helpers

## Shared FFI helpers (factorized)

This crate uses shared backend-agnostic FFI helpers from `crates/utils`:

- `UIGlue` / `MetaGlue`
- C string allocation/free helpers
- `freeCMemory` string helper
- `argv` decoding
- error buffer writing (`4096` bytes)
- C-string argument decoding helpers
- empty `char**` helper
- minimal FFI option parsing (`-I <path>`, `-cn <name>`)

Cranelift-specific factory/runtime state and backend semantics remain local to
this crate.

## Factory creation paths

`createCCraneliftDSPFactoryFromFile` and `createCCraneliftDSPFactoryFromString`
share common FFI boilerplate (error handling, allocation, cache insertion), but
still keep distinct backend preflight paths:

- file path preflight preserves file-based import search semantics
- string preflight uses inline-source compilation path

This is intentional and matches the interpreter FFI refactor strategy.

## Build / test

Targeted checks:

```bash
cargo clippy -p cranelift-ffi --all-targets -- -D warnings
cargo test -p cranelift-ffi -- --nocapture
```

Header smoke checks (examples):

```bash
cc -fsyntax-only -I crates/cranelift-ffi/include -I /path/to/faust/architecture \
  crates/cranelift-ffi/tests/header-smoke/cranelift_dsp_c_header_smoke.c

c++ -std=c++11 -fsyntax-only -I crates/cranelift-ffi/include -I /path/to/faust/architecture \
  crates/cranelift-ffi/tests/header-smoke/cranelift_dsp_cpp_header_smoke.cpp
```

## Known limitations

- Some LLVM-specific API families are intentionally omitted/deferred in V1
  (target getters, LLVM IR/machine/object serialization, memory-manager hooks,
  foreign-function registration).
- The bitcode API family currently uses a temporary scaffold format marker
  (`CRANELIFT_FFI_SCAFFOLD_V1`) and is not the final backend serialization.
- Runtime behavior is still progressing toward a fully operational Cranelift
  backend path.

## Related planning docs

- `porting/cranelift-backend-plan-en.md`
- `porting/cranelift-dsp-ffi-parity-matrix-en.md`

