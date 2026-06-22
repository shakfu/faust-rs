# Experiment: making `signal_prepare.rs` simpler to read without losing rigor or speed

**Date:** 2026-06-22
**Scope:** `crates/transform/src/signal_prepare.rs` (1475 lines) + its module `signal_prepare/tests.rs`,
and the consumers in `crates/transform/src/signal_fir/`.
**Status:** Implemented on branch `main-dev` (2026-06-22). All three proposals landed
behavior-preserving (103 `transform` tests green throughout, doc broken-link gate clean):
B (verifier → `signal_prepare/verify.rs`), C (rewrites → `signal_prepare/rewrites.rs`), and A
(the typed `Staging` driver replacing the five hand-threaded `sig_types_*` snapshots) — see the
2026-06-22 journal entry. `signal_prepare.rs` is now `signal_prepare/mod.rs`; the `file:line`
references below point at the **pre-refactor single-file `signal_prepare.rs`** and are kept as the
design's "before" snapshot.
**Goal:** Restructure the staging phase into steps a human can read independently, document in
isolation, and recombine — while producing a **structurally identical prepared forest** (same
arena, same output roots, same type maps), hence identical downstream FIR and identical
behavior/performance.
**Companion docs:** [`signal-to-fir-transform-analysis-2026-06-20-en.md`](signal-to-fir-transform-analysis-2026-06-20-en.md)
(its §3 / Stage-2 table enumerates the very steps analysed here),
[`signal-to-fir-rewriting-calculus-2026-06-20-en.md`](signal-to-fir-rewriting-calculus-2026-06-20-en.md),
and the sibling experiment [`delay-rs-simplification-experiment-2026-06-21-en.md`](delay-rs-simplification-experiment-2026-06-21-en.md)
(same method, applied to `signal_fir/delay`).

---

## 0. Where this code sits in the compilation chain

### 0.1 The pipeline around `signal_prepare`

```
boxes ──► propagate ──► signals (+ UiProgram)
                            │
                            ▼
        ┌───────────────────────────────────────────────┐
        │ crates/transform  (mid-level lowering)         │
        │                                                │
        │   signal_prepare ──► signal_fir ──► FIR        │
        │     (STAGING)        (lowering)                │
        └───────────────────────────────────────────────┘
                            │
                            ▼
              fir ──► codegen (C / C++ / WASM / Cranelift / FBC)
```

`signal_prepare` is **stage 2** of `transform`: the bridge between *propagation* (which emits a
signal forest that may still contain de-Bruijn recursion, mixed int/real arithmetic, and
non-canonical delay forms) and *`signal_fir`* (the fast-lane FIR lowerer, which assumes a much
tighter shape). Its single job: take the propagated forest and hand `signal_fir` a **staged forest
that satisfies the fast-lane contract**, computed in a *private* arena so the caller's source arena
is never mutated.

The public entry is `prepare_signals_for_fir_verified(src_arena, outputs, ui)`
([`signal_prepare.rs:374`](../crates/transform/src/signal_prepare.rs)), called from
[`signal_fir/mod.rs:214`](../crates/transform/src/signal_fir/mod.rs) just before
`module::build_module`. A second entry `prepare_signals_for_fir` (unwrapped) is used by
`signal_fir/siggen.rs`. Both delegate to the private driver `prepare_signals_for_fir_unverified`.

### 0.2 The boundary contract (what it guarantees)

| | Input (from propagation) | Output (to `signal_fir`) |
|---|---|---|
| Recursion | de-Bruijn recursion groups | symbolic `SYMREC` / `SYMREF` |
| Numerics | mixed int/real, no casts | `SignalPromotion` casts inserted where the fast-lane needs them |
| Delays | `Delay(x, 1)` and `Delay1(x)` both | one canonical `Delay1(x)` form |
| Types | none attached | one reduced `SimpleSigType` (`Int`/`Real`/`Sound`) per reachable node, plus the full `sig_types` map |
| Source arena | — | left untouched (work happens in a private clone) |

