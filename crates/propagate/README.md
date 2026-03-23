# propagate

Box-to-signal propagation — Phase 4, section 2.4 of the Faust compiler pipeline.

Takes the evaluated box tree from `eval` and produces a flat signal list by
recursively applying the Faust composition algebra.

## C++ provenance

| C++ file | Role |
|---|---|
| `compiler/propagate/propagate.hh` / `propagate.cpp` | Core propagation logic |
| `compiler/boxes/boxtype.cpp` | Box arity inference |

## What this crate does

- **Arity inference** (`box_arity_typed`) — determines `(inputs, outputs)` for any flat box.
- **Signal lowering** (`propagate_typed_with_ui`) — walks the box tree and emits `signals` nodes via `SigBuilder`.
- **Composition algebra** — `seq`, `par`, `split`, `merge` with explicit bus routing.
- **Recursive forms** — De Bruijn-style placeholders (`sigRec` / `sigProj` shape).
- **Grouped UI** — builds a canonical `UiProgram` as an explicit propagation product.

## Public API

### Entry points — typed (post-`eval/a2sb`)

Preferred for callers that already hold a validated `FlatBoxId`.

| Function | Description |
|---|---|
| `box_arity_typed(arena, flat, cache)` | Infer `(inputs, outputs)` for a validated flat box (memoized) |
| `propagate_typed_with_ui(arena, flat, inputs, cache)` | Lower flat box to `PropagateOutput` (signals + grouped UI) |
| `propagate_typed_with_ui_options(arena, flat, inputs, cache, opts)` | Same with explicit `PropagateUiOptions` |
| `propagate_typed(arena, flat, inputs, cache)` | DSP-only variant; drops grouped UI |

### Entry points — compatibility wrappers (raw `BoxId`)

| Function | Description |
|---|---|
| `box_arity(arena, box_id, cache)` | Arity inference from a raw `BoxId` |
| `propagate_with_ui(arena, box_id, inputs, cache)` | Propagation + grouped UI from a raw `BoxId` |
| `propagate(arena, box_id, inputs, cache)` | DSP-only propagation from a raw `BoxId` |

### Flat-box boundary

| Item | Description |
|---|---|
| `FlatBoxId` | Validated `TreeId` wrapper for the post-`eval/a2sb` flat box subset |
| `try_build_flat_box(arena, box_id)` | Validate and convert a raw `BoxId` into a `FlatBoxId` |
| `FlatBoxBuildError` | Error returned when a box family is rejected at the flat boundary |

### Types

| Item | Description |
|---|---|
| `BoxArity` | `{ inputs: usize, outputs: usize }` pair |
| `PropagateOutput` | Propagation products: `signals: Vec<SigId>` + `ui: UiProgram` |
| `PropagateUiOptions` | Grouped-UI construction policy (synthesized root label) |
| `PropagateError` | Typed error covering arity mismatches and unsupported nodes |
| `ArityCache` | Memoization cache (`AHashMap<FlatBoxId, Result<BoxArity, PropagateError>>`) |

### Utilities

| Item | Description |
|---|---|
| `make_sig_input_list(arena, n)` | Build ordered input signal list (`sigInput(0)` … `sigInput(n-1)`) |
| `CRATE_NAME` | Crate identity string constant |
| `crate_id()` | Returns `CRATE_NAME` (used in diagnostics) |

## Integer convention

Integer signals emitted by this pass use `i32` semantics.  Conversions from
`usize` indices are explicit and fallible to preserve deterministic diagnostics
on overflow.

## Position in the pipeline

```
eval  →  [propagate]  →  signals  →  transform  →  codegen
```
