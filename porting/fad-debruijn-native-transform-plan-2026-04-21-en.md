# FAD on De Bruijn Recursion — Native Transform Port Plan

> **SUPERSEDED (2026-04-21):** The DeBruijn-native dispatch with lane
> expansion (Step 2) was not needed. Removing the `de_bruijn_to_sym_many`
> pre-conversion from `generate_fad_signals_multi` was sufficient because
> `sig == self.diff_seed` short-circuits before the transform ever descends
> into a `DEBRUIJNREC` body. Steps 1, 3–6 were carried out as described.
> See `porting/journal/2026-04-21.md` ("Implement Plan B" entry)
> and commits `73c04ab` / `a25682e`.

**Date:** 2026-04-21
**Scope:** Remove the DeBruijn→Sym conversion from `generate_fad_signals_multi`
so that the forward-mode AD transform operates directly on the raw de Bruijn
signal graph produced by `propagate`. Defer the single symbolic-recursion
conversion to `signal_prepare`, where it already runs on the full process
output list through one shared `Converter` instance.

**Motivation:**
See `porting/journal/2026-04-21.md` for the concrete reproduction
(`fad_pendulum_cello1.dsp`, FAD 2nd-order branch). The immediate bug is a
nested `fad(fad(eq, phi), phi)` call that emits a phantom second recursion
slot and a stuck-at-zero integer recursion that silently zeroes the audio.

The first-level FAD converts the phasor `DEBRUIJNREC` into `SYMREC(W0, …)`
and returns the tangent `vel` in that symbolic form. The second-level FAD
receives `vel` (already SYMREC) as its body and the original `DEBRUIJNREC`
phasor as its seed (still in de Bruijn form, as `propagate` never re-converts
slot values). The `de_bruijn_to_sym_many` call in `generate_fad_signals_multi`
then walks `vel` through `match_sym_rec` (keeping `W0`) and the seed through
`match_de_bruijn_rec` (which calls `fresh_var()`, finds `W0` interned, and
bumps to `W1`). The seed ends up as `SYMREC(W1, …)` while every `phi`
reference inside the body is `SYMREC(W0, …)`: two structurally identical but
`TreeId`-distinct nodes. The structural-equality `sig == self.diff_seed` check
then never fires inside the body, so the outer FAD differentiates the inner
`frac` accumulator instead of `phi`.

The previous single-level fix (`de_bruijn_to_sym_many`, 2026-04-21) repaired
the single-level case by sharing the `Converter` memo across outputs and seeds.
It cannot repair the nested case: the body already contains SYMREC nodes from
the inner FAD, which are opaque to the converter's `DEBRUIJNREC→SYMREC`
mapping.

**Reference documents:**
- `porting/journal/2026-04-21.md` — the initial single-level fix and the
  diagnosis of the nested case.
- `porting/autodiff-forward-ad-port-plan-2026-04-13-en.md` — original FAD
  port plan (symbolic-recursion era).
- `porting/fad-explicit-diff-variable-plan-2026-04-15-en.md` — seed-box
  generalisation.

**C++ source (parity anchors):**
- `compiler/transform/forwardADSignalTransform.{hh,cpp}` — the canonical
  transform operates on symbolic recursions, but the C++ compiler pins the
  conversion to a single pipeline stage that runs before FAD. The actual fix
  moves faust-rs to the same discipline: one conversion, at `signal_prepare`,
  on a tree guaranteed to be entirely in de Bruijn form.
- `compiler/signals/signals.cpp` — `simplification` / `deBruijn2Sym`
  sequencing.

---

## Problem restated

```
process = phi_gen : kin
kin(phi) = eq, vel with {
    eq  = sin(phi) + 0.3 * cos(2.5 * phi);
    vel = fad(eq, phi) : !, _;
};
```

adds a second level:

```
acc = fad(vel, phi) : !, _;
```

At the propagate layer (before the fix):

1. `fad(eq, phi)` runs `generate_fad_signals_multi`:
   - converts `eq` and `phi` together via `de_bruijn_to_sym_many`;
   - runs `ForwardADTransform` on SYMREC-form signals;
   - returns `[primal, tangent]` in SYMREC form (`SYMREC(W0, …)`).
2. `fad(vel, phi)` is reached; `vel` is the SYMREC tangent from step 1:
   - `generate_fad_signals_multi` calls `de_bruijn_to_sym_many([vel, phi])`;
   - `vel` is already SYMREC: converter walks it via `match_sym_rec` (keeps `W0`);
   - `phi` is still DEBRUIJNREC: converter calls `fresh_var()`, sees `W0`
     interned, bumps to `W1`;
   - seed ends up at `SYMREC(W1, …)`, body stays at `SYMREC(W0, …)`:
     two structurally identical but `TreeId`-distinct nodes.

