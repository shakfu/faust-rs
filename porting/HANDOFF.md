# Session Handoff

Date: 2026-07-18

## Repo State

- Branch: `codex/add-ondemand-impulse-tests`
- HEAD: `Expand clocked impulse test corpus` (this commit); parent `4de7c8f5`
  (`Adapt ma.SR in multirate domains`), based on `main-dev` at `434f2eb6`

## Working Tree

- Tracked corpus changes in HEAD: 21 new `ondemand_*.dsp` files, 18 new paired
  `upsampling_*`/`downsampling_*` files, impulse README, journal, and this
  handoff.
- Generated ignored files: C++ references, interpreter responses, reference
  binaries/sources, and `tools/filesCompare` under `tests/impulse-tests/`.

## Current Goal

- Land the validated ondemand and multirate DSP corpus in the faust-rs impulse
  harness.

## What Changed This Session

- Copied `ondemand_01_basic` through `ondemand_21_nested_delay_counter` exactly
  from the C++ `tests/impulse-tests/od/` corpus.
- Added nine upsampling and nine downsampling cases, including complex state,
  nesting, selection, dynamic-rate, and `ma.SR`-dependent filter cases.
- Fixed input-consuming upsampling ZeroPad FIR access in separate commit
  `e75b4289`.
- Ported and verified C++ `ma.SR` multirate adaptation in separate commit
  `4de7c8f5`.
- Documented the resulting 132-DSP corpus size while keeping the historical
  93-DSP status table distinct.

## Decisions / Constraints

- Non-silence was audited before integration rather than inferred from a
  successful compilation or a reference comparison alone.
- A valid case must emit at least one finite non-zero scalar sample and match
  the C++ reference prefix at the default tolerance.
- `ma.SR` follows `SR*H` under US and `SR/H` under DS, with nested factors
  composed through the clock-domain parent chain.

## Validation Run

- Exact byte comparison of all 21 imported DSPs against the C++ source corpus
  -> passed.
- `cargo build --release -p impulse-runner` -> passed.
- Direct 15000-frame scalar run of every case -> all finite and non-silent;
  non-zero counts range from 144 to 15000.
- Direct comparison of all 21 scalar responses to existing C++ references with
  `filesCompare -part` -> passed.
- `make -j8 -f Make.ref reference` on the 21-case subset -> passed.
- `make -j8 -f Make.interp all` on the 21-case subset -> passed.
- Direct 15000-frame scalar runs of all 18 multirate cases -> finite and
  non-silent; non-zero counts range from 3 to 30000.
- C++ 60000-frame reference generation plus interpreter prefix comparison for
  all 18 multirate cases -> passed at the default tolerance.
- Targeted `make -j3 all` over all 39 additions -> passed on C++, C,
  interpreter, Cranelift, WebAssembly, AssemblyScript, Rust, and Julia.
- `ma.SR` runtime checks at 48 kHz -> US(3) = 144 kHz, DS(3) = 16 kHz,
  nested US(2)/DS(3) = 32 kHz.
- The same three `ma.SR` cases -> sample-for-sample parity with pinned C++.
- `cargo fmt --all -- --check` -> passed.
- `cargo clippy --workspace --all-targets -- -D warnings` -> passed.
- `cargo test --workspace --all-targets` -> passed.
- `cargo run -p xtask -- golden-check` -> passed.

## Open Issues / Blockers

- None for adding these DSPs.
- Scheduling/vector matrices were not rerun on the extended corpus in this
  session; all scalar backend targets were run.

## Next Steps

1. Commit the corpus addition separately from both compiler fixes.
2. Rebase/merge the linear three-commit branch into `main-dev` when requested.
3. Expand scheduling/vector matrix measurements to the 132-DSP corpus
   separately.

## Useful Commands to Resume

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo run -p xtask -- golden-check`

## Notes

- Worktree: `/private/tmp/faust-rs-ondemand-impulse-tests`.
- The original checkout and its pre-existing untracked files were not modified.
