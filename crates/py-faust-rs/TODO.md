# py-faust-rs TODO

Remaining work for the proof-of-concept binding. Resolved items (persistence,
single/double precision, import search paths, UI parameter bridge) are recorded
in `LIMITATIONS.md`.

## NumPy zero-copy I/O  (next up)

`compute()` currently takes and returns Python lists of lists, copying every
sample to/from `Vec<f64>`. This is the last open item from `LIMITATIONS.md` (#5).

- **Goal:** accept and return NumPy arrays for block I/O, avoiding per-sample
  copies on large renders and matching the ergonomics of cyfaust's
  `player.compute(count, inputs, outputs)`.
- **Approach:** add the `numpy` crate (PyO3 bindings) and accept
  `PyReadonlyArray2<f32/f64>` inputs / write into a preallocated
  `PyArray2` output, honoring the engine precision. Keep the current
  list-based path for dependency-free use, or make NumPy an optional feature.
- **Tests:** mirror the existing `test_compute.py` value checks with `np.ndarray`
  buffers; verify dtype/shape validation and that a float DSP accepts an
  `f32` array without a copy.

## Nice-to-have (unordered, lower priority)

- **Type stubs (`.pyi`)** for `Dsp`, `Param`, `compile`, `version` so editors
  and type checkers see the API. maturin can ship a stub alongside the module.
- **Compile from file:** a `compile_file(path, ...)` entry point using the
  compiler's file-backed APIs (which also auto-merge default import search
  paths), complementing the current string-only `compile`.
- **Metadata / introspection:** expose factory JSON, `sha_key`, and compile
  options (cyfaust surfaces these via `get_sha_key` / `get_compile_options`).
- **Packaging & CI:** `cibuildwheel` + GitHub Actions to build wheels across
  platforms and Python versions, toward a PyPI release like cyfaust. Bundling a
  Faust standard library would let the import tests run in CI instead of
  skipping.
- **Param metadata declarations:** capture `declare`/`[unit:...]`-style widget
  metadata (`FbcUiInstruction.key`/`value`) onto `Param`.
