# Signal graph input/output latency analysis plan

Date: 2026-06-19

## Objective

Add a static analysis over the `signals` graph that computes an audio
input/output latency interval `[min, max]` for each observable DSP output.

The observable outputs are:

- final audio outputs, meaning the propagated/prepared signal roots later passed
  to FIR lowering;
- output controls of bargraph kind, meaning `SigMatch::VBargraph(control,
  value)` and `SigMatch::HBargraph(control, value)`.

For each observable, the result must report the minimum and maximum latency from
an audio input `Input(i)` to the observed value. UI input controls (`button`,
`checkbox`, `slider`, `numentry`) are control sources, not audio inputs; they may
influence a value, but they do not directly contribute to audio-input to output
latency.

## Expected Uses

The analysis is useful as a static property of the signal graph, not only as a
backend implementation detail. The main expected uses are:

1. **Automatic time alignment.** Detect branches with different latencies and
   support delay-compensation decisions. For example,
   `process = _, @(10) : +` mixes a direct branch with a delayed branch; the
   analysis can expose that mismatch before lowering.
2. **DSP documentation and diagnostics.** Report, for each audio output, which
   audio inputs reach it and with which latency interval, for example
   `out[0] depends on input[0] with latency [3, 3]`. The same applies to
   bargraphs, so a meter can be identified as observing either the current
   signal or a delayed value.
3. **Causality validation.** Help distinguish valid delayed feedback from
   problematic instantaneous dependency cycles. This does not replace typing or
   normalization, but it gives a focused guardrail around delay and recursion
   behavior.
4. **Optimization and scheduling support.** Provide explicit temporal facts to
   later `transform`/FIR passes instead of forcing each pass to rediscover them
   indirectly. This can help with buffering, delay merging, and placement of
   computed values.
5. **Output-control analysis.** Treat bargraphs as observable outputs even
   though they are not audio outputs. This matters for meters, trackers, and
   monitoring signals whose displayed value may have a different latency from
   the audio path.
6. **Parity and regression tests.** On simple linear graphs, expected latency
   can be checked against impulse behavior. This gives a structural
   complement to golden-output tests and can catch mistakes in delay,
   recursion, and clock-domain lowering.

Some results will necessarily be intervals, `Unknown`, or `Unbounded`,
especially with variable delays, tables, `select2`, soundfiles, recursion, or
clock-domain wrappers. That is still useful: it identifies exactly where the
compiler cannot guarantee a fixed latency.

## Definitions

Represent latency with an explicit domain:

```rust
enum LatencyBound {
    Exact(u64),
    Unbounded,
    Unknown,
}

struct LatencyInterval {
    min: LatencyBound,
    max: LatencyBound,
}

enum LatencyFact {
    NoAudioInput,
    Audio {
        aggregate: LatencyInterval,
        per_input: BTreeMap<i32, LatencyInterval>,
    },
}
```

`NoAudioInput` means the observable does not depend on any audio input. This
must stay distinct from `[0, 0]`, which means an immediate audio dependency.

The aggregate interval is computed over all reachable audio inputs:

- `min = min(per_input.min)`;
- `max = max(per_input.max)`;
- if any branch contains `Unknown`, the corresponding aggregate bound becomes
  `Unknown`;
- if any branch contains `Unbounded`, the aggregate maximum becomes
  `Unbounded`.

This domain is sufficient only inside one clock environment. The presence of
`ondemand`, `upsampling`, and `downsampling` means that "one sample" is local to
the signal's clock domain. The full analysis therefore needs a clock-aware
wrapper:

```rust
struct TimedLatencyFact {
    clock_env: ClockEnvId,
    fact: LatencyFact,
}

enum ClockScale {
    TopLevel,
    Upsampled { factor: LatencyInterval },
    Downsampled { factor: LatencyInterval },
    OnDemand { activation: ActivationLatency },
}
```

The public report should still be expressed in top-level audio samples, but
internal propagation must keep the clock environment until a domain boundary
converts it. Otherwise a `Delay1` inside an `upsampling`, `downsampling`, or
`ondemand` body would be reported in the wrong unit.

## Recommended Integration Point

The analysis should live in `crates/transform`, probably in a dedicated module:

```text
crates/transform/src/signal_latency.rs
```

