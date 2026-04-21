# FAD on De Bruijn Recursion — Native Transform Port Plan

**Date:** 2026-04-21
**Scope:** Remove the DeBruijn→Sym conversion from the forward-mode AD
transform path. Teach `ForwardADTransform` to operate directly on
`DEBRUIJNREC` / `DEBRUIJNREF` nodes. Defer the single symbolic-recursion
conversion to the existing signal-preparation pipeline, where every other
consumer already applies it once at the process boundary.

**Motivation:**
See `porting/journal/2026-04-21.md` for the concrete reproduction
(`fad_pendulum_cello1.dsp`, FAD 2nd-order branch). The immediate bug is a
nested `fad(fad(eq, phi), phi)` call that emits a phantom second recursion
slot and a stuck-at-zero integer recursion that silently zeroes the
audio. The first-level FAD already rewrote the phasor into
`SYMREC(W0, …)`; the second-level FAD receives that rewritten body but
looks up `phi` out of `slot_env` in its original `DEBRUIJNREC` form. The
converter in `generate_fad_signals_multi` sees both roots, walks the
SYMREC through `match_sym_rec` (keeping `W0`), and walks the DEBRUIJNREC
through `match_de_bruijn_rec` which calls `fresh_var()`. `fresh_var()`
observes `W0` already interned and returns `W1`, so the seed ends up as
`SYMREC(W1, …)` while the body still references `SYMREC(W0, …)`. The
structural-equality `sig == self.diff_seed` check in the FAD transform
then misses the seed inside the body and differentiates the inner `frac`
accumulator instead of `phi`, leaking chain-rule factors (`2π`, `1.875·2π`)
and forking the recursion into two distinct slots at codegen time.

The previous fix (`de_bruijn_to_sym_many`, 2026-04-21) repaired the
single-level case by sharing the `Converter` memo across outputs and
seeds. It cannot repair the nested case: the outputs already contain
SYMREC nodes from the inner FAD, which are opaque to the converter's
`DEBRUIJNREC→SYMREC` mapping.

**Reference documents:**
- `porting/journal/2026-04-21.md` — the initial single-level fix and the
  diagnosis of the nested case.
- `porting/autodiff-forward-ad-port-plan-2026-04-13-en.md` — original FAD
  port plan (symbolic-recursion era).
- `porting/fad-explicit-diff-variable-plan-2026-04-15-en.md` — seed-box
  generalisation.

**C++ source (parity anchors):**
- `compiler/transform/forwardADSignalTransform.{hh,cpp}` — the canonical
  transform operates on symbolic recursions, but the C++ compiler pins
  the conversion to a single pipeline stage that runs before FAD. We
  move faust-rs to the same discipline, not to a per-node conversion.
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

At the propagate layer today:

1. `fad(eq, phi)` runs `generate_fad_signals_multi`:
   - converts `eq` and `phi` together via `de_bruijn_to_sym_many`;
   - runs `ForwardADTransform` on SYMREC-form signals;
   - returns `[primal, tangent]` in SYMREC form.
2. `fad(vel, phi)` is reached; `vel` is the SYMREC tangent from step 1.
   - `generate_fad_signals_multi` converts again: body already SYMREC,
     seed still DEBRUIJNREC (from `slot_env`);
   - `Converter::convert` walks the DEBRUIJNREC via `match_de_bruijn_rec`
     and calls `fresh_var()`, which collides with the existing `W0` and
     bumps to `W1`.
   - seed ends up at `SYMREC(W1, …)`, body stays at `SYMREC(W0, …)`:
     two structurally identical but `TreeId`-distinct nodes.

No naming discipline on top of the current architecture can paper this
over, because the converter has no structural equivalence test between
`DEBRUIJNREC` and `SYMREC`. The only way to stop the drift is to stop
converting twice — do it once, at a fixed pipeline boundary, on a tree
that is guaranteed to be entirely in one form.

---

## Target architecture

### Pipeline after the change

```
[eval → boxes]                      // DeBruijn throughout
    ↓
[propagate]                         // DeBruijn throughout; FAD runs here
    ↓  (signals still in DeBruijn)
[signal_prepare]
    ├─ simplification
    ├─ de_bruijn_to_sym             // ONE pass, at the boundary
    └─ downstream transforms (FIR, codegen)
```

