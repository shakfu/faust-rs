# Plan — Decomposing the `SignalToFirLower` god struct (W9)

Date: 2026-06-20
Status: actionable plan (not started)
Scope: `crates/transform/src/signal_fir/module/` (the `SignalToFirLower` struct and its `impl`s)
Drivers: `signal-to-fir-transform-analysis-2026-06-20-en.md` **W9 / I4**;
`faust-rs-code-quality-assessment-2026-05-25-en.md` §5.1

## Relationship to the existing factorization plan

`factorization-god-files-plan-2026-05-25-en.md` targeted the god *files*. Its **Part A**
(splitting `module.rs` into per-concern files via `impl SignalToFirLower` blocks spread across
child modules) is **done**: `module.rs` is now
`module/{mod,arithmetic,bra,build,core_lowering,setup,state,tables,ui_lowering,rad_formula_builder}.rs`.

That plan deliberately **kept the struct intact** — child modules reach the private fields, so no
struct change was needed to split the file. This plan is the next layer it left untouched: the god
*struct* `SignalToFirLower`, which has since grown from ~20 to ~51 fields.

## 0. Honest assessment (is this worth doing?)

- **Value is leverage, not correctness.** This is a pure, behavior-preserving refactor with **zero
  functional payoff**. Its return is future velocity: the struct is the central state of the most
  frequently edited part of the compiler, and every upcoming feature (lowering the W8 `Deferred`
  families, vectorization, the W1 type-pass work, the I5 placement/CSE unification) threads more
  state through it.
- **Lower risk than it looks.** Most of the work is compiler-checked mechanical field regrouping
  (`self.bra_grad_cache` → `self.bra.grad_cache`), guarded by the existing test net (transform 103 +
  the full `compiler` corpus + impulse tests). The one genuinely invasive cluster is
  `ModuleSections` (the statement buckets), touched by almost every lowering method.
- **The `*Ctx` bundles do not fully disappear.** `DelayFirCtx` / `DelayLoweringCtx` /
  `RecursionLoweringCtx` / `RecursionAllocCtx` exist because a method needs `&mut store` *plus*
  several disjoint accumulators at once. Grouping fields reduces how many things are threaded and
  lets *some* bundles go away, but the `store + sub-manager` split-borrow is inherent. The realistic
  win is "fewer, cohesive fields + some `*Ctx` removed", not "no `*Ctx`".
- **Viable alternative: lazy extraction.** Instead of a dedicated campaign, extract a cluster the
  next time that area is touched. For god-struct debt this is often the right economic call.

