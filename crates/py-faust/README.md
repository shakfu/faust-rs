# faust-rs-py (proof of concept)

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

Requires a Python interpreter and [maturin](https://www.maturin.rs/).

```bash
cd crates/py-faust
python3 -m venv .venv && . .venv/bin/activate
pip install maturin
maturin develop            # builds + installs `faust_rs` into the venv
# or: maturin build --release   # produce a wheel
```

## Test

A pytest suite lives in `tests/`. Build the extension first, then run it:

```bash
maturin develop
pip install pytest         # or: pip install -e '.[test]'
pytest                     # from crates/py-faust
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

`compile()` and `compute()` raise `ValueError` on compile errors, bad bytecode,
channel-count mismatches, and interpreter runtime errors.

## Scope / limitations (deliberate for a PoC)

See `LIMITATIONS.md` for the full list and lift paths. In brief:

- No `import("stdfaust.lib")` search-path wiring in this example surface — pass
  self-contained sources.
- No UI-parameter (button/slider) get/set bridge yet; the interpreter exposes
  zone read/write (`get_real_zone`/`set_real_zone`) that a fuller binding would map
  to Python.
- Whole-block render over plain Python lists (no NumPy zero-copy).

**Resolved:** cross-call state persistence (`Dsp` holds a safe, factory-owning
`OwnedFbcDspInstance`; `reset()` clears state), and single/double precision
(`double=True`).
