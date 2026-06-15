# Backend Lifecycle C++ Parity Plan

Date: 2026-06-15

## Context

The impulse-test divergences in the interpreter and Cranelift backends exposed a
process risk: backend runtimes can silently drift from the lifecycle emitted by
the Faust C++ compiler. Local backend fixes must not introduce ad-hoc runtime
policies that compensate for one DSP while contradicting the reference backend.

The external Faust C++ backend was checked with:

```sh
/Users/letz/Developpements/RUST/faust/build/bin/faust \
  tests/corpus/rep_10_two_in_two_out_ui.dsp \
  -o /private/tmp/faust_cpp_lifecycle.cpp
```

The generated C++ lifecycle contract is:

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
generated FIR clear body; other backends must not add a parallel clear policy
based on field-name heuristics.

## Required Invariants

1. `init(sample_rate)` calls `classInit(sample_rate)` before
   `instanceInit(sample_rate)`.
2. `instanceInit(sample_rate)` calls only the instance lifecycle phases, in this
   order:
   `instanceConstants(sample_rate)`, `instanceResetUserInterface()`,
   `instanceClear()`.
3. `instanceInit(sample_rate)` must not call `classInit(sample_rate)`.
4. Backend runtimes must not duplicate `instanceClear` with a second
   runtime-side field clearing policy. Missing behavior must be fixed in FIR
   lowering or the compiled lifecycle body.
5. When a backend has a compiled `instanceConstants` body, that body is the
   lifecycle authority. Descriptor/replay initialization is allowed only as a
   fallback when no compiled body exists.

## Implementation Plan

1. Align the interpreter runtime:
   - move `class_init(sample_rate)` from `instance_init` to `init`;
   - keep `instance_init` as `instance_constants`, reset UI, clear.
2. Align the interpreter FBC-to-C++ backend:
   - emit `classInit(sample_rate)` in `init`;
   - keep `instanceInit` free of `classInit`.
3. Align the Cranelift FFI runtime:
   - move `class_init_instance` from `instanceInit` to `init`;
   - make `instanceClear` execute only the compiled JIT `instanceClear` body;
   - use descriptor constant initialization only when the JIT
     `instanceConstants` body is absent.
4. Add regression tests:
   - generated C++ and C lifecycle order tests must check both `init` and
     `instanceInit`;
   - interpreter runtime test must execute order-sensitive lifecycle bytecode;
   - interpreter FBC-to-C++ test must check the emitted lifecycle scaffold;
   - Cranelift FFI test must guard lifecycle scaffolding and reject a
     runtime-side clear policy.
5. Update the impulse known-failure list when a fixed case is no longer
   excluded.

## Validation

Minimum targeted validation for this plan:

```sh
cargo fmt --all
cargo test -p codegen --lib backends::interp
cargo test -p cranelift-ffi lifecycle_scaffold_matches_faust_cpp_backend_contract
cargo test -p cranelift-ffi runtime_descriptor_tracks_sample_rate_fields
cargo test -p compiler --test signal_fir_lane fastlane_cpp_lifecycle_order_matches_faust_instance_init_flow
cargo test -p compiler --test signal_fir_lane fastlane_c_lifecycle_order_matches_faust_instance_init_flow
cargo build --release -p compiler -p impulse-runner -p cranelift-ffi --bin impulse_cranelift --bin impulse-runner
make -C tests/impulse-tests -f Make.cranelift ir/cranelift/table2.ir
```

Broader follow-up validation remains the standard workspace gate:

```sh
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo run -p xtask -- golden-check
```

