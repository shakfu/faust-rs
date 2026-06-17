# Runtime Trace Corpus (Phase 1)

Curated DSPs used by the `xtask interp-trace-dump` prototype and upcoming
runtime trace snapshot checks.

Goal: favor cases that produce well-typed FIR in the fast-lane and execute
reliably through the `interp` backend.

The source of truth for the current strict validation subset is:

- `tests/runtime_corpus/METADATA.toml` (`[strict_safe_cases]`)

## Files and suggested scenarios

- `trace_01_passthrough.dsp`: `impulse`, `ramp`
- `trace_02_gain_bias_typed.dsp`: `impulse`
- `trace_03_stereo_mix.dsp`: `impulse`
- `trace_07_nonlinear_clip.dsp`: `sine`
- `trace_09_ui_slider.dsp`: `impulse`
- `trace_22_parallel_mix.dsp`: `impulse`
- `trace_31_extended_primitives_typed.dsp`: `zeros`
- `trace_38_sine_phasor.dsp`: `zeros`
- `trace_40_int_plus_one.dsp`: `ramp`

Notes:
- `*_typed` variants force float literals in places where the current fast-lane
  may otherwise emit under-typed math calls (e.g. `abs/min/max` on integer
  literals in the extended-primitives corpus case).
- These files are trace-harness fixtures; the main compile/parity corpus remains
  in `tests/corpus/`.
- Phase 2 snapshot scaffold (`interp-trace-gen` / `interp-trace-check`) only
  enables a subset for now (currently `trace_01_passthrough`,
  `trace_09_ui_slider`, `trace_31_extended_primitives_typed`,
  `trace_40_int_plus_one`) and skips the others until known fast-lane FIR
  typing issues are fixed.
- The same subset is currently the recommended starting point for runtime
  semantic validation before introducing a Faust C++ differential oracle.
