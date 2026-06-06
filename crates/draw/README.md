# draw

SVG block-diagram rendering for Faust box expressions.

This crate ports the Faust C++ `compiler/draw/` module. It is used by the
`faust-rs -svg` CLI path and by the raw `faustwasm` auxiliary-file service.

## C++ provenance

| Rust module | C++ source |
|---|---|
| `device` | `compiler/draw/device/device.h`, `compiler/draw/device/SVGDev.*` |
| `schema` | `compiler/draw/schema/schema.h`, `compiler/draw/schema/collector.cpp` |
| `schemas::*` | `compiler/draw/schema/*Schema.*` |
| `translate` | `compiler/draw/drawschema.cpp` |

## What this crate does

- Translates evaluated Faust box trees into recursive drawing schemas.
- Renders SVG diagrams with deterministic layout and connector ordering.
- Supports C++-style drawing options for shadows, responsive SVG output,
  route frames, label truncation, and folded sub-diagrams.
- Provides a filesystem-backed API for the CLI and an in-memory API for
  `wasm32-unknown-unknown` hosts.

## Public API

| Item | Description |
|---|---|
| `DrawConfig` | SVG layout/rendering options (`shadow_blur`, `scaled_svg`, route frame, label size, folding thresholds) |
| `draw_schema(arena, root, name, out_dir, config, def_names)` | Write the root SVG and optional folded sub-diagrams into a directory |
| `draw_schema_to_memory(arena, root, name, config, def_names)` | Return SVG artifacts as `(filename, bytes)` without filesystem access |
| `DrawError` | Typed rendering and I/O error surface |
| `Schema` / `TraitCollector` / `SvgDevice` internals | Schema placement, wiring collection, and SVG emission |
| `crate_id()` | Returns the stable crate identifier |

## CLI integration

The top-level compiler exposes this crate through:

```bash
faust-rs -svg foo.dsp
faust-rs -svg -sc -f 25 foo.dsp
```

The output directory is derived from the input stem (`foo-svg/`). Folded
sub-diagrams are emitted as additional `.svg` files in the same hierarchy.

## Position in the pipeline

```
parser  ->  boxes  ->  eval  ->  draw
```
