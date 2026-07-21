# WAC 2017 directory — why 24 DSPs still do not vectorize

Date: 2026-07-20
Commit: `f81e8d9e` (branch `main-dev`)
Corpus: `/Users/letz/Developpements/Recherche/WAC/WAC 2017/Faust` — 197 `.dsp`
files (non-recursive)
Mode: `-vec -lv 0 -ss 0`, f64, default vector size, fast lane
Scanner: `cargo run -p compiler --example count_vector_corpus -- 0 0 --json`,
run from a scratch directory where `tests/impulse-tests/dsp` is a symlink to
the WAC directory (the scanner's corpus root is hardcoded and cwd-relative;
the symlinked root also serves as the import path, so local `.lib`/`.dsp`
imports resolve in place).

Update 2026-07-21: Class 1's implementation defect is fixed by commit
`3639e266` (`Fix vector identity recursion aliases`). The counts below are the
historical scan at `f81e8d9e`; they are intentionally retained until the full
external WAC corpus is re-qualified with its vector-mode oracle matrix.

## 1. Headline result

| Outcome | Count |
|---|---|
| Certified vector pipeline | **173** |
| Fallback to scalar (fail-closed) | **20** |
| Front-end error (no FIR at all) | **4** |

This exactly reproduces the X3 qualification figure (173/197 at `a180aa6b`);
nothing has regressed or silently improved since. The 24 non-vectorized DSPs
split into four fallback classes plus one error class, below in decreasing
size. Every fallback is fail-closed: the DSP still compiles and runs
correctly through the scalar path; only the checked vector pipeline declines
to certify it.

## 2. Class 1 — `FRS-VEC-FALLBACK-ASSEMBLY`, identity pass-through recursion
projections (11 files; fixed, re-qualification pending)

Diagnostic: `recursion group N projection P has no routed definition`.

| File | Detail |
|---|---|
| `clarinetMIDI.dsp` | group 2165 projection 2 |
| `clarinetMIDI1.dsp` | group 2165 projection 2 |
| `clarinetMIDI-exp.dsp` | group 2179 projection 2 |
| `clarinetMIDIReverb.dsp` | group 2165 projection 2 |
| `clarinetMIDITITI.dsp` | group 2165 projection 2 |
| `clarinetMIDI_esp32.dsp` | group 2153 projection 2 |
| `clarinetMIDI_vcv.dsp` | group 2153 projection 2 |
| `elecGuitarMIDI.dsp` | group 3457 projection 2 |
| `guitarMIDI.dsp` | group 3685 projection 3 |
| `violinMIDI.dsp` | group 3857 projection 3 |
| `integrator.dsp` | group 77 projection 2 |

Root cause: the P6.1 state-plan producer converted the aggregate effects of a
structural `SYMREC` carrier into a `RecursionStep` projection for **every**
body slot. Identity/pass-through slots have no reachable `SIGPROJ` and no
routed computation, but the assembler consequently required a fused member
definition and failed. This was a faulty state-model boundary, not an absence
of a valid recursive value to route.

One-line repro isolated from `integrator.dsp` (verified on this commit):

```faust
process = (_*0.5,_*0.5,_,_)~(_,_);   // pre-fix: Fallback(FirAssembly), projection 2
process = (_*0.5,_*0.5)~(_,_);       // Certified before and after the fix
```

The fix derives `RecursionProjectionTransition`s only from reachable `SIGPROJ`
records; its independent checker repeats the derivation and rejects a forged
transition for an identity slot. The pure-lowering boundary treats an
unmanaged recursion effect as aggregate carrier metadata only after that
checker has established complete reachable-projection coverage. C++ 2.87.1
`-vec` confirms this shape: it emits recurrence storage only for the first two
slots, while the final `_,_` slots are direct input/output loops.

Qualification now includes scalar/vector exactness at vector sizes 32 and 24,
both loop variants, and all scheduling strategies. The dedicated
`tests/impulse-tests/dsp/recursive_identity_passthrough.dsp` additionally
matches the genuine C++ 60,000-frame four-pass impulse reference under all
eight `-lv 0/1 × -ss 0..3` C++ backend configurations. The original eleven
WAC files still require external-corpus oracle re-qualification before the
headline count is changed.

## 3. Class 2 — `FRS-VEC-FALLBACK-PURE`, foreign functions of unknown purity
(6 files)

Diagnostic: `signal N is effectful and cannot enter pure P5.2 lowering:
SIGFFUN(...)`.

