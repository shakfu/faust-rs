# Current Faust Source Subset Supported by `faust-rs`

Last updated: 2026-03-21

Status: living document

## 1. Purpose

This document records, in one place, what subset of Faust programs currently
compiles with `faust-rs`, how that differs from the C++ compiler, and why.

It is intentionally written as a **living status document**:

- it should be updated when the supported subset grows or shrinks,
- it should stay tied to executable evidence,
- it should distinguish clearly between front-end support and end-to-end backend
  support.

## 2. What "compiles" means here

There are two materially different questions:

1. **Front-end compilation**
   - `parse -> eval -> propagate -> signals`
   - measured here by `compiler::Compiler::compile_file_default_to_signals`
2. **End-to-end backend compilation**
   - `parse -> eval -> propagate -> signal_prepare -> signal_fir -> C/C++ backend`
   - measured here by the current fast-lane route
     `compiler::SignalFirLane::TransformFastLane`

This distinction matters because the front-end language surface is now much
broader than the active signal-to-FIR lowering slice.

## 3. Evidence Used for This Snapshot

This snapshot is based on:

- local history and porting notes under `porting/journal/`
- current implementation status in:
  - `crates/parser`
  - `crates/eval`
  - `crates/propagate`
  - `crates/transform/src/signal_prepare.rs`
  - `crates/transform/src/signal_fir/mod.rs`
  - `crates/transform/src/signal_fir/module.rs`
- fresh local status scans run on 2026-03-14 (last formal run; counts below reflect
  subsequent journal evidence — re-run required for precise current numbers):
  - `cargo run -p xtask -- corpus-status-report`
  - `cargo run -p xtask -- backend-full-corpus-diff-report`

The corresponding generated reports are:

- [phase-4-corpus-status-diff-report-en.md](./phases/phase-4-corpus-status-diff-report-en.md)
- [phase-6-backend-full-corpus-diff-report-en.md](./phases/phase-6-backend-full-corpus-diff-report-en.md)

## 4. Executive Summary

### 4.1 Source-language status

At the front-end level, the active corpus currently shows **full status parity**
with the C++ compiler:

- total corpus cases: `88`
- valid cases accepted by both Rust and C++: `73`
- invalid cases rejected by both Rust and C++: `15`
- `OK/ERR` mismatches: `0`
- `ERR/OK` mismatches: `0`

In other words:

- on the current tracked corpus, `faust-rs` now accepts the same valid source
  programs as Faust C++ up to the `signals` boundary,
- and it rejects the same invalid corpus programs.

### 4.2 End-to-end backend status

At the current backend route (`TransformFastLane`), the supported subset is
still narrower:

- total corpus cases: `88`
- end-to-end C backend parity: `OK=72`, `DIFF=0`, `UNSUPPORTED=16`
- end-to-end C++ backend parity: `OK=72`, `DIFF=0`, `UNSUPPORTED=16`

The `16` unsupported cases are:

- `15` corpus entries that are intentionally invalid Faust programs,
- `1` remaining valid corpus case:
  - `rep_18_stream_wrappers.dsp`

So for **valid** corpus programs, the current backend route compiles:

- `72 / 73` valid corpus cases,
- and misses `1 / 73`, currently in the non-trivial stream-wrapper family.

## 5. Synthetic Characterization of the Supported Subset

## 5.1 Source-level Faust subset currently supported

As of this snapshot, `faust-rs` front-end support is no longer a tiny parser
prototype subset. On the tracked corpus, it includes:

- standard arithmetic and signal composition forms,
- local definitions and `with` scopes,
- recursive definitions / `letrec`,
- lambda abstraction and application,
- `case` / pattern-based evaluation paths,
- metadata and `declare`,
- local-file imports,
- UI widgets and UI groups,
- modulation-bearing source programs,
- waveform and table forms,
- representative feedback and delay programs,
- label interpolation cases used by modulation/UI paths,
- representative noise/additive-synthesis fixtures.

Stated differently:

- the current front-end subset is best described as **"the current tracked Faust
  corpus"**, not as a small manually enumerated syntax fragment.
- the main residual front-end caveat is not basic language acceptance anymore,
  but long-tail parity outside the exercised corpus and some deferred tooling
  concerns such as remote-import parity.

## 5.2 End-to-end backend subset currently supported