The result is a `PreparedSignals { arena, outputs, types, sig_types }`
([`signal_prepare.rs:104`](../crates/transform/src/signal_prepare.rs)); `VerifiedPreparedSignals`
wraps it once `PreparedSignals::verify` has certified the postconditions. `signal_fir` consumes the
`types`/`sig_types` maps to drive delay/recursion/table type selection.

### 0.3 The producing pipeline (the heart)

`prepare_signals_for_fir_unverified` ([`signal_prepare.rs:384`](../crates/transform/src/signal_prepare.rs))
is a **fixed linear sequence of ~16 full-forest traversals** (numbering follows the Stage-2 table in
`signal-to-fir-transform-analysis-2026-06-20-en.md` §3):

```
2.1  clone forest into a fresh private TreeArena         (rebuild)
2.2  de_bruijn_to_sym                                    (rewrite: de Bruijn → SYMREC/SYMREF)
2.3  canonicalize_unary_rec_projections                  (rewrite)
     └─ debug_assert: no de Bruijn remains  (W4)
2.4  infer_full_types  #1  → sig_types_before            (analysis)
2.5  promote_signals_fastlane  #1  (insert casts)        (rewrite, needs types)
2.6  infer_full_types  #2  → sig_types_after_promotion   (analysis)
2.7  simplify_signals_fastlane  #1                       (rewrite, needs types)
2.8  merge_isomorphic_symrec_groups                      (rewrite)
2.9  infer_full_types  #3  → sig_types_after_merge       (analysis)
2.10 simplify_signals_fastlane  #2                       (rewrite, needs types)
2.11 canonicalize_one_sample_delays  (Delay(x,1)→Delay1) (rewrite)
     └─ debug_assert: no Delay(_,1) remains  (W4)
2.12 infer_full_types  #4  → sig_types_after_canonicalize(analysis)
2.13 promote_signals_fastlane  #2                        (rewrite, needs types)
     └─ debug_assert: Sym + D1 preserved  (W4)
2.14 infer_full_types  #5  → sig_types  (final)          (analysis)
2.15 derive_simple_types  (SigType → SimpleSigType)      (projection)
     → PreparedSignals { arena, outputs, types, sig_types }
2.16 verify  (postconditions; in the *_verified entry)   (analysis)
```

Five of these sixteen traversals are **full-forest type inference** (`infer_full_types`, steps
2.4/2.6/2.9/2.12/2.14), each producing a differently-named `sig_types_*` snapshot that the next
typed rewrite consumes. That re-typing is the dominant cost of the stage and, as §2 argues, the
dominant *comprehension* cost too.

---

## 1. What the file contains today

One 1475-line module packs **five distinct concern clusters**:

| # | Cluster | Lines (approx.) | Kind |
|---|---------|-----------------|------|
| 1 | Public types + accessors (`SimpleSigType`, `PreparedSignals`, `VerifiedPreparedSignals`, `SignalPrepareError`) | 89–355 | data + API |
| 2 | The **pipeline** (`prepare_signals_for_fir*` + the private driver) | 361–443 | orchestration |
| 3 | Debug-contract scan (`forest_any_node` + `forest_has_*` predicates) | 445–508 | W4 assertions |
| 4 | The **verification boundary** (`verify_prepared_signal`, `verify_control_exists`, `verify_promotion_invariant`) | 510–1200 | postcondition checker |
| 5 | The **canonicalization rewrites** (`canonicalize_unary_rec_projections` + helpers, `canonicalize_one_sample_delays` + helpers) and the **typing glue** (`infer_full_types`, `derive_simple_types`) | 1203–1475 | rewrites + analysis |

Cluster 4 alone (~700 lines) is the largest, and is a *different job* from the pipeline that
produces the forest: it **checks** the result rather than building it.

---

## 2. Why it is hard to read today (the concrete friction)

**F1 — Five hand-threaded type snapshots.** The driver names and passes five `sig_types_*` values
(`sig_types_before`, `sig_types_after_promotion`, `sig_types_after_merge`,
`sig_types_after_canonicalize`, final `sig_types`) by hand
([`signal_prepare.rs:404-435`](../crates/transform/src/signal_prepare.rs)). A typed rewrite
(`promote`, `simplify`) takes "the current snapshot," but *which* snapshot is correct at each line is
enforced only by reading order. Passing a stale snapshot to a pass would still compile and would
mis-type silently.

