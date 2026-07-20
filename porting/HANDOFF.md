# Session Handoff

Date: 2026-07-20

## Repo State

- Branch: `transform-cleanup` (linear on top of `main` @ `86be9426`).
- HEAD: see `git log` — R0 through R9 of
  `porting/transform-cleanup-documentation-factorization-plan-2026-07-19-en.md`
  are complete and committed (one milestone per commit, §4.8 guards listed
  in the R6/R7 commit messages).
- Working tree: clean (plus untracked local `tests/impulse-tests/node_modules`
  etc. for the asc gate).

## Plan progress

| Phase | State | Commits |
|---|---|---|
| R0 freeze | done | `ab14a1ed` |
| R1 docs rewrite | done (2 parts) | `0c53de09`, `deebe3d7` |
| R2 test splits | done (3 parts) | `6c49e1c5`, `559a79cd`, `c1fd79f3` |
| R3 namespace | done | `b643fdd7`, `0c829798` |
| R4.4 walker + R4.2p1 | done | `7d527c96`, `37271bce` |
| R5 analysis/verify/plan splits | done | `a1a98c79`, `f51e0068`, `246b9702` |
| R6 state/clock_ad/route/lower splits | done | `35ca7be2`, `5168eec6`, `1c4dd3ad`, `4632db74` |
| R7 events/assemble/module splits | done | `3fac3536`, `c489c3b2`, `55629619` |
| R8 explicit scalar imports | done | `51eadfd9` |
| R9 structure-check, 798→0 docs, warn(missing_docs) | done | `81c6689d`, `a62b2509` |

Every split milestone: transform lib tests green, clippy (+1.97.0, = CI
stable) clean, byte-identity arbiter 319/319 identical / 0 defects.
Coverage gate (1,536 certified mode/DSP pairs) green at R5.3, R6.2 and the
R6.3–R7.3 batch (see journal). §4.8: `reject_unadopted_stateful_reads`
now has rejection tests through BOTH the producer path and the standalone
checker entry (`clock_ad/tests.rs`).

## Architecture after the cleanup

Each vector stage is a directory with physically separated producer and
checker files plus a shared vocabulary module:

- `analysis/{conditions,dependencies,effects,uses}.rs`
- `verify/{model,error,check,fused_groups,checker_reachability}.rs`
- `plan/{model? (in mod), build,fusion,producer_reachability}.rs`
- `state/{model,build,check,simulation}.rs`
- `clock_ad/{model,build,check,simulation}.rs`
- `route/{model,session,check}.rs`
- `lower/{program,signal,tables,check}.rs`
- `events/{model,produce,check}.rs`   ← assurance boundary
- `assemble/{model,materialize,check}.rs`
- `module/{build,outputs,lifecycle,check}.rs`

Intentionally retained duplication (plan §3.2 — do NOT merge; module
headers repeat this in place): events `producer_*` vs
`independently_*`/`checker_required_*`/`independent_checked_sample_count`;
assemble materializers vs `independently_expected_clock_cursor` /
`state_cursor_advance_matches` / shape matchers; plan
`producer_reachability` vs verify `checker_reachability`. clock_ad/state
checkers re-derive through the same derivation functions (pre-existing
architecture, preserved as-is).

## Byte-identity arbiter (R0.5)

- Frozen worktree: `/Users/peter/git/faust-rs-baseline-worktrees/r0-freeze`
  (commit `86be9426`, release compiler built).
- Script: `/Users/peter/git/faust-rs-baseline-worktrees/compare-emissions.sh`
  (outside the repo; 3-emission recheck for new diffs).
- **Defect FIXED (2026-07-20, post-cleanup follow-up):** the intermittent
  run-to-run nondeterminism of scalar emission on delay-heavy DSPs was
  root-caused to `HashMap` iteration order in the delay subsystem
  (`DelayPlan.lines` driving struct-field/clear-loop allocation order,
  `DelayManager.delay_lines` driving maintenance emission) and fixed by
  converting those collections to `BTreeMap`/`BTreeSet`
  (`porting/scalar-emission-determinism-plan-2026-07-20-en.md`). The
  determinism invariant is now enforced in-repo by
  `cargo run -p xtask -- emission-determinism` (396-case matrix, empty
  allowlist `tests/impulse-tests/emission-determinism-allowlist.txt`);
  the external frozen-list workflow is obsolete for determinism checking.
- Environment: Faust libs via gitignored symlink
  `target/share/faust -> <faust install>/share/faust` in both trees
  (`/opt/homebrew/...` on ARM, `/usr/local/...` on Intel).
- The baseline worktree is still in place; remove it only after the final
  battery is accepted (`git worktree remove`).

## New quality gates (R9)

```bash
cargo run -p xtask -- structure-check                 # layout contract
cargo rustdoc -p transform --lib -- -D missing-docs   # docs completeness
```

`#![warn(missing_docs)]` is active in `crates/transform/src/lib.rs`;
rustdoc is fully silent (0 warnings, all categories).

## Deferred / not done (recorded, intentional)

- R4 remainder: `index_unique_by` extraction (55 sites use the
  `(x.id, x) → BTreeMap` idiom with differing admission semantics — plan
  R4.3 same-policy rule not met); `ValueType→FIR` conversion stayed
  per-stage (route/lower/assemble each own a variant with different
  admission).
- R8.2: no extra split of `bra.rs`/`build.rs`/`core_lowering.rs`
  (explicit imports revealed no independent responsibility).
- R8.4: 63 scalar panic/expect sites surveyed — all documented local
  invariant assertions, none cross-phase, none converted.
- pv_slice retained (its retirement gate was not run).

## Validation commands

```bash
cargo test -p transform --lib                       # quick loop (387)
/Users/peter/git/faust-rs-baseline-worktrees/compare-emissions.sh
cargo run -q -p xtask -- golden-check               # 196 OK
cargo run -q -p xtask -- vector-coverage-check      # 1,536 pairs, ~1.5-2 h
cargo run --release -p xtask -- vector-compile-budget-check
make -C tests/impulse-tests backend-matrix-smoke    # delete ir/ first!
cargo test -p compiler --test vector_mode           # 35 oracle tests
```

Impulse-harness traps: delete `tests/impulse-tests/ir/<mode>/` before runs
(cached `.ir` reports green), and check the `filesCompare` invocation count
matches the DSP count.
