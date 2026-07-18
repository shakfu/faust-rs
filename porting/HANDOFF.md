# Session Handoff

Date: 2026-07-18

## Repo State

- Branch: `codex/fix-ondemand-18-toggle-morph`
- HEAD: this fix commit (rebased onto `main-dev` before integration)

Recent commits (most recent first):

- `5b147984` Expand Rust and Julia impulse test matrices
- `bd7f2000` Canonicalize pure FIR drop scaffolding
- `dc4e8b91` Align Rust backend with Faust C++ runtime

## Working Tree

- Tracked changes: contextual delay, recursion, and state ownership; focused
  regressions; journal and this handoff.
- Untracked local files/directories: none.

## Current Goal

- Fix `ondemand_18_toggle_morph.dsp` without sharing persistent state between
  sibling clock-domain occurrences.

## What Changed This Session

- Keyed planned/allocated delay lines and recursion-output analysis by signal
  identity plus occurrence clock domain.
- Applied the same context to recursion carriers, scalar current bindings,
  standalone state slots, and update/write deduplication.
- Kept each wrapper's defining clock in its parent context while planning its
  state, matching the guarded-block precondition emission.
- Added deterministic contextual state names and regressions for a shared
  stateful payload in sibling `ondemand` regions.

## Decisions / Constraints

- Clock domain is part of state identity; hash-consing alone does not authorize
  state sharing across independently firing regions.
- `TempVar` state belongs to the parent emission domain, matching scalar
  ancestor redirection.
- A wrapper's defining clock is evaluated in the parent domain; only its held
  payloads enter the new domain.
- Top-rate state names remain unchanged; contextual names use `_d<domain>`.
- This is an internal adapted API mapping with no CLI/C/C++ compatibility
  impact.

## Validation Run

- Original `ondemand_18_toggle_morph.dsp` compilation -> passed; no undeclared
  FIR variable.
- Generated C++ impulse binary, 60,000 frames, `filesCompare` against the C++
  reference `.ir` -> exact match.
- `cargo test -p transform` -> 384 passed.
- `cargo test -p compiler --test clocked_emission_structure` -> 7 passed.
- `cargo test -p compiler --test clocked_waveform_regression` -> 1 passed.
- `cargo test -p compiler --test ondemand_pipeline` -> 32 passed.
- `cargo fmt --all -- --check` -> passed.
- `cargo clippy --workspace --all-targets -- -D warnings` -> passed.
- `cargo run -p xtask -- golden-check` -> passed.
- `cargo test --workspace --all-targets --exclude cranelift-ffi` -> passed.
- `cargo test --workspace --all-targets` -> all tests reached before
  `cranelift-ffi` passed; that crate then failed as described below.

## Open Issues / Blockers

- No blocker for the reported bug.
- Baseline `cranelift-ffi` test
  `boxes_and_signals_constructor_match_string_constructor_sha` compares FIR
  dumps containing different `TreeId` allocations and fails at clean commit
  `5b147984`; the shared test mutex is then poisoned, causing 24 cascade
  failures. This change does not touch that crate.

## Next Steps

1. Review and commit the coherent working-tree change if requested.
2. Fix the independent `cranelift-ffi` baseline test in its own change.

## Useful Commands to Resume

- `cargo run -p compiler -- /Users/letz/Developpements/RUST/faust/tests/impulse-tests/od/ondemand_18_toggle_morph.dsp`
- `cargo test -p compiler --test clocked_emission_structure`
- `cargo test -p compiler --test ondemand_pipeline`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo run -p xtask -- golden-check`

## Notes

- Worktree: `/private/tmp/faust-rs-ondemand-18-toggle-morph`.
- The original checkout and its pre-existing changes were not modified.