**F2 — The "re-type after a structural change" rule is convention, not structure.** Every rewrite
that changes the forest is followed by a fresh `infer_full_types`, but nothing in the code expresses
"this pass invalidates the types." Insert a new rewrite between 2.8 and 2.10 and forget to re-type,
and the following `simplify` consumes stale types — a latent correctness cliff that no signature
prevents. The five re-types are also the stage's dominant runtime cost (the transform analysis flags
the repeated typing in its Stage-2 table: 5 of ~16 traversals).

**F3 — The W4 inter-pass contracts are inline clutter.** Three `debug_assert!` blocks with
multi-line justification comments ([`signal_prepare.rs:396-403, 414-418, 422-434`](../crates/transform/src/signal_prepare.rs))
sit *inside* the pipeline body. They are valuable (they localize a regression to the pass that broke
an invariant), but interleaving them with the pass calls triples the line count of the pipeline and
hides its actual shape.

**F4 — Producer and checker share one file.** The ~700-line verification boundary (cluster 4) is a
self-contained concern — "given a built forest, certify the fast-lane postconditions" — yet it lives
beside the pipeline that builds the forest. A reader after the *producer* must scroll past the
*checker*, and vice-versa; the two evolve for different reasons.

**F5 — No uniform pass contract.** Each pass has an ad-hoc signature:
`de_bruijn_to_sym(&mut arena, list)`, `canonicalize_unary_rec_projections(&mut arena, list)`,
`promote_signals_fastlane(&mut arena, &sig_types, &outputs)`,
`simplify_signals_fastlane(&mut arena, &sig_types, &outputs)`,
`merge_isomorphic_symrec_groups(&mut arena, &outputs)`. Some take a list, some a vec; some need
types, some don't; some return `Result`, some don't. There is no single shape that says "a staging
pass is X," so the sequence cannot be read as a uniform list and the list/vec conversions
(`vec_to_list`/`list_to_vec`) leak into the body.

None of these is a bug. They are comprehension taxes paid on every read of the stage.

---

## 3. Three independent ways to restructure

Three orthogonal axes — adoptable one at a time or stacked:

- **A — the pipeline** — make the pass sequence + the type schedule *explicit and uniform*.
- **B — split the checker** — move the verification boundary into its own module (producer vs.
  checker).
- **C — extract the rewrites** — lift the canonicalization passes into standalone, individually
  testable units.

### Proposal A — A typed staging pipeline (the sequence as data)

**Idea.** Replace the hand-threaded driver with a small mutable `Staging` state and a uniform pass
contract, so the body reads as a *list of named passes* and the "re-type after a structural change"
rule (F2) becomes the driver's job, not the reader's.

```rust
/// The evolving staging value: one private arena, the current roots, and the
/// current full-type snapshot (lazily refreshed by the driver).
struct Staging {
    arena: TreeArena,
    outputs: Vec<SigId>,
    sig_types: HashMap<SigId, SigType>,   // always "fresh for `outputs`"
}

/// A staging pass declares what it needs and what it invalidates.
enum Pass {
    /// Pure structural rewrite (no types needed); invalidates the type snapshot.
    Structural { name: &'static str, run: fn(&mut TreeArena, &[SigId]) -> Result<Vec<SigId>, SignalPrepareError> },
    /// Typed rewrite (consumes the fresh snapshot); invalidates it.
    Typed      { name: &'static str, run: fn(&mut TreeArena, &HashMap<SigId, SigType>, &[SigId]) -> Result<Vec<SigId>, SignalPrepareError> },
    /// Debug-only inter-pass contract (the W4 checks), e.g. "no de Bruijn remains".
    Contract   { name: &'static str, holds: fn(&TreeArena, &[SigId]) -> bool },
}
```

The pipeline becomes a declaration:

