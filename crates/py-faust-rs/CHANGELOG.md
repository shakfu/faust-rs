# Changelog

All notable changes to `py-faust-rs` are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This crate is an experimental proof of concept (`publish = false`); its version
tracks the faust-rs workspace and its API may change at any time.

## [Unreleased]

### Added

- Initial proof-of-concept PyO3/maturin bindings exposing the faust-rs
  interpreter (FBC) backend to Python as the `faust_rs` extension module.
- `compile(source, name="FaustDSP", sample_rate=48000, double=False)` -> `Dsp`:
  compiles a Faust `.dsp` source string to FBC bytecode (transform fast lane)
  and loads it at the selected precision.
- Double-precision (`f64`) support via `double=True`; single precision (`f32`)
  remains the default. `Dsp` carries a precision-erased engine (`f32`/`f64`
  owning instance) and a `precision` getter reporting `"float"` / `"double"`.
  Audio crosses the Python boundary as `f64` (lossless for double, cast for
  single).
- `import(...)` resolution: `compile(..., search_paths=[dir, ...])` resolves
  `import("stdfaust.lib")` and friends against the given directories, with
  `FAUST_LIB_PATH` entries appended automatically. Backed by a new
  `compiler::Compiler::compile_source_to_interp_with_lane_and_search_paths`
  method. Adds a skip-guarded import test group and a vendored `osc.dsp`
  fixture (both need a discoverable Faust standard library).
- `Dsp` class with a persistent, stateful interpreter instance:
  - `compute(inputs, frames=None)` renders one block (list of input channels ->
    list of output channels); DSP state carries across calls.
  - `compute_into(inputs, outputs)` renders one block **in place** through the
    Python buffer protocol: `inputs`/`outputs` are 2-D `(channels, frames)`
    C-contiguous buffers (NumPy array, shaped `memoryview`, `array.array`, ...)
    whose dtype must match the DSP precision (`float32`/`float64`; never silently
    cast). This avoids the per-sample `PyFloat` boxing of the list-based path via
    a single bulk copy each way (`PyBuffer::copy_to_slice`/`copy_from_slice`), so
    it needs no NumPy build dependency and keeps the binding free of hand-written
    `unsafe`. Same persistent state as `compute()`.
  - `reset()` re-initializes the instance, clearing filter memory, oscillator
    phase, and delay lines, and restoring control parameters to their defaults.
  - `num_inputs`, `num_outputs`, `sample_rate`, `name`, `precision`, and `cycle`
    getters.
- UI parameter bridge: `params()` lists DSP controls as `Param` objects (full UI
  path, leaf label, kind, `init`/`min`/`max`/`step`, `is_input`, zone offset);
  `get_param(key)` / `set_param(key, value)` address a control by full path or
  unambiguous leaf label. Buttons, checkboxes, sliders, and nentries are
  settable inputs; bargraphs are read-only outputs. A `set` takes effect on the
  next `compute()`. Backed by the interpreter's `ui_instructions()` and
  `get_real_zone`/`set_real_zone`.
- `version()` returning the underlying faust-rs compiler version.
- `LIMITATIONS.md` documenting known scope reductions and their lift paths.
- pytest suite under `tests/` (75 tests) covering module surface, compilation
  and errors, exact compute output, persistence/reset, single/double precision,
  and instance lifetime/determinism. Self-contained DSP snippets and a vendored
  `noise.dsp` fixture are adapted from the sibling `cyfaust` project's tests,
  but assert exact sample values rather than only non-null factories. The suite
  skips (does not error) when the extension is not built. The `compute_into`
  buffer-protocol tests use stdlib `array.array`/`memoryview` (no NumPy needed)
  and additionally exercise the NumPy path when NumPy is installed.
- `numpy` added as a dev-only dependency so the `compute_into` tests exercise the
  primary real-world consumer directly; it is not a runtime dependency of the
  extension.

### Changed

- Persistence now uses the safe, factory-owning `OwnedFbcDspInstance<f32>` from
  the `codegen` interpreter backend. An earlier iteration held a boxed factory
  plus a `'static` self-referential borrow inside the binding; that
  hand-written `unsafe` has been removed. The binding contains no hand-written
  `unsafe` (only PyO3 macro expansion requires relaxing `unsafe_code`), and the
  `Dsp` pyclass is `Send`.
- Development tooling migrated from the pip/`venv` workflow to
  [uv](https://docs.astral.sh/uv/). `uv sync` creates the venv, builds and
  installs the extension through the maturin backend, and installs the dev
  tools; dev dependencies moved from `[project.optional-dependencies]` to a
  PEP 735 `[dependency-groups]` table, pinned by a committed `uv.lock`. Build
  and test docs (README, `conftest.py`) updated accordingly.
- Added a `Makefile` with self-documenting targets (`make help`): `sync`,
  `develop`, `build`, `test`, `lint` (`fmt-check` + `clippy`), `clean`, and
  friends. `make test` rebuilds the extension before running pytest.
- Renamed the crate to `py-faust-rs` (Cargo package and `crates/py-faust-rs`
  directory) and the Python distribution to `py-faust-rs`, for consistency with
  the parent `faust-rs` workspace. The importable module is unchanged
  (`import faust_rs`), as is the `faust_rs` Cargo `[lib]` name.

### Fixed

- `make test` now runs pytest with `uv run --no-sync`. A bare `uv run pytest`
  re-syncs the environment first, which reinstalls the project from uv's cache
  and clobbers the fresh `make develop` build whenever the version is unchanged
  (an editable rebuild keeps the same `0.5.0` version). The stale extension then
  lacked any newly added method. `develop` already syncs the dev tools, so the
  test step can safely skip the sync and keep the just-built extension.
- Compiling a source that expands `import("stdfaust.lib")` no longer crashes the
  interpreter with a stack overflow (SIGSEGV). `compile()` ran the compiler's
  deeply-recursive structural-lowering pass on Python's main-thread stack
  (~8 MiB on CPython), but the evaluator's guarded recursion budgets are sized
  against the workspace's 64 MiB stack contract (see `compiler::main`), which
  every other embedder honors. `compile()` now runs the compile pipeline on a
  64 MiB-stack worker thread, releasing the GIL while it runs. The overflow was
  reliable in debug builds and latent in release (deep enough inputs could still
  overflow before the fix).

### Notes

- Compile errors, malformed bytecode, channel-count mismatches, and interpreter
  runtime errors are raised as Python `ValueError`.
- The crate is kept out of the main Cargo workspace (`exclude` in the root
  `Cargo.toml`) so its `extension-module` linkage never affects
  `cargo build --workspace` or CI.
