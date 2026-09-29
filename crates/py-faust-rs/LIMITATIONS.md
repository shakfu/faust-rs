# py-faust-rs — Known Limitations

Status of the proof-of-concept PyO3 bindings. Each item notes the cause and what
a fuller implementation would require. Updated as items are addressed.

## 1. Single vs double precision  [RESOLVED]

**Original limitation:** `compile()` hard-coded `RealType::Float32` and loaded
the factory with `read_fbc::<f32>`, so only single precision was available.

- **Resolution:** `compile(..., double=True)` compiles with `-double`. A
  `precision` getter reports `"float"` / `"double"`. Audio crosses the Python
  boundary as `f64` (Python's native float) and is cast for a single-precision
  DSP. On the interpreter, a double-precision DSP still has `f32` I/O (item 6).

## 2. No cross-call state persistence  [RESOLVED — see below]

**Original limitation:** each `compute()` call built a fresh `FbcDspInstance`
and re-ran `init()`, so DSP state (recursive filters, oscillator phase, delay
lines) reset every call. State was correct *within* one block but never carried
*across* calls.

- **Cause:** `FbcDspInstance<'a>` borrows the factory (`&'a FbcFactory`) for its
  whole lifetime. Holding both factory and instance in one `#[pyclass]` is a
  self-referential struct, which the one-shot design sidestepped by rebuilding
  the instance per call.
- **Resolution:** `Dsp` holds a `faust::Dsp`, which owns a reference to its
  factory and has no lifetime parameter. `init()` runs once at `compile()`;
  each `compute()` advances the same instance; `reset()` clears state; a `cycle`
  getter exposes the running block count. `faust::Dsp` is `Send + Sync`, as
  PyO3 requires.

## 3. UI parameter (button/slider) bridge  [RESOLVED]

**Original limitation:** the interpreter exposed control zones
(`get_real_zone`/`set_real_zone`) and a UI instruction list
(`ui_instructions()`), but the bindings did not map Faust UI widgets to named
Python accessors.

- **Resolution:** `params()` converts `faust::Dsp::params()` to `Param`
  objects. Paths follow the C++ `MapUI` (`/group/label`). It
  exposes:
  - `dsp.params()` -> list of `Param` (path, shortname, label, kind,
    `init`/`min`/`max`/`step`, `is_input`, metadata), in UI order;
  - `dsp.get_param(key)` / `dsp.set_param(key, value)`, looked up by the
    facade as the C++ `MapUI` does: path, shortname, then label (a shared
    label designates the last one declared). Set takes effect on the next
    `compute()`.
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
  variable are appended automatically. They reach the compiler as `-I`
  arguments. The faust-rs workspace does not bundle the full Faust standard
  library, so point `search_paths` (or `FAUST_LIB_PATH`) at an existing stdlib
  install; the import test suite skips when none is discoverable.

## 5. Whole-block render, no host loop / streaming  [BUFFER PROTOCOL ADDED]

**Original limitation:** `compute()` renders one block passed entirely from
Python (a list of lists). No streaming, no NumPy buffer protocol, no real-time
callback integration. Large renders copy Python lists to `Vec<f32>` and back,
boxing every sample as a `PyFloat`.

- **Resolution (buffer protocol):** `compute_into(inputs, outputs)` renders one
  block **in place** through the Python buffer protocol. `inputs` and `outputs`
  are 2-D `(channels, frames)` C-contiguous buffers — a NumPy array, a shaped
  `memoryview`, an `array.array`, or any object exposing the buffer protocol —
  whose dtype must match the DSP precision (`float32` for `"float"`, `float64`
  for `"double"`; a mismatch raises rather than silently casting). This removes
  the per-sample Python-object marshaling of the list path: each direction is a
  single bulk copy via PyO3's `PyBuffer::copy_to_slice` / `copy_from_slice`, so
  the extension needs **no NumPy build dependency** and keeps **no hand-written
  `unsafe`**. The list-based `compute()` remains for convenience.
- **Still open:** this is a bulk *copy* into and out of the interpreter's own
  buffers, not a true zero-copy in which the backend reads and writes the
  caller's memory directly. That would require reinterpreting the buffer's
  `Cell` view as `&mut [R]` — hand-written `unsafe` — which the crate
  deliberately avoids. Rendering is still block-at-a-time: no
  streaming ring buffer and no real-time audio-callback integration.

## 6. Interpreter double precision has `f32` I/O  [RESOLVED]

With `backend="interp"` and `double=True`, audio was rounded to `f32` at the
input and output, because the interpreter C ABI exchanges `f32` buffers.

- **Resolution:** the `faust` crate uses a Rust-only `f64` entry of
  `interp-ffi` (upstream `c7a9d5a2`); the C ABI is unchanged. Both backends
  now exchange the compiled precision.
