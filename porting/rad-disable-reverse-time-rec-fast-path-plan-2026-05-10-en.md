# Disable the `ReverseTimeRec` LTI/IIR Fast Path in RAD

Date: 2026-05-10

Status: design plan, follows
[`rad-block-reverse-ad-signal-ir-plan-2026-05-07-en.md`](rad-block-reverse-ad-signal-ir-plan-2026-05-07-en.md).

## 1. Decision

`ReverseTimeRec` is no longer the dispatch target for any recursive or
temporal RAD branch. The buggy LTI/IIR fast path is short-circuited and the
`SigBlockReverseAD` carrier (TBPTT(BS, BS) tape-and-replay) becomes the sole
backend for every recursive, IIR, or `Delay`/`Prefix` adjoint contribution
that the symbolic feed-forward sweep cannot resolve locally.

The Signal-IR `ReverseTimeRec` carrier and its FIR lowering are not deleted
in this patch — they stay as dormant infrastructure with one clear contract:
*RAD propagation never produces them*. A separate, follow-up pass (out of
scope here) may garbage-collect the unreachable code.

Updated layering after this plan lands:

```text
rad(expr, seeds)
  -> reverse_ad::generate_rad_signals
       -> ReverseADTransform::run            // symbolic feed-forward only
       -> SigBlockReverseAD(...)             // sole fallback for any temporal /
                                             //   recursive / IIR adjoint
  -> signal_prepare validation
  -> signal_fir lowering: forward sweep + reverse-loop adjoint sweep
```

The `ReverseTimeRec` arrow that previously sat between the two paths is
gone.

## 2. Motivation

The LTI fast path bundles five coupled pieces:

1. `classify_recursive_projection_rad_mode` returning `LinearTranspose`,
2. `is_lti_recursive_projection` stashing pairs onto
   `ReverseADTransform::recursive_projection_frontier`,
3. `propagate_iir_adjoint` invoking the IIR → de Bruijn bridge for
   second-order feedback,
4. `build_lti_recursive_adjoint_*` helpers that call
   `transpose_lti_de_bruijn_rec_with_cotangents`,
5. `signal_fir/module.rs::emit_reverse_time_rec_compute_resets` plus the
   `reverse_time_rec_group_ids` filter set in `RecursionState`.

This pipeline has produced two separate correctness incidents in the last
release train (cf. 2026-05-09 journal):

- A recursion-array-reset regression where the BRA primal carriers were
  zeroed at every `compute()` call because `emit_reverse_time_rec_compute_resets`
  did not discriminate between LTI adjoint carriers and ordinary primal
  recursion carriers (fix: `reverse_time_rec_group_ids` filter).
- Drive- and feedback-coefficient seed routing bugs through
  `propagate_lti_drive_adjoint` for second-order recursions where a seed
  appears in both a drive position and a recursion coefficient.

`SigBlockReverseAD` already covers the same class of graphs (B0–B6
implemented through 2026-05-09): one-pole, biquad, comb, time-varying SVF,
saturating feedback, and any FAD-surface combination of those. Routing
*all* recursive/temporal RAD cases through BRA removes a class of bugs
without removing user-visible functionality, and concentrates the remaining
optimisation work on a single carrier.

The performance trade-off is real but acceptable for now: BRA emits a
per-sample tape (size `BS × K`) and a reverse sample loop, whereas the LTI
fast path can collapse to a closed-form transposed recurrence with no tape.
Re-enabling the fast path is reserved as a future optimization phase, gated
on a fixed and tested LTI bridge (see §9).

## 3. Where We Stand Today (2026-05-10 audit)

Implemented and shipped on `main`:

- `crates/propagate/src/reverse_ad.rs`:
  - `ReverseADTransform::run` performs three passes (DFS collect, adjoint
    accumulate, seed extract).
  - The LTI frontier is filled inside `propagate_adjoint` via two arms:
    `SigMatch::Proj(_, _) if self.is_lti_recursive_projection(y) && self.contains_seed(group)`
    pushes onto `recursive_projection_frontier`; `SigMatch::Iir(_)` calls
    `propagate_iir_adjoint`.
  - After the postorder sweep, `propagate_recursive_projection_frontier`
    consumes the frontier and emits one `ReverseTimeRec` group per
    recursion via `build_lti_recursive_adjoint_projections`.
  - `generate_rad_signals` runs the transform once and only falls back to
    `build_block_reverse_ad` on
    `RadUnsupportedNode { kind ∈ { "delay-or-prefix",
      "recursive-bptt-required", "recursive-block-linear-time-varying",
      "recursive-projection" } }`.
  - `propagate_iir_adjoint` raises `RadUnsupportedNode { kind = "iir-state-space" }`
    when the IIR has feedback length > 2 *or* the bridge fails — and
    `iir-state-space` is **not** in the fallback whitelist, so a third-order
    direct IIR currently fails the entire RAD call.

