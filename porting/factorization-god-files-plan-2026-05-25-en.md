# Factorization Plan — Splitting the Three "God Files"

Date: 2026-05-25
Status: actionable plan (not yet started)
Scope: `transform/src/signal_fir/module.rs`, `eval/src/lib.rs`, `compiler/src/lib.rs`
Driver: `porting/faust-rs-code-quality-assessment-2026-05-25-en.md` §5.1, §8 Tier 2

---

## 0. Goal and Hard Constraints

Reduce the three largest source units into cohesive, single-concern modules
**without changing any public API or runtime behavior**.

Hard constraints (non-negotiable):

- **Zero behavior change.** This is a pure code-move refactor. No logic edits.
- **No public API change.** External crates must not need to touch their imports.
- **Green gate after every step** (see §1.3). Never batch multiple extractions
  before validating.
- **One concern per commit.** Each extracted module is its own small commit so a
  regression bisects to a single move.

Why these three, in this order:

1. `module.rs` (5020 LOC) — **lowest risk, highest mechanical leverage**: the
   logic is already partitioned into 5 `impl` blocks; the struct is module-private
   so the split cannot affect any other file.
2. `eval/lib.rs` (4946 LOC) — medium risk: free functions, so moves need
   `pub(crate)` visibility bumps, but clusters are clean.
3. `compiler/lib.rs` (4599 LOC) — highest judgment: the façade goal requires
   distinguishing orchestration from helper logic, not just moving code.

---

## 1. Shared Mechanics

### 1.1 Pattern A — split an `impl` across child modules (used for `module.rs`)

Rust rule that makes this free: **a child module can access private items of an
ancestor module**, including private struct fields. So if `SignalToFirLower` is
defined in `module/mod.rs`, then `module/core.rs`, `module/tables.rs`, … can each
contain `impl<'a> SignalToFirLower<'a> { … }` blocks that touch private fields,
**with no visibility changes**.

Procedure to convert a file `foo.rs` into a directory module:

```
git mv crates/.../foo.rs crates/.../foo/mod.rs
# then create sibling files crates/.../foo/<part>.rs
# add `mod <part>;` lines in foo/mod.rs
```

Each new file starts with the same `use` block as the parent (or `use super::*;`)
plus `use super::SignalToFirLower;`. Move the `impl SignalToFirLower` block
verbatim. Nothing else changes.

### 1.2 Pattern B — split free functions (used for `eval/lib.rs`, `compiler/lib.rs`)

Free functions that move out of `lib.rs` into a sibling module must be reachable
from `lib.rs` and from each other. Procedure:

- Bump each moved `fn` from private to `pub(crate)` (or `pub(super)` if only the
  parent uses it).
- Add `mod <name>;` in `lib.rs`.
- Add `use <name>::*;` (or explicit `use`) in `lib.rs` and in any sibling module
  that calls across.
- Shared private free helpers that everything calls (e.g. small arena utilities)
  stay in `lib.rs` and become `pub(crate)` so children can call them.

### 1.3 The verification gate (run after EVERY extraction)

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p <touched-crate>
cargo run -p xtask -- golden-check        # output parity guardrail
```

Full `cargo test --workspace` + `golden-check` before the final commit of each
Part. Because this is a code-move, the golden snapshots must be **byte-identical**
before and after — any golden diff means a real (forbidden) behavior change slipped
in.

### 1.4 Pre-flight check before touching each file

```bash
git grep -n "<PrivateType>" crates/<crate>/src | grep -v "<file being split>"
```

Confirms the type/fn being relocated is not referenced elsewhere (already verified
for `SignalToFirLower` — module-private, single entry `build_module`).

---

## 2. Part A — `transform/src/signal_fir/module.rs` (5020 → ~8 files)

### 2.1 Current state (verified 2026-05-25)

- Single private `struct SignalToFirLower<'a>` (defined at `:875`), ~20 fields,
  **127 methods already grouped into 5 `impl` blocks** aligned to section
  dividers.
