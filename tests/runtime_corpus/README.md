# Runtime Trace Corpus (Phase 1)

Curated DSPs used by the `xtask interp-trace-dump` prototype and upcoming
runtime trace snapshot checks.

Goal: favor cases that produce well-typed FIR in the fast-lane and execute
reliably through the `interp` backend.

## Files and suggested scenarios

- `trace_01_passthrough.dsp`: `impulse`, `ramp`
- `trace_02_gain_bias_typed.dsp`: `impulse`
- `trace_03_stereo_mix.dsp`: `impulse`
- `trace_07_nonlinear_clip.dsp`: `sine`
- `trace_09_ui_slider.dsp`: `impulse`
- `trace_22_parallel_mix.dsp`: `impulse`
- `trace_31_extended_primitives_typed.dsp`: `zeros`
- `trace_38_sine_phasor.dsp`: `zeros`

Notes:
- `*_typed` variants force float literals in places where the current fast-lane
  may otherwise emit under-typed math calls (e.g. `abs/min/max` on integer
  literals in the extended-primitives corpus case).
- These files are trace-harness fixtures; the main compile/parity corpus remains
  in `tests/corpus/`.
