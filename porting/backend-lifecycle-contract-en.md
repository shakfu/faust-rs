# Backend Lifecycle Contract

Date: 2026-06-15

This document defines the backend lifecycle contract that every generated,
interpreted, or JIT backend must preserve. The reference behavior is the Faust
C++ backend lifecycle.

## Reference Contract

The Faust C++ backend emits this lifecycle shape:

```cpp
virtual void init(int sample_rate) {
    classInit(sample_rate);
    instanceInit(sample_rate);
}

virtual void instanceInit(int sample_rate) {
    instanceConstants(sample_rate);
    instanceResetUserInterface();
    instanceClear();
}
```

`classInit` belongs to `init`, not to `instanceInit`. `instanceClear` is the
generated FIR clear body; backend runtimes must not add a second clearing policy
based on field names or local runtime heuristics.

## Required Invariants

1. `init(sample_rate)` calls `classInit(sample_rate)` before
   `instanceInit(sample_rate)`.
2. `instanceInit(sample_rate)` calls only the instance lifecycle phases, in this
   order:
   `instanceConstants(sample_rate)`, `instanceResetUserInterface()`,
   `instanceClear()`.
3. `instanceInit(sample_rate)` must not call `classInit(sample_rate)`.
4. Backend runtimes must not duplicate `instanceClear` with a runtime-side field
   clearing policy. Missing clear behavior must be fixed in FIR lowering or in
   the compiled lifecycle body.
5. When a backend has a compiled `instanceConstants` body, that body is the
   lifecycle authority. Descriptor/replay initialization is allowed only as a
   fallback when no compiled body exists.

## Required Tests For New Backends

Every new backend must add a lifecycle conformance test before it is added to
impulse, golden, or parity gates.

For source-emitting backends, the test must inspect generated code and prove:

- `init` calls `classInit` before `instanceInit`;
- `instanceInit` calls `instanceConstants`, `instanceResetUserInterface`, and
  `instanceClear` in that order;
- the `instanceInit` body does not contain a `classInit` call.

For interpreted, bytecode, JIT, or runtime-backed backends, the test must
execute an order-sensitive fixture and prove:

- `init` executes `classInit`, then `instanceConstants`, then
  `instanceResetUserInterface`, then `instanceClear`;
- direct `instanceInit` executes only `instanceConstants`,
  `instanceResetUserInterface`, then `instanceClear`;
- `instanceClear` delegates to the compiled/decoded lifecycle body and does not
  perform a parallel runtime clear.

A compact fixture is preferred: each lifecycle phase can append one digit to a
state slot, so `init` yields `1123` and direct `instanceInit` yields `123`.

## Existing Regression Tests

Current guardrails include:

- C++ fast-lane lifecycle order:
  `cargo test -p compiler --test signal_fir_lane fastlane_cpp_lifecycle_order_matches_faust_instance_init_flow`
- C fast-lane lifecycle order:
  `cargo test -p compiler --test signal_fir_lane fastlane_c_lifecycle_order_matches_faust_instance_init_flow`
- interpreter runtime lifecycle order:
  `cargo test -p codegen --lib backends::interp::instance::tests::instance_lifecycle_order_matches_cpp_backend_contract`
- interpreter FBC-to-C++ scaffold:
  `cargo test -p codegen --lib backends::interp::fbc_to_cpp::tests::generate_lifecycle_matches_cpp_backend_contract`
- Cranelift FFI scaffold:
  `cargo test -p cranelift-ffi lifecycle_scaffold_matches_faust_cpp_backend_contract`

## Review Rule

Do not accept backend-specific fixes that compensate for one failing DSP by
changing lifecycle behavior locally. If a backend diverges from this contract,
first compare with the Faust C++ generated code, then fix the shared FIR
lifecycle body or the backend's implementation of this contract.