- One public entry point: `pub(super) fn build_module(...)` at `:299`.
- Helper free fns at column 0: `classify_reverse_time_outputs` (`:795`),
  `match_ffunction_node` (`:1049`), `map_binop` (`:4996`), plus structs
  `SamplePhases` (`:153`), `ForeignFunProto` (`:1042`), `FirRadFormulaBuilder`
  (`:4871`).
- `SignalToFirLower` is **not referenced outside `module.rs`** → the split is
  internal to the `signal_fir::module` subtree; `signal_fir/mod.rs` (`mod module;`
  at `:65`, calls `module::build_module` at `:226`) needs **no change**.

### 2.2 Target layout

Convert `module.rs` → `module/` directory:

| New file | Content (move verbatim) | Source range |
|---|---|---|
| `module/mod.rs` | `build_module`, struct `SignalToFirLower` + fields, `SamplePhases`, `ForeignFunProto`, free fns (`classify_reverse_time_outputs`, `match_ffunction_node`, `map_binop`), the setup `impl` (`new`, `ensure_sample_rate_var`, `prepare_delay_lines`, `real_ty`, `variability_of`, `konst_escapes`, `materialize_in_bucket`, `simple_type`, `signal_fir_type`, `zero_value_for_signal`, …), and `mod` declarations | `:1`–`:1432` core + glue |
| `module/core.rs` | "Core signal lowering" impl: `lower_signal`, `lower_output_signal`, `reset_sample_loop_state`, `lower_fconst`, `lower_fvar`, `lower_ffun`, `decode_foreign_fun_proto`, `foreign_sig_type`, `lower_input`, `lower_delay`, `lower_fixed_delay`, `lower_delay_state`, `lower_forward_output_delay1_for_reverse_loop`, `lower_shift_delay1` | `:1434`–`:2211` |
| `module/state.rs` | Delay/recursion/state helpers (non-BRA): `resolve_recursion_delay_ref`, `resolve_recursion_carrier`, `bind/load_scalar_recursion_current_value`, `ensure_state_slot`, `ensure_delay_line_decl`, `delay_line_info`, global-circular-cursor trio, `with_active_recursion_group`, `register_clear_recursion_array`, `fresh_loop_var`, `ensure_named_struct_var`, `register_reset_init`, `register_clear_init`, `register_constant_table_init` | `:2225`–`:2503`, `:3973`–`:4028` |
| `module/bra.rs` | Block-Reverse-AD lowering cluster: `propagate_bra_unary_math_adj`, `propagate_bra_binary_math_adj`, `emit_reverse_time_rec_compute_resets`, `emit_bra_compute_resets`, `lower_block_reverse_ad_proj`, `ensure_bra_backward_sweep`, `propagate_bra_adj`, `ensure_bra_delay1_carry`, `ensure_bra_delay_array_carry`, `ensure_bra_tape_stores`, `load_bra_fwd_value`, `add_to_adjoint`, `float_const`, `initial_state_from_signal` | `:2504`–`:3398` (BRA subset) |
| `module/ui.rs` | UI lowering: `ensure_button_zone`, `lower_button`, `lower_slider`, `lower_bargraph`, `lower_soundfile`, `soundfile_var_from_signal`, `lower_soundfile_length/rate/buffer`, `label_text`, `ui_control_var_name`, `control_spec`, `control_range`, `emit_ui_metadata_for_target`, `control_metadata_value`, `emit_control_ui_metadata`, `ensure_slider_zone`, `ensure_bargraph_zone`, `ensure_soundfile_zone`, `emit_ui_program`, `emit_ui_node` | `:3404`–`:3567`, `:4037`–`:4391` |
| `module/tables.rs` | Table & waveform lowering: `lower_waveform`, `lower_rdtbl`, `lower_wrtbl`, `resolve_table`, `ensure_waveform_table`, `ensure_readonly_table`, `ensure_wrtbl_table`, `table_size_from_sig`, `expand_generator_values`, `fir_const_for_table_value`, `normalized_table_index`, `table_index_with_bounds` | `:3568`–`:3972` |
| `module/arithmetic.rs` | Arithmetic/selection/projection: `lower_binop`, `lower_math1`, `lower_math2`, `lower_minmax`, `lower_abs`, `lower_cast`, `lower_bitcast`, `lower_select2`, `lower_proj` | `:4402`–`:4870` |
| `module/rad_formula_builder.rs` | `struct FirRadFormulaBuilder` + `impl FirRadFormulaBuilder` + `impl RadFormulaBuilder for FirRadFormulaBuilder` | `:4871`–end |