Rationale:

- it consumes the `signals` graph after propagation/preparation, like
  `signal_fir` lowering;
- it needs `SigType`/interval annotations produced by
  `prepare_signals_for_fir` to bound `Delay(value, amount)`;
- it should consume the clock-domain analysis planned in
  `ondemand-clock-domains-analysis-port-plan-2026-06-10-en.md` before claiming
  exact results for `OD`/`US`/`DS` graphs;
- it should not pollute `signals`, which should remain the representation,
  builder, and matcher crate.

Target API:

```rust
pub struct ObservableLatency {
    pub kind: ObservableKind,
    pub signal: SigId,
    pub control: Option<ControlId>,
    pub latency: LatencyFact,
}

pub enum ObservableKind {
    AudioOutput { index: usize },
    Bargraph { orientation: BargraphOrientation },
}

pub fn analyze_observable_latencies(
    arena: &TreeArena,
    outputs: &[SigId],
    sig_types: &HashMap<SigId, SigType>,
) -> Result<Vec<ObservableLatency>, SignalLatencyError>;
```

`outputs` is the prepared DSP output list. Bargraphs are discovered by walking
those outputs because they are UI-effect signals whose value is lowered by
`lower_bargraph`.

## Reporting And User-Facing Access

Latency data should be exposed through two surfaces with different stability
contracts.

### Signal-Dump Companion

The first implementation should expose a debug/reporting mode attached to the
signal graph dump rather than changing normal compilation output. This keeps the
feature close to the IR it analyzes and avoids making unstable clock-domain
policy part of the user-facing compiler contract too early.

Possible CLI shapes:

```text
--dump-sig-latency
```

or, if the CLI already grows structured dump modes:

```text
--dump-sig --with-latency
--dump-sig=latency
```

Example text output:

```text
audio[0]:
  signal: SIGDELAY(SIGINPUT(0), 3)
  latency: aggregate [3, 3]
  per-input:
    input[0]: [3, 3]

bargraph#2 "/meter":
  latency: aggregate [1, 1]
  per-input:
    input[0]: [1, 1]
```

This surface is primarily for porting, compiler debugging, and golden-style
structural checks. It may include signal IDs, readable signal dumps, clock
environment annotations, and `Unknown`/diagnostic explanations.

### Stable Latency Report

Once the analysis is clock-aware and its semantics are stable, add a
user/tool-oriented report option:

```text
--latency-report=text
--latency-report=json
```

The text form should be concise and human-readable. The JSON form should be
stable enough for IDEs, documentation tools, CI, and regression tests.

Example JSON shape:

```json
{
  "audio_outputs": [
    {
      "index": 0,
      "latency": { "min": 3, "max": 3 },
      "per_input": { "0": { "min": 3, "max": 3 } }
    }
  ],
  "bargraphs": [
    {
      "control": 2,
      "path": "/meter",
      "latency": { "min": 1, "max": 1 }
    }
  ]
}
```

Do not silently append latency lines to the default `--dump-sig` output. Keep
the existing dump stable by default, and require an explicit latency mode or
report option when latency facts are requested.

## Latency Propagation Rules

### Sources

- `Input(i)` -> `Audio { per_input[i] = [0, 0] }`
- constants, waveform, fconst, fvar, UI input controls, and soundfile handles ->
  `NoAudioInput`, unless a separate soundfile policy is explicitly requested.
- `SoundfileBuffer(...)`: open decision. By default, treat it as an external
  non-audio-input source (`NoAudioInput`) for Faust audio input latency. If the
  desired scope includes file-to-output latency, add a second source domain.

### Time-Transparent Nodes

The following nodes do not change the latency of their data dependencies:

- casts;
- arithmetic and binary operators;
- math functions;
- `Output(_, x)`;
- `Attach(x, y)`, `Enable(x, y)`, `Control(x, y)` for data latency: the primary
  observable follows `x`, but `y` should still be walked when discovering nested
  bargraphs or detecting control dependencies.

For a node with several data inputs, merge facts:

- `NoAudioInput + X = X`;
- `Audio + Audio` merges `per_input`;
- for the same input `i`, use `min = min(a.min, b.min)` and
  `max = max(a.max, b.max)`.

### Delays