```rust
const PIPELINE: &[Pass] = &[
    Structural { "de_bruijn_to_sym",            de_bruijn_to_sym_vec },
    Structural { "canon_unary_rec_projections", canonicalize_unary_rec_projections },
    Contract   { "Sym established",             |a, o| !forest_has_de_bruijn(a, o) },
    Typed      { "promote #1",                  promote_signals_fastlane },
    Typed      { "simplify #1",                 simplify_signals_fastlane },
    Structural { "merge_isomorphic_symrec",     merge_isomorphic_symrec_groups },
    Typed      { "simplify #2",                 simplify_signals_fastlane },
    Structural { "canon_one_sample_delays",     canonicalize_one_sample_delays },
    Contract   { "D1 established",              |a, o| !forest_has_delay_of_one(a, o) },
    Typed      { "promote #2",                  promote_signals_fastlane },
    Contract   { "Sym + D1 preserved",          |a, o| !forest_has_de_bruijn(a, o) && !forest_has_delay_of_one(a, o) },
];
```

and the driver owns the schedule:

```rust
fn run(mut s: Staging) -> Result<Staging, SignalPrepareError> {
    let mut types_fresh = false;
    for pass in PIPELINE {
        match pass {
            Typed { run, .. }      => { if !types_fresh { s.retype()?; }      s.outputs = run(&mut s.arena, &s.sig_types, &s.outputs)?; types_fresh = false; }
            Structural { run, .. } => { s.outputs = run(&mut s.arena, &s.outputs)?; types_fresh = false; }
            Contract { holds, .. } => { debug_assert!(holds(&s.arena, &s.outputs), "..."); }
        }
    }
    s.retype()?;          // final sig_types for PreparedSignals
    Ok(s)
}
```

**What gets simpler.**
- *To read:* the pipeline is a flat list of named passes — the "what" is visible at a glance, free
  of `sig_types_*` threading and `vec_to_list`/`list_to_vec` plumbing (F1, F5).
- *To document:* one place states the invariant "the driver guarantees `sig_types` is fresh before
  every `Typed` pass and re-infers after any structural change" (F2). The W4 contracts become first
  class `Contract` entries in the list, self-documenting where they hold (F3).
- *To extend:* a new pass is one line in the list with the right kind; the driver does the rest —
  including not forgetting to re-type.

**Composability / interaction surface.** Passes interact only through `Staging` and the `Pass`
contract; the driver is the single owner of the type schedule. Three pass kinds, one driver loop.

**Behavior/perf note.** The driver must reproduce the **exact** current schedule. The current code
re-types *eagerly* after every structural change (5 infers); the `types_fresh` flag above reproduces
that as written **as long as a `Typed` pass follows** — but note that lazy re-typing could in
principle *reduce* the infer count (e.g. two consecutive structural passes need only one re-type).
That would be a performance *improvement* but a behavior risk (a `simplify` keyed on a particular
snapshot), so for a strictly behavior-preserving port the driver should re-type at exactly the
2.4/2.6/2.9/2.12/2.14 points. Making the schedule explicit is what later *enables* safely auditing
those five re-types (see §4.5).

**Independence.** Independent of B and C. Touches only cluster 2 (the driver) and the pass
signatures' adapters.

### Proposal B — Split the verification boundary into its own module

**Idea.** Move cluster 4 (the ~700-line postcondition checker) out of the producer file into
`signal_prepare/verify.rs`, leaving `signal_prepare/mod.rs` with the pipeline + the
`PreparedSignals` data + the public API.

```
signal_prepare/
  mod.rs       // SimpleSigType, PreparedSignals, VerifiedPreparedSignals, errors,
               //   prepare_signals_for_fir{,_verified}, the pipeline driver, re-exports
  verify.rs    // PreparedSignals::verify, verify_prepared_signal, verify_control_exists,
               //   verify_promotion_invariant, the forest_* debug-contract scan
  rewrites.rs  // (Proposal C)
  tests.rs     // (unchanged)
```

`PreparedSignals::verify` can stay a method whose body lives in `verify.rs` (an `impl PreparedSignals`
block in that file). The `forest_any_node` scan + `forest_has_*` predicates move with it (they back
both the W4 contracts and the verifier).

