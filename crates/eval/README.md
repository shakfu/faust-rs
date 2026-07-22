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

### Entry points

| Function | Description |
|---|---|
| `eval_process(arena, root)` | Evaluate `process` from a parsed program |
| `eval_entrypoint(arena, root, name)` | Evaluate a named definition instead of `process` |
| `eval_process_with_stats(arena, root)` | Like `eval_process`, also returns `EvalStats` |
| `eval_entrypoint_with_stats(arena, root, name)` | Like `eval_entrypoint`, also returns `EvalStats` |
| `eval_process_with_source_context(arena, root, ctx)` | With source location tracking |
| `eval_entrypoint_with_source_context(arena, root, name, ctx)` | With source location tracking |
| `eval_process_with_stats_and_source_context(arena, root, ctx)` | Stats + source context |
| `eval_entrypoint_with_stats_and_source_context(arena, root, name, ctx)` | Stats + source context |
| `eval_entrypoint_with_source_context_and_cancel(arena, root, name, ctx, token)` | Cooperative cancellation |

### Types

| Item | Description |
|---|---|
| `EvalError` | Typed error covering all evaluation failure modes |
| `EvalStats` | Environment, lookup, node-visit, loop-depth, and definition-name statistics |
| `Environment` | Lexical environment (name → `TreeId`) |
| `EvalSourceContext` | Source-location tracking context passed to evaluator |
| `LoopDetector` | Per-symbol expansion depth guard (mirrors C++ `loopDetector`) |
| `SamplePrecision` | `Float32` / `Float64` precision selector |
| `SymId` | Interned symbol identifier |
| `EnvId` | Environment frame identifier |

### Pattern matcher

| Item | Description |
|---|---|
| `make_pattern_matcher(rules)` | Build a deterministic automaton from a list of case-rules |
| `apply_pattern_matcher(matcher, args, env)` | Run the automaton against argument list; returns matched body or `None` |
| `Rule` | A single `(patterns, body)` case-rule |
| `Automaton` | Compiled deterministic pattern-matching automaton |
| `State`, `Trans`, `TransKind` | Automaton internals (states and transitions) |

### Utilities

| Item | Description |
|---|---|
| `crate_id()` | Returns the crate identity string (used for diagnostics) |

## Parity notes

- Non-closure partial application follows C++ `applyList` semantics (implicit wire insertion).
- Loop detection mirrors C++ `loopDetector` (per-symbol expansion depth guard).

## Position in the pipeline

```
parser  →  boxes  →  [eval]  →  propagate  →  signals
```
