# Runtime Trace Known Failures (Documented / Excluded)

This folder contains small DSP fixtures used to document known runtime issues in
the current Rust `interp` backend or surrounding FIR/runtime validation path.

These files are intentionally **excluded** from `xtask` runtime trace corpus
discovery, which only scans `tests/runtime_corpus/*.dsp`.

## Current entries

- `int_plus_one_interp_stack_bug.dsp`
  - DSP: `process = int(_) + 1;`
  - status:
    - FIR verifier reports no error, but emits type warning `FIR-B03`
    - `xtask interp-trace-dump` (without `--strict-fir-types`) can reach a
      runtime panic in the `interp` executor (`StoreOutput` expects a real value
      but the stack state is inconsistent)
  - purpose:
    - preserve a minimal repro
    - keep it visible while excluded from the semantic validation subset

### Reproduce

```bash
cargo run -p xtask -- interp-trace-dump \
  --case tests/runtime_corpus_known_failures/int_plus_one_interp_stack_bug.dsp \
  --scenario ramp \
  --lane fast \
  --num-blocks 1
```

### Guarded mode (expected early rejection)

```bash
cargo run -p xtask -- interp-trace-dump \
  --case tests/runtime_corpus_known_failures/int_plus_one_interp_stack_bug.dsp \
  --scenario ramp \
  --lane fast \
  --num-blocks 1 \
  --strict-fir-types
```