- `crates/transform/src/signal_fir/module.rs`,
  `crates/transform/src/signal_fir/recursion.rs`:
  - `allocate_group_arrays` records group ids in
    `RecursionState::reverse_time_rec_group_ids` whenever the group is a
    `SigMatch::ReverseTimeRec(_)`.
  - `emit_reverse_time_rec_compute_resets` filters
    `rec_array_by_group_index` by that set.
  - `classify_reverse_time_outputs` flags an output as reverse-loop when it
    contains a `Proj(_, ReverseTimeRec(_))` *or* a
    `Proj(idx, BlockReverseAD)` with `idx >= primal_count`.
  - `lower_proj` strips `ReverseTimeRec(body)` and falls through to the
    normal recursion lowering.

- `crates/normalize/src/normalform.rs`,
  `crates/sigtype/src/rules.rs`,
  `crates/transform/src/signal_prepare.rs`:
  - `ReverseTimeRec` is preserved through normalization, given a type rule,
    and recognised by the projection arity validator.

- `crates/signals/src/lib.rs`:
  - `SIG_REVERSE_TIME_REC_TAG`, `SigBuilder::reverse_time_rec`,
    `SigMatch::ReverseTimeRec(body)`.

Tests asserting `ReverseTimeRec` is produced by RAD propagation:

- `crates/propagate/tests/core_api.rs`:
  - `propagate_reverse_ad_lti_recursive_one_pole_*` family (one-pole +~
    feedback).
  - `propagate_reverse_ad_lti_recursive_strict_lti_*` family.
  - `propagate_reverse_ad_iir_state_space_bridge` for second-order IIR.
- `crates/propagate/src/reverse_ad.rs` `mod tests` (in-file):
  - `lti_recursive_projection_frontier_shares_reverse_group` and adjacent
    cases asserting `match_sig` returns `SigMatch::ReverseTimeRec(_)`.
- `crates/transform/src/signal_fir/tests.rs`:
  - `reverse_time_rec_projection_lowers_to_reverse_sample_loop` and the
    surrounding goldens.

Tests building `ReverseTimeRec` directly via `SigBuilder::reverse_time_rec`
to validate carrier infrastructure (signals, normalize, signal_prepare,
sigtype) are **kept as-is** — they exercise the IR shape, not the RAD
dispatcher.

## 4. Scope

In scope:

- Stop emitting `SigMatch::ReverseTimeRec` from the RAD propagation pass.
- Route every previously-LTI/IIR recursive RAD case through
  `build_block_reverse_ad`.
- Update propagation-level tests that assert the LTI carrier shape to
  assert the `BlockReverseAD` carrier shape (or remove obsolete-shape
  expectations and assert numerical equivalence at runtime where the test
  is end-to-end).
- Document, in code and in the journal, that the LTI fast path is dormant.

Out of scope (deliberate):

- Deleting the `ReverseTimeRec` carrier from `signals`, `normalize`,
  `sigtype`, `signal_prepare`, or `signal_fir`. The IR node and its
  lowering remain compilable so we can re-enable the fast path later
  without re-introducing the carrier and its tests from scratch.
- Deleting the `transpose_ad.rs` LTI bridge, the
  `build_lti_recursive_adjoint_*` helpers, or
  `iir_filter_to_de_bruijn_rec_group`. They become unreachable from RAD
  propagation but stay covered by their unit tests, which exercise the
  algebraic transposition independently of the dispatcher.
- Deleting `RecursionState::reverse_time_rec_group_ids` or
  `emit_reverse_time_rec_compute_resets`. Both are dead under the new
  dispatcher but leaving them in place is zero-risk and keeps the diff
  small.
- BRA performance work (checkpointing, ring-buffer TBPTT). The previous
  plan §11.5b reservations are unchanged.

## 5. Implementation

### 5.1 Phase D1 — neuter the LTI/IIR fast path in propagate

Files touched:

