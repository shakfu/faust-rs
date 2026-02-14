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
