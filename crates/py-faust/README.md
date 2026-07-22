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
Python  ──▶  compiler::Compiler          # .dsp source -> FBC bytecode text (fast lane)
        ──▶  read_fbc::<f32>             # -> FbcDspFactory<f32>
        ──▶  FbcDspInstance::try_compute # -> rendered audio blocks
```

## Build

Requires a Python interpreter and [maturin](https://www.maturin.rs/).

```bash
cd crates/py-faust
python3 -m venv .venv && . .venv/bin/activate
pip install maturin
maturin develop            # builds + installs `faust_rs` into the venv
# or: maturin build --release   # produce a wheel
```

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

`compile()` and `compute()` raise `ValueError` on compile errors, bad bytecode,
channel-count mismatches, and interpreter runtime errors.

## Scope / limitations (deliberate for a PoC)

- Single precision (`f32`) only; no `double` path yet.
- One-shot block render. Each `compute()` builds a fresh interpreter instance
  and re-`init()`s, so state does not persist **across** calls (it does within a
  single block, e.g. recursive filters). A persistent-instance API would hold the
  `FbcDspInstance` across calls (needs a self-referential holder or an
  owning-instance redesign).
- No `import("stdfaust.lib")` search-path wiring in this example surface — pass
  self-contained sources, or extend `compile()` to set import search paths.
- No UI-parameter (button/slider) get/set bridge yet; the interpreter exposes
  zone read/write (`get_real_zone`/`set_real_zone`) that a fuller binding would map
  to Python.