| File | Change |
|------|--------|
| `crates/propagate/src/reverse_ad.rs` | In `ReverseADTransform::active_children`, replace the `SigMatch::Proj(_, _) if self.is_lti_recursive_projection(sig)` arm with a `Proj(_, group)` arm that detects an LTI-classified projection and **falls through** by raising `RadUnsupportedNode { kind: "recursive-projection" }`. Replace the IIR arms (`SigMatch::Iir(_) if !self.contains_seed(sig)` and `SigMatch::Iir(coefs)`) with a single `SigMatch::Iir(_)` arm raising `RadUnsupportedNode { kind: "iir-state-space" }`. Remove or `#[allow(dead_code)]` the `recursive_projection_frontier` field, `is_lti_recursive_projection`, `propagate_iir_adjoint`, `propagate_recursive_projection_frontier`, `propagate_lti_drive_adjoint`, and the `Proj`/`Iir` arms in `propagate_adjoint`. |
| `crates/propagate/src/reverse_ad.rs` (`generate_rad_signals`) | Add `"iir-state-space"` to the kind whitelist that triggers `build_block_reverse_ad`. |
| `crates/propagate/src/stateful_rad.rs` | Demote `RecRadMode::LinearTranspose` to `BlockLinearTimeVarying` at the *classifier dispatch* boundary used by `reverse_ad.rs`, or wire `is_lti_recursive_projection` to always return `false`. The classifier itself (`classify_de_bruijn_rec_rad_mode`) keeps its three-way return type, since its `LinearTranspose` arm is also consulted by other passes (LTI library work, `lti-filter-intermediate-form-plan-2026-05-06-en.md`) that are unaffected by RAD dispatch. |

The shape of the `active_children` change:

```rust
// before
SigMatch::Proj(_, _) if self.is_lti_recursive_projection(sig) => {
    // Phase-E1 LTI bridge: keep the recursive projection as a leaf during
    // the ordinary reverse sweep…
}
SigMatch::Iir(_) if !self.contains_seed(sig) => { /* skipped */ }
SigMatch::Iir(coefs) => { /* validate length, walk children */ }

// after
SigMatch::Proj(_, _) => {
    // RAD always defers recursive projections to the block fallback now.
    return Err(PropagateError::RadUnsupportedNode {
        node: sig,
        kind: "recursive-projection",
    });
}
SigMatch::Iir(_) => {
    return Err(PropagateError::RadUnsupportedNode {
        node: sig,
        kind: "iir-state-space",
    });
}
```

The corresponding `propagate_adjoint` arms vanish in the same patch — they
are dead once `active_children` refuses to descend into those nodes,
because nothing reaches `propagate_adjoint` for them.

The `generate_rad_signals` whitelist becomes:

```rust
match transform.run(primals, seeds) {
    Ok(r) => r,
    Err(PropagateError::RadUnsupportedNode { kind, .. })
        if matches!(
            kind,
            "delay-or-prefix"
                | "recursive-bptt-required"
                | "recursive-block-linear-time-varying"
                | "recursive-projection"
                | "iir-state-space"            // NEW
        ) =>
    {
        build_block_reverse_ad(arena, primals, seeds)
    }
    Err(e) => return Err(e),
}
```

Pass criteria:

- `cargo check --workspace` clean (after the `dead_code` annotations).
- The dispatcher only reaches `build_block_reverse_ad` for the five kinds
  above; every other RAD-rejected primitive (writable tables, soundfile,
  unrecognised FFun, mutable Rd/Wr) keeps its targeted diagnostic.
- No code path can produce `SigMatch::ReverseTimeRec(_)` from
  `generate_rad_signals`.

### 5.2 Phase D2 — propagate test corpus

Files touched:

| File | Change |
|------|--------|
| `crates/propagate/tests/core_api.rs` | For the LTI/IIR cases listed in §3, replace the `SigMatch::ReverseTimeRec(_)` shape assertion with a `SigMatch::BlockReverseAD { … }` assertion that checks the primal/seed slot count. Keep the seed/primal output count assertion. Where an end-to-end finite-difference equivalence assertion exists, keep it unchanged — the gradient is the same modulo block-truncation, which the test corpus already accommodates for BRA. |
| `crates/propagate/src/reverse_ad.rs` (`mod tests`) | Update or move the `lti_recursive_projection_frontier_shares_reverse_group` family to live next to the LTI bridge unit tests in `transpose_ad.rs` (where they exercise the LTI transposition without going through `generate_rad_signals`). Tests that are inherently dispatcher-level — e.g. `recursive_projection_frontier_dispatch_*` — assert `BlockReverseAD` instead of `ReverseTimeRec`. |
| `crates/transform/src/signal_fir/tests.rs` | The `reverse_time_rec_projection_lowers_to_reverse_sample_loop` test stays — it builds the carrier directly, exercising the FIR lowering. Mark it `#[ignore = "dormant infrastructure: RAD never emits ReverseTimeRec; lowering is exercised by the in-source test only"]` if we want the test to keep tracking the lowering shape *without* signalling carrier production. (Decision: keep enabled. The lowering still works and we want regressions caught.) |
| `crates/compiler/tests/rad_runtime.rs` | If any test currently exercises an LTI path through `rad(...)` and asserts the LTI carrier shape, replace the carrier check with an output-count + numerical-equivalence check against the FAD oracle. Most of the runtime tests already do that. |

