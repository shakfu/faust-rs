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

## 3. No UI parameter (button/slider) bridge

The interpreter exposes control zones via `get_real_zone(offset)` /
`set_real_zone(offset, value)` and a UI instruction list (`ui_instructions()`),
but the bindings do not yet map Faust UI widgets (buttons, sliders, nentries) to
named Python get/set accessors.

- **Cause:** requires walking the factory `ui_block` to build a label -> zone
  offset map and exposing it as a Python dict / property API.
- **To lift:** parse `ui_instructions()` into a `{label: offset}` table at
  compile time; add `set_param(label, value)` / `get_param(label)` that call the
  instance zone accessors.

## 4. No import search-path wiring

The example `compile()` surface does not configure `import("stdfaust.lib")`
search paths, so sources using the standard libraries (`os.osc`, `fi.lowpass`,
etc.) fail to resolve. Only self-contained sources compile.

- **Cause:** the fast-lane string compile path is called without search paths.
- **To lift:** add a `search_paths` / `use_stdlib` argument routed to the
  compiler's file/search-path aware entry points (see
  `compiler::default_import_search_paths`).

## 5. Whole-block render, no host loop / streaming

`compute()` renders one block passed entirely from Python (a list of lists). No
streaming, no NumPy buffer protocol, no real-time callback integration. Large
renders copy Python lists to `Vec<f32>` and back.

- **Cause:** PoC uses plain Python lists for portability (no NumPy dependency).
- **To lift:** accept and return NumPy arrays via the buffer protocol / `numpy`
  crate for zero-copy block I/O.