The backend subset is narrower and is better characterized structurally:

A Faust program currently compiles end-to-end when its post-eval/post-propagate
signal forest stays within the active `signal_prepare + signal_fir` lowering
slice.

That slice currently includes, in broad terms:

- numeric constants and audio inputs/outputs,
- arithmetic, comparison, bitwise, and selected math operators,
- `select2`, `min`, `max`, `abs`,
- `delay1`, `prefix`, and recursive feedback forms — including
  **multi-output recursion groups** (`SIGPROJ index > 0`),
- `SIGDELAY` with:
  - constant integer amounts (fixed-size circular buffer),
  - **variable amounts with a bounded non-negative interval** — the buffer is
    allocated to `next_power_of_two(max + 1)` using the interval upper bound
    from `crates/sigtype`.  This covers:
    - UI parameters (slider, numentry) with explicit `[lo, hi]` bounds,
    - audio-rate signals (`Variability::Samp`) whose interval is bounded and
      non-negative (e.g. `@(int(_+10))` where `int(_+10) ∈ [9, 11]`),
    - expressions derived from `ma.SR` (sampling rate), which carries the
      open interval `[f64::MIN, f64::MAX]` — clamped to a finite range by the
      surrounding `min(192000, max(1, …))` algebra,
  - structural fallback: when interval analysis fails to yield a finite bound
    but the amount expression is `SIGMIN(SigInt(n), _)`, `n` is used as a
    conservative ceiling (defence-in-depth, not normally reached with correct
    `FConst` typing),
- waveform/table forms:
  - `SIGWAVEFORM`
  - `SIGRDTBL`
  - `SIGWRTBL`
- UI widget families:
  - buttons
  - checkboxes
  - sliders
  - bargraphs
- control-style wrappers already mapped in the fast-lane:
  - `attach`
  - `enable`
  - `control`
- foreign constants (`fconstant`) and foreign variables (`fvariable`), typed
  with the fully-open interval `[f64::MIN, f64::MAX]` — matching the C++
  default-constructed `interval()`.

This is why corpus cases such as:

- `rep_04_delay_echo`
- `rep_06_comb_feedback`
- `rep_20_environment_waveform`
- `rep_35_table_rwtable_runtime_write`
- `rep_46_case_pattern_constant_folding`
- `rep_55_sine_phasor_echo_feedback`
- `rep_56_noise_smoo_slider`
- `rep_65_variable_delay_slider`
- `rep_66_variable_delay_feedback`
- `rep_67_variable_delay_shifted_slider`
- `rep_68_variable_delay_audio_rate`
- `rep_69_variable_delay_sr_millisec`
- `rep_70_route_arithmetic_params`

now compile end-to-end through the Rust backends.

## 5.3 Important current backend exclusions

The backend subset is **not** "all front-end-accepted Faust programs".

The most important current exclusions are:

- **Variable delays with no statically bounded interval**
  - `SIGDELAY` whose amount expression has an unbounded interval (i.e., the
    interval upper bound is infinite or indeterminate) is rejected.
  - This is narrower than the previous `Variability::Samp` blanket rejection:
    audio-rate amounts are now accepted when their interval is provably bounded
    and non-negative.
  - The one remaining case that cannot be sized: a pure audio input used
    directly as a delay amount (interval `[-1, 1]`, hi = 1) — accepted and
    allocated to 2 samples.  An audio signal with no structural bound and no
    type constraint still cannot be sized statically.

- **Non-trivial stream wrappers**
  - the current valid backend corpus gap is
    `tests/corpus/rep_18_stream_wrappers.dsp`
  - trivial wrappers such as `inputs(_)` / `outputs(_)` are covered,
    but the full `ondemand(_)`, `upsampling(_)`, `downsampling(_)` family is
    not yet fully lowered end-to-end.

- **Complex table generators in `SIGGEN`**
  - the fast-lane now handles a broad class of computed `SIGGEN` generators
    via the compile-time `GeneratorInterpreter` (oscillator sine tables,
    recursive phasor-driven tables, etc.),
  - the remaining gap is `tabulateNd`-style generators whose table-size
    expression is non-constant arithmetic (e.g. `8*8` → `BinOp(Mul, Int(8), Int(8))`):
    the signal-level constant-folding pass (`normalize/simplify`) is now wired
    into `signal_prepare`, but the table-size extractor still requires a literal
    `Int` node.  See `porting/missing.md` entry 1.

