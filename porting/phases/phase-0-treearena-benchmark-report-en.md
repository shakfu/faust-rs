# Phase 0 — TreeArena Benchmark Report (Rust/C++ Baseline)

> Scope: Gate A benchmark run for parser-driven `tlib-core` operations.
> Rust workspace: `faust-rs`
> C++ source of truth: `/Users/letz/Developpements/RUST/faust`

## 1. Benchmark harnesses

Rust harness:
- `crates/tlib/src/bin/treearena_bench.rs`
- command: `cargo run -p tlib --release --bin treearena_bench -- 200000`

C++ harness:
- `porting/tools/treearena_cpp_bench.cpp`
- build command:
  - `INCLUDES=$(find /Users/letz/Developpements/RUST/faust/compiler -type d | sed 's#^#-I#' | tr '\n' ' ')`
  - `c++ -std=c++17 -O3 -DNDEBUG $INCLUDES porting/tools/treearena_cpp_bench.cpp /Users/letz/Developpements/RUST/faust/compiler/tlib/symbol.cpp /Users/letz/Developpements/RUST/faust/compiler/tlib/node.cpp /Users/letz/Developpements/RUST/faust/compiler/tlib/tree.cpp -o target/bench/treearena_cpp_bench`
- run command: `./target/bench/treearena_cpp_bench 200000`

Measured operations:
- create/intern pass (new nodes),
- repeated intern lookup pass (expected interner hits),
- list traversal pass (`cons`/`tl` chain),
- property set/get passes.

## 2. Results (`n=200000`)

Rust (`--release`):
- `create_ms=58.701`
- `lookup_ms=45.905`
- `traversal_ms=33.444`
- `property_set_ms=2.469`
- `property_get_ms=1.829`
- `arena_nodes=600002`

C++ (`-O3`):
- `create_ms=78.483`
- `lookup_ms=60.262`
- `traversal_ms=77.944`
- `property_set_ms=35.679`
- `property_get_ms=1.436`

Rust/C++ ratio:
- `create`: `0.748x`
- `lookup`: `0.762x`
- `traversal`: `0.429x`
- `property_set`: `0.069x`
- `property_get`: `1.274x`

## 3. Gate A decision

Status: **Go** (2026-02-15).

Accepted:
- create/lookup/traversal/property-set/property-get are within target envelope (`<= 2x`) or faster than C++,
- benchmark process is reproducible on both Rust and C++ baselines.

Implemented optimization that unblocked Gate A:
- `PropertyStore` now uses interned property keys + direct vector slots indexed by `TreeId` for hot-path lookup (`set_with_key`/`get_with_key`).
- `TreeArena` now uses shared string payloads (`Arc<str>`) in `NodeKind` and arity-specialized interning maps for arity `0/1/2`.
- This removed repeated string/key allocation pressure and brought:
  - `lookup` from `1.524x` to `0.762x`,
  - `create` from `1.331x` to `0.748x`,
  - while keeping `property_get` under threshold (`1.274x`).

## 4. Required follow-up

1. Keep tracking memory footprint tradeoff of sparse high `TreeId` spaces (vector slots per property key).
2. Preserve this benchmark in Phase 0 evidence when `TreeArena` internals evolve.
