# JOURNAL

Journal entries are split by journal day under `porting/journal/`.

For each day file, entries are ordered from most recent commit to oldest using Git history.

## Daily Files (oldest day first)

- [`porting/journal/2026-02-14.md`](porting/journal/2026-02-14.md)
- [`porting/journal/2026-02-15.md`](porting/journal/2026-02-15.md)
- [`porting/journal/2026-02-16.md`](porting/journal/2026-02-16.md)
- [`porting/journal/2026-02-17.md`](porting/journal/2026-02-17.md)
- [`porting/journal/2026-02-18.md`](porting/journal/2026-02-18.md)
- [`porting/journal/2026-02-19.md`](porting/journal/2026-02-19.md)
- [`porting/journal/2026-02-20.md`](porting/journal/2026-02-20.md)
- [`porting/journal/2026-02-21.md`](porting/journal/2026-02-21.md)
- [`porting/journal/2026-02-22.md`](porting/journal/2026-02-22.md)
- [`porting/journal/2026-02-23.md`](porting/journal/2026-02-23.md)
- [`porting/journal/2026-02-24.md`](porting/journal/2026-02-24.md)
- [`porting/journal/2026-02-25.md`](porting/journal/2026-02-25.md)
- [`porting/journal/2026-02-26.md`](porting/journal/2026-02-26.md)
- [`porting/journal/2026-02-27.md`](porting/journal/2026-02-27.md)
- [`porting/journal/2026-02-28.md`](porting/journal/2026-02-28.md)
- [`porting/journal/2026-03-01.md`](porting/journal/2026-03-01.md)
- [`porting/journal/2026-03-02.md`](porting/journal/2026-03-02.md)
- [`porting/journal/2026-03-03.md`](porting/journal/2026-03-03.md)
- [`porting/journal/2026-03-04.md`](porting/journal/2026-03-04.md)
- [`porting/journal/2026-03-06.md`](porting/journal/2026-03-06.md)
- [`porting/journal/2026-03-07.md`](porting/journal/2026-03-07.md)
- [`porting/journal/2026-03-09.md`](porting/journal/2026-03-09.md)
- [`porting/journal/2026-03-10.md`](porting/journal/2026-03-10.md)
- [`porting/journal/2026-03-11.md`](porting/journal/2026-03-11.md)
- [`porting/journal/2026-03-12.md`](porting/journal/2026-03-12.md)
- [`porting/journal/2026-03-13.md`](porting/journal/2026-03-13.md)
- [`porting/journal/2026-03-14.md`](porting/journal/2026-03-14.md)
- [`porting/journal/2026-03-15.md`](porting/journal/2026-03-15.md)
- [`porting/journal/2026-03-16.md`](porting/journal/2026-03-16.md)
- [`porting/journal/2026-03-17.md`](porting/journal/2026-03-17.md)
- [`porting/journal/2026-03-18.md`](porting/journal/2026-03-18.md)
- [`porting/journal/2026-03-21.md`](porting/journal/2026-03-21.md)

See [`porting/journal/README.md`](porting/journal/README.md).

## 2026-03-22 — fix(serial): UI labels with embedded newlines crash fbc parser

### Problem
`elecGuitarMIDI.fbc` (WAC 2017) failed to parse with
`parse failed: errors=1, recoveries=0, diagnostics=1`.

The label of one slider was `"sustain\n"` — a literal `0x0a` byte inside
the quoted string, produced by the original Faust C++ compiler.
`read_ui_block` called `read_line` once per instruction; `read_line` stopped
at the embedded `\n`, so the remaining fields (`key`, `value`, `init`, …)
ended up on the **next** physical line and caused a parse failure for every
subsequent instruction.

### Fix — `crates/codegen/src/backends/interp/serial.rs`
- Added `read_quoted_logical_line`: reads physical lines and joins them with
  `\n` until all `"` characters are balanced (even count = every opened quote
  is closed).
- `read_ui_block` and `read_meta_block` now call
  `read_quoted_logical_line` instead of `read_line` when reading per-instruction
  lines.
- New regression test `test_ui_instruction_label_with_embedded_newline`
  reproduces the exact layout from `elecGuitarMIDI.fbc` and verifies that
  the label is preserved as `"sustain\n"` and all numeric fields are correct.
