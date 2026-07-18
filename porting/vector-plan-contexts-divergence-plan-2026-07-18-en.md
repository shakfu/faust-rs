# Vector Plan Contexts Divergence Plan

Date: 2026-07-18
Status: X0 diagnosed (class A traced to its mechanism; B/C mechanism pending X1)
Scope: agreement between the vector plan's execution-context model and what the
lowering materializes

## 1. Objective and baseline

The 93-DSP impulse corpus stands at 91/93 with only the E3 foreign-function
class open. Measured against a real-world directory of 197 DSPs
(`WAC 2017/Faust`), the picture is different: 159 vectorize, 34 fall back, 4
fail to compile. **24 of the 34 fallbacks are one bug** under three
manifestations, none of which the corpus exercises.

The shared cause is the `contexts` map in `build_vector_plan`
(`vector/plan.rs`): the plan's model of *which loops execute which signal*.
Transports are derived from it, while the lowering materializes values by
walking the actual signal expression tree. Where the two disagree, each
direction of disagreement surfaces as a different error:

| Class | Divergence | Diagnostic | Files |
|---|---|---|---|
| A | context absent | `vector-plan dependency names missing signal N` | 11 |
| B | context too narrow | `no planned transport for signal N from loop O to loop C` | 8 |
| C | context too wide | `transport T for signal N load is absent from its consumer body` | 5 |

Class A covers the MIDI physical models (`clarinetMIDI*`, `guitarMIDI`,
`violinMIDI`, `elecGuitarMIDI`); class B the Karplus family (`karplus*`,
`chain`); class C the virtual-analog family (`virtualAnalogForBrowser`,
`WAWE`, `exp`, `minimoog-novation`).

The remaining 10 fallbacks are unrelated and already classified: 6 foreign
`ffunction` (the planned E3 class), plus one each of recursive-projection
assembly, event-bound overflow, undeclared module variable, and dependence
reversal.

## 2. Established mechanism - class A

Fully traced on `clarinetMIDI.dsp`:

- `contexts` is populated only inside `PlacementState::visit`, after three
  early returns: structural carriers, non-sample-required signals, and
  signals already placed `Control`. The insert sits *before* the `visited`
  guard, so any `visit` call on a signal creates its context; a signal with
  no context was therefore never visited.
- `visit` returns at a structural carrier **without recursing into its
  children**. The module doc states the opposite contract: "Symbolic
  recursion carriers and table containers remain structural: their executable
  children, rather than the containers, enter the sample closure."
- Independently, `Owned` placements are pre-seeded from the `owner` map
  before any traversal, and that loop does not touch `contexts`.
- `add_dependency_edges` then iterates over **every** certificate dependency,
  not only those reached by the traversal, and fails on the first missing
  context.

Measured: signal 2196 is `sample_required`, non-structural, in `owner`
(`Owned(8)`), and has a record - but no context. Its only two parents are
2202 (structural, so `visit` stops there) and 2205 (not itself reached).
Pruned traversal covers **256 of 305** signals; the raw child graph covers all
305. The 49-signal gap is the exposure.

The diagnostic is also misleading: the record is present, only the context is
missing, yet the message says "dependency names missing signal 2196". This
cost real diagnosis time and is fixed independently of the substance.

## 3. Established facts - classes B and C

What is established:

- **B** (`karplus.dsp`, `chain.dsp`, identical shape): the lowering resolves a
  use of signal S owned by loop 2 from consumer loop 3; the plan planned
  **zero** transports for S (`planned_routes=[]`). The signal is
  non-duplicable, `Scal`, with **38 effects**.
- **C** (`virtualAnalogForBrowser.dsp`): the plan planned
  `transport_s5550_l4_l148`; loop 148's body loads four other transports and
  never that one. The signal is non-duplicable, `Scal`, `Owned(4)`, with
  **4 effects**.
- C is **not** the typed-walker blindness fixed for soundfiles in E2: a
  generic child enumeration over the consumer body does not find the load
  either, so the load is genuinely absent rather than invisible.
- C is **not** a state-plan consumption: the signal appears in no prefix,
  delay, recursion, or waveform transition of the consumer loop. The existing
  escape hatch in the body check covers prefixes only.
- Both classes involve heavily effectful, non-duplicable, `Scal` signals -
  exactly where duplicability and effects decide materialization.

