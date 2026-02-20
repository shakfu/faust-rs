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

- **Arity inference** (`box_arity`) — determines `(inputs, outputs)` for any box family.
- **Signal lowering** (`propagate`) — walks the box tree and emits `signals` nodes via `SigBuilder`.
- **Composition algebra** — `seq`, `par`, `split`, `merge` with explicit bus routing.
- **Recursive forms** — De Bruijn-style placeholders (`sigRec` / `sigProj` shape).

## Public API

| Item | Description |
|---|---|
| `box_arity(arena, box_id)` | Infer `(inputs, outputs)` for a box |
| `propagate(arena, process_box, inputs)` | Lower box tree to signal list |
| `make_sig_input_list(arena, n)` | Build ordered input signal list |
| `BoxArity` | `{ inputs: usize, outputs: usize }` |
| `PropagateError` | Typed error covering arity mismatches and unsupported nodes |

## Integer convention

Integer signals emitted by this pass use `i32` semantics.  Conversions from
`usize` indices are explicit and fallible to preserve deterministic diagnostics
on overflow.

## Position in the pipeline

```
eval  →  [propagate]  →  signals  →  transform  →  codegen
```
