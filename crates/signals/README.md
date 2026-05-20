# signals

Signal-graph construction and pattern-matching helpers backed by `tlib::TreeArena`.

Signals are the Faust compiler's representation between box propagation and
code-generation.  They carry the full DSP computation graph: math operations,
delays, tables, UI bindings, and recursion projections.

## C++ provenance

| C++ file | Role |
|---|---|
| `compiler/signals/signals.hh` / `signals.cpp` | Signal node families and constructors |
| `compiler/signals/binop.hh` | Binary operator opcodes |

## Public API

| Item | Description |
|---|---|
| `SigBuilder` | Construction API — aligned with the `BoxBuilder` style in `crates/boxes` |
| `SigMatch` / `match_sig` | Pattern-matching API — structural inspection of a signal node |
| `SigId` | `TreeId` alias for signal nodes |
| `dump_sig_readable` | Debug pretty-printer for a signal subtree |
| `ad_rules` | Backend-neutral local reverse-AD rule classification and formulas shared by `propagate` and FIR/BRA lowering |

## Parity invariants

- Signal nodes are represented as tagged trees with **deterministic child order**.
- Numeric constants are direct `Int` / `FloatBits` nodes (no wrapper indirection).
- Slider parameter payload keeps Faust list encoding: `list4(init, min, max, step)`.
- Public integer surface uses `i32` semantics; underlying storage remains `NodeKind::Int(i64)`.
- `BlockReverseAD` is a semantic Signal-IR carrier for `rad(...)` block
  fallback; concrete tapes, loop scheduling, and storage typing are owned by
  `transform::signal_fir`.
- `ad_rules` is the narrow exception to this crate being representation-only:
  it contains backend-neutral local RAD algebra for math/binop nodes, but does
  not allocate tapes or inspect temporal scheduling.

## Position in the pipeline

```
propagate  →  [signals]  →  transform  →  codegen
```