Pass criteria:

- `cargo test -p signals -p sigtype -p normalize -p transform -p propagate -p compiler`
  green.
- Every `SigMatch::ReverseTimeRec` assertion in propagation-level tests is
  either (a) replaced with a BRA-shape assertion or (b) moved to a unit
  test that does not go through `generate_rad_signals`.

### 5.3 Phase D3 — documentation and journal

Files touched:

| File | Change |
|------|--------|
| `crates/propagate/src/reverse_ad.rs` (top-of-file Rustdoc) | Replace the dispatch-order paragraph in `generate_rad_signals` Rustdoc with: "Symbolic feed-forward sweep, then `SigBlockReverseAD` fallback. The legacy `ReverseTimeRec` LTI/IIR path is dormant — see `porting/rad-disable-reverse-time-rec-fast-path-plan-2026-05-10-en.md`." |
| `crates/transform/src/signal_fir/module.rs` (`emit_reverse_time_rec_compute_resets` Rustdoc) | Add a one-line note: "Dormant under the 2026-05-10 dispatcher change; kept compilable for a future LTI fast-path revival." |
| `crates/transform/src/signal_fir/recursion.rs` (`reverse_time_rec_group_ids` field Rustdoc) | Same note. |
| `crates/signals/src/lib.rs` (`reverse_time_rec` Rustdoc) | Add: "RAD propagation does not currently produce this node; it remains as IR-level carrier infrastructure. See the 2026-05-10 plan." |
| `JOURNAL.md` + `porting/journal/2026-05-10.md` | One entry summarising the disable + the plan reference. |

Pass criteria:

- A reader of `reverse_ad.rs` learns from Rustdoc alone that
  `ReverseTimeRec` is never produced by RAD.
- The `ReverseTimeRec`-related public symbols carry a "dormant" note so
  future contributors do not chase a dead dispatcher arm.

## 6. Diagnostics

No new diagnostic kinds. Two existing kinds widen their meaning:

- `recursive-projection`: now covers every recursive `Proj` reached by the
  reverse sweep, not only the non-LTI ones. The `BlockReverseAD` fallback
  consumes it.
- `iir-state-space`: now covers every `SigIir` reached by the reverse sweep
  (including order ≤ 2 cases that the LTI bridge previously handled). The
  `BlockReverseAD` fallback consumes it.

The kinds that already escape the fallback whitelist
(`writable-table`, `soundfile`, `ffun`, `int-cast` etc.) are unchanged and
continue to surface as user-facing diagnostics with their own messages.

## 7. Tests

New test (Phase D1, end-to-end):

```rust
// crates/propagate/tests/core_api.rs
#[test]
fn rad_lti_one_pole_now_falls_back_to_block_reverse_ad() {
    // process = rad((seed : + ~ *(0.5)), seed)
    // Before: produces Proj(_, ReverseTimeRec(_)).
    // After:  produces Proj(_, BlockReverseAD { primal_count = 1, … }).
    let outs = /* … build via BoxBuilder, propagate_typed … */;
    assert_eq!(outs.len(), 2);
    let SigMatch::Proj(_, group) = match_sig(&arena, outs[1]) else { panic!() };
    assert!(matches!(
        match_sig(&arena, group),
        SigMatch::BlockReverseAD { primal_count: 1, .. }
    ));
}
```

Existing tests to update (list, exhaustive within `propagate`):

- `propagate_reverse_ad_lti_recursive_one_pole_strict`
- `propagate_reverse_ad_lti_recursive_one_pole_seed_shared_with_coefficient`
- `propagate_reverse_ad_lti_recursive_strict_lti_drive_seed`
- `propagate_reverse_ad_lti_recursive_strict_lti_feedback_coefficient`
- `propagate_reverse_ad_iir_state_space_bridge`
- `propagate_reverse_ad_lti_recursive_grouped_projections`

For each, the assertion `SigMatch::ReverseTimeRec(_)` becomes
`SigMatch::BlockReverseAD { … }`. Where the test also asserts the presence
of a `Delay1` inside the gradient (e.g. coefficient-gradient cases), drop
that assertion — BRA encodes the delay state inside the tape, not in a
visible `Delay1` of the output signal tree.