No naming discipline on top of this architecture can paper it over: the
converter has no structural equivalence test between `DEBRUIJNREC` and
`SYMREC`.

---

## Actual fix (Plan B as implemented)

The fix is a one-line removal. `generate_fad_signals_multi` no longer calls
`de_bruijn_to_sym_many` (or any conversion). It passes `outputs` and `seeds`
directly to `ForwardADTransform` in their original de Bruijn form.

This is sufficient because:

- Seed recognition is `sig == self.diff_seed` by `SigId`. Because the
  TreeArena hash-conses every node, all external references to `phi` share
  the same `SigId`, regardless of recursion depth.
- The transform short-circuits at the seed leaf and never descends into the
  `DEBRUIJNREC` body of `phi`. The binder-expansion machinery described in
  Step 2 of the original plan is therefore unnecessary.
- `signal_prepare` already calls `tlib::de_bruijn_to_sym(&mut arena, cloned_list)`
  on the full process output cons-list, using one shared `Converter` instance.
  Every occurrence of the same `DEBRUIJNREC` sub-term maps to exactly one
  fresh `Wn` name, whether reached from a primal lane or a tangent lane.

For nested `fad(fad(eq, phi), phi)`:
1. Inner `fad(eq, phi)`: seed = `phi` (DeBruijn SigId). Transform visits
   `eq`, reaches `phi` leaf → tangent = 1. Output is DeBruijn.
2. Outer `fad(vel, phi)`: seed = same `phi` SigId. Body = `vel` (DeBruijn
   expressions containing `phi`). Transform visits `vel`, reaches `phi` →
   tangent = 1 again. Second derivative computed correctly. Output is DeBruijn.
3. `signal_prepare` converts the whole output list (`pos, vel, acc`) in one
   shared pass, producing one `SYMREC(W0)` for `phi` everywhere.

---

## What was actually done vs. the original plan

| Step | Original plan | Actual outcome |
|------|--------------|----------------|
| 1 — Audit DeBruijn-only path | Confirm no SYMREC before `signal_prepare` | Confirmed; existing structure already guarantees this |
| 2 — DeBruijn-native FAD dispatch | Add `DEBRUIJNREC`/`DEBRUIJNREF` arms with lane expansion | **Not needed.** Seed check fires before any binder descent |
| 3 — Move conversion to `signal_prepare` | Add `de_bruijn_to_sym_many` call | **Already present** at line 391; no change required |
| 4 — Retire in-FAD converter call | Delete `de_bruijn_to_sym_many` from `generate_fad_signals_multi` | Done (commit `73c04ab`) |
| 5 — Regression tests | Two new corpus tests | Done: `fad_nested_on_recursive_seed.dsp` + `signal_pipeline.rs` test |
| 6 — Documentation | Journal entry + supported-subset update | Done: journal entry in `2026-04-21.md` |

---

## Changes made

**`crates/propagate/src/forward_ad.rs`** (commit `73c04ab`, corrected `a25682e`):
- `generate_fad_signals_multi`: removed the `de_bruijn_to_sym_many` conversion
  (the outputs + seeds bundling / splitting). The function now drives
  `ForwardADTransform` directly on de Bruijn signals.
- Removed the `de_bruijn_to_sym_many` import from the `tlib` use block.
- Module docstring and function docstring updated to describe the actual
  contract: no pre-conversion; `signal_prepare` is the single conversion point.
- `SYMREC`/`SYMREF` arms in `transform_uncached` retained as defensive code;
  they are unreachable in the normal pipeline but guard against callers that
  pass already-converted signals. Note: they produce a zero tangent for
  mixed-form trees (DeBruijn seed inside a symbolic body) — they do not
  correctly handle that case.

**`signal_prepare.rs`** — no change. Line 391 already performs the correct
single-pass conversion.

**`tests/corpus/fad_nested_on_recursive_seed.dsp`** (commit `73c04ab`):
self-contained reproduction (no stdfaust) of the nested-FAD pattern using a
linear accumulator for `phi`. Compiles to 3 outputs with a single `fRec`.

**`crates/compiler/tests/signal_pipeline.rs`** (commit `73c04ab`):
`corpus_fad_nested_on_recursive_seed_emits_three_lanes_one_recursion` asserts
`process_arity = 0 → 3` with no UI controls.

---

## Exit criteria (all met)

- `fad_pendulum_cello1.dsp` produces audible output: single `fRec` for
  `phi_gen`, noise trigger driven by the correct second-derivative tangent. ✓
- `generate_fad_signals_multi` contains no reference to `de_bruijn_to_sym*`. ✓
- `signal_prepare` has exactly one `de_bruijn_to_sym` call (pre-existing). ✓
- `cargo test --workspace` is green, including the two new regression tests. ✓
- Journal entry in `porting/journal/2026-04-21.md`. ✓
