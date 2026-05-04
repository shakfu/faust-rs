# `faustwasm` SVG Auxiliary Files Plan — 2026-05-03

## Status

Fully implemented — 2026-05-04.
Rust side (`faust-rs`) and TypeScript integration (`faustwasm`) are both complete.

## Scope

Make Rust-generated SVG block diagrams usable from
`/Users/letz/Developpements/faust-wasm/faustwasm` when the embedded compiler is
the raw Rust `faust_wasm_ffi.wasm` module.

This plan covers the integration boundary between:

- `crates/draw`: Rust SVG block-diagram renderer,
- `crates/compiler`: `generate_aux_files(...)` service facade,
- `crates/wasm-ffi`: raw WebAssembly ABI exported by `faust_wasm_ffi.wasm`,
- `faustwasm`: TypeScript adapter and `FaustSvgDiagrams` helper.

## Current State

The Rust compiler can generate SVG auxiliary artifacts internally:

- CLI `-svg` writes `<name>-svg/*.svg` through `draw::draw_schema(...)`.
- `Compiler::generate_aux_files(...)` detects `-svg`, renders diagrams, reads
  the generated `.svg` files, and returns `Vec<AuxFileArtifact>`.
- `wasm-ffi` exports `faust_wasm_generate_aux_files(...)`, but that raw export
  currently keeps the historical boolean shape and discards the returned
  artifacts.

The `faustwasm` Rust path can call `generateAuxFiles(...)`, but it cannot read
the produced SVGs:

- the historical Emscripten backend writes SVG files into an in-memory `FS`;
- `FaustSvgDiagrams.from(...)` reads `/<name>-svg/*.svg` from that `FS`;
- the raw Rust backend intentionally has no Emscripten `FS`, so `LibFaust.fs()`
  returns a proxy that throws.

Result: `generateAuxFiles("-svg")` may report success, but the SVG payload is
not observable from JavaScript.

## SVG Diagram Hierarchy

The Faust compiler produces a *tree* of SVG files, not a single flat diagram.
For any non-trivial DSP program the output looks like:

```
<name>-svg/
  process.svg          ← entry point, always present
  process_0x...svg     ← sub-diagram for a named definition
  process_0x...svg     ← sub-diagram for another definition
  ...
```

`process.svg` is the root of the hierarchy. It contains standard SVG `<a
xlink:href="...">` (or `href="..."`) elements that link to the child `.svg`
files by *relative path*. Those child files may themselves link deeper, so the
full block diagram is explored by following these links.

The Emscripten backend writes every file of the hierarchy into the in-memory
`FS`, and `FaustSvgDiagrams.from(...)` reads the entire `<name>-svg/` directory
at once: the relative `href` links between files therefore resolve naturally when
a browser or Node application renders them side-by-side from the same in-memory
map.

**The Rust backend must preserve this navigability.** Concretely:

- `faust_wasm_generate_aux_files_json(...)` must return *all* `.svg` files in the
  hierarchy, not only `process.svg`.
- The relative `href` links embedded in the SVG source must remain intact (they
  already are, since the Rust renderer writes them as relative paths).
- The `faustwasm` adapter must reconstitute a complete `Record<string, string>`
  map keyed by the relative path (`"process.svg"`, `"process_0x1234.svg"`, …)
  so that client code can satisfy cross-file references however it chooses
  (in-memory map lookup, virtual filesystem, object-URL map, etc.).

## Design Decision

Expose auxiliary files as explicit in-memory artifacts on the Rust Wasm ABI.

Do not emulate Emscripten `FS` in the raw Rust compiler module. The Rust path
should return the generated files directly as structured data, because this is
the stable compatibility contract that `faustwasm` actually needs:

- file path/name,
- binary/text marker,
- byte content.

The existing boolean `faust_wasm_generate_aux_files(...)` can remain for
backward compatibility, but `faustwasm` should use a richer Rust-only helper
when the compiler module exposes it.

## Proposed ABI

Add a new text-result export:

```text
faust_wasm_generate_aux_files_json(
    name_ptr, name_len,
    source_ptr, source_len,
    args_ptr, args_len
) -> text_result_handle
```

The returned text is UTF-8 JSON:

```json
[
  {
    "path": "process.svg",
    "binary": false,
    "content_base64": "PHN2ZyB4bWxucz0i..."
  },
  {
    "path": "process_0x7f3a1b.svg",
    "binary": false,
    "content_base64": "PHN2ZyB4bWxucz0i..."
  }
]
```

The array must contain **all** files in the `<name>-svg/` hierarchy, ordered
with the entry-point file (`process.svg`) first. The `path` values are relative
within the hierarchy (no leading slash, no `<name>-svg/` directory prefix) so
that the `href` links embedded in the SVG source match the map keys exactly.

Use base64 content for all artifacts, including textual SVG. This avoids
special cases for generated Wasm or other future binary auxiliary outputs and
keeps the JSON valid for arbitrary bytes.

The existing text-result lifetime API is reused:

- `faust_wasm_text_result_is_ok(handle)`
- `faust_wasm_text_result_ptr(handle)`
- `faust_wasm_text_result_len(handle)`
- `faust_wasm_text_result_free(handle)`

## Rust Implementation Steps

1. Add a serializable auxiliary-file DTO in `crates/wasm-ffi`.

   ```rust
   struct WasmAuxFileArtifact {
       path: String,
       binary: bool,
       content_base64: String,
   }
   ```