> The line ranges above interleave slightly (e.g. `register_*_init` sits between
> the UI and arithmetic sections). When moving, **move by method name, not by raw
> line slice** — group each method with its target file regardless of its current
> position. The single existing `impl` blocks will fragment cleanly because Rust
> permits multiple `impl SignalToFirLower<'a>` blocks across files.

### 2.3 Step sequence (8 commits)

1. `git mv module.rs module/mod.rs`; add empty `mod core; … mod rad_formula_builder;`
   stubs are **not** created yet — add each `mod X;` line only when file `X` is
   created, so the tree always compiles. Gate.
2. Extract `module/rad_formula_builder.rs` (most self-contained — its own struct).
   Gate.
3. Extract `module/arithmetic.rs`. Gate.
4. Extract `module/tables.rs`. Gate.
5. Extract `module/ui.rs`. Gate.
6. Extract `module/bra.rs`. Gate.
7. Extract `module/state.rs`. Gate.
8. Extract `module/core.rs`; `mod.rs` now holds only struct + setup + glue
   (~700–900 LOC target). Final full gate (`test --workspace` + `golden-check`).

Expected end state: `module/mod.rs` ≈ 700–900 LOC; the rest 200–700 LOC each.

### 2.4 Risk

Very low. No visibility changes, no cross-file consumers, behavior frozen by
golden snapshots. Only failure mode is a missed `use` import → compile error
(caught immediately, not a silent bug).

---

## 3. Part B — `eval/src/lib.rs` (4946 → core + ~6 modules)

### 3.1 Current state (verified 2026-05-25)

`eval` already extracted `environment`, `error`, `loop_detector`,
`source_context`, `pattern_matcher` (2026-03-24). `lib.rs` retains the evaluator
core (150 free fns, `&mut TreeArena`-threaded). Existing section dividers:
Propagation+simplification (`:1915`), Route normalization (`:2041`), Seq numeric
folding (`:2142`), Box simplification (`:2229`), Label node (`:2432`). Beyond
`:2432` the functions fall into clear, currently-undivided clusters.

### 3.2 Target layout (Pattern B — `pub(crate)` moves)

| New file | Functions to move | Approx source |
|---|---|---|
| `eval/src/ui_widgets.rs` | `evaluated_label_node`, `eval_button`, `eval_checkbox`, `eval_vslider`, `eval_hslider`, `eval_num_entry`, `eval_slider_like`, `simplify_slider_param`, `eval_soundfile`, `eval_vgroup`, `eval_hgroup`, `eval_tgroup`, `eval_vbargraph`, `eval_hbargraph` | `:2439`–`:2680` |
| `eval/src/modulation.rs` | `eval_modulation`, `eval_modulation_label`, `eval_modulation_circuit`, `implant_modulation`, `implant_widget_if_match`, `widget_matches_modulation_target`, `modulation_target_path` | `:1560`–`:1668`, `:2680`–`:2890` |
| `eval/src/label.rs` | `eval_label_node`, `eval_label`, `is_eval_label_ident_char`, `write_label_ident_value`, `strip_label_node`, `strip_label_metadata`, `label_node_text`, `is_subsequence` | `:1669`–`:1820`, `:2432`–`:2940` |
| `eval/src/definitions.rs` | `bind_definitions`, `rewrite_captured_env`, `copy_env_replace_defs`, `decode_definition`, `top_level_definition_names`, `ident_name`, `build_abstr_from_parser_args`, `map_children` | `:2941`–`:3236` |
| `eval/src/apply.rs` | `rev_eval_list`, `apply_value_list`, `apply_value_list_value`, `apply_pattern_matcher_value`, `apply_list`, `larg2par`, `concat_lists`, `nwires`, `list_outputs_for_apply`, `infer_box_arity_for_apply`, `infer_box_arity`, `is_binary_primitive_non_prefix` | `:3237`–`:3857` |
| `eval/src/iteration.rs` | `iteration_var_name`, `eval_non_negative_count`, `eval_iter_body`, `empty_iteration_route`, `neutral_seq_body`, `iterate_par`, `iterate_seq`, `iterate_sum`, `iterate_prod` | `:3858`–`:4114` |
| → `eval/src/pattern_matcher.rs` (existing) | `rule_parts`, `case_expected_arity`, `eval_rule_list`, `eval_pattern_list`, `eval_pattern`, `pattern_simplification`, `simplify_pattern`, `eval_case_value` — consolidate case/pattern logic with the existing matcher | `:1416`, `:1967`, `:4125`–`:4274` |

