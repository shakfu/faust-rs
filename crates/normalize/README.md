# normalize

Signal normalization and algebraic simplification — ported from `compiler/normalize/`.

## C++ provenance

| C++ path | Role |
|---|---|
| `compiler/normalize/normalize.hh` / `normalize.cpp` | Add-term and delay-term normalization |
| `compiler/normalize/simplify.hh` / `simplify.cpp` | Memoized signal rewrite engine |
| `compiler/normalize/normalform.hh` / `normalform.cpp` | Normal-form pipeline coordinator |

## Architecture

The normalization pipeline follows a five-layer dependency order:

```
normalform   ← pipeline coordinator (de-Bruijn → symbolic → typed → promoted)
  simplify   ← memoized rewrite engine
    normalize  ← add-term + delay-term normalization
      aterm    ← additive term (sum of mterms)
        mterm  ← multiplicative term (k · x^n · y^m / …)
```

## Current status

- `mterm`: complete.
- `aterm`, `normalize`, `simplify`, `normalform`: in progress.

## Public API

| Item | Description |
|---|---|
| `normalform::prepare_signals(arena, sigs, opts)` | Phase 1 normal-form preparation (de-Bruijn → symbolic → typed → promoted) |
| `normalform::prepare_signals_multi(arena, sigs, opts)` | Multi-output variant of `prepare_signals` |
| `normalform::promote_signals(arena, sigs)` | Signal promotion pass only |
| `normalform::promote_signals_fastlane(arena, sigs)` | Promotion fast path for `transform::signal_fir` |
| `normalform::NormalFormOpts` | Options controlling the preparation pipeline |
| `normalform::NormalFormError` | Typed error covering recursion and type failures |
| `simplify_const(arena, sig)` | Fold constant sub-expressions in a signal tree |
| `crate_id()` | Returns the stable crate identifier |

## Position in the pipeline

```
propagate  →  [normalize]  →  transform  →  codegen
```
