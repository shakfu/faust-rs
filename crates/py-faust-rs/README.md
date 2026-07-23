# py-faust-rs (proof of concept)

Minimal PyO3/maturin bindings that expose the faust-rs **interpreter (FBC)
backend** to Python: compile a Faust `.dsp` source string and render audio
through the native Rust interpreter, with no C toolchain in the loop.

This crate is intentionally kept **out of the main workspace** (`exclude` in the
root `Cargo.toml`) so its `extension-module` linkage never affects
`cargo build --workspace` or CI. It path-depends on the `compiler` and `codegen`
crates.

## Binding path

```
Python  ──▶  compiler::Compiler               # .dsp source -> FBC bytecode text (fast lane)
        ──▶  read_fbc::<f32>                  # -> FbcDspFactory<f32>
        ──▶  OwnedFbcDspInstance::from_factory # persistent, factory-owning instance
        ──▶  .try_compute(...)                # -> rendered audio blocks (state persists)
```

The binding holds a `codegen::backends::interp::OwnedFbcDspInstance<f32>`, which
owns its factory alongside the runtime state (no lifetime, no self-referential
borrowing). As a result the binding contains **no hand-written `unsafe`** — the
persistent-instance machinery lives, fully safe and unit-tested, in the `codegen`
interpreter backend.

## Build

