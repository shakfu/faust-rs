# py-faust-rs vs cyfaust

A functionality comparison between `py-faust-rs` (this crate) and
[`cyfaust`](https://github.com/shakfu/cyfaust), a sibling project that wraps
Faust from Python via Cython.

Snapshot basis: `py-faust-rs` 0.5.0; cyfaust 0.1.4 (Faust 2.85.x).

## Architectural framing

The most important difference is what each project binds:

- **cyfaust** wraps the upstream C++ **`libfaust`** (the interpreter backend,
  plus an optional LLVM JIT backend and the RtAudio driver). It therefore
  inherits Faust's full C/C++ API, and its wheels must build or vendor
  `libfaust`.
- **py-faust-rs** binds the pure-Rust **`faust-rs`** reimplementation's
  interpreter (FBC) backend. There is no C toolchain and no `libfaust` in the
  build. This is the narrower surface, but the absence of a native Faust
  dependency is the main reason the crate exists.

That difference explains the rest: cyfaust is broad because it re-exposes a
mature C++ API; py-faust-rs is deliberately narrow (persistent single-block
render from a source string) because it is a proof of concept over a young Rust
backend.

## Where py-faust-rs already leads

- **Double precision.** py-faust-rs supports `double=True` (both `f32` and
  `f64` engines). cyfaust is float32-only: its `build_user_interface`
  hardcodes `is_double=False` and its compute buffers are `float[::1]`
  memoryviews.
- **Runtime UI parameter get/set.** py-faust-rs exposes a real
  `params()` / `get_param` / `set_param` bridge bound to the interpreter's real
  heap zones. cyfaust has no runtime parameter API: it derives parameter lists
  by regex-parsing expanded DSP source in its CLI and can read the static JSON
  UI description, but it cannot set live control values.

## Capability comparison

| Capability | cyfaust | py-faust-rs |
|---|---|---|
| Compile source string, render audio | Yes | Yes |
| Compile from file | Yes | No (string only) |
| Block `compute` | Yes (plus one-sample `frame`, timestamped compute) | Yes (block only) |
| Audio buffers | float32 memoryviews (numpy / `array`) | Python lists (no numpy zero-copy) |
| Double precision | No | Yes |
| Runtime parameter get/set | No | Yes |
| Vendored Faust standard library | Yes (54 libraries shipped in the wheel) | No (must point at an external library dir) |
| JSON / metadata introspection | Yes (`get_json`, `metadata()`) | Partial (`params()`, channel counts, name; no JSON) |
| Bitcode serialize + SHA factory cache | Yes | No |
| Box API + Signal API (programmatic DSP construction) | Yes (full, object-oriented and functional) | No |
| SVG / block-diagram generation | Yes | No |
| Source-codegen backends (c, cpp, rust, codebox) | Yes | No |
| LLVM JIT execution | Yes (separate `cyfaust-llvm` wheel) | No (Rust FBC interpreter) |
| Real-time audio (RtAudio) | Yes | No |
| Offline WAV render | No | No |
| Polyphony / voice allocation | No | No |
| MIDI | No | No |

## Pending items to close the gap

Ranked by usability impact. Several are "expose a capability `faust-rs` already
has" rather than new engine work.

1. **Vendor the Faust standard library.** The largest usability gap. Today
   `import("stdfaust.lib")` requires an external search path (`search_paths=` or
   `FAUST_LIB_PATH`); cyfaust ships the libraries and resolves them
   automatically. This is also why the stdlib-dependent tests skip on a bare
   checkout.
2. **NumPy zero-copy buffers.** Audio currently crosses the boundary as Python
   lists, one block at a time; cyfaust uses float32 memoryviews. A real
   performance and ergonomics gap.
3. **Compile from file** and **JSON metadata** (`get_json`). Small surface,
   high value.
4. **SVG diagrams** and **source-codegen** (c / cpp / rust / wasm). `faust-rs`
   already has `draw`, `codegen`, `cranelift-ffi`, and `wasm-encoder` crates, so
   these are wrapping work, not new engines.
5. **Box API / Signal API** for programmatic DSP construction. Large surface;
   `faust-rs` has `boxes`, `signals`, and `propagate` crates. Matches cyfaust's
   biggest feature area.
6. **Bitcode / factory caching**, **real-time audio**, and **polyphony / MIDI**.
   Larger and lower priority for a proof of concept.

## Reference points

- py-faust-rs API: `crates/py-faust-rs/src/lib.rs`, `README.md`.
- cyfaust API: `src/cyfaust/interp.pyx` (compile/run), `box.pyx`, `signal.pyx`
  (programmatic APIs), `player.pyx`, `common.pyx` (vendored-resource
  resolution), and `src/cyfaust/__main__.py` (CLI). Capability ceiling is set by
  the `faust_*.pxd` C declarations.
