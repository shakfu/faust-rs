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
- `create_ms=95.306`
- `lookup_ms=71.726`
- `traversal_ms=56.583`
- `property_set_ms=2.575`
- `property_get_ms=1.621`
- `arena_nodes=600002`

C++ (`-O3`):
- `create_ms=71.606`
- `lookup_ms=47.066`
- `traversal_ms=65.282`
- `property_set_ms=34.472`
- `property_get_ms=1.502`

Rust/C++ ratio:
- `create`: `1.331x`
- `lookup`: `1.524x`
- `traversal`: `0.867x`
- `property_set`: `0.075x`
- `property_get`: `1.079x`

## 3. Gate A decision

Status: **Go** (2026-02-15).

Accepted:
- create/lookup/traversal/property-set/property-get are within target envelope (`<= 2x`) or faster than C++,
- benchmark process is reproducible on both Rust and C++ baselines.

Implemented optimization that unblocked Gate A:
- `PropertyStore` now uses interned property keys + direct vector slots indexed by `TreeId` for hot-path lookup (`set_with_key`/`get_with_key`).
- This removed repeated string allocation/hashing on get-path and brought `property_get` from `12.126x` to `1.079x`.

## 4. Required follow-up

1. Keep tracking memory footprint tradeoff of sparse high `TreeId` spaces (vector slots per property key).
2. Preserve this benchmark in Phase 0 evidence when `TreeArena` internals evolve.
