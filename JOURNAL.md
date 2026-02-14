# JOURNAL

## 2026-02-14

- Applied the structure defined in `porting/faust-rust-porting-plan-en.md`, section `4. Cargo Workspace Architecture`.
- Created Cargo workspace members for all crates listed in the plan.
- Added scaffold source files for each crate.
- Added `crates/compiler` as both library facade and binary entry point (`faust-rs`).
- Created `cffi/` and `tests/` placeholders.
- Extended CI to include `windows-latest` and split a dedicated `cargo check --workspace --all-targets` job from lint/test jobs.
- Updated `README.md` with a dedicated "How to compile" section (`cargo build --workspace`, release build, package build, and run command).
- Added a GitHub Actions CI badge in `README.md` for visual build status feedback.
- Added `AGENTS.md` at repository root with contribution and coding-agent guidelines (workspace rules, CI gates, porting discipline, and documentation hygiene).
- Enriched `AGENTS.md` with key constraints from `porting/` documents: frozen scope exclusions, Phase 0 validation gate, critical risk checklist, canonical pipeline target, and recursion/RouteIR coexistence guidance.
- Added a `xtask` crate implementing golden workflow commands:
  - `golden-check`
  - `golden-gen-rust`
  - `golden-gen-cpp` (using `FAUST_CPP_BIN`)
- Added initial corpus/golden scaffolding:
  - `tests/corpus/pass_through.dsp`
  - `tests/golden/cpp/pass_through/compiler_stdout.txt`
  - `tests/golden/METADATA.toml` with pinned C++ baseline metadata.
- Added CI golden validation step (`cargo run -p xtask -- golden-check`).
- Updated `README.md` and `AGENTS.md` with golden workflow documentation.
- Fixed cross-platform golden stability by normalizing source newlines before snapshot hashing/counting; added `.gitattributes` and a unit test to prevent LF/CRLF divergence.
- Added dual golden-reference mode in `xtask`:
  - `golden-check` (default Rust reference, used by CI),
  - `golden-check-cpp` (strict C++ parity target),
  - separate storage under `tests/golden/rust/` and `tests/golden/cpp/`.
- Refreshed C++ goldens with local `faust` and corrected invalid corpus case `rep_03_stereo_mix.dsp`.
- Consolidated all backend scaffolds into the `codegen` crate under `crates/codegen/src/backends/<backend>/mod.rs` (one folder per backend).
- Removed standalone workspace members `crates/backend-*` and updated the workspace manifest accordingly.
- Updated `codegen` public surface to expose `codegen::backends::*`.
- Aligned porting documentation with the new backend layout:
  - `porting/faust-rust-porting-plan-en.md`
  - `porting/phases/phase-6-fir-backends-en.md`
  - `porting/phases/phase-7-backends-supp-en.md`
  - `porting/phases/phase-9-integration-en.md`
