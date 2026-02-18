# Phase 5 Recursive Baseline Matrix

> Date: February 18, 2026  
> C++ reference branch: `master-dev-ocpp-od-fir-2-FIR19`  
> C++ reference commit: `8eebea429`

## 1. Scope

This baseline tracks recursion-relevant behavior for the Phase 5 addendum:

- de Bruijn recursion placeholders (`DEBRUIJN`, `DEBRUIJNREF`)
- symbolic recursion conversion (`de_bruijn_to_sym`)
- fast-lane policy (de Bruijn accepted directly)

## 2. Acceptance matrix (Rust vs C++ classification)

| Fixture / check | Rust | C++ | Status |
|---|---|---|---|
| `rep_23_feedback_simple` (recursive projection shape) | OK | OK | parity |
| `err_07_propagate_rec_mismatch_alias` (rec mismatch diagnostic class) | ERR | ERR | parity |
| `signal_pipeline::corpus_feedback_simple_exposes_recursive_projection` | OK | n/a (Rust test mirror) | covered |
| `signal_fir_lane::legacy_and_fastlane_both_compile_feedback_projection_fixture` | OK | n/a (Rust test mirror) | covered |
| `tlib::recursive_trees::de_bruijn_to_sym_converts_nested_scopes` | OK | n/a (C++ semantic mirror) | covered |

## 3. Commands used

- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo run -p xtask -- golden-check`

All three commands passed locally on February 18, 2026.