- **Unproven long-tail families outside the tracked corpus**
  - even when a node family exists in Rust, if it is not exercised by the
    current corpus and differential tests, it should not yet be described as
    parity-complete.

## 6. Comparison with Faust C++

## 6.1 Where Rust is now close to C++

Relative to the tracked corpus and the current production-oriented route:

- front-end acceptance is now effectively at corpus parity,
- C and C++ backend shell signatures match on all currently supported backend
  cases,
- many language families that were earlier missing are no longer front-end
  blockers:
  - `case` and recursive pattern-matching with computed numeric arguments
  - lambda/closure forms (first-class `boxClosure` node)
  - modulation
  - local imports
  - metadata
- multi-output recursion groups (`SIGPROJ index > 0`) are now supported,
  enabling `freeverb`, `Birds.dsp`, and any `~ _`-based multi-output feedback,
- the algebraic `simplify` pass from `normalize/` is now wired into the
  `signal_prepare` pipeline, folding constant arithmetic before FIR lowering,
- computed `SIGGEN` table generators are now evaluated at compile time by
  `GeneratorInterpreter`, enabling `os.osc`, `os.saw`, and other
  oscillator-table patterns,
- Variable delays driven by a UI slider, numentry, **or bounded audio-rate
  expression** now compile end-to-end — that restriction has been significantly
  relaxed,
- the full C++ interval algebra is now available in Rust through
  `crates/interval`, and the signal type lattice is fully modeled in
  `crates/sigtype` with correct parity including `FConst`/`FVar` intervals.

## 6.2 Where C++ is still broader

Faust C++ still supports a broader end-to-end language/runtime envelope.

Most importantly, the C++ compiler still has:

- fuller support for stream-wrapper lowering,
- broader mature transform/backend coverage on long-tail signal families,
- the `simplify` constant-folding pass applied to table-size positions
  (the Rust pipeline now runs `simplify` in `signal_prepare`, but the
  table-size extractor in the FIR builder still requires a literal `Int` node;
  multi-dimensional `tabulateNd` is therefore still unsupported),
- the historical production path beyond the active Rust fast-lane slice.

The variable-delay gap is now **substantially closed**:

- **C++**:
  - accepts `delay(x, d)` when the delay amount has a valid, bounded,
    non-negative interval; sizes the ring buffer from the interval upper bound
  - C++ `occMarkup` computes the max delay in a dedicated pre-pass
    (`incOcc`/`getMaxDelay`), with `checkDelayInterval` called there
  - `gCausality = true` in production (`compile_scal` calls
    `typeAnnotation(L2, true)`): invalid delay intervals are already caught
    during type annotation

- **Rust (as of 2026-03-14)**:
  - accepts constant integer delays (unchanged),
  - accepts variable delays whenever `check_delay_interval` succeeds:
    bounded non-negative interval, regardless of variability,
  - `scan_delay_lines` is the Rust equivalent of the C++ `occMarkup` pre-pass;
    `delay_size_for_amount` replaces `checkDelayInterval(getCertifiedSigType(y))`,
    driven by the same interval algebra,
  - `min_const_upper_bound` provides structural safety for any residual case
    where interval analysis yields an unbounded result despite a `SIGMIN(n, …)`
    ceiling in the expression.

The architectural difference (inline sizing vs. separate `occMarkup` pass) is
cosmetic: both derive the delay line size from the same interval algebra and
produce the same result.  The only currently unimplemented C++ feature in this
area is the `fOutDelayOcc` / `fCountDelay` optimisation that avoids allocating
a delay vector when a signal is only read at offset zero — a code-quality
difference, not a correctness gap.

### Key parity fix (2026-03-14): `FConst`/`FVar` interval

A subtle parity bug was identified and corrected: the C++ `interval()` default
constructor uses **member initializers**:

```cpp
// compiler/interval/interval_def.hh
double fLo{std::numeric_limits<double>::lowest()};  // f64::MIN
double fHi{std::numeric_limits<double>::max()};     // f64::MAX
int    fLSB{-24};
```

