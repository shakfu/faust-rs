# Clock/AD checker independence plan (`vector/clock_ad`)

**Date:** 2026-07-20

**Baseline:** `transform-cleanup` after the scalar-emission determinism fix
(follow-up 2 of the post-R9 solidification sequence).

**Status:** executed 2026-07-20 — §2/§3 implemented, all four rejecting
mutations now caught (journal 2026-07-20); full battery green

**Scope:** make the `clock_ad` stage's independent checker actually
independent of its producer, so the assurance claim in
`crates/transform/src/signal_fir/vector/mod.rs` ("a checker never calls its
producer … it re-derives the facts it validates") holds for every stage.
Also move `VectorModuleFailure` out of `vector/module/build.rs` so no
`check.rs` imports anything from a producer file. **No change to accepted or
rejected inputs is intended**: the same plans must verify, the same defects
must be rejected, with the same error variants.

Related documents:

- [`transform-cleanup-documentation-factorization-plan-2026-07-19-en.md`](transform-cleanup-documentation-factorization-plan-2026-07-19-en.md)
  (§3.2 producer/checker boundary, §4.8 shared admission guards)
- [`vector-mode-signal-level-analysis-cpp-port-plan-2026-07-10-en.md`](vector-mode-signal-level-analysis-cpp-port-plan-2026-07-10-en.md)
  (P6.2 clock/AD artifact)
- [`scalar-emission-determinism-plan-2026-07-20-en.md`](scalar-emission-determinism-plan-2026-07-20-en.md)
  (follow-up 1; lands first)

## 1. The gap

`clock_ad/check.rs:9` imports `derive_clock_islands`,
`derive_transport_policies`, and `derive_reverse_fallbacks` from
`build.rs`, and the shared terminal verify path
(`verify_vector_clock_ad_plan_after_vector_plan`) validates the artifact by
**re-running the producer's own derivation and comparing for equality**.
This catches artifact corruption and drift, but a defect inside a shared
`derive_*` is invisible: producer and checker reproduce the same wrong
answer. Every other split stage (`plan`, `state`, `events`, `assemble`)
re-derives on the checker side with its own code; `clock_ad` is the one
exception, currently blessed by the structure-check's narrow
entry-point-only ban.

## 2. Design: property checks against the sources, not equality with a re-run

Replace the three equality comparisons with checker-owned obligations that
validate the artifact's claims directly against the sources (`prepared`
arena, `ClockDomainTable`, decoration certificate, vector plan). This is
*stronger* than duplicating the derivation: the checker states the spec as
per-field properties instead of reproducing a construction.

For each `ClockIsland` in the artifact (and exactly one island per domain in
the table — coverage both ways):

- `kind`/`parent_domain` match the `ClockDomainTable` entry;
- `wrapper_signal_id` matches, in the arena, a wrapper of that kind
  (`OnDemand`/`Upsampling`/`Downsampling`) whose clocked first child carries
  a `ClockEnvToken` equal to `domain_id` and whose clock is
  `clock_signal_id`; no *other* decoration record is such a wrapper for this
  domain (uniqueness);
- `guard` is consistent with the clock record's canonical type (checker-owned
  re-statement of the boolean-interval / int-nature admission rules);
- `boundary_loop_id` is the wrapper's `Placement::Owned` loop and that loop
  is `LoopKind::Island`;
- `signal_ids` equals the set of decoration records with
  `clock_domain == domain_id`;
- `clock_state_signal_ids`: every member has an owned `StateCell::Clock`
  effect and resolves to this domain; completeness: no non-nil clocked
  record with an owned clock effect is missing from the union over islands;
- `nested_loop_ids` equals the set of loops with a root whose plan
  `clock_id == domain_id + 1`.

For each `ClockTransportPolicy` (bijective with `plan.transports`): the mode
is the one dictated by (fused-group membership, producer signal `clock_id`,
arena `PermVar`-ness) — the decision table restated as a check.

For each `ReverseAdFallback`: bijective with the set of arena
`ReverseTimeRec`/`BlockReverseAD` records; owner loop is the signal's
`Placement::Owned`; kind/epochs/diagnostic as specified.

The small leaf helpers (`wrapper_domain_and_clock`, `clock_state_domain`,
`is_owned_clock_effect`, `guard_for`, `scalar_clock_facts`) are duplicated
into `check.rs` under checker-owned names. **This duplication is the
assurance boundary** (vector/mod.rs contract) and must not be re-shared.

The call graph keeps §4.8 intact: `build_vector_clock_ad_plan`'s terminal
step still calls the shared `verify_vector_clock_ad_plan_after_vector_plan`,
which now cross-checks the producer's construction against the checker's
independent obligations — a real producer-vs-checker comparison instead of
`f(x) == f(x)`. `reject_unadopted_stateful_reads` and
`verify_source_alignment` are already checker-owned and stay on both paths.

## 3. `module/check.rs` hygiene

`vector/module/check.rs:5` imports `VectorModuleFailure` from `build.rs`.
It is vocabulary, not a producer entry point; move it to the module's
`mod.rs` (or a new `model.rs` if `mod.rs` would exceed the facade role),
re-exported unchanged. After this move plus §2, **no `check.rs` under
`vector/` imports from a producer file**, which follow-up 3 (structure-check
hardening) will then enforce mechanically with an empty allowlist.

## 4. Validation

1. `cargo test -p transform` (including `clock_ad/tests.rs`) unchanged.
2. `cargo run -p xtask -- structure-check`, `golden-check`,
   `vector-coverage-check` (1,536 certified pairs unchanged), and the
   `vector_mode` oracle 35/35.
3. `emission-determinism` gate stays green (no emission change expected at
   all — this is checker-side only).
4. **Rejecting mutations** (the step that fails today): temporarily corrupt
   each producer derivation in `build.rs` — wrong `guard`, dropped
   `nested_loop_ids` entry, swapped transport mode, dropped reverse
   fallback — and confirm the shared terminal verify now rejects the
   artifact (it demonstrably cannot today, since both sides re-run the same
   code). Revert the mutations; record results in the journal.
5. `cargo clippy -p transform -- -D warnings`, `cargo fmt`.

## 5. Non-goals

- No change to the P6.2 artifact schema or error taxonomy.
- No attempt to prove effect commutation or other `verify/mod.rs` deferred
  obligations (separate follow-up 5).
- No de-duplication of the checker-owned leaf helpers back into shared code.