**What gets simpler.**
- *To read:* the producer file shrinks to the pipeline + data (~600 lines); the checker is one file
  you open when you care about *what "prepared" means*, not how it's built (F4).
- *To document:* the boundary contract (the §0.2 table) is documented once, next to the code that
  enforces it.
- *To trust:* the verifier — the part most worth auditing — is isolated and reviewable on its own.

**Composability / interaction surface.** One direction: `mod.rs` calls `verify`. `verify.rs` depends
only on `PreparedSignals` (read-only) + the arena. No back-edge.

**Cost.** Pure code move + Rust visibility wiring (re-exports), exactly like the `signal_fir/delay`
split already done on this branch. Low risk; the gate is "compiles + tests green."

**Independence.** Fully independent of A and C.

### Proposal C — Extract the canonicalization rewrites as standalone passes

**Idea.** Lift the two structural rewrites out of the producer file into `signal_prepare/rewrites.rs`,
each a cohesive, separately-documented, separately-tested unit:

- `canonicalize_unary_rec_projections` + `collect_unary_sym_groups` + `rewrite_unary_rec_projections`
  ([`signal_prepare.rs:1203, 1239, 1294`](../crates/transform/src/signal_prepare.rs)) — collapse a
  logical projection index onto slot 0 for single-slot symbolic recursion groups.
- `canonicalize_one_sample_delays` + `rewrite_one_sample_delays`
  ([`signal_prepare.rs:1220, 1368`](../crates/transform/src/signal_prepare.rs)) — rewrite
  `Delay(x, 1)` to `Delay1(x)`.

Each is a pure `(&mut arena, &outputs) -> outputs` rewrite with a precise spec and an obvious
unit-test surface ("feed a `Delay(x,1)`, assert a `Delay1(x)` comes out"). Today they are ~230 lines
of tree-walking interleaved with the verifier and the typing glue.

**What gets simpler.**
- *To read:* one file holds "the structural canonicalizations the stage applies," each with its own
  `//!`/`///` spec and its C++-provenance note (the module header already disclaims that these are
  *not* 1:1 ports of `inlineDegenerateRecursions`; that caveat belongs next to the code).
- *To test:* each rewrite gets focused unit tests independent of the full pipeline.
- *To compose:* they become two `Structural` entries in Proposal A's `PIPELINE` list, with nothing
  else to know about them.

**Composability / interaction surface.** None beyond the `(&mut arena, &outputs) -> outputs`
signature. Leaf rewrites; no shared state.

**Cost.** Pure move + tests. Low risk.

**Independence.** Independent; composes naturally under A and alongside B.

### 3.x Side-by-side

| Axis | A — typed pipeline | B — split checker | C — extract rewrites |
|------|--------------------|-------------------|----------------------|
| Friction removed | F1, F2, F3, F5 | F4 | F4 (rewrites half) |
| Slicing | the orchestration | horizontal (concern) | leaf units |
| New artifact | `Staging` + `Pass` + driver | `verify.rs` | `rewrites.rs` |
| Test style unlocked | assert pipeline output vs. old | verifier unit tests | per-rewrite unit tests |
| Risk | medium (schedule fidelity) | low (pure move) | low (pure move) |
| Lines moved/changed | ~80 reworked | ~700 moved | ~230 moved |

**Recommended order** (each ships green on its own): **B → C → A.** B and C are pure moves that
shrink the 1475-line file to the pipeline + small types (mirroring the `signal_fir/delay` split that
already landed on this branch — same playbook, same low risk). With the file down to the pipeline, A
— the one conceptual change — is small and easy to review. If only one is done, do **B** (largest,
cleanest reduction). If two, **B + C** (the file becomes producer-only).

---

## 4. Migrating safely and testably

**The invariant.** "Same prepared output" = same staged forest structure (the `outputs` roots and
the nodes they reach), same `types` (`SimpleSigType`) map, and same `sig_types` map ⇒ identical
`signal_fir` input ⇒ identical FIR ⇒ identical behavior and performance.

### 4.1 The safety net that already exists