This means `interval()` in C++ = `[f64::MIN, f64::MAX]` (fully open),
not `[NaN, NaN]` (empty).  The Rust `inferFConstType` / `inferFVarType`
had incorrectly used `interval::empty()` after a wrong parity audit read it
as `(NaN, NaN)`.  With `Interval::new_default()` restored, expressions like
`ma.SR = min(192000, max(1, fSamplingFreq))` correctly produce a finite
interval `[1.0, 192000.0]`, which then propagates through delay arithmetic
to yield a bounded delay amount — exactly as in C++.

## 7. Historical Progression

The current shape of the subset follows a clear history.

### 7.1 February 17, 2026: fast-lane bootstrap and first executable FIR slices

The large `signalFIRCompiler` fast-lane rollout on 2026-02-17 established the
first real backend subset:

- Step 2A: inputs/constants/binops/output passthrough
- Step 2B: math + control/state bootstrap
- Step 2C: first stateful slice
- Step 2C.2: recursion support
- Step 2D..2H: breadth coverage, table lowering, non-trivial table coverage

At that point the backend subset was real, but still much narrower than the
front-end language.

### 7.2 February 28, 2026: parser parity became operational but not yet "closed"

The parser parity audit documented a strong but still not fully closed parser
state:

- production entrypoint wired,
- wide grammar coverage,
- good differential guardrails,
- but residual architecture and diagnostic-fidelity gaps remained.

This matters because, historically, the supported source subset was initially
limited by parser/eval work before backend lowering became the dominant gap.

### 7.3 March 7, 2026: parser / pattern / eval closure

By 2026-03-07, the parser/pattern/eval area was documented as having no known
remaining parity gap in its tracked scope.

This is the key point where the limiting factor stopped being "can Rust read
this Faust source?" and became "can the backend lower the resulting signal
graph end-to-end?"

### 7.4 March 9, 2026: signal preparation and fixed-delay fast-lane maturity

The 2026-03-09 work introduced the current pre-FIR preparation boundary:

- whole-forest staging after `de_bruijn_to_sym`,
- reduced simple typing,
- reduced signal promotion,
- typed delay/recursion/table handling,
- fixed-size circular delay lines for constant `SIGDELAY`.

This was also the point where the project explicitly chose to:

- support constant fixed delays first,
- reject variable delays explicitly,
- defer interval-driven variable-delay parity to later work.

### 7.5 March 10–12, 2026: remaining valid backend gap shrank to a very small tail

Subsequent work tightened integer typing, recursive state reuse, noise/sample
rate cases, UI pathname normalisation, relative group label rebasing,
class-name CLI parity, and other backend details.

By the 2026-03-12 snapshot:

- the front-end corpus mismatch count was `0` (`75` cases, `60` valid),
- the backend valid-case gap had shrunk to one tracked corpus case:
  `rep_18_stream_wrappers.dsp`.

### 7.6 March 13, 2026: interval algebra, signal type lattice, variable delay

Three interlinked architectural layers landed on 2026-03-13:

#### `crates/interval` — full C++ interval library ported

The complete interval arithmetic library from `compiler/interval/` was ported
to a native Rust crate:

- `Interval { lo: f64, hi: f64, lsb: i32 }` value type with IEEE-aware
  bounds arithmetic,
- all four arithmetic operators, comparisons, math functions
  (`sin`, `cos`, `exp`, `log`, `pow`, …), cast helpers, and UI constructors
  (`vslider`, `hslider`, `nentry`, `button`, `checkbox`, `bargraph`),
- `saturated_int_cast` for safe conversion to delay-line sizes,
- 62 unit tests all passing.

A precision overflow in `ipow_scalar` (`lsb * k` → `lsb.saturating_mul(k)`)
was also fixed during this session; the fix prevents a panic when a signal
with an extreme interval (e.g., audio input placeholder `[f64::MIN, f64::MAX]`)
is raised to a power.

#### `crates/sigtype` — full C++ signal type system ported

The complete `AudioType` class hierarchy from `compiler/signals/sigtype.hh/cpp`
and the inference rules from `sigtyperules.cpp` were ported to a new Rust
crate:

- `SigType` enum: `Simple(SimpleType) | Table(TableType) | Tuplet(TupletType)`,
  each carrying `fInterval : Interval` directly as a member (faithful to C++),
