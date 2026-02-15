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
- `create_ms=89.449`
- `lookup_ms=74.758`
- `traversal_ms=58.153`
- `property_set_ms=21.451`
- `property_get_ms=17.376`
- `arena_nodes=600002`

C++ (`-O3`):
- `create_ms=71.635`
- `lookup_ms=60.259`
- `traversal_ms=78.529`
- `property_set_ms=35.460`
- `property_get_ms=1.433`

Rust/C++ ratio:
- `create`: `1.249x`
- `lookup`: `1.241x`
- `traversal`: `0.741x`
- `property_set`: `0.605x`
- `property_get`: `12.126x`

## 3. Gate A decision

Status: **Conditional Go** (2026-02-15).

Accepted:
- create/lookup/traversal/property-set are within target envelope (`<= 2x`) or faster than C++,
- benchmark process is reproducible on both Rust and C++ baselines.

Blocking hotspot to clear before Gate A final closure:
- `property_get` is currently `12.126x` slower than C++ baseline.

## 4. Required follow-up

1. Align Rust property key model with C++ usage pattern (tree-key lookup path) and re-measure.
2. Re-run both harnesses with same workload (`n=200000`) and update ratios.
3. Convert Gate A from `Conditional Go` to `Go` only when `property_get <= 2x` or with an approved mitigation and explicit ownership/date.
