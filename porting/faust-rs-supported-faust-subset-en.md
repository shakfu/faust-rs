# Current Faust Source Subset Supported by `faust-rs`

Last updated: 2026-03-12

Status: living document

## 1. Purpose

This document records, in one place, what subset of Faust programs currently
compiles with `faust-rs`, how that differs from the C++ compiler, and why.

It is intentionally written as a **living status document**:

- it should be updated when the supported subset grows or shrinks,
- it should stay tied to executable evidence,
- it should distinguish clearly between front-end support and end-to-end backend
  support.

## 2. What “compiles” means here

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
- fresh local status scans run on 2026-03-12:
  - `cargo run -p xtask -- corpus-status-report`
  - `cargo run -p xtask -- backend-full-corpus-diff-report`

The corresponding generated reports are:

- [phase-4-corpus-status-diff-report-en.md](./phases/phase-4-corpus-status-diff-report-en.md)
- [phase-6-backend-full-corpus-diff-report-en.md](./phases/phase-6-backend-full-corpus-diff-report-en.md)

## 4. Executive Summary

### 4.1 Source-language status

At the front-end level, the active corpus currently shows **full status parity**
with the C++ compiler:

- total corpus cases: `75`
- valid cases accepted by both Rust and C++: `60`
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

- total corpus cases: `75`
- end-to-end C backend parity: `OK=59`, `DIFF=0`, `UNSUPPORTED=16`
- end-to-end C++ backend parity: `OK=59`, `DIFF=0`, `UNSUPPORTED=16`

The `16` unsupported cases are:

- `15` corpus entries that are intentionally invalid Faust programs,
- `1` remaining valid corpus case:
  - `rep_18_stream_wrappers.dsp`

So for **valid** corpus programs, the current backend route compiles:

- `59 / 60` valid corpus cases,
- and misses `1 / 60`, currently in the non-trivial stream-wrapper family.

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

- the current front-end subset is best described as **“the current tracked Faust
  corpus”**, not as a small manually enumerated syntax fragment.
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
- `delay1`, `prefix`, and recursive feedback forms covered by the active
  recursion lowering,
- fixed-size `SIGDELAY` with constant integer amount,
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
- at least the active foreign-constant special case for sampling frequency.

This is why corpus cases such as:

- `rep_04_delay_echo`
- `rep_06_comb_feedback`
- `rep_20_environment_waveform`
- `rep_35_table_rwtable_runtime_write`
- `rep_46_case_pattern_constant_folding`
- `rep_55_sine_phasor_echo_feedback`
- `rep_56_noise_smoo_slider`

now compile end-to-end through the Rust backends.

## 5.3 Important current backend exclusions

The backend subset is **not** “all front-end-accepted Faust programs”.

The most important current exclusions are:

- **Variable delays**
  - explicitly rejected in the fast-lane today
  - see:
    - `crates/transform/src/signal_fir/mod.rs`
    - `crates/transform/src/signal_fir/module.rs`
  - current contract:
    - fixed integer delays are supported,
    - variable `SIGDELAY` remains deferred until Rust gains interval-driven
      delay-bound analysis comparable to the C++ compiler.

- **Non-trivial stream wrappers**
  - the current valid backend corpus gap is
    `tests/corpus/rep_18_stream_wrappers.dsp`
  - trivial wrappers such as `inputs(_)` / `outputs(_)` are covered,
    but the full `ondemand(_)`, `upsampling(_)`, `downsampling(_)` family is
    not yet fully lowered end-to-end.

- **Complex table generators in `SIGGEN`**
  - the fast-lane supports the active table slice,
    but still rejects more complex `SIGGEN` initializers depending on runtime
    context or loop variables.

- **Interval-dependent backend behaviors**
  - the Rust `interval` crate is still a scaffold,
  - so backend features that require a proven static interval contract remain
    blocked or conservative.

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
  - `case`
  - lambda/closure forms
  - modulation
  - local imports
  - metadata

## 6.2 Where C++ is still broader

Faust C++ still supports a broader end-to-end language/runtime envelope.

Most importantly, the C++ compiler already has:

- interval analysis used by signal typing and backend decisions,
- variable-delay support sized from static delay bounds,
- fuller support for stream-wrapper lowering,
- broader mature transform/backend coverage on long-tail signal families,
- the historical production path beyond the active Rust fast-lane slice.

The variable-delay difference is representative:

- **C++**:
  - accepts variable `delay(x, d)` when the delay amount has a valid, bounded,
    non-negative interval
  - sizes the ring buffer from the interval upper bound
- **Rust today**:
  - supports only constant integer delay amounts in the current fast-lane
  - rejects variable delays explicitly because the interval/bound contract does
    not exist yet in Rust

So the current Rust compiler is best seen as:

- **front-end language coverage: broad on the tracked corpus**
- **backend lowering coverage: still a restricted but now fairly practical
  subset**

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

### 7.2 February 28, 2026: parser parity became operational but not yet “closed”

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

This is the key point where the limiting factor stopped being “can Rust read
this Faust source?” and became “can the backend lower the resulting signal
graph end-to-end?”

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

### 7.5 March 10 onward: remaining valid backend gap shrank to a very small tail

Subsequent work tightened integer typing, recursive state reuse, noise/sample
rate cases, and other backend details.

By the current 2026-03-12 snapshot:

- the front-end corpus mismatch count is `0`,
- the backend valid-case gap has shrunk to one tracked corpus case:
  `rep_18_stream_wrappers.dsp`.

## 8. Practical Reading Rule

Today, the simplest accurate rule is:

- If a Faust program stays within the language families already exercised by
  the tracked corpus, `faust-rs` front-end support is likely good.
- If that program also lowers to the current fast-lane signal subset
  (especially no variable delays, no non-trivial stream wrappers, no
  interval-dependent backend requirement), end-to-end C/C++ generation is
  likely to work.

The most common mistake is to assume:

- “front-end accepted” means “backend supported”.

That is no longer true as a general rule.

## 9. Update Policy for This Document

When this document is updated, the update should:

1. rerun:
   - `cargo run -p xtask -- corpus-status-report`
   - `cargo run -p xtask -- backend-full-corpus-diff-report`
2. update the top-level counts,
3. record any newly supported or newly unsupported valid corpus families,
4. update the “Important current backend exclusions” section,
5. add one historical note when a major boundary moves
   (for example: interval analysis landed, variable delays enabled, stream
   wrappers closed, new long-tail family supported).