- lattice enums with bitwise-OR join semantics: `Nature`, `Variability`,
  `Computability`, `Vectorability`, `Boolean`,
- `TypeAnnotator`: bottom-up memoized inference with a fixed-point loop for
  recursive types (seeded with `make_maximal()` = Real/Samp/Exec),
- full merge/cast/check helpers (`union_types`, `int_cast`, `float_cast`,
  `check_delay_interval`, …),
- `PreparedSignals` extended with a dual `sig_types: HashMap<SigId, SigType>`
  map so FIR lowering can access full interval data without breaking existing
  callers,
- 32 unit tests all passing (27 initial + 5 added during parity audit).

#### Variable delay support via interval upper bound

With the interval contract in place, `SIGDELAY` now accepts UI-parameterized
amounts:

- `delay_size_for_amount(sig)` tries (in order): constant literal, interval
  upper bound from `sig_types`, structural `SIGMIN(n, _)` ceiling,
- the delay line is allocated to `next_power_of_two(hi + 1)`, matching C++,
- the runtime read index uses `(fIOTA - amount) & mask` where `amount` is the
  lowered amount signal evaluated each sample — identical to C++ circular
  buffer semantics.

This raised the end-to-end backend corpus score from `59 / 60` to `68 / 69`
valid cases, with the same single remaining gap (`rep_18_stream_wrappers.dsp`).

### 7.7 March 14, 2026: audio-rate delays, FConst parity fix, echo.dsp

Three further improvements landed on 2026-03-14:

#### Audio-rate delay amounts now accepted when bounded

The `variable_delay_max_bound` function previously rejected all
`Variability::Samp` delay amounts via a blanket guard.  That guard was a
workaround from before `TINPUT` carried the correct interval `[-1, 1]`.  With
`TINPUT` properly typed, audio-rate expressions such as `int(_+10)` produce
interval `[9, 11]` — bounded and non-negative — and are now accepted.

The acceptance criterion is now purely interval-based: any delay amount whose
type satisfies `check_delay_interval` (bounded, `hi ≥ 0`) compiles, regardless
of variability.  This is consistent with the C++ `checkDelayInterval` contract.

#### `FConst` / `FVar` interval parity fix

The 2026-03-13 sigtype parity audit had introduced a wrong fix: it changed
`FConst` and `FVar` intervals from `Interval::new_default()` to
`interval::empty()`, citing "C++: `interval()` = NaN, NaN".

Investigation of the actual C++ source (`interval_def.hh`) revealed that the
C++ `interval()` default constructor uses member initializers:

```cpp
double fLo{std::numeric_limits<double>::lowest()};  // NOT NaN
double fHi{std::numeric_limits<double>::max()};     // NOT NaN
```

`interval()` = `[f64::MIN, f64::MAX]` — the fully-open interval, identical to
Rust `Interval::new_default()`.  With `empty()` erroneously applied to
`fSamplingFreq`, the interval algebra propagated `empty()` through
`min(192000, max(1, fSamplingFreq))`, poisoning any delay amount derived from
`ma.SR` (e.g. `ef.echo1s`, `de.delay(n, hslider * ma.SR / 1000)`).

With `Interval::new_default()` restored, the correct interval chain is:

```
fSamplingFreq          ∈ [MIN, MAX]
max(1.0, …)            ∈ [1.0, MAX]
min(192000, …)         ∈ [1.0, 192000]   ← interval algebra clamps at 192000
hslider[0,1000] * …   ∈ [0, 192000]
int_cast(…) − 1        ∈ [−1, 191999]
max(0, …)              ∈ [0, 191999]
min(65536, …)          ∈ [0, 65536]      ← check_delay_interval returns 65536
delay line size        = next_pow2(65537) = 131072
```

This matches the C++ output for `echo.dsp` (`fRec0[131072]`, mask `131071`)
exactly.

#### Structural delay fallback (`min_const_upper_bound`)

A defence-in-depth fallback was added to `delay_size_for_amount`: if the
interval path yields no finite bound, but the delay amount expression is
structurally `SIGMIN(SigInt(n), _)` (as produced by `de.delay(n, d, x)`),
`n` is used as a conservative ceiling.  With correct `FConst` typing this path
is not reached for `ma.SR`-based patterns, but it guards against any future
case where interval analysis yields an unbounded or empty result.

