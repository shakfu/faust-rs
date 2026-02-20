# eval

Box-level evaluator — Phase 4, section 2.2 of the Faust compiler pipeline.

Takes a parsed box environment and reduces the `process` definition to a fully
evaluated box tree ready for `propagate`.

## C++ provenance

| C++ file | Role |
|---|---|
| `compiler/evaluate/eval.hh` / `eval.cpp` | Core evaluation logic |
| `compiler/evaluate/environment.hh` | Lexical environment model |
| `compiler/evaluate/loopDetector.hh` | Recursive-expansion loop detection |

## What this crate does

1. Builds an `Environment` from the parser's name→expression bindings.
2. Resolves `process`.
3. Evaluates recursively by box family:
   - Lexical forms: `abstr`, `with`, `letrec`, `access`
   - Application: `appl` / `case` (with pattern matching)
   - Iterative forms: `ipar`, `iseq`, `isum`, `iprod`
   - Structural fallback map for non-reducing nodes

## Public API

| Item | Description |
|---|---|
| `eval_process(arena, root)` | Evaluate `process` from a parsed program |
| `EvalError` | Typed error covering all evaluation failure modes |
| `Environment` | Lexical environment (name → `TreeId`) |

## Parity notes

- Non-closure partial application follows C++ `applyList` semantics (implicit wire insertion).
- Loop detection mirrors C++ `loopDetector` (per-symbol expansion depth guard).

## Position in the pipeline

```
parser  →  boxes  →  [eval]  →  propagate  →  signals
```