### Transform contract

`ForwardADTransform` consumes DeBruijn signals and produces DeBruijn
signals. Two new dispatch arms are added on top of the current
symbolic-recursion arms:

- `DEBRUIJNREF(level)` → `Dual { primal: id, tangent: DEBRUIJNREF(level)
  with tangent projection offset }`.
  In practice: every primal lane is expanded to `primal + tangent` at
  the binder, so a reference at depth `level` becomes a reference with
  the same depth but pointing at the tangent lane of the enclosing
  `DEBRUIJNREC`. The binder-side rule below defines the lane layout.

- `DEBRUIJNREC(body)` → `DEBRUIJNREC(body')` where `body'` carries both
  the primal body and one tangent body per active seed. Concretely, if
  `body = cons(p0, cons(p1, …, nil))` has `N` primal lanes, `body'` has
  `N * (1 + M)` lanes, with `M` = number of live seeds:
  `cons(p0, cons(t0_s0, cons(t0_s1, …, cons(p1, cons(t1_s0, …, nil))))`.
  References `DEBRUIJNREF(level)` inside the body pick the appropriate
  projection of the expanded binder. The tangent of a primal reference
  to lane `i` through depth `level` is the reference to the corresponding
  tangent lane `i * (1 + M) + 1 + seed_index` at the same depth.

The existing symbolic-recursion arms are kept so the transform stays
defensive against any residual SYMREC that may reach it during
migration, but the happy path is purely DeBruijn.

### Seed matching

`ForwardADTransform::transform_uncached` still shortcuts on
`sig == self.diff_seed`. Because nothing converts between forms any
more, the seed sub-term is hash-consed once and shares `TreeId` with
every occurrence inside the body (including across nested FAD calls).

### Conversion point

`crates/compiler/src/signal_prepare.rs` already runs on the top-level
signal list. It grows one call to `tlib::de_bruijn_to_sym` (or the
many-root helper if we keep the per-process list intact) after
simplification, before FIR lowering. Every downstream stage keeps its
current SYMREC-form contract.

---

## Work items

### Step 1 — Audit current DeBruijn-only signal path

- Confirm that `propagate` never *reads* SYMREC nodes it didn't
  introduce itself via FAD. Grep for `match_sym_rec` / `match_sym_ref`
  inside `crates/propagate/`.
- Confirm that `signals::dump_sig` and the existing propagate tests do
  not rely on SYMREC being produced before `signal_prepare`.
- Confirm that `eval` emits DeBruijn only. It already does, but nail
  the property down in a unit test (build a minimal `rec(_)` program,
  propagate it, assert no SYMREC node reachable from the process
  outputs).

### Step 2 — DeBruijn-native FAD transform

File: `crates/propagate/src/forward_ad.rs`.

- Add `match_de_bruijn_rec` / `match_de_bruijn_ref` helpers to the
  transform's dispatch, mirroring the existing `match_sym_rec` /
  `match_sym_ref` arms.
- Implement the binder-expansion rule above:
  - Split the primal body list with `list_to_vec`.
  - Compute `transform_list`-style duals for each element under one
    placeholder cache entry that breaks self-reference.
  - Interleave primals and tangents into the new body list.
  - Rebuild the `DEBRUIJNREC` with `de_bruijn_rec(arena, new_body)`.
- Implement the reference rule:
  - For a `DEBRUIJNREF(level)` that resolves to primal lane `i` within
    the enclosing binder of arity `N` (before expansion), the primal
    stays at index `i * (1 + M)` of the expanded list; its tangent
    with respect to seed `s` is at index `i * (1 + M) + 1 + s`.
  - Requires the transform to carry a per-binder lane map. Encode it
    as a stack alongside `cache` inside `ForwardADTransform`.
  - Use `SigBuilder::proj` against the rewritten binder to emit the
    correct projection shape expected by downstream consumers.
- Update the multi-seed orchestration in `generate_fad_signals_multi`:
  - Remove the `de_bruijn_to_sym_many` call introduced on 2026-04-21.
  - Build the lane map for the outermost request
    (`M = seeds.len()`, `N = outputs.len()` before expansion).
  - Drive one `ForwardADTransform` pass per seed, composed over all
    outputs, same as today, but with the DeBruijn-native dispatch.