**Recommendation:** do the `BraState` **pilot** first (one commit), measure the real cost/benefit,
then choose campaign vs. lazy. Do **not** attempt a big-bang rewrite. This refactor is higher
*leverage* than the remaining correctness item (#4, `simplify`/CSE preservation property tests) but
the latter is the only outstanding *correctness* work — neither is urgent.

## 1. Goal and hard constraints

Turn `SignalToFirLower` (~51 fields) into an **orchestrator** (~14 fields) holding a handful of
cohesive sub-state structs.

- **Zero behavior change.** Pure field-regrouping + mechanical renames; no logic edits.
- **No public API change.** `SignalToFirLower` is module-private; the split is internal to
  `signal_fir::module`.
- **Green gate after every commit:** `cargo test -p transform && cargo test -p compiler`,
  `cargo clippy -p transform --all-targets`, `cargo fmt -p transform -- --check`.
- **One cluster per commit** so a regression bisects to a single extraction.

## 2. Current field inventory → target clusters

Keep at the top (genuinely shared / already extracted): the read-only config (`arena`,
`ui_program`, `types`, `sig_types`, `num_inputs`, `real_ty`), the shared output `store`, `cache`,
the already-extracted `recursion: RecursionState` and `delay: DelayManager`, `sample_phases`, and
the small delay-adjacent state (`state_name_by_node`, `scheduled_state_updates`, `uses_iota`,
`input_ptr_aliases`).

Extract into cohesive sub-state structs:

| New sub-state | Fields | Consumer files | Risk |
|---|---|---|---|
| `BraState` | `bra_state_scheduled`, `bra_grad_cache`, `bra_delay1_carry_vars`, `bra_delay_array_carry_vars`, `bra_tape_store_var` | `bra.rs` | low (pilot) |
| `PlacementInfo` | `sig_ref_counts`, `sig_at_boundary`, `konst_escapes` (read-only after analysis) | `core_lowering.rs`, `setup.rs` | trivial |
| `RadReverseState` | `forward_output_by_sig`, `forward_output_by_sig_key`, `lowering_reverse_loop` | `build.rs`, `core_lowering.rs` | low |
| `UsedPrototypes` | `used_math_ops`, `used_int_fun_names`, `used_foreign_fun_protos`, `used_foreign_vars` | `arithmetic.rs`, `core_lowering.rs`, `build.rs` | low |
| `NameGen` | `next_loop_var_id`, `fconst_counter`, `iconst_counter`, `fslow_counter`, `islow_counter` | many + `*Ctx` | medium |
| `UiLoweringState` | `ui_controls`, `soundfiles`, `waveform_tables`, `waveform_table_len`, `table_access_by_sig`, `ui_statements` | `ui_lowering.rs`, `tables.rs` | medium |
| `ModuleSections` | `struct_declarations`, `static_declarations`, `global_declarations`, `constants_statements`, `reset_statements`, `clear_statements`, `control_statements`, `named_struct_vars`, `reset_init_seen`, `clear_init_seen` | almost everything + `*Ctx` | high (do last) |

Result: the top struct drops from ~51 fields to ~14, each remaining field either shared, config, or
a named sub-state with a clear concern.

## 3. Phase 1 — regrouping (one commit per row, risk ascending)

For each cluster:

1. Define `struct <Cluster> { … }` (default-derivable where possible) in the file that owns the
   concern (e.g. `BraState` in `bra.rs`), with field names dropping now-redundant prefixes
   (`bra_grad_cache` → `grad_cache`).
2. Replace the individual fields on `SignalToFirLower` with one `field: <Cluster>`.
3. Update `SignalToFirLower::new` (`setup.rs`) to build the sub-state once.
4. Mechanically rename accesses (`self.bra_grad_cache` → `self.bra.grad_cache`). The borrow checker
   verifies correctness; the only manual judgement is where a method holds `&mut self.<cluster>`
   across a `&mut self` call (rare — most accesses are short-lived `insert`/`get`).
5. Run the green gate; commit.

Order:

1. **`BraState`** — pilot. 5 fields, confined to `bra.rs`, almost entirely renames.
2. **`PlacementInfo`** — read-only, trivial.
3. **`RadReverseState`**.
4. **`UsedPrototypes`**.
5. **`NameGen`** — `next_loop_var_id` is threaded through the `*Ctx` bundles; this is the first
   cluster that interacts with Phase 2.
6. **`UiLoweringState`**.
7. **`ModuleSections`** — the most cross-cutting cluster; every lowering method appends to one of
   these buckets. Highest re-threading effort; do it last when the pattern is well established.

## 4. Phase 2 (optional) — reduce the `*Ctx` bundles

Once `ModuleSections` and `NameGen` exist, the manual split-borrow literals
(`DelayFirCtx`, `DelayLoweringCtx`, `RecursionLoweringCtx`, `RecursionAllocCtx`) can be simplified:

- replace the hand-written struct literal with `let Self { store, sections, name_gen, delay, .. } =
  self;` destructuring at the call site, or move the method onto the owning sub-manager taking
  `&mut FirStore` + `&mut ModuleSections` explicitly;
- delete bundles that become redundant; for those that remain (inherently needing `store` + several
  sub-states), document them as inherent rather than accidental.

This phase is judgement-heavy and lower-priority; treat it as cleanup that follows naturally, not a
goal in itself.

## 5. Pilot mechanics — `BraState` (concrete first commit)

```rust
// bra.rs
#[derive(Default)]
pub(super) struct BraState {
    pub(super) scheduled: HashSet<SigId>,
    pub(super) grad_cache: HashMap<(SigId, usize), FirId>,
    pub(super) delay1_carry_vars: HashMap<SigId, String>,
    pub(super) delay_array_carry_vars: HashMap<SigId, (String, usize)>,
    pub(super) tape_store_var: HashMap<SigId, String>,
}
```

- `SignalToFirLower`: replace the five `bra_*` fields with `bra: BraState`.
- `setup.rs::new`: replace the five initializers with `bra: BraState::default()`.
- `bra.rs`: rename `self.bra_state_scheduled` → `self.bra.scheduled`, `self.bra_grad_cache` →
  `self.bra.grad_cache`, etc. (these fields are referenced only from `bra.rs`).
- Green gate, commit: `refactor(transform): extract BraState from SignalToFirLower`.

Expected diff: small, mechanical, −5 top-struct fields, no behavior change. The pilot's purpose is
to confirm the cost is as low as predicted before committing to clusters 2–7.

## 6. Expected outcome

- Top struct ≈ 14 fields; explicit state boundaries; a readable `new`.
- A few `*Ctx` bundles removed; the remaining ones documented as inherent.
- No behavior change; no public API change; every commit independently green and bisectable.
- The codebase is then materially easier to extend for the deferred lowering families (W8) and the
  larger performance/structure work (W1, I5).
