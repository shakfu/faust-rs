# Session Handoff

Date: 2026-02-26

## Repo State

- Branch: `signals-after-deBruijn2Sym`
- HEAD: `30fa74c2bcfa86a24bb0afd95d97f627a54b3bf8`

Recent commits (most recent first):

- `30fa74c` Fix cargo test by adding rlib to cranelift-ffi
- `5eb476f` Document daily journal workflow in AGENTS
- `3c45774` Split JOURNAL into daily files ordered by Git history
- `4eebb49` Add experimental Cranelift compiler CLI mode
- `8d1e296` Fix Cranelift verifier failures on corpus subset

## Working Tree

- Tracked changes:
  - none
- Untracked local files/directories (user scratch/local tooling):
  - `.vscode/`
  - `crates/cranelift-ffi/examples/`
  - various local `*.dsp`, `*.c`, `*.cpp`
  - `tests/runtime_traces/cppfbc/`

## Current Goal(s)

- Cranelift backend bring-up continuation (backend first, C/C++ wrappers secondary)
- Keep journaling workflow stable after daily split migration

## What Changed This Session

- Added experimental compiler CLI path for Cranelift:
  - `--dump-cranelift`
  - `-lang cranelift` (alias `clif`)
- Split monolithic `JOURNAL.md` into daily files under `porting/journal/`
- Documented journaling workflow in `AGENTS.md`
- Fixed `cargo test` failure caused by local Rust example linking `cranelift-ffi`
  without an `rlib` crate artifact

## Decisions / Constraints (important for resume)

- `JOURNAL.md` must remain an index/redirect, not a large monolithic body
- `porting/journal/YYYY-MM-DD.md` preserves semantic day buckets
- Entries inside each day file are sorted by Git commit recency (newest first)
- Cranelift compiler CLI path is explicitly experimental and reports
  `compute_body_lowered` to distinguish real lowering vs stub fallback

## Validation Run

- `cargo test` -> ✅ workspace pass (after adding `rlib` to `cranelift-ffi`)
- `cargo run -p compiler -- -lang cranelift tests/corpus/rep_01_passthrough.dsp` -> ✅ backend report emitted

## Open Issues / Blockers

- Cranelift backend still partial on corpus (not all FIR shapes lowered)
- `cranelift_dsp` C/C++ runtime/export layer remains scaffold-heavy beyond core wiring

## Next Steps (ordered)

1. Continue Cranelift FIR lowering coverage based on `corpus_scan_cranelift` gaps
2. Add optional strict experimental CLI mode (fail when Cranelift uses stub fallback)
3. Progress `cranelift_dsp` runtime/FFI behavior beyond scaffold placeholders
4. Add CI smoke coverage for experimental Cranelift compiler path (opt-in)

## Useful Commands to Resume

- `cargo test`
- `cargo run -p compiler -- --dump-cranelift tests/corpus/rep_01_passthrough.dsp`
- `cargo run -p compiler -- -lang cranelift tests/corpus/rep_01_passthrough.dsp`
- `cargo run -p compiler --example corpus_scan_cranelift`
- `cargo run -p compiler --example corpus_scan_cranelift rep_05 rep_07 rep_09 rep_10`

## Notes

- If working on Cranelift backend, start in:
  - `crates/codegen/src/backends/cranelift/mod.rs`
  - `crates/compiler/examples/corpus_scan_cranelift.rs`
  - `crates/compiler/src/main.rs`
- If regenerating/reordering journal files, use Git history of `JOURNAL.md` as
  source of truth and preserve semantic day buckets.