- Keep `generate_fad_signals_multi`'s public signature stable; the
  change is confined to the body.

### Step 3 — Move conversion to `signal_prepare`

File: `crates/compiler/src/signal_prepare.rs`.

- After the existing simplification step, call
  `tlib::de_bruijn_to_sym_many` over the primal output list.
- Gate the call behind a property assertion:
  `debug_assert!(is_de_bruijn_closed(arena, out))` for every output.
- Thread the converted signals through the existing return path; no
  downstream consumer observes a change.

### Step 4 — Retire the in-FAD converter call

- Delete the bundling / splitting in `generate_fad_signals_multi`.
- Keep `de_bruijn_to_sym_many` exported from `tlib`; it is now the
  pipeline-level helper and is useful to any caller that converts a
  multi-root list.
- Verify with `rg "de_bruijn_to_sym[_(]" crates/propagate` returns
  nothing.

### Step 5 — Regression and parity tests

- `tests/corpus/fad_lambda_recursive_seed.dsp` already exercises the
  single-level case — keep as-is.
- Add `tests/corpus/fad_nested_on_recursive_seed.dsp`:
  the `fad(fad(eq, phi), phi)` shape from the pendulum cello, reduced
  to a minimum reproduction (no stdfaust) so the test can run with the
  default debug-build stack.
- Add `crates/compiler/tests/signal_pipeline.rs::
  corpus_fad_nested_on_recursive_seed_shares_single_recursion_slot`:
  assert three primal lanes (`pos, vel, acc`), and, after
  `signal_prepare`, exactly one SYMREC reachable from the outputs that
  contains the expected `frac` body.
- Add `crates/propagate/tests/core_api.rs::
  fad_nested_on_debruijn_keeps_seed_identity`:
  propagate the nested FAD, walk the output graph, assert the seed's
  `SigId` appears once per expected site inside each tangent row.
- Run `cargo test -p propagate -p tlib -p compiler --workspace`.
- Run the binary: `cargo run --bin faust-rs -- fad_pendulum_cello1.dsp`;
  the emitted C++ must have exactly one `fRec` for the phasor and no
  stuck-at-zero integer recursion multiplying the acceleration trigger.

### Step 6 — Documentation

- Update `porting/faust-rs-supported-faust-subset-en.md` to note that
  nested `fad` calls on a shared seed are now supported and scheduled
  to a single recursion slot.
- Journal entry on the landing date covering scope, validation, and
  parity notes against the C++ pipeline.

---

## Risks and mitigation

- **Lane-layout mistakes.** Interleaving primals and tangents inside
  `DEBRUIJNREC` is the one load-bearing invariant. Unit-test the
  projection rule in isolation before wiring it through the signal
  pipeline. Build a handful of hand-written DeBruijn trees and run
  them through `transform_list` checking every projection lands on
  the expected index.
- **Hidden SYMREC producers.** Step 1 must be airtight. If any stage
  other than `signal_prepare` emits SYMREC, FAD will see a mixed tree
  and we are back to the current fork-on-fresh-name bug. The unit test
  under Step 1 is the safety net.
- **Performance.** The lane expansion grows each `DEBRUIJNREC` body by
  `(1 + M)` per primal lane. For single-seed FAD this is a 2× factor,
  same as today's symbolic expansion. For multi-seed FAD it is strictly
  equal to what `generate_fad_signals_multi` already emits at the top
  level, just moved inside the binder. No new cost.
- **Backwards compatibility.** The public `de_bruijn_to_sym` API is
  unchanged. Plan (A) was strictly additive; Plan (B) is strictly
  subtractive inside propagate and additive inside `signal_prepare`.
  External consumers of `propagate` that read SYMREC from the output
  will continue to see SYMREC — the conversion still happens, just
  one layer up.

---

## Exit criteria

- `fad_pendulum_cello1.dsp` produces audible output: single `fRec` for
  `phi_gen`, noise trigger driven by the correct second-derivative
  tangent.
- `generate_fad_signals_multi` contains no reference to
  `de_bruijn_to_sym*`.
- `signal_prepare` has exactly one `de_bruijn_to_sym_many` call.
- `cargo test --workspace` is green, including the two new regression
  tests.
- Journal entry lands alongside the implementing commit.