#### Corpus additions

Three new corpus entries:

- `rep_67_variable_delay_shifted_slider`: slider shifted into `[-200, -100]`
  — must be rejected (hi < 0).
- `rep_68_variable_delay_audio_rate`: `process = @(int(_+10))` — audio-rate
  amount with bounded interval `[9, 11]`, accepted.
- `rep_69_variable_delay_sr_millisec`: `de.delay(65536, hslider * ma.SR / 1000)`
  — exercises the `ma.SR` interval chain, accepted.

End-to-end backend corpus: `71 / 72` valid cases, same single gap.

### 7.8 March 14, 2026 (continued): eval×normalize parity — constant folding at eval time

Six C++ call sites (`CS-1` through `CS-6`) that invoke `boxPropagateSig` +
`simplify` in the C++ evaluator (`eval.cpp`) were ported to Rust.  The work
connects `normalize::simplify_const` into `crates/eval` so that the Rust
evaluator folds constants at the same points as C++.

#### Background: C++ `boxPropagateSig` + `simplify` pattern

In C++ `eval.cpp`, several evaluation paths finish with:

```cpp
// e.g. in evalSeq, evalRoute, evalWidgetParams …
Tree sig = boxPropagateSig(sig_env, box, {});   // 0 inputs → extract scalar
return simplify(sig);                            // algebraic simplification
```

The Rust equivalent is `propagate_box_and_simplify(arena, box_id)`: builds a
flat box, propagates with 0 inputs, then calls `simplify_const` (which calls
`normalize::simplify` with an empty type map).

#### CS-1 / CS-7 — `BoxSeq` numeric folding

When both sides of a `BoxSeq` are fully numeric (integer/real literals or
parallel compositions thereof), `try_fold_seq_numeric` propagates and simplifies
the whole expression into a single scalar node.  This matches C++ `evalSeq`
which short-circuits to `boxPropagateSig` when `isNumericalTuple(e1)`.

Helper: `is_numerical_tuple_box` recursively checks that a box is a Par-spine
of Int/Real leaves, matching C++ `isNumericalTuple`.

#### CS-2 — Route arithmetic parameter normalization

`BoxMatch::Route` now has a dedicated arm in `eval_value` that evaluates all
three children (`ins`, `outs`, `routes`) with `eval_box` and then:

- converts `ins`/`outs` to a literal `boxInt(n)` via `eval_box_to_int_node`,
- normalises the route specification through `normalize_route_spec`:
  flattens the Par spine, reduces each leaf to a `boxInt`, and rebuilds a
  right-spine Par.

This matches C++ `evalRoute` which calls `normalizeRouteList` and
`boxPropagateSig` for `ins`/`outs`.  It fixes the
`PropagateError::InvalidIntegerValue` that occurred when `ins`/`outs` were
arithmetic expressions (e.g. `1+1`) rather than literal integers.

#### CS-4, CS-5, CS-6 — UI widget parameter reduction

`eval_slider_like`, `eval_vbargraph`, `eval_hbargraph`, and `eval_soundfile`
now reduce each numeric parameter (cur/min/max/step, chan) with
`simplify_slider_param`:

- evaluate the parameter expression with `eval_box`,
- if reducible to a floating-point constant, replace with `boxReal(x)`,
- otherwise keep as-is.

This matches C++ `evalSlider` / `evalBargraph` / `evalSoundfile` which call
`boxPropagateSig` on each parameter before embedding it in the UI node.

#### Box simplification family

A general memoised pass `box_simplification` was added:

- `numeric_box_simplification`: for any box that is not already a literal,
  tries `propagate_box_and_simplify`; if it reduces to a scalar Int/Real,
  rebuilds a `boxInt`/`boxReal`; otherwise recurses structurally via
  `inside_box_simplification`.
- `inside_box_simplification`: recursively simplifies all children, then
  rebuilds the same node kind.

This corresponds to C++ `simplifyToNormalForm` wired inside `propagateSignals`
at the box level.

#### `normalize::simplify_const` public API

`crates/normalize/src/simplify.rs` exposes `simplify_const(arena, sig)` as a
public entry-point: runs the full `simplify` rewriter with an empty type-map
cache, sufficient for compile-time constant folding without requiring a
`TypeAnnotator` pass.

