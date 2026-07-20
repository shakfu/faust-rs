# Scalar emission determinism plan (delay-line ordering)

**Date:** 2026-07-20

**Baseline:** `transform-cleanup` head (`4443272e`), i.e. the tree validated by
the R0–R9 cleanup battery. This plan is the first follow-up the cleanup plan
explicitly deferred ("Fixing the nondeterminism … belongs to a separate change
with its own validation", journal 2026-07-19).

**Status:** executed 2026-07-20 — D0–D4 complete (see the 2026-07-20 journal:
gate landed red→green, fix qualified 393/393 stable over 5 passes, allowlist
dissolved, 0 out-of-allowlist byte diffs vs the pre-fix baseline)

**Scope:** make scalar FIR emission byte-deterministic run-to-run by fixing the
hash-order-driven delay-line allocation and maintenance order, then dissolve
the `nondeterministic-frozen.txt` exclusion list so the byte-identity arbiter
covers the full emission corpus. **No intentional change to audio semantics,
signal preparation, scheduling, vector admission, or generated-code *content***
— only the run-to-run *stability* of declaration and statement order changes,
and, on the currently unstable cases only, the pick of one canonical order.

Related documents:

- [`transform-cleanup-documentation-factorization-plan-2026-07-19-en.md`](transform-cleanup-documentation-factorization-plan-2026-07-19-en.md)
  (R0.5 arbiter design; recorded this defect and deferred it)
- [`journal/2026-07-19.md`](journal/2026-07-19.md) (defect record, reproducer,
  suspected mechanism, frozen-list construction)
- [`HANDOFF.md`](HANDOFF.md) (arbiter environment; frozen worktree paths)
- [`delay-rs-simplification-experiment-2026-06-21-en.md`](delay-rs-simplification-experiment-2026-06-21-en.md)
  (current delay-subsystem layout)
- [`../AGENTS.md`](../AGENTS.md) (commit and journal discipline)

## 1. Executive decision

Fix the defect at its allocation source — the hash-ordered maps that drive
delay-line declaration and maintenance emission — by converting the delay
subsystem's order-observable collections to ordered collections
(`BTreeMap`/`BTreeSet` keyed by `(SigId, Option<u32>)`), and add a permanent,
repo-internal `cargo xtask emission-determinism` gate so run-to-run
byte-stability is enforced mechanically from now on, independent of the
external R0.5 arbiter worktree.

Why this is worth doing first (before any further semantic work): 77 of 396
emission cases are currently excluded from byte-identity refereeing. Every
future refactor is blind on those cases. Dissolving the exclusion list is a
direct strengthening of the strongest gate the project has, and the recorded
validation-gate traps (cached false greens, structural-only certification)
show why exclusions rot.

## 2. Recorded defect (measured at R0/R3 of the cleanup)

- **Symptom:** compiling the same DSP twice with the same release binary
  (`-lang cpp -double`) yields different bytes: DSP-struct `fVec*` field
  order and `lDelayN` loop-variable numbering differ between runs.
  Reproducer: `tests/impulse-tests/dsp/zita_rev1.dsp`.
- **Blast radius:** 77 of 396 emission cases frozen in
  `nondeterministic-frozen.txt` (47 scalar + the `-vec` emissions of fallback
  DSPs whose `-vec` output goes through the scalar path). **Zero certified-vec
  emissions affected**; the certified corpus × both `-lv` variants is fully
  byte-deterministic.
- **Invisibility:** the defect is invisible to the golden gate (corpus does
  not include these DSPs) and to the impulse gate (audio comparison, and the
  audio is unaffected). Only byte-level double emission sees it.

## 3. Root cause (verified in code at the baseline)

The chain is fully traced; every link below was read at this baseline.

1. **Planning:** `DelayPlan.lines: HashMap<(SigId, Option<u32>), i32>`
   (`crates/transform/src/signal_fir/delay/plan.rs:61`). The planner itself
   walks the signal forest deterministically, but stores its result in a
   hash map.
2. **Allocation order:** `prepare_delay_lines`
   (`crates/transform/src/signal_fir/module/setup.rs:221`) iterates that map
   directly: `for ((carried, clock_context), delay) in plan.lines`. Each
   iteration calls `ensure_delay_line_decl_in_context`, which
   - pushes the struct field declaration
     (`delay/manager.rs:163`, `ctx.struct_declarations.push(decl)`) — this
     fixes the **`fVec*`/`iVec*` struct field order**;
   - registers the clear loop (`delay/manager.rs:164`,
     `register_delay_clear`) — clear-loop emission order then drives
     `fresh_loop_var("lDelay")` (`delay/context.rs:122`), i.e. the
     **`lDelayN` numbering**.
3. **Maintenance order:** `DelayManager.delay_lines:
   HashMap<(SigId, Option<u32>), DelayLineInfo>` (`delay/manager.rs:50`) is
   iterated in emission-visible positions:
   - `lines()` (`delay/manager.rs:180`), consumed by the clocked-block pass
     (`module/clocked.rs:371`);
   - `emit_sample_end_updates` (`delay/manager.rs:248`, `.values()`) — order
     of emitted `IfWrapping` counter advances;
   - `global_circular_carriers` (`delay/manager.rs:200`) — today consumed
     only through `is_empty()` (`module/state.rs:311`), so order-insensitive,
     but it returns an ordered `Vec` and must not stay a latent trap.
4. **Neighbors to classify (expected keyed-lookup-only, must be verified in
   D2):** `DelayPlan.rec_outputs` → `rec_output_analysis`
   (looked up by key in `recursion.rs:618`), `scheduled_delay_writes:
   HashSet` (dedup guard), `DelayPlanner.best_seen_delay` / `scanned`
   (planner-internal).

`HashMap`'s per-instance `RandomState` randomizes iteration order per run;
any map above holding ≥ 2 entries flips order run-to-run. This matches the
observed blast radius exactly: delay-heavy DSPs are unstable, everything else
is stable.

## 4. Design decision: canonical key order, not insertion order

Convert the collections in §3.1–3.3 to `BTreeMap`/`BTreeSet`. The key
`(SigId, Option<u32>)` already totally orders lines, and the generated names
(`contextual_name(prefix, carried, clock_context)`) are derived from the same
key, so the canonical order is "sorted by carrier id, then clock occurrence"
— self-describing in the emitted code.

Alternative considered and rejected: an insertion-ordered structure
(`Vec<(key, value)>` or `indexmap`) preserving the planner's discovery order,
arguably closer to the C++ compiler's creation-order emission. Rejected
because (a) byte-alignment with C++ has never been a gate (impulse compares
audio, golden compares our own output), (b) it either adds a dependency or a
dual structure with a duplicate-key discipline `common/ids.rs` was written to
avoid, and (c) it produces no benefit the sorted order lacks.

**Why the already-deterministic 319 cases cannot change:** a case is
byte-stable today only if every randomized iteration it exercises holds ≤ 1
entry (with ≥ 2 entries, per-run `RandomState` would have flipped it).
Order-insensitive at size ≤ 1 means *any* stable order — including sorted —
emits identical bytes. The expected diff surface versus the frozen baseline
is therefore **exactly the 77 frozen cases, nothing else**; any 320th
differing case is a defect in this work.

## 5. Steps

### D0 — Re-anchor the arbiter

Re-run the three-pass identical-commit emission sweep at this plan's baseline
to refresh the frozen list (union rule, as in R0.5). This is the "before"
photograph; the list may have drifted by a case or two since R9.

### D1 — Land the mechanical gate first

New `cargo xtask emission-determinism`: for a fixed corpus (the
impulse-tests DSP directory, scalar `cpp` float/double plus the flag sets
represented in the frozen list; exact matrix calibrated in this step against
the 396-case arbiter corpus), emit each case twice in separate processes and
byte-compare. Deterministic, sorted, repo-relative findings (same reporting
discipline as `structure_check.rs`). Landed **before** the fix with an
explicit allowlist initialized to the D0 frozen list, so the gate is green on
day one and its red/green behavior is demonstrated on the reproducer. This
removes the dependency on the external baseline worktree for determinism
checking going forward.

### D2 — The fix

- Convert `DelayPlan.lines`, `DelayManager.delay_lines`, and
  `scheduled_delay_writes` to `BTreeMap`/`BTreeSet`; have
  `prepare_delay_lines` iterate the now-ordered plan.
- Classify every remaining hash collection in `signal_fir/delay/` and the
  `module/` consumers as *keyed-lookup-only* (fine) or *iterated into
  emission* (must be ordered); record the classification in the module docs
  where iteration order is semantic (one sentence on `delay_lines`: "iteration
  order is emission order; must stay canonical").
- Shrink the D1 allowlist to empty.
- If the empty-allowlist gate exposes further unstable sites outside the
  delay subsystem (e.g. a hash-ordered adjoint map on a BRA path), fix them
  inside this step with the same recipe; the gate, not a manual audit of all
  29 hash-using files, is the completeness argument.

### D3 — Qualification

All gates at once, on a clean tree:

1. `emission-determinism` with empty allowlist: 5 passes, 396/396 byte-stable.
2. Byte-identity versus the frozen baseline worktree: the 319 previously
   deterministic cases **identical**; diffs confined to the 77 (77-only rule,
   §4).
3. Diff triage on the 77: differences are exclusively struct-field order,
   clear-loop order, and `lDelayN`/cursor numbering — no computational
   statement changes.
4. Golden suite, impulse tests (baselines: cpp 92/93, c 87/93, interp 74/93),
   `vector-coverage-check` (WAC 173/197), certified corpus × both `-lv`
   variants byte-identical: all unchanged.
5. Dissolve `nondeterministic-frozen.txt`; update `HANDOFF.md` (defect record
   → fixed, arbiter now exclusion-free) and write the journal entry.

### D4 — Rejecting mutation

Per the phase methodology, prove the gate rejects the defect class:
temporarily reintroduce `HashMap` on `delay_lines` (or shuffle the iteration
with a per-run seed) and verify `emission-determinism` goes red on the
reproducer. Note: a *stable but different* order (e.g. `.rev()` on the
`BTreeMap`) is intentionally **not** caught by the determinism gate — that
mutation class belongs to the golden/byte-identity gates, which D3.2 already
exercises. Record both mutation results in the journal, then revert.

## 6. Non-goals

- No byte-alignment with the C++ compiler's emission order.
- No "improvement" of emission shape beyond stabilization.
- No changes to the vector pipeline's ordering contracts (already
  deterministic and certified; `zero certified-vec cases affected`).
- No general-purpose ban on `HashMap` in `transform` — keyed-lookup-only maps
  are untouched; the gate, not a lint, owns the invariant.

## 7. Risks

- **Hidden second source:** the suspected mechanism explains both symptoms,
  but D2 assumes nothing — the empty-allowlist gate is the arbiter of
  completeness, and D3.1's five passes bound the residual risk of a
  low-probability flip.
- **Baseline drift:** the frozen worktree predates this branch; D0 re-anchors
  the list so D3.2's 77-only rule compares like with like.
- **Environment coupling:** the R0.5 arbiter script and worktree live outside
  the repository. D1 deliberately internalizes the determinism check;
  the external worktree is only needed once more, for D3.2, and can be
  removed afterward (`git worktree remove`, as HANDOFF already plans).

## 8. Deliverables

1. Delay-subsystem ordering fix (one commit, D2).
2. `cargo xtask emission-determinism` gate + corpus manifest (D1).
3. Empty exclusion list; updated `HANDOFF.md`.
4. Journal entry (English, per journal discipline) recording D0 list, D3 gate
   results, and both D4 mutation outcomes.
