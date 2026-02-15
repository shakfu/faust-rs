# Phase 0 — TreeArena Benchmark Report (Initial Rust Pass)

> Scope: Gate A initial benchmark run for parser-driven `tlib-core` operations.
> Rust workspace: `faust-rs`
> C++ source of truth: `/Users/letz/Developpements/RUST/faust`

## 1. Benchmark harness

Harness location:
- `crates/tlib/src/bin/treearena_bench.rs`

Run command:
- `cargo run -p tlib --bin treearena_bench -- 200000`

Measured operations:
- create/intern pass (new nodes),
- repeated intern lookup pass (expected interner hits),
- list traversal pass (`cons`/`tl` chain),
- property set/get passes (`PropertyStore`).

## 2. Initial Rust results

Run output snapshot:

- `n=200000`
- `create_ms=674.245`
- `lookup_ms=331.478`
- `traversal_ms=376.075`
- `property_set_ms=149.930`
- `property_get_ms=85.656`
- `arena_nodes=600002`

## 3. Gate A status

Status: **conditional (in progress)**.

What is validated:
- operations are implemented and measurable on realistic cardinality,
- no correctness failures observed in `tlib` semantic tests,
- benchmark harness is now reproducible from the workspace.

What is still required for Gate A closure:
- run an equivalent C++ baseline benchmark on `/Users/letz/Developpements/RUST/faust` workloads,
- compute Rust/C++ ratios per operation (target: `<= 2x`),
- document memory-growth comparison and mitigation if needed.

## 4. Next actions

1. Implement/collect C++ benchmark numbers for matching workloads.
2. Add ratio table (Rust vs C++) in this report.
3. Record final Gate A decision (`Go` / `Conditional Go` / `No-Go`) with owner/date.