| File | Foreign symbol |
|---|---|
| `acosh.dsp` | `copysign` (via `ma.copysign`) |
| `copysign.dsp` | `copysign` |
| `isinf.dsp` | `isinf` |
| `math.dsp` | `isnan` |
| `noise2.dsp` | `copysign` |
| `random.dsp` | `arc4random` |

This is the known **E3 class**: every `ffunction` primitive carries purity
Unknown, and pure P5.2 lowering rejects Unknown-purity calls wholesale. The
class splits in two:

- `copysign` / `isnan` / `isinf` are libm-pure; a conservative whitelist (or
  a purity declaration channel) would certify five of the six files with no
  semantic risk.
- `arc4random` is genuinely stateful. Rejecting it from pure lowering is
  *correct*; vectorizing `random.dsp` needs the effect-event machinery (the
  E-stream direct-effect-attribution path), not a purity whitelist.

The open design question is unchanged from the corpus-side E3 notes: when may
a foreign call be trusted pure — declared, inferred, or via a libm whitelist?

## 4. Class 3 — `FRS-VEC-FALLBACK-EVENTS`, event-order certificate limits
(2 files)

- `poly_detect.dsp` — `bounded event model requires at least 38906 events,
  limit is 32768`. A capacity ceiling, not a semantic rejection: the program
  (polyphonic detection, many voices × stateful ops) needs more events than
  `DEFAULT_COMPACT_EVENT_LIMIT = 32_768`
  (`vector/events/model.rs`). Raising the budget or compacting further would
  admit it; the certificate itself did not find any ordering problem.
- `karplus_freeverb_esp32.dsp` — `vector execution reverses scalar dependence
  248 -> 1021`. Here the certificate found a real ordering reversal between
  the planned vector execution and the scalar dependence order, and correctly
  refused. This is a genuine scheduling/plan limitation (Karplus excitator +
  embedded `library("freeverb.dsp")`), not a budget issue.

## 5. Class 4 — `FRS-VEC-FALLBACK-PLAN`, cyclic epoch graph (1 file)

- `clarinetMIDI_REVERB.dsp` — `constructed vector plan is invalid: epoch 0
  induced graph has a cycle: [0, 80, 82, 203, 205, ...]`.

Same instrument family as Class 1, but this variant (clarinet + reverb wired
so that the plan's epoch-0 induced graph closes on itself) dies earlier, at
plan verification rather than assembly. It is the only cyclic-plan case in
the directory. Plausibly related to the alias-projection modeling (the same
group shape feeding the reverb), to be re-checked after a Class 1 fix.

## 6. Front-end errors (4 files) — not vectorization issues

All four were arbitrated against C++ Faust 2.87.1, which **rejects the same
four files**:

| File | faust-rs | C++ 2.87.1 |
|---|---|---|
| `EchoMatrix.dsp` | missing `process` definition | `undefined symbol : process` |
| `esp32_multi.dsp` | missing `process` definition | `undefined symbol : process` |
| `bang_karplus.dsp` | sequential composition mismatch at node 33674 (1 output vs 0 inputs) | same mismatch, `pulsen(1)(10000) : ks_ui_MIDI` |
| `rand2.dsp` | symbol `rnoises` redefined in same scope | `noises.lib:304 redefinition of symbols … rmultinoise` |

These are invalid programs (or programs broken by stdlib evolution since
2017); they never reach the vector pipeline in either compiler. The only
cosmetic difference is which redefined symbol is reported first in
`rand2.dsp`.

## 7. Priority levers

1. **Re-qualify Class 1 externally** — the producer/checker correction is
   committed and the minimal impulse oracle passes, but each of the eleven
   promoted WAC files must pass the full vec-mode oracle matrix before the
   historical 173/197 headline is updated. Re-scan
   `clarinetMIDI_REVERB` and `karplus_freeverb_esp32` at the same time; both
   may shift class or resolve.
2. **Libm purity whitelist** (E3, narrow form) — 5 files for a bounded,
   auditable list (`copysign`, `isnan`, `isinf`); leaves `arc4random` to the
   effect path by design.
3. **Event budget for `poly_detect`** — measure memory/compile-time cost of a
   raised compact limit before moving it; the certificate passed structurally.

Caveats carried over from the stream memory: this directory is external and
cannot be a CI gate; certification is structural, so any file newly admitted
by these levers must pass the vec-mode oracle matrix before being claimed as
progress (the X3 trap: scalar-only numeric validation on promoted files).
