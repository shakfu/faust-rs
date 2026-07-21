# Faust-rs Diagnostics Guide (User)

This guide explains how to read and use compiler diagnostics in the Rust port.

## 1. Run with diagnostics

```bash
# Human output (default)
cargo run -p compiler -- --dump-sig tests/corpus/err_03_propagate_split_mismatch.dsp --error-format human

# Human output with extra internal context (debug)
cargo run -p compiler -- --dump-sig tests/corpus/err_03_propagate_split_mismatch.dsp --error-format human --error-verbosity debug

# JSON output (for tooling/automation)
cargo run -p compiler -- --dump-sig tests/corpus/err_03_propagate_split_mismatch.dsp --error-format json
```

## 2. How to read one error

Typical human output includes:

- header: `error [FRS-...-....] message`,
- source label with line/caret when available,
- notes:
  - `cause:` why the compiler failed,
  - `rule:` semantic rule being checked,
  - `computed:` concrete values computed by the compiler,
  - context notes (`expr=...`, owner definition, alias trace),
- `help:` concrete fix suggestions.

Example interpretation:

- `FRS-PROP-0002` means a Phase 4 connection/arity mismatch.
- If notes say `split(A, B) requires inputs(B) % outputs(A) == 0`, the fix is to make
  `inputs(B)` a multiple of `outputs(A)`.

## 3. Error families (quick map)

- `FRS-PARSE-*`: lexer/parser syntax/recovery issues.
- `FRS-EVAL-*`: box evaluation issues (`process`, symbols, arity, iteration).
- `FRS-PROP-*`: signal propagation/connectivity issues.
- `FRS-SRC-*`: source loading/import resolution issues.

## 4. Standard vs debug verbosity

- `standard`: concise output focused on actionable notes.
- `debug`: includes low-level internal context (for example `node_id`, compact `box_expr`).

Use `debug` for bug reports and parity investigations, `standard` for day-to-day usage.

## 5. JSON contract (for tools)

JSON diagnostics include stable fields:

- `severity`, `stage`, `code`, `message`,
- `labels[]` with exact spans,
- `notes[]`, `help[]`,
- optional `debug` object in debug verbosity.

This format is intended for CI snapshots and editor/tool integrations.

Under `--error-format json`, the payload is written to **stdout alone**: no
human-readable prefix line precedes it, and stdout is a single well-formed
JSON document with no leading or trailing non-JSON bytes, on both success and
failure paths that emit one. (`--error-format human` is unaffected by this
and keeps writing to stderr exactly as before.) A `CompilerError` variant
that carries no structured bundle (backend codegen failures, unresolved
imports) still gets a minimal envelope with `"code": null` rather than
silence, so a JSON consumer never has to special-case "no output". See the
frozen code table in `docs/diagnostics-codes-en.md` for the full `FRS-*`
list, including which codes are reachable in practice today.

## 6. `--check`: diagnostics without codegen

`--check` runs the full front-end (parse → eval → propagate → type) plus FIR
verification, does no code generation, and exits `0` (no errors) or `1`
(errors). Under `--error-format json` it **always** emits a payload, with an
empty `diagnostics` array on success, so success and failure share exactly
one schema:

```bash
cargo run -p compiler -- tests/corpus/rep_01_passthrough.dsp --check --error-format json
# {"diagnostics": []}

cargo run -p compiler -- tests/corpus/err_03_propagate_split_mismatch.dsp --check --error-format json
# {"diagnostics": [{"code": "FRS-PROP-0002", ...}]}
```

This is the mode automated tooling (CI, an IDE, a future MCP server) should
prefer over `--dump-cpp`/`--dump-sig` when it only needs to know whether a
DSP is valid: it is the same front-end work with no codegen or dump-text
side channel to filter out.
