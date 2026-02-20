# parser-proto

Parser migration prototype using `lrpar` / `lrlex`, kept isolated from `crates/parser`.

This crate hosts the incremental port of the Faust grammar and lexer.  It is the
staging area for new parser slices before they graduate to the production `parser` crate.

## C++ provenance

| C++ file | Role |
|---|---|
| `compiler/parser/faustparser.y` | Bison grammar |
| `compiler/parser/faustlexer.l` | Flex lexer |
| `compiler/errors/errormsg.hh` / `errormsg.cpp` | `setDefProp` / `setUseProp` hooks |
| `compiler/global.hh` | `gWaveForm`, `gResult` globals |

## Scope

- `ParserCtx` — parser-local state and property hooks (`def`/`use` properties).
- Lexer subset ported from `faustlexer.l` with token-priority tests.
- Parser slices 1–12 with real semantic actions wired to `crates/boxes`.
- `SourceReader` — import resolution and cycle detection.

## Public API

| Item | Description |
|---|---|
| `ParserCtx` | Per-parse session state |
| `SourceReader` | Multi-file import graph resolver |
| `parse_program` | Parse one Faust source string into a box tree |

## Integer literal convention

Parser integer tokens are lowered to `boxes` integer nodes with `i32` semantic
width.  Token parsing uses `i64` as an intermediate and clamps to `i32` bounds
at the parser boundary for deterministic behavior.

## Position in the pipeline

```
source text  →  [parser-proto]  →  boxes  →  eval  →  …
```
