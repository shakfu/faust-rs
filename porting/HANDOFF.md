# Session Handoff

Date: 2026-07-19

## Repo State

- Branch: `transform-cleanup` (linear on top of `main` @ `86be9426`).
- HEAD: `246b9702` — R0 through R3 of
  `porting/transform-cleanup-documentation-factorization-plan-2026-07-19-en.md`
  are complete and committed; R4 is next.
- Working tree: clean (plus untracked local `tests/impulse-tests/node_modules`
  etc. for the asc gate).

## Plan progress

| Phase | State | Commits |
|---|---|---|
| R0 freeze | done, all gates green | `ab14a1ed` |
| R1 docs rewrite | done (2 parts), full battery green | `0c53de09`, `deebe3d7` |
| R2 test splits | done (3 parts), full battery green | `6c49e1c5`, `559a79cd`, `c1fd79f3` |
| R3 namespace | done, gates green (see note below) | `b643fdd7`, `0c829798` |
| R4.4 walker | done, arbiter net 0 defects, coverage 1,536 unchanged | `7d527c96` |
| R4.2 part 1 (common/ids) | done | `37271bce` |
| R5.2 verify split | done, arbiter 319/319, 0 defects | `a1a98c79` |
| R5.1 analysis split | done, arbiter 319/319, 0 defects | `f51e0068` |
| R5.3 plan split — **R5 complete** | done, arbiter 319/319, 0 defects | `246b9702` |
| R4 rest, R6–R9 | not started | — |

## Byte-identity arbiter (R0.5)

- Frozen worktree: `/Users/peter/git/faust-rs-baseline-worktrees/r0-freeze`
  (commit `86be9426`, release compiler built).
- Script: `/Users/peter/git/faust-rs-baseline-worktrees/compare-emissions.sh`
  (outside the repo). Emits `-lang cpp -double` × {scalar, vec0, vec1} for the
  132 impulse-corpus DSPs from both trees, byte-compares, and rechecks any new
  difference by *three* working-tree emissions before declaring a defect.
- **Pre-existing defect (recorded in the R0 journal entry, do not fix inside
  the cleanup):** scalar emission is nondeterministic run-to-run on
  delay-heavy DSPs (intermittently!). 77 of 396 cases are frozen in
  `nondeterministic-frozen.txt`; zero certified-vec cases affected.
  Reproducer: compile `zita_rev1.dsp` twice, diff. Suspect: `HashMap`
  iteration in `signal_fir/delay/manager.rs` (`delay_lines`).
- Environment: Faust libs resolved via gitignored symlink
  `target/share/faust -> /opt/homebrew/share/faust` in both trees.

## Decisions taken

- R0 alias policy (plan default): workspace migrated to
  `signal_fir::vector::{...}`; `pub use vector_*` facade re-exports retained.
- R2 layout: `signal_fir/tests/` and `signal_prepare/tests/` grouped by
  contract with `pub(super)` fixtures; the 12 vector stages are now
  `X/mod.rs` + `X/tests.rs` directories, ready for the R5–R7 splits.
- Upstream test failures on `main` (if any reappear): verify against a clean
  worktree before attributing to this branch.

## Validations run (latest per milestone)

- `cargo test -p transform --lib`: 385 (R0-recorded count, unchanged).
- Workspace clippy (`+1.97.0`) and tests: green.
- `golden-check`: 196 OK. `vector-coverage-check`: 1,536 pairs, 16 modes.
- Arbiter: 0 defects at every milestone (R2: 323/325 + 2 ND; R3: 320/323 +
  3 ND reclassified after 4×4 manual re-emission — journal entry
  "R3 milestone gates" has the details).

## Next steps (R4)

1. (done, `7d527c96`) Exhaustive FIR walker shared from `crates/fir`.
2. Prepared-ID indexing extraction (R4.2): the repeated shape is the
   `(signal_id -> record)` map plus `u64::from(record.signal_id)` checked
   conversions — see `vector/clock_ad/mod.rs` ~620-720 for the densest
   instance; `state` and `analysis` repeat it. Review each caller's
   admission semantics before sharing (plan R4.3 rule: same policy only;
   otherwise separate wrappers around a shared total conversion).
3. DTO model modules (prep for R5–R7), re-exported from old paths.
4. Then R5–R9 per plan; §4.8 guard rule applies to the R6/R7 splits
   (`reject_unadopted_stateful_reads` must stay on both producer and checker
   paths, each with a rejection test through the checker entry point).

## Useful commands

```bash
cargo test -p transform --lib                       # quick loop
/Users/peter/git/faust-rs-baseline-worktrees/compare-emissions.sh   # byte gate
cargo run -q -p xtask -- golden-check
cargo run -q -p xtask -- vector-coverage-check      # ~1-2 h
```
