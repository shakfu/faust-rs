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
- [`porting/journal/2026-03-22.md`](porting/journal/2026-03-22.md)
- [`porting/journal/2026-03-23.md`](porting/journal/2026-03-23.md)

See [`porting/journal/README.md`](porting/journal/README.md).

## 2026-03-23 — refactor(boxes,eval): dead-code sweep — macros, dead helpers, stale allow attributes

Deleted `define_is_prim!` macro + 52 `is_node_prim_*` invocations from `boxes` (C++ parity
stubs superseded by `BoxMatch`). Deleted four dead structural helpers (`match_binary`,
`match_ternary`, `match_unary`, `match_slider`) and their now-orphaned internal helper
`match_tag_arity`, plus `list_nth` (test file has its own copy). Removed 3 stale
`#[allow(dead_code)]` attributes from `eval` on functions actively used in production.
Net: −120/+0 lines; zero warnings; all tests pass.

## 2026-03-23 — refactor(compiler): extract error helper + generic LowerError + de-dup C fastlane

Three complexity hot-spots in `compiler/src/lib.rs`: (1) extracted
`make_propagate_compiler_error` free function, replacing 3 near-identical ~30-line `map_err`
closures in `pipeline_to_signals`; (2) unified `LowerToCppError` + `LowerToCError` into
`LowerError<E>` generic type alias; (3) `lower_signals_to_c_transform_fastlane` now calls
`lower_signals_to_fir_transform_fastlane` instead of duplicating it.
Net: −40/+25 lines; zero warnings; all 52 compiler tests pass.

## 2026-03-23 — refactor: dead-code sweep — compatibility wrappers, unused predicates, orphaned utilities

Removed four public `BoxId` compatibility wrappers from `propagate`
(`box_arity`, `propagate`, `propagate_with_ui`, `box_arity_flat`); migrated all
callers to the typed API (`try_build_flat_box` + typed entry point). Deleted 47
private C++ parity predicates (`is_node_*`) from `boxes` — superseded by
`BoxMatch` pattern matching. Removed `encode_legacy_source_backed_bitcode` +
`esc_bitcode_field` from `cranelift-ffi` (V2 format abandoned),
`parse_string_token` from `codegen/interp`, and `union_s` from `interval`.
Net: −563/+151 lines, zero warnings, all tests pass.

## 2026-03-23 — fix(eval): integer div folding — `4/2` must produce `Int(2)`, not `Real(2.0)`

`zita_rev1.dsp` failed (`sequential composition mismatch: left outputs (0) !=
right inputs (16)`).  `fold_binop` produced `Real(2.0)` for `4/2`; pattern
matching in `selector` requires `Int(2)`.  Fix: `try_fold_seq_numeric`
converts exact-integer `Real` back to `Int` when all inputs are `Int`.

---

## 2026-03-23 — docs: parser-to-FIR parity analysis report

Full-pipeline parity analysis (parser → FIR) of faust-rs vs Faust C++.
Front-end stages at 95–100%, signal→FIR lowering at 60–70% (main gap: no
normalization in fast-lane, no occurrence analysis/CSE, no VectorCompiler).
Report: `porting/parser-to-fir-parity-analysis-2026-03-23-en.md`.

---

## 2026-03-23 — fix(interval): `hi_or2` mask rule off-by-one → exponential recursion in type annotator

### Problem
macOS `sample` profile of `dynamic-jack-gtk` showed 100% of CPU (1904/1904
samples) inside `interval::bitwise::hi_or2` recursing on itself.  The type
annotator (`TypeAnnotator::infer_binop` → `logic::and` → `bitwise_signed_or`)
was hanging indefinitely on complex DSPs.

### Root cause — `crates/interval/src/bitwise.rs`
The mask short-circuit in `hi_or2` should test `a.hi == 2*m - 1`, but the code
computed `2 * m.wrapping_sub(1)` (= `2m-2`, off by one).  For a full-range
interval `[0, 2^n-1]` the rule never fired, causing 3 recursive sub-calls per
level → O(3^32) ≈ 10^15 calls.  `bitwise_signed_and` (used by the `&` type rule)
calls De Morgan which produces exactly such wide intervals from `NOT`.

### Fix
`ma.wrapping_add(ma).wrapping_sub(1)` replaces `2 * ma.wrapping_sub(1)`.

### Result
`guitarEffectChain.dsp`: 9.7 s → 2.0 s.
`minimoog-novation.dsp`: 4.7 s (beats reference faust compiler at 8.7 s).
Added `hi_or2_mask_rule_terminates_on_full_power_of_two_range` regression test.
All 65 interval unit tests pass.

---

## 2026-03-22 — fix(parser): stdfaust.lib + demos.lib triggers 19 "multiple definitions" parse errors

### Problem
`guitarEffectChain.dsp` (WAC 2017) failed with 19 parse errors ("multiple
definitions of a zero-argument symbol are not allowed") when combining
`import("stdfaust.lib")` and `import("demos.lib")`.

Both libraries define the same 19 library-alias symbols (`ma`, `ba`, `de`,
`si`, …).  `format_definitions` collected both as pattern-match variants and
errored.  The C++ compiler silently shadows the earlier definition.

### Fix — `crates/parser/src/lib.rs`
In `make_definition_from_variants`, replaced the hard error for multiple
zero-arity definitions with last-import-wins: use `first_body` (the newest
definition from `variants_rev.iter().rev()`).

---

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