Keep in `lib.rs` (the evaluator dispatch core): `eval_process*` /
`eval_entrypoint*` family, `eval_box`, `eval_value`, `eval_value_uncached`,
`eval_ident_value`, `eval_route_value`, `eval_seq_value`, `eval_access_value`,
`eval_loaded_source_value`, `a2sb*`, plus the already-sectioned route/seq/box
simplification blocks (or move those to a `simplify.rs` if `lib.rs` is still
>2000 LOC after the above).

### 3.3 Step sequence (per cluster, gate between each)

Order by independence: `ui_widgets` → `label` → `modulation` (depends on widgets)
→ `iteration` → `apply` → `definitions` → pattern/case consolidation. Bump moved
fns to `pub(crate)`, add `mod`/`use` to `lib.rs`, gate.

### 3.4 Risk

Low–medium. The only mechanical hazard is visibility: a moved fn that calls a
still-private `lib.rs` helper needs that helper bumped to `pub(crate)`. Compiler
catches all such cases. Behavior frozen by `core_eval.rs` tests + golden gate.

---

## 4. Part C — `compiler/src/lib.rs` (4599 → façade + helper modules)

### 4.1 Principle

`compiler` is the orchestration crate (`AGENTS.md` §2). `lib.rs` should read as
**declarative composition of pipeline stages**, not contain stage-internal logic.
Today it mixes the `Compiler` orchestration (`impl Compiler` at `:386`–`:1866`,
~1480 LOC) with seven distinct helper concerns that already carry section
dividers.

### 4.2 Target layout (extract helper concerns first; thin the impl last)

| New file | Content (move verbatim, Pattern B) | Source section |
|---|---|---|
| `compiler/src/diagnostics.rs` | `DiagCtx` + diagnostic enrichment, error mapping, error-node extraction, propagate diagnostic enrichment, source label helpers, source span resolution | `:2144`, `:2276`, `:2698`, `:3018`, `:3338`, `:3394`, `:3524` |
| `compiler/src/signal_lowering.rs` | `SignalLoweringContext` struct + `LowerError<E>` / `LowerToInterpError` / `LowerToFirError` + signal-to-FIR lower functions | `:2351`–`:2697` |
| `compiler/src/json_options.rs` | `StrictJsonContext`, `compile_options_json_string` | `:2780`–`:3017` |
| `compiler/src/box_preview.rs` | box preview helpers | `:3057`–`:3337` |
| `compiler/src/paths.rs` | `default_import_search_paths` + path resolution helpers | `:2022`–`:2143` |
| `compiler/src/definition_graph.rs` | definition graph helpers | `:3736`–`:3894` |
| `compiler/src/golden.rs` | `golden_snapshot`, `golden_snapshot_from_file` | `:3895`–`:3957` |
| `compiler/src/names.rs` | name utilities | `:2744`–`:2779` |

Keep in `lib.rs`: public output types (`SignalCompileOutput`, `FirCompileOutput`,
`WasmArtifact*`, `AuxFileArtifact`, `ExpandDspRequest`, `GenerateAuxFilesRequest`,
`FaustwasmService*`, `FirVerifyOptions`), `Compiler`, `SignalFirLane`,
`CompilerError`, and the `impl Compiler` reduced to orchestration.

### 4.3 Thinning `impl Compiler` (the judgment step — do last)