2. Implement `faust_wasm_generate_aux_files_json(...)`.

   The export should:

   - decode `name`, `source`, and `args`,
   - call `Compiler::generate_aux_files(...)`,
   - encode each `AuxFileArtifact.content` as base64,
   - return the JSON through the existing text-result registry,
   - return a text-result error with the compiler diagnostic on failure.

3. Keep `faust_wasm_generate_aux_files(...)` as a compatibility wrapper.

   It can continue returning `1` on success and `0` on error. It must not be
   the path used by the Rust `FaustSvgDiagrams` integration.

4. Add the new export to `xtask`'s required `wasm-ffi` export list.

   This makes `cargo run -p xtask -- build-faustwasm-compiler-module` fail if
   the ABI surface regresses.

5. Add unit tests in `crates/wasm-ffi`.

   Minimum coverage:

   - JSON helper returns at least one `.svg` artifact for a simple DSP and
     `-svg`;
   - returned JSON can be parsed and base64-decoded;
   - invalid source returns a text-result error;
   - old boolean helper still returns success/failure.

## Compiler/Draw Follow-Up

`Compiler::generate_aux_files(...)` currently renders SVGs through a temporary
directory and reads the files back. That is acceptable for native builds but is
not a good long-term model for `wasm32-unknown-unknown`.

Follow-up target:

- add an in-memory draw sink in `crates/draw`,
- expose a `draw_schema_to_artifacts(...)` style helper returning
  `(path, bytes)` pairs,
- make `Compiler::generate_aux_files(...)` choose the in-memory path for SVG,
  keeping the CLI `-svg` directory writer unchanged.

This follow-up removes filesystem assumptions from the embedded compiler path
and makes browser use deterministic.

## `faustwasm` Integration Steps

1. Extend `RustFaustModule` TypeScript types with the new raw export.

2. Add `RustLibFaust.generateAuxFilesJson(...)` or equivalent private helper.

   It should call `faust_wasm_generate_aux_files_json(...)`, decode the text
   result, parse the JSON, and return an artifact list.

3. Update `FaustSvgDiagrams.from(...)`.

   When the compiler is backed by the raw Rust module:

   - call the JSON helper with `-lang wasm -o binary -svg ...`,
   - filter returned artifacts to `.svg` entries,
   - decode base64 content to UTF-8 strings,
   - build and return the `Record<string, string>` map keyed by relative path
     (e.g. `"process.svg"`, `"process_0x7f3a1b.svg"`, …).

   The map must be *complete*: it must include every file returned by the
   compiler, not only `process.svg`. Callers that render the block diagram must
   be able to resolve cross-file `href` references by looking up the target key
   in this map. Navigation in the hierarchy works exactly like the legacy
   Emscripten path — only the source of the files changes.

   When the compiler is backed by the historical Emscripten module, keep the
   existing `FS`-directory-scan path unchanged; it already returns the full
   hierarchy.

4. Update `scripts/faust2svg.js` validation.

   The script should work with either compiler module:

   - legacy Emscripten compiler: read SVGs from `FS`,
   - Rust compiler module: read SVGs from returned artifacts.

## Compatibility Notes

- No Emscripten `FS` emulation is introduced for the Rust backend.
- Existing `generateAuxFiles(...) -> boolean` remains available.
- The richer JSON helper is additive and Rust-specific; it can later become the
  preferred cross-backend helper if the legacy path grows a similar artifact
  extraction layer.
- SVG path names should stay relative (`process.svg`, sub-diagram names) so
  browser and Node callers can decide where to persist them.

## Validation Plan

In `faust-rs`:

- `cargo fmt --all`
- `cargo test -p wasm-ffi`
- `cargo test -p xtask verify_wasm_ffi_exports`
- `cargo run -p xtask -- build-faustwasm-compiler-module`

In `faustwasm`:

- run the existing TypeScript build,
- run `node scripts/faust2svg.js <input.dsp> <output-dir>` with the historical
  compiler,
- run the same command with a Rust compiler-module hook/path,
- confirm at least `process.svg` is emitted and contains valid `<svg`.

## Open Questions

- Whether `faustwasm` should expose all auxiliary artifacts publicly, or only
  keep the SVG-specific helper for now.
- Whether the Rust JSON artifact helper should also include a MIME-like field
  (`image/svg+xml`, `application/wasm`, `application/json`) for downstream UI
  code.
- Whether the temporary-directory implementation in `Compiler::generate_aux_files`
  should be replaced before or after the first `faustwasm` TypeScript adapter
  patch.
- ~~How browser UI code should satisfy the cross-file SVG `href` links.~~ **Resolved: use
  application-managed navigation (option c).** The other options fail at depth > 1:
  `data:` URLs and `blob:` URLs cannot resolve relative `href` attributes
  embedded in the rendered SVG. A Service Worker could emulate a virtual origin
  but adds lifecycle complexity for no practical gain.

  **Recommended navigation model** — the host application maintains a path
  stack and renders SVGs from the in-memory map:

  1. Initialise stack to `["process.svg"]`; display the SVG at the top.
  2. Intercept `click` on `<a>` elements inside the rendered SVG: push the
     `href` attribute value (e.g. `"process_0x7f3a1b.svg"`) onto the stack,
     look it up in the map, render the result.
  3. Intercept `click` on the SVG background (outside any block): pop the
     stack; render the new top (go up one level). If the stack has only one
     entry, the click is a no-op.

  This matches the existing Emscripten-based Faust IDE browser behaviour exactly.
  The `faustwasm` adapter does not need to implement the stack itself; it only
  needs to guarantee that the `Record<string, string>` map is complete so that
  step 2 never misses a key.
