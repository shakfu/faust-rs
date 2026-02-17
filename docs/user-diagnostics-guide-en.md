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
