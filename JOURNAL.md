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
- [`porting/journal/2026-03-24.md`](porting/journal/2026-03-24.md)
- [`porting/journal/2026-03-25.md`](porting/journal/2026-03-25.md)

See [`porting/journal/README.md`](porting/journal/README.md).

## 2026-03-25 — docs(porting): WebAssembly backend development plan with C++ parity constraints

Created `porting/wasm-backend-plan-2026-03-25-en.md` — full plan for the Rust WASM backend
port.  Section 3 documents 19 structural constraints from direct C++ source analysis (section
ordering, 11 exports in fixed order, 14 function bodies in alphabetical order, field alignment
= `audioSampleSize` for all types, JSON at offset 0 in data segment, math import naming with
`_` prefix, `genMemSize()` formula with backpatching, etc.).  Sections 4–5 define the Rust
architecture (using `wasm-encoder` crate) and 10-step implementation plan.

## 2026-03-25 — feat(cranelift): add soundfile support in Cranelift backend

`soundfile_zone_ptr()` added to `cranelift-ffi/src/instance.rs`: for `FirType::Sound` fields
in the `dsp*` struct it returns the address cast as `*mut *mut c_void` — the `Soundfile**`
expected by `SoundUI::addSoundfile`.  The `buildUserInterfaceCCraneliftDSPInstance` Soundfile
arm now calls `soundfile_zone_ptr` so the host writes the loaded `Soundfile*` directly into
the JIT struct field.  Three new `ComputeLowering` methods in `codegen/cranelift/mod.rs`
implement `LoadSoundfileLength/Rate/Buffer` using the C++ packed-struct offsets (fBuffers=0,
fLength=8, fSR=16, fOffset=24); `subset_expr_gap_reason` updated to accept all three variants.

## 2026-03-25 — fix(interp-ffi): wire soundfile zones so loaded audio reaches executor

After commit `5facc6a` the crash was gone but `compute()` played silence because
`executor.soundfiles[slot]` still held `default_silence()` stubs.  Fix: `ui.rs` passes
`&mut soundfile_zones[slot]` as the zone pointer; after `dispatch_ui_glue` returns,
`sync_soundfiles_from_zones` reads each non-null C++ `Soundfile*` via a `#[repr(C, packed)]`
mirror struct, copies `fLength`/`fSR`/`fOffset` arrays and per-channel audio buffers
(handling both `fIsDouble=false` f32 and `fIsDouble=true` f64), then replaces the stubs via
`set_soundfile()`.

## 2026-03-25 — feat(interp): implement soundfile support in interpreter backend

Full end-to-end soundfile pipeline for `faust-rs -lang interp`.  New module
`codegen/backends/interp/soundfile.rs` defines `Soundfile` with `read_sample(chan,part,idx)`.
Compiler: `soundfile_slots` map, `AddSoundfile` → slot + URL, three opcodes
(`LoadSoundFieldInt` for length/rate, `LoadSoundFieldReal` for buffer).  Executor: pops
`part`/`chan`/`idx` and dispatches to `sf.read_sample`.  Factory pre-populates with
`default_silence()`.

## 2026-03-24 — feat(signal_fir): lower SIGSOUNDFILELENGTH, SIGSOUNDFILERATE, SIGSOUNDFILEBUFFER

End-to-end soundfile support for the fast-lane pipeline.  Three new `FirMatch` variants added
to `crates/fir`.  Signal-FIR lowering maps the three Faust signal nodes to
`fSoundN->fLength[part]`, `fSoundN->fSR[part]`, and the full buffer-index expression.  C and
C++ backends emit the correct field-access expressions; C backend gains the missing
`AddSoundfile` handler.  `tp0.dsp` compiles to correct output.

## 2026-03-24 — feat(cli): add -v / -version / --version flag

`faust-rs` now accepts `-v`, `-version`, and `--version`, printing `faust-rs <version>` and
exiting.  Mirrors the reference Faust compiler `-version` option.  One file changed
(`crates/compiler/src/main.rs`, +11 lines).

## 2026-03-24 — fix(parser): block comment parsing and formatting

`crates/parser/src/source_reader.rs` — improved handling of multi-line block comments
(`/* … */`).  Also removed 14 stale `#[allow(unused_imports)]` / `#![allow(…)]` attributes
across `codegen`, `fir`, `transform`, and `parser`.  24 files changed.

## 2026-03-24 — refactor(boxes,eval): module splits

`crates/boxes/src/lib.rs` (2169 lines) split into `tags.rs`, `internals.rs`, `builder.rs`,
`matcher.rs`, `dump.rs`.  `crates/eval/src/lib.rs` (6523 lines) split into
`source_context.rs`, `error.rs`, `environment.rs`, `loop_detector.rs`.  All public APIs
re-exported unchanged from `lib.rs`.

## 2026-03-24 — fix(eval): float literal pattern matching (`t8.dsp`)

`simplify_pattern` illegally coerced integer-valued `Real` constants to `Int` before
`TreeId` comparison, causing `foo2(1.0) = 456; process = foo2(1.0)` to fail.  Fix aligns
with C++ `simplifyPattern`: literals are returned as-is with no Real→Int coercion.  Corpus
entry `rep_72_float_literal_pattern.dsp` added.

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
