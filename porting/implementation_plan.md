# Implemented Plan — Factoring `crates/compiler/src/main.rs`

This document closes the restructuring of the main `faust-rs` CLI binary. The
goal was to isolate argument parsing, timing, diagnostic formatting, and
compilation orchestration into a dedicated modular directory, without changing
user-visible behavior.

## Status

Implemented on 2026-06-01.

The restructuring preserves the existing CLI behavior: historical options,
Faust-style aliases, output modes, diagnostic formats, and the extended-stack
thread launcher are unchanged. The CLI tests were moved together with the code
they cover.

## Implemented Changes

### Crate `compiler`

#### [NEW] [`cli/mod.rs`](file:///Users/letz/Developpements/RUST/faust-rs/crates/compiler/src/cli/mod.rs)

Declares child modules and keeps the CLI-local test module:

```rust
pub mod args;
pub mod diagnostics;
pub mod runner;
pub mod timer;

#[cfg(test)]
mod tests;
```

#### [NEW] [`cli/args.rs`](file:///Users/letz/Developpements/RUST/faust-rs/crates/compiler/src/cli/args.rs)

Groups CLI configuration types and command-line parsing:

- `CliLang`
- `ErrorFormat`
- `ErrorVerbosity`
- `CliSignalFirLane`
- `CliArgs`
- `normalize_legacy_args`

Historical aliases (`-lang`, `-cn`, `-scn`, `-pn`, `-svg`, etc.) are still
normalized before `clap` parsing.

#### [NEW] [`cli/timer.rs`](file:///Users/letz/Developpements/RUST/faust-rs/crates/compiler/src/cli/timer.rs)

Contains the global timing/timeout helper:

- `CompilationTimer`

#### [NEW] [`cli/diagnostics.rs`](file:///Users/letz/Developpements/RUST/faust-rs/crates/compiler/src/cli/diagnostics.rs)

Contains human and JSON rendering for CLI diagnostics:

- `print_structured_diagnostics`
- `format_diagnostics_human`
- `format_diagnostics_human_with_verbosity`
- `format_diagnostics_json`
- `format_diagnostics_json_with_verbosity`
- helpers for context, snippets, caret spans, filtered notes, and debug fields

#### [NEW] [`cli/runner.rs`](file:///Users/letz/Developpements/RUST/faust-rs/crates/compiler/src/cli/runner.rs)

Contains the operational CLI logic:

- global help and dedicated error-format help
- text/binary output, WASM + companion JSON, WAST
- architecture wrapping
- selection of compilation options, internal real type, and Signal/FIR lane
- FIR fixture compilation and dump modes
- `run_main` orchestration

#### [NEW] [`cli/tests.rs`](file:///Users/letz/Developpements/RUST/faust-rs/crates/compiler/src/cli/tests.rs)

Contains the unit tests that previously lived in `main.rs`:

- CLI parsing and legacy argument normalization
- human/JSON diagnostic rendering
- WASM + companion JSON output
- WAST rendering
- complex diagnostic snapshots

#### [MODIFIED] [`main.rs`](file:///Users/letz/Developpements/RUST/faust-rs/crates/compiler/src/main.rs)

`main.rs` is reduced to a thread launcher, with the 64 MiB stack contract
documented:

```rust
//! `faust-rs` CLI launcher.

mod cli;

fn main() {
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(cli::runner::run_main)
        .expect("failed to spawn compiler thread")
        .join()
        .expect("compiler thread panicked");
}
```

## Verification

Executed:

```bash
cargo fmt --all
cargo test -p compiler --all-targets
```

Result:

- `cargo fmt --all`: OK
- `cargo test -p compiler --all-targets`: OK
  - `src/lib.rs`: 33 tests OK
  - `src/main.rs` / `cli::tests`: 43 tests OK
  - compiler integration tests: OK, with one pre-existing ignored test in
    `rad_runtime`

Not executed in this pass:

- `cargo build --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo run -p xtask -- golden-check`

Those broader checks remain useful before global integration, but the CLI
factorization itself is covered by the targeted `compiler` crate tests.
