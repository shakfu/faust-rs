# faust-rs-py — Known Limitations

Status of the proof-of-concept PyO3 bindings. Each item notes the cause and what
a fuller implementation would require. Updated as items are addressed.

## 1. Single vs double precision  [RESOLVED]

**Original limitation:** `compile()` hard-coded `RealType::Float32` and loaded
the factory with `read_fbc::<f32>`, so only single precision was available.

- **Resolution:** `compile(..., double=True)` now selects `RealType::Float64`
  and `read_fbc::<f64>`. `Dsp` holds a precision-erased `Engine` enum
  (`OwnedFbcDspInstance<f32>` / `<f64>`), mirroring the FFI's `FbcDspFactoryAny`,
  and a generic `render::<R>` helper marshals audio. A `precision` getter
  reports `"float"` / `"double"`. Audio crosses the Python boundary as `f64`
  (Python's native float): lossless for a double-precision DSP and cast to/from
  `f32` for a single-precision one.

## 2. No cross-call state persistence  [RESOLVED — see below]

**Original limitation:** each `compute()` call built a fresh `FbcDspInstance`
and re-ran `init()`, so DSP state (recursive filters, oscillator phase, delay
lines) reset every call. State was correct *within* one block but never carried
*across* calls.

- **Cause:** `FbcDspInstance<'a>` borrows the factory (`&'a FbcFactory`) for its
  whole lifetime. Holding both factory and instance in one `#[pyclass]` is a
  self-referential struct, which the one-shot design sidestepped by rebuilding
  the instance per call.
- **Resolution:** the interpreter backend now provides an owning instance,
  `codegen::backends::interp::OwnedFbcDspInstance<R>`, which holds the factory
  and the runtime executor as sibling fields (no lifetime, no self-reference).
  `FbcDspInstance` and `OwnedFbcDspInstance` are two aliases of one generic base
  (`FbcDspInstanceImpl<F, R>` over `F: Borrow<FbcDspFactory<R>>`) sharing a
  single, fully **safe** implementation — the factory is read through `Borrow`
  while the mutable executor lives in a disjoint field. `Dsp` simply owns an
  `OwnedFbcDspInstance<f32>`, so the binding contains **no hand-written
  `unsafe`**. `init()` runs once at `compile()`; each `compute()` advances the
  same instance; `reset()` clears state; a `cycle` getter exposes the running
  block count. The owning type is covered by `cargo test` (and is Miri-clean, as
  it carries no unsafe) in `codegen`'s interp instance tests.

## 3. UI parameter (button/slider) bridge  [RESOLVED]

**Original limitation:** the interpreter exposed control zones
(`get_real_zone`/`set_real_zone`) and a UI instruction list
(`ui_instructions()`), but the bindings did not map Faust UI widgets to named
Python accessors.

- **Resolution:** at compile time the binding walks `ui_instructions()` into a
  `Param` list (tracking enclosing box labels to build each control's full UI
  path). It exposes:
  - `dsp.params()` -> list of `Param` (path, leaf label, kind, `init`/`min`/
    `max`/`step`, `is_input`, zone offset), in declaration order;
  - `dsp.get_param(key)` / `dsp.set_param(key, value)` keyed by full path or an
    unambiguous leaf label. Set takes effect on the next `compute()`.
  Buttons, checkboxes, h/v sliders, and nentries are settable inputs; h/v
  bargraphs are outputs (readable via `get_param`, reflecting the most recent
  `compute`; not settable). `reset()` restores all controls to their defaults.
  Values are not clamped to `[min, max]`, matching Faust's `setParamValue`.

## 4. Import search-path wiring  [RESOLVED]

**Original limitation:** `compile()` did not configure `import("stdfaust.lib")`
search paths, so sources using the standard libraries (`os.osc`, `fi.lowpass`,
etc.) failed to resolve. Only self-contained sources compiled.

- **Resolution:** `compile(..., search_paths=[...])` resolves imports against
  the given directories; directories in the `FAUST_LIB_PATH` environment
  variable are appended automatically. This is backed by a new compiler method,
  `compile_source_to_interp_with_lane_and_search_paths`. The faust-rs workspace
  does not bundle the full Faust standard library, so point `search_paths` (or
  `FAUST_LIB_PATH`) at an existing stdlib install; the import test suite skips
  when none is discoverable.

## 5. Whole-block render, no host loop / streaming

`compute()` renders one block passed entirely from Python (a list of lists). No
streaming, no NumPy buffer protocol, no real-time callback integration. Large
renders copy Python lists to `Vec<f32>` and back.

- **Cause:** PoC uses plain Python lists for portability (no NumPy dependency).
- **To lift:** accept and return NumPy arrays via the buffer protocol / `numpy`
  crate for zero-copy block I/O.