- `Delay1(x)` adds exactly `1` tick in the current clock environment to every
  bound of `x`.
- `Delay(x, amount)` adds the interval of `amount`.
  - if `amount` is an integer constant `k >= 0`: add `[k, k]`;
  - otherwise use `SigType`/interval data to obtain `[lo, hi]`;
  - if `lo < 0`: return an error or diagnostic, because negative delay is not a
    valid causal latency;
  - if `hi` is unbounded: `max = Unbounded`;
  - if the interval is absent or not integer-valued: `Unknown`.

The analysis must reuse the same source of truth as FIR delay-line sizing, so
variable-delay latency and variable-delay allocation cannot silently diverge.

When the current clock environment is not the top-level audio domain, delay
bounds must remain tagged with that environment until the analysis crosses the
corresponding `Clocked`/`TempVar`/`PermVar`/`Seq`/`OD`/`US`/`DS` boundary.

### Prefix

`Prefix(init, x)` is a C++ parity point to confirm before implementation. For
steady-state latency analysis, the expected rule is probably:

- `init` only contributes to the first sample;
- `x` contributes with `+1` sample.

The implementation plan must verify C++ `sigPrefix` behavior before freezing
this rule, then add a dedicated regression test.

### Selectors And Signal Control Flow

- `Select2(sel, a, b)` depends temporally on the selector and both branches:
  merge `sel`, `a`, and `b`.
- `AssertBounds(x, lo, hi)`, `Lowest(x)`, and `Highest(x)` follow `x`, unless
  the node encodes an explicit additional data dependency.
- `RdTbl(table, index)` and `WrTbl(...)`: scope decision required. For the first
  version, conservatively merge all visible data dependencies. Stateful table
  behavior must have a non-regression test before being claimed exact.

### Clock Domains: `ondemand`, `upsampling`, `downsampling`

`OD`/`US`/`DS` do not invalidate latency analysis, but they invalidate a
single-unit interpretation of `[min, max]`. The analysis must distinguish:

- latency in the inner execution domain;
- latency of the last held value observed in the parent domain;
- latency to the next fresh value after an input changes.

Rules to model:

- `Clocked(c, x)` annotates that `x` belongs to clock environment `c`; it must
  not be treated as time-transparent unless the current environment is also
  tracked.
- `TempVar(x)` snapshots a parent-domain value for use by an inner domain. It
  carries the parent-domain latency into the child, with no delay by itself.
- `PermVar(x)` is sample-and-hold state initialized to zero. Reads in the parent
  domain observe the last value produced by the child domain. This introduces a
  freshness distinction: the held value can be old even when the read is
  immediate.
- `Seq(block, y)` enforces execution order. Its value follows `y`, but the
  block side effect must be analyzed because it may refresh a `PermVar`.
- `ZeroPad(x, H)` used by `upsampling` injects the real parent sample only on a
  specific inner iteration and zeros elsewhere. This adds an inner-domain phase
  component that must not be collapsed into a plain scale factor.
- `US(H)` executes the child `H` times per parent tick. For constant `H`,
  child-domain delay can be converted to a rational parent-domain latency
  interval, but the observable parent output is still sample-and-hold.
- `DS(H)` executes the child every `H` parent ticks. For constant `H`, a child
  delay of `k` ticks contributes roughly `k * H` parent ticks plus trigger
  phase. For interval-valued `H`, the reported parent-domain latency must become
  an interval or `Unknown`.
- `OD(H)` executes conditionally. For a boolean or variable clock, freshness
  latency may be `Unbounded` if the block may never fire again. For a constant
  `H == 1`, propagation should already make the wrapper transparent; for
  `H == 0`, the output is the initialized/held value and normally has
  `NoAudioInput`.

The first implementation may explicitly reject or mark `OD`/`US`/`DS` as
`Unknown`, but exact analysis for those nodes requires the clock environment
inference described in `ondemand-clock-domains-analysis-port-plan-2026-06-10-en.md`.
It is incorrect to silently apply top-level delay rules inside a nested clock
domain.

### Recursion

Recursive groups must not be handled by naive recursive descent, otherwise the
analysis can either loop forever or underestimate latency.

Plan:

