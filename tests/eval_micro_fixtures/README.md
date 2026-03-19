# Eval Micro-Fixtures

This folder contains tiny DSP fixtures used to differentially guard the
`eval -> propagate` boundary against the Faust C++ compiler.

The intent is not to replace the larger corpus under `tests/corpus/`, but to
lock the reduced forms that previously surfaced only through much larger
library programs.

Current fixture families:

- `eval_01_inputs_residual_closure.dsp`
  - `inputs(...)` over a residual closure, used as an iterator count.
- `eval_02_waveform_rdtable_leaf.dsp`
  - `waveform{...}` passed to `rdtable(...)`, guarding leaf semantics.
- `eval_03_seq_zero_neutral.dsp`
  - zero-iteration `seq` neutral element parity with C++ `neutralExpSeq`.
- `eval_04_case_exact_integer_real_match.dsp`
  - exact integer reals reused in `case`-matching contexts.
- `eval_05_route_arithmetic_params.dsp`
  - computed `route(...)` parameters with exact-integer real route leaves.

These fixtures are consumed by the compiler differential tests so they exercise:

- Rust `compile_file_default_to_signals(...)` / `--dump-sig`-equivalent lowering
- Faust C++ acceptance through `-norm` and `-lang cpp`

This makes `eval` parity debugging proactive instead of library-reproducer
driven.
