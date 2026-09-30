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
- **Packaging & CI:** `cibuildwheel` + GitHub Actions to build wheels across
  platforms and Python versions, toward a PyPI release like cyfaust. Bundling a
  Faust standard library would let the import tests run in CI instead of
  skipping.
- **`declare` metadata:** `Dsp.metadata()` and `get_json()["meta"]` lack the
  program's `declare`s: the FIR backends do not carry them (upstream gap).
- **Validate `args`:** the backends accept unknown flags silently, so a typo in
  `args=` is ignored. Belongs in the facade or the C entry points.