Requires a Rust toolchain and [uv](https://docs.astral.sh/uv/). uv manages the
virtualenv, builds the extension through the [maturin](https://www.maturin.rs/)
backend, and installs the dev tooling (`maturin`, `pytest`).

```bash
cd crates/py-faust
uv sync                        # create .venv, build + install `faust_rs`, add dev tools
```

`uv sync` builds the extension once. After editing the Rust sources, rebuild it
into the venv with:

```bash
uv run maturin develop --uv    # fast in-place rebuild
# or: uv sync --reinstall-package faust-rs
# or: uv run maturin build --release   # produce a wheel
```

## Test

A pytest suite lives in `tests/`; `uv sync` has already built the extension:

```bash
uv run pytest                  # from crates/py-faust
```

The suite verifies exact rendered sample values (compile, compute, persistence,
precision, lifetime/determinism) and exercises a vendored `noise.dsp` fixture.
If the extension is not built, the suite skips rather than errors.

## Usage

```python
import faust_rs

faust_rs.version()                       # underlying faust-rs compiler version

# process = _, _ : + : *(0.5);  -> 2 inputs, 1 output
dsp = faust_rs.compile("process = _, _ : + : *(0.5);", name="mixer")
dsp.num_inputs, dsp.num_outputs          # (2, 1)

# render one block: list of input channels -> list of output channels
dsp.compute([[1.0, 2.0], [1.0, 2.0]])    # [[1.0, 2.0]]

# zero-input generator needs an explicit frame count
faust_rs.compile("process = 0.7;").compute([], frames=4)   # [[0.7, 0.7, 0.7, 0.7]]
```

### Precision

Single precision (`f32`) is the default; pass `double=True` for `f64`. Audio
crosses the boundary as Python floats (`f64`) either way.

```python
# 2^24 + 1 is exact in f64, but rounds to 2^24 in f32
faust_rs.compile("process = 16777217.0;").compute([], frames=1)               # [[16777216.0]]
faust_rs.compile("process = 16777217.0;", double=True).compute([], frames=1)  # [[16777217.0]]

faust_rs.compile("process = _;", double=True).precision   # "double"
```

### Standard-library imports

Sources that `import("stdfaust.lib")` (and use `os.osc`, `fi.lowpass`, etc.)
need the Faust standard libraries on an import search path. Pass `search_paths`,
or set `FAUST_LIB_PATH` (appended automatically). The faust-rs workspace does
not bundle the full stdlib, so point these at an existing install.

```python
libs = "/path/to/faust/libraries"          # dir containing stdfaust.lib
dsp = faust_rs.compile('import("stdfaust.lib"); process = os.osc(440);',
                       sample_rate=48000, search_paths=[libs])
dsp.compute([], frames=8)                  # a 440 Hz sine block
```

### Persistent, stateful instance

`compile()` initializes a single interpreter instance that is reused across
`compute()` calls, so DSP state (recursive filters, oscillator phase, delay
lines) carries from one block to the next.

```python
# counter: y[n] = y[n-1] + 1
c = faust_rs.compile("process = (+(1))~_;")
c.compute([], frames=4)   # [[1.0, 2.0, 3.0, 4.0]]
c.compute([], frames=4)   # [[5.0, 6.0, 7.0, 8.0]]   <- continues (persists)
c.cycle                   # 2  (monotonic block counter)
c.reset()                 # clear DSP state
c.compute([], frames=4)   # [[1.0, 2.0, 3.0, 4.0]]   <- restarts
```

### In-place rendering via the buffer protocol

`compute_into(inputs, outputs)` is the zero-marshaling counterpart to
`compute()`. Instead of Python lists (which box every sample as a `PyFloat`), it
reads and writes contiguous native buffers, so large blocks skip per-sample
conversion. `inputs` and `outputs` are 2-D `(channels, frames)` C-contiguous
buffer-protocol objects — a NumPy array, a shaped `memoryview`, or an
`array.array` — whose dtype must match the DSP precision (`float32` for a
`"float"` DSP, `float64` for a `"double"` one; a mismatch raises). The block is
written into `outputs` in place; state persists exactly as with `compute()`. No
NumPy build dependency is required, and the binding stays free of hand-written
`unsafe` (each direction is one bulk copy).

```python
import numpy as np

dsp = faust_rs.compile("process = _, _ : + : *(0.5);")   # 2 in, 1 out (f32)
ins = np.array([[1.0, 2.0], [1.0, 2.0]], dtype=np.float32)  # (channels, frames)
outs = np.zeros((dsp.num_outputs, ins.shape[1]), dtype=np.float32)
dsp.compute_into(ins, outs)                # outs -> [[1.0, 2.0]]

# zero-input generator: pass a (0, frames) input; frames come from outputs
gen = faust_rs.compile("process = 0.7;")
gen.compute_into(np.zeros((0, 4), np.float32), np.zeros((1, 4), np.float32))
```

### UI parameters (sliders, buttons, bargraphs)

DSP controls are exposed as parameters. `params()` lists them; `get_param` /
`set_param` address a control by full UI path or unambiguous leaf label. A set
takes effect on the next `compute()`.

```python
dsp = faust_rs.compile('process = _ * hslider("gain", 1, 0, 2, 0.01);')
[p.path for p in dsp.params()]     # ['/FaustDSP/gain']
dsp.params()[0].kind               # 'hslider'  (init/min/max/step also exposed)

dsp.set_param("gain", 0.5)         # by leaf label (or "/FaustDSP/gain")
dsp.compute([[2.0, 4.0]])          # [[1.0, 2.0]]
dsp.get_param("gain")              # 0.5
dsp.reset()                        # restores gain to its init (1.0)
```

Buttons, checkboxes, sliders, and nentries are settable inputs; bargraphs are
outputs (read-only, reflecting the most recent `compute`).

`compile()` and `compute()` raise `ValueError` on compile errors, bad bytecode,
channel-count mismatches, and interpreter runtime errors; `get_param`/`set_param`
raise on unknown/ambiguous keys (and `set_param` on an output).

## Scope / limitations (deliberate for a PoC)

See `LIMITATIONS.md` for the full list and lift paths. In brief:

- Whole-block render only: no streaming ring buffer or real-time audio-callback
  integration. `compute_into` avoids per-sample marshaling but still bulk-copies
  into and out of the interpreter's own buffers (not a true zero-copy, which
  would need hand-written `unsafe`).

**Resolved:** cross-call state persistence (`Dsp` holds a safe, factory-owning
`OwnedFbcDspInstance`; `reset()` clears state), single/double precision
(`double=True`), `import(...)` resolution (`search_paths=` / `FAUST_LIB_PATH`),
the UI parameter bridge (`params()` / `get_param` / `set_param`), and
buffer-protocol block I/O (`compute_into`, accepting NumPy/`memoryview`/
`array.array`).
