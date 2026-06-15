# Known failures and tolerance overrides

Baseline characterized 2026-06-14 (93 DSPs, `-double`, default tolerance
`2e-06`). The machinery for these lives in [`known.mk`](known.mk):

- `PRECISION_<dsp>` — a looser `filesCompare` tolerance for a genuinely bounded
  rounding band (applied to every backend; the C/C++ backends already pass these
  at delta 0, so it only affects the cases that need it).
- `KNOWN_FAIL_<backend>` / `KNOWN_FAIL_all` — cases excluded from a backend's
  default pass/fail gate. They are not built by the aggregate targets; build one
  explicitly (`make ir/interp/sound.ir`) to reproduce the failure.

When a divergence is fixed, remove the entry so the gate re-covers it.

## Shared compile gap

| DSP | Backends | Cause |
|---|---|---|
| `subcontainer1` | all | faust-rs sub-container codegen gap (does not compile) |

## Tolerance overrides (bounded rounding)

| DSP | Tol | Max \|Δ\| | Where | Note |
|---|---|---|---|---|
| `mixer` | 1e-5 | 6e-6 | pass 1 | smoothed-gain init |
| `cubic_distortion` | 1e-4 | 1.4e-5 | pass 1 | |
| `gate_compressor` | 1e-3 | 2e-4 | pass 1 | |
| `vcf_wah_pedals` | 1e-3 | 1.45e-4 | pass 1 | |
| `harpe` | 1e-5 | 2e-6 | poly pass | C backend |
| `noise` | 1e-5 | 2e-6 | poly pass | C backend (not an LCG bug) |
| `noiseabs` | 1e-5 | 3e-6 | poly pass | C backend |
| `comb_bug_exp` | 1e-3 | 1.1e-4 | poly pass | C backend |

## C backend — excluded

| DSP | Max \|Δ\| | Where | Likely cause |
|---|---|---|---|
| `grain3` | 2.6e-3 | pass 1 (frame ~14k) | grain/table path drift |

## Interpreter backend — excluded

The C++ backend reproduces all of these exactly, so they are interpreter-runtime
divergences, not DSP or harness issues.

Structural (max \|Δ\| ≈ 1):

| DSP | Cause |
|---|---|
| `comb_delay1`, `comb_delay2` | delay line emits silence where the reference has the comb echo |
| `math_simp` | math primitive divergence (output 24) |
| `norm3` | primitive divergence (output 2) |
| `UITester` | UI/button default semantics |
| `sound` | soundfile not supported by the interpreter runtime |

Numerical drift (max \|Δ\| 5e-3 … 1e-1, recursive/filter paths):

| DSP | Max \|Δ\| |
|---|---|
| `virtual_analog_oscillators` | 6.1e-2 |
| `carre_volterra` | 9.9e-2 |
| `parametric_eq` | 1.7e-2 |
| `reverb_designer` | 1.0e-2 |
| `phaser_flanger` | 8.3e-3 |
| `spectral_tilt` | 5.8e-3 |
| `tester` | 5.5e-3 |
| `reverb_tester` | 4.9e-3 |