After the helper modules exist, walk the ~1480-line `impl Compiler` and, for each
method, classify:

- **Orchestration** (sequences stage calls, maps errors, returns outputs) → stays
  in `lib.rs`, ideally readable as `parse → eval → propagate → … → backend`.
- **Stage-internal logic** (heavy private method bodies) → extract the body into a
  free `pub(crate) fn` in the relevant new module; the method becomes a one-line
  delegate.

Do **not** force a façade by gutting orchestration — the goal is that stage
*logic* lives in stage modules, while `Compiler` methods stay as thin sequencers.
Stop when `lib.rs` is dominated by type definitions + short orchestration methods.

### 4.4 Step sequence

Extract in dependency-leaf order: `names` → `golden` → `paths` →
`json_options` → `box_preview` → `definition_graph` → `signal_lowering` →
`diagnostics` (largest, most cross-referenced — last). Gate between each. Then the
§4.3 thinning pass as a final, separately-reviewed commit. Move the `#[cfg(test)]`
block (`:3958`) into a `tests` submodule or alongside the code it exercises.

### 4.5 Risk

Medium. `compiler` is the most-depended-on crate (box-ffi, cranelift-ffi,
interp-ffi, wasm-ffi, xtask all consume it), so **any accidental change to a `pub`
item breaks downstream crates**. Mitigations:
- Extracted helpers are overwhelmingly private → bump only to `pub(crate)`, never
  `pub`, so the external surface is untouched.
- Run `cargo build --workspace` (not just `-p compiler`) in the gate so FFI crates
  are checked.
- The §4.3 thinning is the only step with judgment; keep it its own commit for
  clean review/revert.

---

## 5. Sequencing, Effort, and Milestones

| Milestone | Deliverable | Risk | Est. effort |
|---|---|---|---|
| M1 | Part A done — `module/` split, mod.rs ≤ 900 LOC | very low | ~0.5 day |
| M2 | Part B done — `eval` core ≤ ~2000 LOC, 6 new modules | low–med | ~1 day |
| M3 | Part C extractions done (§4.2) | medium | ~1 day |
| M4 | Part C façade thinning (§4.3) | medium | ~0.5 day |

Do them in order M1→M4. M1 builds confidence in the mechanics and the golden gate
on the safest target. Each milestone is independently shippable and leaves the
tree green.

### Suggested commit messages

```
refactor(transform): split signal_fir/module.rs into per-concern modules
refactor(eval): extract ui_widgets/modulation/label/apply/iteration modules
refactor(compiler): extract diagnostics/signal_lowering/json helpers from lib.rs
refactor(compiler): thin impl Compiler to stage orchestration
```

---

## 6. Definition of Done

- [ ] No file among the three exceeds ~2000 LOC; `module/mod.rs` ≤ 900.
- [ ] `cargo build --workspace` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [ ] `cargo test --workspace` green.
- [ ] `cargo run -p xtask -- golden-check` reports **zero** diffs (proves behavior frozen).
- [ ] No new `pub` item added to any crate's external surface (only `pub(crate)`/`pub(super)`).
- [ ] Each Part landed as its own reviewable commit(s) per §1 ("one concern per commit").
- [ ] `JOURNAL.md` updated with the splits (per `AGENTS.md` §11).

---

## 7. Notes / Open Questions for the Author

- **Part A `module/bra.rs` naming**: `signal_fir/` already has a
  `block_reverse_ad.rs`. To avoid confusion, the in-module BRA *lowering* cluster
  should be named distinctly (`module/bra_lowering.rs`) or the existing file's
  role clarified. Confirm preferred name.
- **Part B pattern/case consolidation**: the §3.2 last row folds case/pattern fns
  into the existing `pattern_matcher.rs`. If you'd rather keep `pattern_matcher.rs`
  strictly as the matcher engine, create `eval/src/case.rs` instead. Author's call.
- **Part C façade depth**: §4.3 is deliberately conservative (extract logic, keep
  orchestration). If you want a stricter façade (e.g. a dedicated `pipeline.rs`
  that owns the stage sequence and leaves `Compiler` as a pure handle), that is a
  larger follow-up and should be its own plan.