1. Detect `Rec`/`Proj` groups and their prepared symbolic forms
   (`SYMREC`/`SYMREF` through existing `tlib` helpers).
2. Build one latency equation per projection slot:
   `L[group.slot] = latency(body[slot])`.
3. While analyzing a body, a direct recursive reference
   `Proj(slot, SYMREF(group))` reads the current slot value; a reference under
   `Delay1^k` adds `k`.
4. Solve the equations with a monotone fixed point:
   - initialize each slot to `NoAudioInput`;
   - iterate until intervals stabilize;
   - detect zero-delay cycles that cannot converge to a finite causal latency
     and return `Unknown` or a structured diagnostic.
5. For delayed feedback cycles, preserve external input paths and accumulated
   delays; do not turn feedback itself into infinite latency when it does not
   create a new input-to-output path.

Minimum test coverage:

- `process = _`;
- `process = @(1)`;
- `process = _ <: @(1), @(3)`;
- direct bargraph: `process = _ <: _, hbargraph("x", 0, 1)`;
- simple recursion, for example `process = (+ : @(1)) ~ _` or an accepted
  equivalent;
- recursion with an external input and delayed feedback.

## Observable Discovery

Audio:

- each root `outputs[index]` produces an `ObservableKind::AudioOutput`;
- if the root is `Output(_, inner)`, analyze `inner` but keep the audio index
  from the position in `outputs`.

Bargraphs:

- walk all audio outputs and collect each `VBargraph(control, value)` /
  `HBargraph(control, value)`;
- analyze `value`;
- produce an `ObservableKind::Bargraph`;
- deduplicate by `ControlId` if the same bargraph is reached several times,
  either by merging latencies or by reporting a diagnostic if values diverge by
  context.

This discovery must be independent from FIR lowering. The current lowering
stores bargraph values into `sample_phases.immediate`; latency analysis should
therefore run before lowering, or use the same source signals, not reconstruct
the information from FIR.

## Differential Validation

There is probably no standard C++ output that directly prints these latencies.
Validation should therefore combine:

- structural Rust unit tests over `signals`;
- Faust fixtures compiled up to the signal graph;
- indirect differential impulse tests for audio outputs: compare the first
  non-zero frame with `min` on simple linear graphs;
- variable-delay tests with known intervals to verify `max`;
- bargraph tests through interpreter execution if the infrastructure can read
  bargraph zones after `compute`.

Differential tests should stay limited to cases where the first impulse response
sample is a reliable proxy. For nonlinear graphs or UI-controlled graphs,
structural tests are more robust.

## Implementation Passes

### Phase 0: Scope And Parity

- Identify the exact C++ functions for `sigDelay`, `sigDelay1`, `sigPrefix`,
  `sigRec`/`sigProj`, bargraphs, tables, and `OD`/`US`/`DS` clock-domain
  nodes.
- Confirm whether the analysis must be:
  - aggregated over all audio inputs;
  - detailed per audio input;
  - limited to audio inputs, or extended to UI/soundfile sources.
- Confirm the user-facing meaning of latency across `PermVar` sample-and-hold:
  held-value latency, next-fresh-value latency, or both.
- Document those decisions in this file before coding.

### Phase 1: Single-Clock Domain And Acyclic Analysis

- Add `transform::signal_latency`.
- Implement `LatencyBound`, `LatencyInterval`, and `LatencyFact`.
- Implement a memoized traversal with `match_sig`.
- Cover sources, transparent operations, math, casts, `Output`,
  `Attach`/`Enable`/`Control`, `Delay1`, and constant `Delay`.
- Reject or mark `Clocked`, `TempVar`, `PermVar`, `Seq`, `ZeroPad`, `OD`, `US`,
  and `DS` as `Unknown` until Phase 3. This keeps the first pass correct for
  single-clock graphs instead of silently reporting wrong top-level latencies.
- Add unit tests without recursion.

### Phase 2: Variable Delays And Intervals

- Connect the analysis to `PreparedSignals::sig_types_map`.
- Extract the integer interval of `amount` with the same assumptions as delay
  lowering.
- Add tests for variable `[min, max]`, `Unknown`, and `Unbounded`.

### Phase 3: Clock-Aware Latency Domain