- **32 `#[test]` functions** in `signal_prepare/tests.rs` (prepare/verify/promotion/canonicalization
  cases), plus the **103 `transform` tests** and the **impulse-tests oracle** (cpp 92/93,
  end-to-end) which exercises the full propagate→prepare→FIR→run path.
- **The verifier is itself a built-in postcondition checker:** any restructuring that changes the
  prepared forest in a contract-visible way trips `PreparedSignals::verify`. Keep
  `prepare_signals_for_fir` (the verifying entry) in the test path.

### 4.2 The golden anchor

A `PreparedSignals` owns an arena, so it is not directly `==`-comparable. Two practical proxies:

1. **FIR-output snapshot** (preferred): for a corpus covering recursion / promotion / delay /
   table / mixed-numeric cases, dump the emitted FIR before any change and diff after. Identical FIR
   ⇒ identical prepared input on every tested program.
2. **Structural prepared dump**: a stable textual dump of `outputs` + a sorted `(node → SimpleSigType)`
   listing, snapshotted per corpus DSP. Cheaper to read in a diff than full FIR, and pinpoints
   exactly where preparation diverged.

### 4.3 Generic principles

- **Keep the public API stable** (`prepare_signals_for_fir`, `prepare_signals_for_fir_verified`,
  `PreparedSignals`, `VerifiedPreparedSignals`, `SimpleSigType`, `SignalPrepareError`) so
  `signal_fir/mod.rs` and `siggen.rs` don't churn while internals move.
- **One structural change per commit**; each compiles and passes `cargo test -p transform` + the
  impulse oracle.
- **Differential testing during a switch** (for A): run the old driver and the new pass-list driver
  side by side, assert the FIR (or the structural dump) matches across the corpus before deleting the
  old driver.

### 4.4 Per-proposal recipe

**B (split checker) — land first.** Create `signal_prepare/verify.rs`; move `verify_*` + the
`forest_*` scan; wire `impl PreparedSignals { pub fn verify … }` from the new file; re-export what
the public API needs. Edit only `signal_prepare/`. Gate: compiles + 32+103 tests green. (This is the
exact playbook used for the `signal_fir/delay` split — see the 2026-06-21 journal.)

**C (extract rewrites) — land second.** Create `signal_prepare/rewrites.rs`; move the two
canonicalization passes + their helpers verbatim; add focused unit tests for each. Gate: tests green.

**A (typed pipeline) — land last.** Introduce `Staging` + `Pass` + the driver *alongside* the
existing body; reproduce the exact 2.4–2.14 re-type schedule. Differential-check (old vs. new driver)
across the corpus; only then replace the old body and fold the W4 asserts into `Contract` entries.

### 4.5 Why performance is preserved (and what it later enables)

- **B and C are pure moves** — byte-identical code in new files; zero runtime effect.
- **A reproduces the exact pass + re-type schedule** — same 5 `infer_full_types`, same 2 promotions,
  same 2 simplifies, same merge — so the traversal count, and thus the cost, is unchanged. Static
  dispatch (`enum Pass` + `match`), no `dyn`.
- **Enabler, not a change:** once the five re-types are explicit `Typed`-pass boundaries, it becomes
  *auditable* whether any is redundant (e.g. a structural pass that provably preserves types could
  skip the following infer). That optimization is **out of scope** here — it changes the traversal
  count and must be proven type-preserving — but the typed pipeline is what makes it safe to attempt
  later, behind the same differential gate.

---

## 5. Recommendation

`signal_prepare.rs` is correct and well-commented; this is a *legibility* experiment. The
highest value-per-risk path is **B → C → A**:

1. **B** removes the largest, most separable chunk (the ~700-line verifier) at pure-move risk,
   leaving a producer-only file.
2. **C** lifts the two canonicalization rewrites into testable units.
3. **A** then turns the hand-threaded, five-snapshot pipeline into a flat list of named passes over a
   `Staging` value, making the type schedule structural instead of conventional.

Stop after any step and the file is strictly clearer than today, with the 32 preparation tests, the
transform suite, and the impulse oracle proving behavior — and the FIR/structural snapshot proving
performance — unchanged.