#### Corpus addition

- `rep_70_route_arithmetic_params`: a `route(2, 2, 1+0, 1, 1+1, 2)` program
  that requires route spec normalisation to compile correctly.

#### Test coverage

26 unit tests were added to `crates/eval/src/lib.rs` in
`simplify_helpers_tests`, covering:
- `propagate_box_and_simplify` for constants and signal expressions,
- `is_numerical_tuple_box` for various box shapes,
- `try_fold_seq_numeric` for Int/Real Seq folding,
- `eval_box_to_f64` / `eval_box_to_i32` for literal extraction,
- `box_simplification` for compound boxes.

End-to-end backend corpus: `72 / 73` valid cases, same single gap.

### 7.9 March 15, 2026: SIGGEN interpreter, multi-output recursion, pattern-matcher parity

A cluster of significant features landed together:

#### Compile-time `GeneratorInterpreter` for SIGGEN

Previously, `expand_generator_values` could only handle constant/waveform
generators.  A computed generator such as the recursive phasor `+(1)~_`
inside `sin()` used by `os.osc` was rejected.  A full compile-time signal
interpreter (`GeneratorInterpreter`) was added: it evaluates 0-input
deterministic signal trees step-by-step at compile time, covering all
arithmetic operators, math functions, casts, `Delay1`, and symbolic
recursion.  `os.osc(440)`, `os.saw`, and similar oscillator patterns now
compile to correct 65536-entry sine tables.

#### Multi-output recursion groups

The FIR lowerer only handled single-output recursion (`SIGPROJ index 0`).
`freeverb.dsp`, `Birds.dsp`, and any DSP using the `~ _` feedback pattern
with 2-output groups failed.  `lower_proj`, `active_recursion_info`, and
supporting helpers were extended to handle all projection indices.

#### `boxClosure` first-class closure node

Closures were previously host-side only (`EvalValue::Closure`).  A new
`boxClosure` tree node carries the closure through the box evaluation chain,
replacing the earlier `boxSlot + substitute_tree` workaround.  This fixes
scoping for higher-order functions in `aanl.lib` and related patterns.

#### Pattern matcher: `match_num` simplification

`poly(max(1, min(6, 4)))` and similar computed numeric arguments previously
failed pattern matching because `max(1, min(6,4)) ≠ Int(4)` by tree identity.
`simplify_pattern` now reduces computed arguments to literals at match time,
matching C++ `simplifyPattern`.  A `boxPatternMatcher` side-table prevents
stack overflow in partially-applied case expressions.

#### `sigtype`: cons-list recursion bodies typed

The type annotator did not recurse into cons-list nodes, leaving all signals
inside `SYMREC` bodies without type information.  `delays.dsp` failed because
the `nentry("d1", 200, 0, 1000, 1)` interval was never propagated.  Fixed by
adding a cons-list → `TupletType` path in `infer_inner`.

#### `par(i, 0, X)` fixed

`iterate_par(n=0)` now returns the empty block (0→0) instead of `X`.

### 7.10 March 16, 2026: `simplify` wired into pipeline, waveform lowering overhaul

#### `signal_prepare`: algebraic simplification pass

`normalize::simplify` is now called inside `prepare_signals_for_fir`, between
full-type annotation and FIR lowering — matching the C++ pipeline order.  This
folds constant arithmetic (`max(0.25, 0.5)` → `Real(0.5)`, `IntCast(Real(1.5))`
→ `Int(1)`, `Delay1(x)` → `Delay(x, Int(1))`) at the signal level before any
backend sees the tree.  Both type maps are recomputed after simplification.

**Note**: the `simplify` pass addresses most constant-folding gaps, but does
not yet fold arithmetic in table-size positions (`BinOp(Mul, Int(8), Int(8))`
is still not reduced to `Int(64)` when used as a `SIGWRTBL` size argument).
This is documented in `porting/missing.md`.

#### Waveform lowering overhaul (three bugs fixed)

- **Cycling index**: bare `SIGWAVEFORM` outputs now advance with
  `(iWave + 1) % N` per sample; previously read `table[0]` every sample.
- **File-scope static tables**: waveform and rdtable-style tables are now
  emitted as `const static` arrays at file scope, matching the reference
  compiler; previously emitted as struct members with a fill loop.
