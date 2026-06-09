# Session Handoff

Date: 2026-06-09

## Repo State

- Branch: `autodiff2`
- HEAD: `6aa549cd472660a9189ce1d0acc4e0da2db24f9f`

Recent commits (most recent first):

- `6aa549cd` Add libfaust export verification
- `4367a107` Add Signal C++ API header
- `736c567c` Add Signal C API header
- `4f258950` Add Signal normal form and source helpers
- `c9d3b3ca` Add Signal foreign constructors
- `c59ee151` Add Signal structural predicates
- `8b407464` Add Signal recursion constructors
- `ba381fd0` Add Signal table soundfile and UI constructors

## Working Tree

- Tracked changes:
  - none after the latest committed step
- Untracked local files/directories:
  - many pre-existing local scratch files remain untracked at the repository
    root; they were left untouched

## Current Goal(s)

- Execute `porting/libfaust-box-signal-api-parity-plan-2026-06-09-en.md`.
- Keep documentation, journal entries, and commits split per completed step.

## What Changed This Session

- Planned and committed the libfaust Box/Signal parity roadmap.
- Generated Box and Signal API matrices under `porting/generated/`.
- Extracted shared tree FFI context support into `tree-ffi`.
- Completed Box API parity fixes for right shift, `exp10`, soundfile wrappers,
  Box-to-Signal arrays, and Box source generation contracts.
- Added the maintained Signal FFI surface, including constructors, recursion,
  predicates, foreign nodes, normal-form helpers, source generation, and C/C++
  headers.
- Added `cargo run -p xtask -- libfaust-export-check` to build `faust-ffi`,
  compare exported symbols against maintained headers, and syntax-check C/C++
  clients.

## Decisions / Constraints (important for resume)

- `tree-ffi` owns shared C tree handle encoding and the process-global
  `TreeFfiContext`.
- Signal recursion uses Rust's canonical external `SIGREC(body)` shape; `CisRec`
  reports a deterministic adapted mapping.
- Signal doc-table predicate wrappers currently return deterministic false until
  Rust has explicit doc-table IR nodes.
- Header wrappers stay thin over the C ABI; no separate C++ object model was
  introduced.
- `JOURNAL.md` remains an index; detailed entries stay in
  `porting/journal/YYYY-MM-DD.md` with newest commit first inside the day file.

## Validation Run

- `cargo run -p xtask -- libfaust-export-check` -> passed; 269 header symbols
  exported by `target/debug/libfaust.dylib`
- `cargo fmt --all` -> passed
- `cargo clippy --workspace --all-targets -- -D warnings` -> passed
- `cargo test --workspace --all-targets` -> passed

## Open Issues / Blockers

- None for the planned Box/Signal API parity work items in this session.
- Longer-term parity work still needs differential tests against the C++ libfaust
  behavior for representative Box/Signal construction and source-generation
  cases.

## Next Steps (ordered)

1. Consider wiring `cargo run -p xtask -- libfaust-export-check` into CI next to
   existing golden/API guardrails.
2. Add C++ differential tests for selected Box and Signal wrapper cases once the
   reference fixture strategy is settled.
3. Continue toward runtime/source-generation parity beyond the maintained
   libfaust Box/Signal API surface.

## Useful Commands to Resume

- `cargo run -p xtask -- libfaust-export-check`
- `cargo run -p xtask -- libfaust-api-matrix --cpp-root /Users/letz/Developpements/RUST/faust --out porting/generated`
- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `git status --short`

## Notes

- The plan file is
  `porting/libfaust-box-signal-api-parity-plan-2026-06-09-en.md`.
- The day journal is `porting/journal/2026-06-09.md`.