In-source unit tests (`crates/propagate/src/reverse_ad.rs::tests`) that
directly exercise the LTI bridge helpers move to
`crates/propagate/src/transpose_ad.rs::tests` if they are not already
there. The bridge functions stay public (`pub(super)`) inside `propagate`,
so the move is mechanical.

End-to-end runtime tests (`crates/compiler/tests/rad_runtime.rs`) that
compile and run a RAD DSP, then compare against the FAD oracle, are
unchanged — they assert numerical equivalence, not carrier shape.

## 8. Risks

- **Performance regression on LTI graphs.** A first-order or second-order
  recursive RAD now allocates a `BS × K` tape and runs a reverse sample
  loop instead of a closed-form transposed recurrence. For a one-pole this
  is ~`BS × 2` floats per `compute()` call plus one additional sample-loop
  pass. Acceptable for correctness-first; revisit if a downstream user
  reports a regression in DSP throughput.
- **Truncation behaviour identical to ReverseTimeRec.** Both paths used the
  same TBPTT(BS, BS) terminal-zero adjoint reset, so users who built code
  on top of the LTI fast path observe the same gradients within tolerance.
  We do not silently change semantics.
- **Dormant code rot.** The `ReverseTimeRec` carrier, its FIR lowering, and
  the LTI bridge become unreachable from RAD. Their unit tests still cover
  algebraic correctness, but a refactor in `signal_fir` could break the
  carrier lowering without anybody noticing. Mitigation: keep
  `reverse_time_rec_projection_lowers_to_reverse_sample_loop` enabled and,
  in a follow-up patch, add a hidden test that instantiates the carrier
  through `SigBuilder::reverse_time_rec` and runs it through
  `prepare_signals_for_fir`.
- **BRA blind spot for unknown-bound delays.** If a test corpus member uses
  a sample-variable `Delay(c, x)` with unbounded interval, the dispatcher
  raises `RadUnsupportedNode { kind: "delay-bound-unknown" }` — already in
  scope of the BRA plan, and not made worse by this change.
- **Re-entry risk on later optimisation work.** When the LTI fast path is
  revived (see §9), the dispatch arm and the test split must be reverted
  together. Annotating both points in code with the plan filename makes
  this a single-grep operation.

## 9. Future-proofing — Re-enabling `ReverseTimeRec`

The dormant infrastructure is preserved precisely so that a future patch
can revive the fast path once the underlying bugs are fixed. The
re-enablement steps would be:

1. Restore `is_lti_recursive_projection` and the `Proj`/`Iir` arms in
   `propagate_adjoint`, gated on a stricter classifier than today (e.g. a
   pure-`LinearTranspose` predicate that also forbids drive/coefficient
   seed sharing or known-buggy compositions).
2. Add a fuzzing harness comparing BRA gradients to the LTI fast-path
   gradients on randomly-generated single-pole / biquad graphs, runtime
   and goldens, before re-enabling by default.
3. Revert §5.1 changes selectively (one arm at a time: LTI projection,
   then IIR state-space bridge).
4. Keep BRA as the universal fallback so any classifier rejection lands on
   correctness instead of failure.

The plan deliberately leaves the carrier and its FIR lowering compilable
so that step 3 is a propagate-only change.

## 10. Recommended Next Patch (Phase D1 + D2 + D3 in one diff)

1. `reverse_ad.rs`: collapse the LTI/IIR arms in `active_children`,
   `propagate_adjoint`; drop or `#[allow(dead_code)]` the frontier helpers;
   widen the `generate_rad_signals` fallback whitelist; update the
   top-of-`generate_rad_signals` Rustdoc.
2. `stateful_rad.rs`: ensure `is_lti_recursive_projection` (or its
   equivalent at the dispatcher boundary) returns `false` for every input.
3. Migrate the propagate-level LTI assertions in
   `crates/propagate/tests/core_api.rs` and the in-source tests of
   `reverse_ad.rs` (move bridge unit tests to `transpose_ad.rs`).
4. Add `rad_lti_one_pole_now_falls_back_to_block_reverse_ad` as the
   regression-pinning test for the new dispatch.
5. Add the "dormant" Rustdoc notes on the four IR/lowering touchpoints
   listed in §5.3.
6. Journal entry in `porting/journal/2026-05-10.md`.

Validation checklist:

- `cargo fmt --all`
- `cargo check --workspace`
- `cargo test -p signals -p sigtype -p normalize -p transform -p propagate -p compiler`
- `cargo test -p compiler --test rad_runtime`
- Inspect a representative one-pole and biquad RAD compilation with
  `--dump-signal-ir` to confirm no `SIGREVERSETIMEREC` tag appears in the
  output.