- **Interval-aware bounds**: `SIGRDTBL` reads with a provably in-bounds index
  no longer emit redundant clamping — direct array access, matching C++.

### 7.11 March 17, 2026: circular buffers for delay1/recursion, select2 fix, CLI parity

#### Circular buffers with `fIOTA` for delay1 and recursion

The fast-lane previously used a scalar state + deferred `compute_updates` shift
for delay1 and recursion.  This caused ordering bugs when depth-first lowering
pushed inner updates before outer ones (e.g. `APF.dsp` gave wrong output).
All delays — SIGDELAY, delay1, and recursion — now uniformly use 2-element
circular buffers indexed by `fIOTA`, matching the C++ `writeReadDelay`
pattern.  The topological sort (`sort_compute_updates`) was removed as no
longer needed.

#### `select2` branch inversion fixed

`select2(sel, x, y)` was emitting `sel ? x : y` instead of `sel ? y : x`.
Fixed by swapping the destructuring of `(then, else)` arms.

#### Two-pass delay-line scan

`prepare_delay_lines` now does a collect pass (max per carried signal) then an
allocate pass, preventing the sizing-mismatch error for signals carried at
multiple different delay amounts (e.g. `frenchBell.dsp` modal synthesis).

#### CLI: `-double`, default class name `mydsp`, wrapping integer literals

- `-double` legacy flag recognized,
- default generated class/struct name is now `mydsp` (matching C++),
- integer literals that overflow 32 bits now use wrapping arithmetic (matching
  C++ `str2int` behavior).

### 7.12 March 18–21, 2026: eval correctness, signal-prepare canonicalization, physical models

#### Unary recursion projection canonicalization (`signal_prepare`)

After `de_bruijn_to_sym`, some symbolic recursion groups have physical arity 1
but are referenced via non-zero logical projection indices.  A canonicalization
step in `prepare_signals_for_fir` rewrites these to `proj(0, group)`, enabling
`zita_rev1_stereo`, `Birds.dsp`, and similar DSPs to compile.

#### Eval correctness fixes

- `a2sb` memoization boundaries tightened: residual sharing preserved.
- Residual closure arity computed correctly in non-closure application paths.
- Exact integer reals (e.g. `Real(2.0)`) canonicalized to `Int(2)` for
  case-matching parity with C++.
- Route-spec normalization aligned with C++ for computed `ins`/`outs`
  arguments.

#### Physical model fixes (2026-03-21)

- `lower_proj` spurious store eliminated: when a body signal was already
  scheduled as a state update by `lower_delay_state`, `lower_proj` no longer
  emits a second overwriting store.  Fixes `clarinetModel` and similar
  waveguide DSPs outputting all zeros.
- `GeneratorInterpreter::eval_delay1` now correctly advances recursive state
  on each step.
- `slot_env` de Bruijn lifting in `FlatNodeKind::Rec` corrected, fixing
  incorrect sample output for `spectralCentroid` and similar recursive analyzers.
- `eval` `FIR` shift binops added to backends.

## 8. Practical Reading Rule

Today, the simplest accurate rule is:

- If a Faust program stays within the language families already exercised by
  the tracked corpus, `faust-rs` front-end support is likely good.
- If that program also lowers to the current fast-lane signal subset
  (no non-trivial stream wrappers), end-to-end C/C++ generation is likely
  to work.
- Variable delays driven by a UI slider, numentry, audio-rate expression, or
  any expression with a provably bounded non-negative interval **are** now
  supported end-to-end.  This includes `de.delay(n, d)` patterns using
  `ma.SR` (sampling rate).
- The only variable-delay case still blocked is one where the delay amount
  has no statically determinable finite upper bound.

The most common mistake is to assume:

- "front-end accepted" means "backend supported".

That is no longer true as a general rule.

## 9. Update Policy for This Document

When this document is updated, the update should:

1. rerun:
   - `cargo run -p xtask -- corpus-status-report`
   - `cargo run -p xtask -- backend-full-corpus-diff-report`
2. update the top-level counts,
3. record any newly supported or newly unsupported valid corpus families,
4. update the "Important current backend exclusions" section,
5. add one historical note when a major boundary moves
   (for example: interval analysis landed, variable delays enabled, stream
   wrappers closed, new long-tail family supported).