- Add `ClockEnvId`/clock-domain metadata to internal latency facts.
- Consume or share the clock environment inference required by
  `ondemand-clock-domains-analysis-port-plan-2026-06-10-en.md`.
- Implement boundary rules for `Clocked`, `TempVar`, `PermVar`, `Seq`,
  `ZeroPad`, `OD`, `US`, and `DS`.
- Add constant-clock tests for transparent `OD`, upsampling, and downsampling.
- Add variable-clock tests that deliberately produce interval, `Unknown`, or
  `Unbounded` freshness latency.

### Phase 4: Bargraphs

- Add bargraph discovery in prepared outputs.
- Return latencies by `ControlId` and orientation.
- Add tests for direct bargraphs, delayed bargraphs, and bargraphs hidden in
  `attach`/`control` if propagation can produce those forms.
- Add at least one bargraph inside or after a clock-domain wrapper to prove the
  reported latency is converted back to the observable parent domain.

### Phase 5: Recursion

- Add a recursive group analyzer with equations per slot.
- Support `Delay1^k(Proj(...))` like the helpers in `signal_fir::recursion`.
- Include the clock environment in recursion equations, because recursive
  groups can be pulled into nested domains by clock inference.
- Add tests for convergence and non-causal cycles.

### Phase 6: Debug Dump Exposure

- Add a stable internal API for backends or diagnostics tools.
- Add a debug-facing signal latency dump, either as `--dump-sig-latency` or as
  an explicit enriched signal dump mode such as `--dump-sig --with-latency`.
- Include observable kind, signal identity/readable signal, aggregate latency,
  per-input latency, and `Unknown`/diagnostic reasons.
- Keep the default `--dump-sig` output unchanged unless a latency mode is
  explicitly requested.
- Optional: expose an `xtask` command for local development that prints the same
  facts without committing to a stable CLI contract:

```text
audio[0]: input aggregate [0, 3], per-input {0: [0, 3]}
bargraph#2 "/meter": input aggregate [1, 1], per-input {0: [1, 1]}
```

The printed format must use portable control paths and must not depend on local
absolute checkout paths.

### Phase 7: Stable Report Option

- Add a user/tool-facing `--latency-report=text|json` option once the analysis
  handles clock-domain wrappers well enough for stable reporting.
- Define a stable JSON schema for audio outputs, bargraphs, aggregate latency,
  per-input latency, clock-domain diagnostics, and unresolved cases.
- Add golden/report tests for both text and JSON output.
- Decide whether latency reports participate in the default golden gates or stay
  behind an explicit diagnostic/reporting gate.

## Risks And Open Decisions

- Exact `Prefix` semantics: confirm against C++ before implementation.
- Tables and soundfiles: define whether they are out of scope, external sources,
  or first-class temporal dependencies.
- Unbounded variable delays: choose between hard diagnostic, `Unknown`, or
  `Unbounded` based on the intended user.
- Clock-domain wrappers: decide whether reports expose only top-level converted
  latency or also internal-domain latency for debugging.
- `PermVar` sample-and-hold: distinguish held-value latency from next-fresh
  latency, especially for `ondemand` clocks that may stop firing.
- CLI/reporting surface: keep `--dump-sig` stable by default, then decide the
  final spelling for debug latency dumps and `--latency-report=text|json`.
- Zero-delay recursion: do not hide a non-causal loop behind an arbitrary
  latency value.
- Deduplicated bargraphs: confirm whether one `ControlId` can be written by
  multiple distinct signals and which merge policy is C++ compatible.

## Readiness Criteria

The work is ready when:

- every audio output and every reachable bargraph has a `LatencyFact`;
- no-audio-input cases are explicitly represented by `NoAudioInput`;
- constant and bounded variable delays produce exact bounds;
- nested clock-domain delays are either converted through a documented
  clock-aware rule or explicitly reported as `Unknown`/diagnostic;
- unresolved cases produce `Unknown` or a structured diagnostic, never a
  silently false value;
- latency facts are exposed first through an explicit signal-dump companion and
  later through a stable `--latency-report=text|json` surface;
- tests cover audio outputs, bargraphs, delays, variable delays, clock-domain
  wrappers, and recursion;
- the usual gates pass:
  - `cargo fmt --all`;
  - `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo test --workspace --all-targets`.