What is **not** established: why `contexts` is too narrow in B and too wide in
C. The unified attribution rests on all three classes reading or writing the
same map, plus the measured symptoms above; the precise divergence path in B
and C has not been traced to a line. X1 establishes it before any fix is
designed.

## 4. Correction shape

One phase, not three. The deliverable is agreement between the plan's context
model and the lowering's materialization, established as an invariant rather
than repaired per symptom. Two candidate shapes, to be decided by X1's
evidence:

- **Close the traversal**: make `visit` recurse through structural carriers to
  their executable children, so `contexts` covers every signal the certificate
  can reach, and make the pre-seeded `owner` placements enter the traversal.
  This directly addresses A and plausibly B.
- **Make the disagreement impossible**: derive transports from the same
  materialization decision the lowering makes, rather than from an
  independently computed context set.

The second is the stronger invariant and the larger change. X1 decides;
whichever is chosen, anything that cannot be proven stays fail-closed with a
diagnostic.

## 5. Independence obligation

The plan is a producer whose independent checker is `verify_vector_plan`, and
the divergence is precisely a fact neither currently checks. The new
obligation must be checkable from the plan and the emitted region bodies
alone:

- X.C1: for every transport in the plan, its consumer loop's emitted body
  contains a load of it (or an accepted state-transition consumption). This is
  today's body check, which already catches class C - it must stay and must
  not be weakened to admit the orphan.
- X.C2: for every cross-loop use the lowering resolves, the plan contains a
  matching transport. This is today's routing check, which already catches
  class B.
- X.C3 (new): every signal carrying a placement carries a context, and every
  context names a loop that exists in the plan. Class A is exactly the absence
  of this check - the failure surfaced by accident, deep inside dependency
  edge construction, rather than as a stated obligation.

X.C1 and X.C2 already exist and are what makes B and C fail closed rather than
miscompile. The phase must not relax them.

## 6. Rejecting mutations and focused tests

- M1: a forged plan whose signal has a placement but no context is rejected by
  X.C3 naming the signal.
- M2: a forged plan with a transport whose consumer body does not load it
  stays rejected (class C's current behaviour, pinned).
- M3: a forged plan missing a transport for a resolved cross-loop use stays
  rejected (class B's current behaviour, pinned).
- P1: a corpus-independent fixture reproducing class A - a sample-required
  signal reachable from the roots only through a symbolic recursion carrier -
  certified under both loop variants and all four strategies. It must fail by
  construction before the fix.
- P2, P3: the same for the B and C shapes, once X1 has reduced them to a
  minimal reproducer. If a class cannot be reduced, its corpus DSP is the
  regression instance and the journal says so.

## 7. Rollout

### X0 - plan

This document. No compiler behavior change.

### X1 - mechanism and diagnostics

Trace the B and C divergence paths to a line; correct the class A diagnostic,
which reports a missing record when the record exists and the context does
not. Add minimal reproducers. No behavior change beyond the message.

### X2 - invariant and fix

The chosen correction shape, X.C3, and M1-M3 with P1-P3. The widening and its
check land together.

### X3 - qualification

Full 16-mode corpus sweep with no DSP losing certification; byte-identity for
every already-certified DSP against the pre-change compiler; the native C++
oracle matrix for every newly certified corpus DSP; the external 197-DSP
directory re-measured with its fallback classes re-counted; compile-budget and
the workspace gates.

## 8. Risks and mitigations

- The external directory is not a committed corpus and cannot become a CI
  gate as it stands. X3 measures it as evidence; if the fix certifies these
  families, promoting one representative of each (a MIDI physical model, a
  Karplus, a virtual-analog) into `tests/impulse-tests` is the durable
  protection and should be weighed then. Without that, the corpus stays blind
  to exactly this bug.
- Closing the traversal widens what the plan admits; every newly admitted
  program must pass the oracle, and byte-identity must hold for everything
  already certified - the change must alter which programs are admitted, never
  what they compile to.
- Class A's 49 unvisited signals in one DSP suggest the gap is broad rather
  than a single edge case; the fix must be measured by the visited/record
  ratio reaching parity, not only by the corpus turning green.
- If B and C prove to have distinct mechanisms after X1, the phase splits
  rather than forcing a unified fix onto unlike causes. The unified attribution
  is a hypothesis backed by shared symptoms, not yet a traced identity.
