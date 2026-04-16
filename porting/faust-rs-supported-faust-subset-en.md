# Current Faust Source Subset Supported by `faust-rs`

Last updated: 2026-04-15

Version: 0.5.0

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
   - `parse -> eval -> propagate -> signal_prepare -> signal_fir -> backend`
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
- corpus/backend reports currently checked into `porting/phases/`:
  - `cargo run -p xtask -- corpus-status-report`
  - `cargo run -p xtask -- backend-full-corpus-diff-report`
- follow-up front-end parity fixes landed on 2026-03-29 and reviewed against:
  - `crates/parser/src/lib.rs`
  - `crates/eval/src/lib.rs`
  - `crates/eval/tests/core_eval.rs`
  - `crates/compiler/tests/inline_environment_import_parity.rs`
  - `porting/journal/2026-03-29.md`
- fast-lane/runtime fixes landed on 2026-04-03 and reviewed against:
  - `crates/transform/src/signal_prepare.rs`
  - `crates/transform/src/signal_fir/module.rs`
  - `crates/codegen/src/backends/interp/serial.rs`
  - `crates/compiler/tests/signal_fir_lane.rs`
  - `porting/journal/2026-04-03.md`
- FIR runtime optimization and signal-type correctness fixes landed on 2026-04-04–05 and reviewed against:
  - `crates/sigtype/src/rules.rs`
  - `crates/transform/src/signal_fir/module.rs`
  - `crates/transform/src/signal_fir/cse.rs`
  - `crates/fir/src/checker.rs`
  - `crates/codegen/src/backends/cranelift/mod.rs`
  - `crates/cranelift-ffi/src/instance.rs`
  - `porting/journal/2026-04-04.md`
  - `porting/journal/2026-04-05.md`
- delay-strategy / recursion-delay parity and sample-loop ordering work landed on 2026-04-08 and reviewed against:
  - `crates/transform/src/signal_fir/delay.rs`
  - `crates/transform/src/signal_fir/module.rs`
  - `crates/transform/src/signal_fir/siggen.rs`
  - `crates/transform/src/signal_fir/tests.rs`
  - `crates/compiler/tests/signal_fir_lane.rs`
  - `porting/journal/2026-04-08.md`
- transform preparation boundary/encapsulation fixes landed on 2026-04-10 and reviewed against:
  - `crates/transform/src/signal_prepare.rs`
  - `crates/transform/src/signal_fir/mod.rs`
  - `crates/transform/src/signal_fir/module.rs`
  - `crates/transform/src/signal_fir/planner.rs`
  - `crates/transform/src/signal_fir/siggen.rs`
  - `porting/journal/2026-04-10.md`
- forward-mode AD (`fad`) implementation documented and 22 corpus entries validated on 2026-04-15
  and reviewed against:
  - `crates/propagate/src/forward_ad.rs`
  - `crates/propagate/src/lib.rs`
  - `crates/propagate/tests/core_api.rs`
  - `crates/compiler/tests/signal_pipeline.rs`
  - `crates/compiler/tests/signal_fir_lane.rs`

The corresponding generated reports are:

- [phase-4-corpus-status-diff-report-en.md](./phases/phase-4-corpus-status-diff-report-en.md)
- [phase-6-backend-full-corpus-diff-report-en.md](./phases/phase-6-backend-full-corpus-diff-report-en.md)

## 4. Executive Summary

### 4.1 Source-language status

At the front-end level, the checked-in corpus-status report currently shows
**full status parity** with the C++ compiler:

- total corpus cases in the latest checked-in report: `104`
- valid cases accepted by both Rust and C++: `88`
- invalid cases rejected by both Rust and C++: `16`
- `OK/ERR` mismatches: `0`
- `ERR/OK` mismatches: `0`

Important note:

- the repository now contains `107` files under `tests/corpus/`
- the checked-in backend full-corpus report is older (`98` cases)
- so the front-end counts above are the latest auditable numbers in-repo,
  while the end-to-end backend counts below remain the latest checked-in
  backend snapshot, not a claim that all `107` corpus entries have already
  been re-benchmarked end-to-end

In other words:

- on the current tracked corpus, `faust-rs` now accepts the same valid source
  programs as Faust C++ up to the `signals` boundary,
- and it rejects the same invalid corpus programs.

### 4.2 End-to-end backend status

At the current backend route (`TransformFastLane`), the supported subset is
still narrower. The latest checked-in full backend report says:

- total corpus cases: `98`
- end-to-end C backend parity: `OK=82`, `DIFF=0`, `UNSUPPORTED=16`
- end-to-end C++ backend parity: `OK=82`, `DIFF=0`, `UNSUPPORTED=16`

The `16` unsupported cases are:

- `15` corpus entries that are intentionally invalid Faust programs,
- `1` remaining valid corpus case:
  - `rep_18_stream_wrappers.dsp`

So for **valid** corpus programs, the current backend route compiles:

- `82 / 83` valid corpus cases,
- and misses `1 / 83`, currently in the non-trivial stream-wrapper family.

### 4.3 WASM / JSON backend status

The Rust backend now also has a real WebAssembly output path, plus an initial
embedded-compiler path for `faustwasm`, but its maturity level still differs
from the C/C++ backend route described above.

What is now true:

- `-lang wasm` emits a matched pair:
  - `<name>.wasm`
  - companion `<name>.json`
- the companion JSON now carries the main provenance/runtime fields expected by
  current Faust web tooling:
  - `filename`
  - `version`
  - `compile_options`
  - `include_pathnames`
  - `library_list`
  - UI widget `index`
  - `size`
  - `sr_index`
- strict `-json` is wired as a first-class CLI mode and can now also be used
  alongside `-lang <backend>`, like in the C++ compiler workflow
- `-lang wast` is now also wired and renders textual WAT/WAST from the same
  backend via `wasmprinter`
- for `-lang wasm`, the companion JSON and exported ABI are now coherent enough
  to work with the standard `faustwasm` runtime on validated cases such as
  `osc.dsp`
- the Rust `wasm-ffi` compiler-module can now be built as a standalone
  `wasm32-unknown-unknown` artifact and loaded by `faustwasm` as an embedded
  compiler
- that compiler-module now embeds the standard Faust library set as read-only
  virtual sources, so source-string compilation can resolve imports such as
  `import("stdfaust.lib")` without an Emscripten-style filesystem
- the embedded-compiler path is validated end-to-end in `faustwasm` on:
  - mono DSP compilation using the standard runtime path
  - polyphonic `faust2wasm.js` generation, including packaged internal mixer
    fallback, on `organ.dsp`

What is not yet true:

- the Rust WASM backend should not yet be described as full semantic parity
  with the C++ WASM backend
- lowering coverage is still an incremental subset and is currently being
  extended from real-world DSP failures and wrapper/runtime tests
- exact byte-for-byte parity of all public UI offsets/layout details against
  the C++ backend is not yet claimed, even though the current ABI contract is
  now much closer and is already functional with `faustwasm` on the validated
  path
- helper parity for the embedded-compiler path is still incomplete:
  - `getInfos(...)` is only partially implemented
  - `expandDSP(...)` and `generateAuxFiles(...)` are present in the API but
    are not yet parity-complete replacement surfaces for the historical C++
    binding

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
- local-file imports, including structural expansion from parsed
  `importFile(...)` nodes,
- inline `environment { import(...) }` forms in the tracked parity scope,
- UI widgets and UI groups,
- modulation-bearing source programs,
- waveform and table forms,
- representative feedback and delay programs,
- label interpolation cases used by modulation/UI paths,
- representative noise/additive-synthesis fixtures,
- **forward-mode AD**: `fad(expr)` is supported at the source and propagation
  level; 22 corpus entries cover the full rule spectrum (arithmetic, trig,
  `pow`, `min`/`max`, delays, recursion, `select2`, multi-control, and the
  `[autodiff:false]` opt-out).

Stated differently:

- the current front-end subset is best described as **"the current tracked Faust
  corpus"**, not as a small manually enumerated syntax fragment.
- the main residual front-end caveat is not basic language acceptance anymore,
  but long-tail parity outside the exercised corpus and some deferred tooling
  concerns such as remote-import parity.
- parser entry points still differ intentionally:
  - `parse_program(...)` parses one in-memory source unit and does **not**
    perform filesystem import resolution,
  - structural import resolution belongs to `parse_file_with_imports(...)` and
    `parse_program_with_imports_and_metadata(...)`.

## 5.2 End-to-end backend subset currently supported

The backend subset is narrower and is better characterized structurally.

For the production C/C++ route, a Faust program currently compiles end-to-end
when its post-eval/post-propagate signal forest stays within the active
`signal_prepare + signal_fir` lowering slice.

More precisely, `signal_prepare` now acts as an explicit verified staging
boundary before FIR emission:

- it clones the output forest into a private staging arena,
- runs the current normalization/type/promotion/canonicalization subset there,
- and verifies the resulting postconditions before `signal_fir` consumes the
  prepared outputs and type maps.

For the Rust WASM route, the same FIR preparation pipeline is reused, but the
backend lowering itself is currently a separate, narrower bring-up slice with
its own documented subset in `crates/codegen/src/backends/wasm/`.

That slice currently includes, in broad terms:

- numeric constants and audio inputs/outputs,
- arithmetic, comparison, bitwise, and selected math operators,
- `select2`, `min`, `max`, `abs`,
- `delay1`, `prefix`, and recursive feedback forms — including
  **multi-output recursion groups** (`SIGPROJ index > 0`),
- `SIGDELAY` with:
  - constant integer amounts lowered through the same three strategy families
    as Faust C++:
    - very small delays: direct `Shift` copies,
    - middle-range delays: power-of-two circular buffers with `fIOTA`,
    - large delays / `-dlt`: exact-size if-wrapping buffers,
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
- `rep_71_degenerate_unary_recursion`
- `rep_72_float_literal_pattern`
- `rep_73_pattern_max_min_fold`
- `rep_60_counter_rem`, `rep_61_fmin_sr`, `rep_62_select2_trigger`,
  `rep_63_rwtable`, `rep_63_store_load_table`, `rep_64_dynamic_rem`,
  `rep_65_fabs_trigger`

now compile end-to-end through the Rust backends.

Since that snapshot, the validated fast-lane/runtime path has also absorbed
several important correctness fixes without changing the high-level subset
classification:

- post-`simplify` re-typing/promotion before FIR lowering,
- canonicalization of `Delay(x, 1)` back to `Delay1(x)` at the preparation
  boundary,
- preservation of non-numeric `Konst` factors in normalization for
  `fSamplingFreq`-driven delay amounts,
- correct `instanceResetUserInterface` generation for UI-only controls in the
  wasm path,
- round-trip-precise FBC real literal serialization,
- correct previous-step semantics for non-recursive `Delay1` inside the SIGGEN
  table-generator interpreter,
- C++-parity delay strategy thresholds at the `-mcd` / `-dlt` boundaries,
- pre-scan coverage for standalone `Delay1(x)` so delay strategy/geometry is
  chosen once up front,
- accumulated delay analysis for recursion carriers, so
  `Delay1^k(Proj(...))` chains can reuse one upsized recursion array instead of
  separate `fVec` buffers,
- simple-recursion lowering aligned with Faust C++ for 2-slot feedback arrays,
- explicit sample-loop emission phases (`Immediate`, `PostOutput`,
  `SampleEnd`) so `Shift` copies and delay counter updates now have a clearer
  documented ordering model.

## 5.3 Important current backend exclusions

The backend subset is **not** "all front-end-accepted Faust programs".

The most important current exclusions are:

- **Variable delays with no statically bounded interval**
  - `SIGDELAY` whose amount expression has an unbounded interval (i.e., the
    interval upper bound is infinite or indeterminate) is rejected.
  - This is narrower than the previous `Variability::Samp` blanket rejection:
    audio-rate amounts are now accepted when their interval is provably bounded
    and non-negative.
  - A pure audio input used directly as a delay amount is no longer the blocker
    it used to be if type/interval inference yields a finite non-negative upper
    bound; the remaining exclusion is the genuinely unbounded case, where no
    finite static capacity can be proven.

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

- **Reverse-mode AD (`rad`)**
  - `rad(expr)` returns [`PropagateError::UnsupportedBox`] unconditionally.
    Forward-mode (`fad`) is the only supported AD variant in this phase.

- **`fad(expr)` — propagation is supported; end-to-end backend is conditional**
  - `fad(expr)` correctly expands the output bundle at the propagation level:
    each primal output gains one tangent signal per reachable differentiable
    control.
  - The expanded tangent signals use the same node families as primal signals
    (BinOp, trig, pow, delays, recursion, …), so end-to-end backend compilation
    succeeds whenever the underlying node kinds are already supported.
  - No special backend handling of tangent outputs is needed; the fast-lane
    sees an ordinary (wider) signal list.
  - `fad_delay.dsp` is validated through the `signal_fir_lane` fast-lane test.
  - Caveat: `[autodiff:false]` metadata causes the box-level arity count to
    be an upper bound only — the actual signal list may be shorter.  The fast-lane
    output-arity validation accounts for this discrepancy.

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
- the Rust CLI now supports `-json` both as a standalone strict JSON mode and
  alongside `-lang <backend>`, with truthful `compile_options` in the emitted
  JSON,
- the Rust WASM backend now emits a usable companion JSON contract for web
  runtimes and writes the same `.wasm` + `.json` pair shape as the C++ CLI,
- the Rust toolchain now also exposes two additional WASM-facing delivery
  shapes:
  - `-lang wast` for textual WAT output,
  - `crates/wasm-ffi` for a raw embedded-compiler module consumed by
    `faustwasm`,
- many language families that were earlier missing are no longer front-end
  blockers:
  - `case` and recursive pattern-matching with computed numeric arguments,
    including xtended-function arguments such as `max`/`min` (full
    `patternSimplification` port)
  - float-literal patterns (`foo(1.0) = …`) now match correctly
  - lambda/closure forms (first-class `boxClosure` node)
  - modulation
  - local imports
  - structural import expansion from parsed `importFile(...)` nodes, including
    inline `environment { import(...) }` cases in the tracked parity scope
  - top-level imports now remain visible after nested transitive imports;
    duplicate-import suppression is scoped per structural definition expansion
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
- the embedded `faustwasm` Rust compiler path can now compile DSP source
  strings that import the standard Faust libraries, because the compiler module
  ships a read-only embedded library bundle instead of depending on the legacy
  Emscripten filesystem,
- the same embedded compiler path now passes the historical `faust2wasm.js`
  polyphonic route on a validated case (`organ.dsp`), including packaged mixer
  fallback when no compiler `FS` is present,
- the full C++ interval algebra is now available in Rust through
  `crates/interval`, and the signal type lattice is fully modeled in
  `crates/sigtype` with correct parity including `FConst`/`FVar` intervals,
- signal-type purity is now enforced: `SIGWAVEFORM` correctly carries `Samp`
  variability (not `Konst`), and pure math functions (`sin`, `cos`, `sqrt`,
  etc.) preserve argument variability without spurious `samp_cast` promotion,
- the `instanceConstants` lifecycle contract is now enforced at FIR level
  (FIR-LC01 diagnostic) and the Cranelift backend compiles `instanceConstants`
  as a real JIT function, matching the C/C++ backend lifecycle model,
- the variability-driven FIR placement pass (Phase 1) is now signal-sharing
  aware, materializing only shared or boundary-crossing nodes into named
  variables and leaving single-use within-tier intermediates inline — matching
  the C++ code shape for Block-rate control chains,
- intra-bucket CSE (Phase 2) further deduplicates shared sub-expressions
  within each execution tier, reducing redundant evaluation in the sample loop,
- **forward-mode AD** (`fad(expr)`) now propagates correctly through the
  full signal graph including recursive groups, variable delays, transcendentals,
  arithmetic, casts, and control-flow — with 22 corpus entries validated
  through the `signal_pipeline` test suite and the fast-lane.

## 6.2 Where C++ is still broader

Faust C++ still supports a broader end-to-end language/runtime envelope.

Most importantly, the C++ compiler still has:

- **reverse-mode AD** (`rad(expr)`): completely unsupported in this phase
  (`PropagateError::UnsupportedBox`),
- fuller support for stream-wrapper lowering,
- broader mature transform/backend coverage on long-tail signal families,
- a fuller embedded-compiler helper surface for web tooling
  (`expandDSP`, `generateAuxFiles`, full `getInfos`, packaged FS semantics),
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

- **Rust (current fast-lane status, 2026-04-12)**:
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

### 7.11 March 17, 2026: delay-state robustness, select2 fix, CLI parity

#### Circular carriers for delay1/recursion (later refined)

The fast-lane moved away from scalar state + ad hoc deferred update ordering
for `delay1` and recursion, which had caused ordering bugs when depth-first
lowering pushed inner updates before outer ones.

That March change established the circular/state-array direction, but the final
model was refined later:

- small standalone `Delay1` now uses the Faust C++-style `Shift` strategy when
  appropriate,
- recursion carriers can be upsized from accumulated delay analysis and reused
  by `Delay1^k(Proj(...))` chains,
- simple 2-slot recursions were later realigned with the exact C++ write/read
  pattern instead of always going through `fIOTA`.

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

### 7.15 April 8, 2026: delay-strategy parity, recursion-delay analysis, explicit emission phases

The fast-lane delay subsystem was substantially tightened to match the C++
compiler more closely.

#### Delay strategy parity with Faust C++

`SIGDELAY` now follows the same practical three-way strategy split as the C++
compiler:

- `Shift` for very small delays,
- circular power-of-two buffers with `fIOTA` for the middle range,
- exact-size if-wrapping buffers for the `-dlt` range.

The threshold behavior at the exact `-mcd` and `-dlt` boundaries was also
aligned with the C++ compiler's strict comparisons.

#### Recursion-delay carrier reuse

The delay pre-pass now performs accumulated delay analysis for recursion
outputs, allowing chains of the form:

- `Delay1(Proj(...))`
- `Delay1(Delay1(Proj(...)))`
- `Delay(Delay1^k(Proj(...)), N)`

to reuse one canonical recursion carrier when possible instead of allocating
separate standalone delay vectors. This materially improves parity on APF /
biquad-like structures.

#### Simple recursion parity restored

The special case `process = + ~ _;` is now lowered with the same 2-slot update
shape as Faust C++:

- current value written to slot 0,
- previous value read from slot 1,
- deferred copy from slot 0 to slot 1 after output observation.

#### Explicit sample-loop phases

The sample-loop assembly now uses an explicit phase model:

- `Immediate`
- `PostOutput`
- `SampleEnd`

This does not change the supported source subset by itself, but it makes the
ordering guarantees around `Shift` copies, recursion updates, and delay counter
maintenance clearer and easier to maintain.

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

### 7.13 March 22–24, 2026: correctness fixes, pattern evaluation parity, cranelift robustness

#### `lower_proj`: separate rec-array and state-slot maps (2026-03-22)

Nested recursion groups (e.g. `tf22` biquad, `filters_direct_ladder_tests.dsp`)
produced wrong audio because `ensure_recursion_array` and `ensure_state_slot`
shared the same `state_name_by_node` map.  When the outer group body was itself
the `Delay1` node, both paths resolved to the same array, causing the wrong
value to be written.

Fix: add a separate `rec_array_by_group_index: HashMap<(u32,usize), RecArrayInfo>`
keyed by `(group_id, output_index)` that never aliases `state_name_by_node`.
`lower_proj` now calls `ensure_recursion_array_for_group(group, i, …)` and the
skip-guard from the previous commit was removed (it is no longer needed).

#### Parser: `stdfaust.lib` + `demos.lib` duplicate zero-arity aliases (2026-03-22)

Both libraries independently declare the same 19 library-alias symbols (`ma`,
`ba`, `de`, `si`, …).  `make_definition_from_variants` previously errored with
"multiple definitions of a zero-argument symbol are not allowed".  Fix: mirror
C++ import-shadowing semantics — for zero-arity symbols with multiple variants,
use the newest definition.  `guitarEffectChain.dsp` now parses and compiles.

The `import_loading_cpp_gap` test was converted from a gap-tracker to a
parity-success test (2026-03-23).

#### Interval: `hi_or2` mask-rule off-by-one → exponential recursion (2026-03-23)

The short-circuit in `hi_or2` checked `a.hi == 2 * m.wrapping_sub(1)` (= 2m−2),
but the correct condition is `a.hi == 2m − 1` (all bits set below the MSB).
The off-by-one prevented early exit for full power-of-2 ranges `[0, 2^n−1]`,
causing 3 recursive sub-calls per level and O(3^32) ≈ 10^15 calls for any DSP
with bitwise-AND signals.  Fix: `ma.wrapping_add(ma).wrapping_sub(1)`.

`guitarEffectChain.dsp`: was hanging / 9.7 s → 2.0 s.
`minimoog-novation.dsp`: type-annotation time 4.7 s (C++ reference: 8.7 s).

#### Eval: integer div folding — `4/2` must produce `Int(2)` (2026-03-23)

`try_fold_seq_numeric` always returned a `Real` for division results.
Patterns from `math.lib` (`selector`, `butterfly`, `hadamard`) use tree identity
where `Real(2.0) != Int(2)`.  Fix: when all inputs are `SigInt` and the result
is an exact integer, produce `SigInt`.  Fixes `zita_rev1.dsp` sequential
composition mismatch.

#### Cranelift: float BinOp comparison type mismatch (2026-03-22)

`BinOp { op: Lt/Le/Gt/Ge/Eq/Ne, lhs: Float32, rhs: Float32, typ: Int32 }` is a
float comparison whose integer result was previously lowered via `fcvt_to_sint`,
truncating the operands instead of performing a float compare.  Fix: detect
comparison ops where the lowered CLIF operand type is float and emit `float_cmp`
regardless of the result type.  `harpeautomation.dsp` (string trigger using
`fabs(x) < 0.5`) now produces correct non-zero output.

#### Cranelift: AArch64 branch-offset overflow → graceful fallback (2026-03-22)

Cranelift's `MachBuffer` can panic (internal assertion) on AArch64 when a
very large `compute` body exceeds the ±1 MiB B.cond displacement limit.  Fix:
wrap JIT compilation in `catch_unwind`; on panic, retry with
`force_stub = true` so the instance falls back to the interpreter sidecar.
`minimoog-novation.dsp` now compiles without crashing the process.

#### FBC serial: embedded newlines in quoted UI/meta labels (2026-03-22)

`elecGuitarMIDI.fbc` contains a label with a literal `\n` byte inside a
quoted string.  `read_ui_block` called `read_line` once per instruction,
stopping at the embedded newline.  Fix: `read_quoted_logical_line` accumulates
physical lines until all double-quote characters are balanced.

#### Eval: float literal patterns match call-site arguments (2026-03-24)

`simplify_pattern` was coercing integer-valued Real constants (e.g. `1.0`,
`4.0/2.0`) to `Int` before the tree-identity check against the automaton's
`Constant(float_bits(…))` transition.  Since `int(1) != float_bits(1.0)`, any
function defined with a float-literal pattern always raised "no case rule
matches".  Fix: align with C++ `simplifyPattern` — return `boxReal` literals
unchanged without promoting integer-valued reals to `Int`.

Corpus addition: `rep_72_float_literal_pattern.dsp`.

#### Normalize: `min`/`max(Int, Int)` folds to `SigInt` (2026-03-24)

`simplify_const` previously folded `min`/`max` to `SigReal` even when both
operands were `SigInt`.  C++ parity: `sigMin`/`sigMax` return `sigInt` when
both inputs are integers.  Without this fix, `poly(max(1,min(N,4)), x)`
pattern matching failed — the argument simplified to `SigReal(2.0)` while
the pattern stored `Constant(boxInt(2))`.

#### Eval: `patternSimplification` — full C++ `isBoxNumeric` semantics (2026-03-24)

The previous `pattern_simplification` (literal arithmetic only) was replaced
with a complete port of C++ `patternSimplification` from `eval.cpp` line 773:

1. Try to fold the whole expression via `simplify_pattern` (full signal
   propagation + `simplify()` — equivalent to C++ `isBoxNumeric`).
2. If that fails, recurse into `PatternOp` children (`Par/Seq/Split/Merge/Rec`
   only — matches C++ `isBoxPatternOp`).  `HGroup/VGroup/TGroup/Route` are no
   longer recursed into, matching C++ exactly.
3. Otherwise return the pattern unchanged.

This enables `patternSimplification` to fold xtended functions such as
`max`/`min` at automaton construction time.  Example:
`f(max(1, min(6, 4)))` now correctly matches `f(4) = 40`.

~185 lines of superseded dead code removed (`simplify_numeric_pattern`,
`eval_numeric_pattern_value`, `eval_numeric_binary_op`, `NumericValue`, and
supporting helpers).

Corpus addition: `rep_73_pattern_max_min_fold.dsp`.

#### Structural refactors (2026-03-22–24, no semantic change)

- Dead-code sweep: compatibility wrappers, unused predicates, orphaned
  utilities removed from `crates/eval`, `crates/boxes`, `crates/compiler`.
- `boxes` and `eval` crates split into focused submodules.
- All embedded test suites extracted into standalone `tests.rs` files
  (`signal_fir`, `signal_prepare`, `fir`, `interp`, `cranelift` backends).

#### Corpus additions (net)

Ten new fixtures added since March 21:

| fixture | description |
|---------|-------------|
| `rep_60_counter_rem` | integer counter with `%` (rem) operator |
| `rep_61_fmin_sr` | `fmin` driven by sampling-rate constant |
| `rep_62_select2_trigger` | `select2`-based trigger |
| `rep_63_rwtable` | read/write table |
| `rep_63_store_load_table` | store-then-load table pattern |
| `rep_64_dynamic_rem` | dynamic `%` with variable operands |
| `rep_65_fabs_trigger` | `fabs(x) < 0.5` string trigger (Cranelift correctness) |
| `rep_71_degenerate_unary_recursion` | docs corpus: degenerate proj(7,W) canonicalization |
| `rep_72_float_literal_pattern` | float-literal pattern matching parity |
| `rep_73_pattern_max_min_fold` | `patternSimplification` with `max`/`min` fold |

End-to-end backend corpus: `82 / 83` valid cases, same single gap
(`rep_18_stream_wrappers.dsp`).

### 7.14 April 3, 2026: simplify integration hardening and latent fast-lane fixes

The 2026-04-03 fixes did not radically widen the declared subset, but they did
make the existing fast-lane subset materially more trustworthy on real DSPs and
web/runtime paths.

The main changes were:

- `simplify` is now a real part of the FIR-preparation pipeline, followed by a
  second `type -> promote -> type` cycle so FIR lowering still sees explicit
  coercions and reduced simple types
- one-sample delays are canonicalized back to `Delay1` at the preparation
  boundary instead of being special-cased only inside one consumer
- normalization now preserves `fSamplingFreq`-bearing konst factors and no
  longer assumes every order-0 bucket is purely numeric
- the wasm fast-lane now resets UI-only controls correctly even when they have
  been optimized out of the executable signal graph
- FBC text serialization now uses round-trip float formatting, which removed a
  latent divergence between interpreter and Cranelift runtimes
- the SIGGEN interpreter used for compile-time table generation now handles
  non-recursive `Delay1` with proper previous-step semantics, fixing
  `table.dsp`

Practically, this means the already-declared subset around:

- oscillator/table generators,
- SR-driven delay expressions,
- wasm standalone wrappers,
- interpreter/Cranelift differential tests,

is now better supported in practice than the older March backend report alone
would suggest.

### 7.15 April 4–5, 2026: FIR runtime optimizations, signal-type correctness, and CSE

#### Cranelift: JIT-compile `instanceConstants` (2026-04-04)

The Cranelift backend previously replayed `instanceConstants` through a
Rust-side subset interpreter.  The backend now compiles `instanceConstants`
as a real JIT function; the `cranelift_dsp` runtime invokes that entry
directly during instance initialization, aligning the lifecycle model with
the C/C++ backends and covering the full FIR subset that Cranelift already
knows how to lower.

#### sigtype: preserve kSamp floor through recursive fixed-point (2026-04-04)

`update_rec_types` in `crates/sigtype/src/rules.rs` was replacing the full
approximation type for each recursive-group component with the freshly
inferred body type on every fixed-point iteration, merging only the
interval.  Variability, computability, and other lattice dimensions were
discarded.  As a result, a recursion body that ignores its feedback input
(e.g. `! : 1`) could converge to `Konst`, allowing the variability
placement pass to hoist projections into `instanceConstants` and break the
sample-by-sample feedback contract.

Fix: after inferring the fresh body component, promote its variability and
computability by the old approximation's values before merging the interval,
matching C++ `joinType` semantics.  The `depends_on_recursive_projection`
workaround in `signal_fir/module.rs` was removed; `is_recursive_projection`
is retained as a belt-and-suspenders guard.

#### FIR-LC01 lifecycle-order checker (2026-04-04)

`phasor.dsp` produced wrong output (waveform index counters hoisted into
`instanceConstants` before `instanceClear` could initialize them).  The FIR
verifier had no check for this ordering violation.

Three additions to `crates/fir/`:

- `FIR-LC01 | E` diagnostic: detects `LoadVar(kStruct, name)` inside
  `instanceConstants` for fields that are only stored by `instanceClear`.
- `check_lifecycle_order` method: locates both bodies, computes the
  `cleared_only` field set, and emits an `Error`-severity diagnostic per
  violation.
- Two regression tests cover the failing and passing patterns.

The diagnostic is `Severity::Error` (not Warning) so it fails compilation
without `--fir-verify-strict`.

#### fix(sigtype): SIGWAVEFORM must carry Samp variability (2026-04-05)

`SIGWAVEFORM` was typed with `Variability::Konst` via `make_table_type`,
which correctly describes the static table content.  However, in the Rust
signal graph after propagation, `SIGWAVEFORM` represents the cycling output
— each sample yields the next element.  With `Konst` variability, FIR-LC01
detected lifecycle-order violations for waveform-driven DSPs (`phasor.dsp`,
`table1.dsp`).

Fix in `crates/sigtype/src/rules.rs`: promote the `SigMatch::Waveform`
result to `Variability::Samp` in both the empty and non-empty cases, so all
dependent expressions inherit the correct floor via `max(Samp, ·) = Samp`.

#### fix(transform): skip Phase 1 hoisting for SIGWRTBL (2026-04-05)

`SIGWRTBL` inherits `Konst` variability from `make_table_type`, but
`lower_wrtbl` returns the write signal's value, which may reference `iWave*`
cycling counters.  A targeted guard added to the Phase 1 placement check
ensures `SIGWRTBL` nodes always remain inline (in the sample loop), bypassing
the variability-based hoist.  A principled sigtype fix (propagating the write
signal's variability into the result type) is deferred.

#### fix(sigtype): remove spurious samp_cast from pure math type inference (2026-04-05)

`infer_unary_math` applied `samp_cast(tx)` unconditionally, promoting the
result variability to `Samp` for all transcendental/math operations
(`sin`, `cos`, `tan`, `sqrt`, `exp`, `log`, `floor`, `ceil`, `rint`, etc.)
regardless of the argument's variability.  The same bug was present in binary
math ops (`atan2`, `fmod`, `remainder`).

The C++ Faust source uses `floatCast(t)` — **without** `sampCast` — for all
pure math primitives: variability is inherited from the argument intact; only
the nature is promoted to Real.

Fix: replace `samp_cast(tx)` with plain `tx` in `infer_unary_math` and remove
`samp_cast` from the binary math arms.  This enables constant-at-rate
expressions such as `sin(ma.SR)` to be correctly hoisted into
`instanceConstants`.

#### feat(transform): CSE materialization pass — Phase 2 (2026-04-05)

Phase 1 variability-driven placement materialized **every** non-trivial
Block/Konst signal node into a named variable, regardless of sharing.  For
`STunedBar.dsp` this created 285 `fSlow` variables where C++ Faust only
needed 43.

Two complementary improvements landed:

1. **Signal-sharing-aware Phase 1 placement** (`analyze_signal_sharing`):
   a pre-lowering DAG pass computes per-signal reference counts and
   variability-boundary flags.  Phase 1 now only materializes a node into a
   named variable when it is shared (`ref_count ≥ 2`) **or** sits at a
   variability-transition boundary.  Single-use within-tier intermediate
   nodes stay inline, producing compact compound expressions matching C++.
   `STunedBar.dsp`: `float fSlow` reduced from 285 to 43 (exact C++ match).

2. **Intra-bucket CSE materialization pass** (`crates/transform/src/signal_fir/cse.rs`):
   after variability-driven placement, a bottom-up rewrite detects
   multi-referenced non-trivial `FirId` value nodes within each execution
   bucket and materializes them into named temporaries (`DeclareVar` +
   `LoadVar`).  Declarations are inserted at the point of first use to
   preserve sequential data dependencies.  Trivial nodes (literals, variable
   loads) are never materialized.  Naming conventions:
   `fConst*` / `fSlow*` / `fTemp*` for constants, control, and sample
   buckets respectively.

All corpus tests continue to pass after both optimizations.

### 7.16 April 15-16, 2026: forward-mode AD (`fad(exp, x)`) — explicit-seed propagation and current support boundary

#### `forward_ad.rs` — complete FAD rule table implemented

The full forward-mode automatic differentiation pass was implemented and
documented in `crates/propagate/src/forward_ad.rs`.

The implementation differentiates each propagated signal with respect to one
explicit seed signal at a time, producing a *dual signal* `(primal, tangent)`.
A [`ForwardADTransform`](forward_ad) instance memoizes results across the shared
signal DAG to prevent exponential blow-up.

Differentiation rules implemented:

| Category | Covered nodes |
|----------|--------------|
| Constants / audio inputs | `int`, `real`, `sigInput` → tangent 0 unless the node is the selected seed |
| UI controls | `hslider`, `vslider`, `numentry` — tangent 0 unless the propagated signal equals the selected seed |
| Discrete UI | `button`, `checkbox` → tangent 0 |
| Arithmetic | `Add`, `Sub`, `Mul` (product rule), `Div` (quotient rule), `Rem` |
| Integer/bitwise | shifts, comparisons, bitwise ops → tangent 0 |
| Unary transcendentals | `sin`, `cos`, `tan`, `exp`, `log`, `log10`, `sqrt`, `abs`, `acos`, `asin`, `atan` |
| Binary math | `pow` (general power rule), `min`, `max` (sub-gradient via `select2`) |
| Casts | `float_cast` (linear), `int_cast` → 0, `bit_cast` → 0 |
| Unit delay | `delay1(x)` → `delay1(x')` |
| Variable delay | discrete Leibniz rule: `delay(x', d) − d' · delay(x − delay1(x), d)` |
| Control-flow | `select2`, `prefix` — transparent |
| Recursion | `sigRec`/`sigRef`/`sigProj` with tangent group `FAD_<var>` and cycle-breaking placeholder |
| Helper nodes | `attach`, `enable`, `control` — left-operand tangent forwarded |
| Bargraphs | tangent 0 |
| Fallback | all other nodes → tangent 0, primal unchanged |

Before differentiation, all outputs pass through `de_bruijn_to_sym` so that
symbolic recursion variable names are explicit and `FAD_` pairing is unambiguous.

#### Output expansion and the `Rec` suppress/expand protocol

`generate_fad_signals` differentiates against one explicit seed and assembles
the output bundle:
```
[primal₀, tangent₀, primal₁, tangent₁, …]
```

For programs containing `fad(exp, x)` inside a recursive group (`~`), a
two-phase protocol prevents dangling references inside the De Bruijn group:

1. **Suppress** — the `suppress_fad` flag makes `ForwardAD` transparent during
   branch propagation; `box_arity_wiring` is used for internal port arithmetic.
2. **Expand** — `generate_fad_signals` is called once on the completed primal
   `sigRec` output, producing the tangent bundle from the fully-formed group.

#### Explicit-seed semantics and current limit

- `fad(exp, x)` computes one directional derivative with respect to the single
  propagated seed signal produced by `x`.
- The seed can be a control, an audio input, or a derived one-output signal.
- The transform does **not** enumerate all reachable controls anymore, and
  `[autodiff:false]` is no longer consulted by the compiler.
- A narrower unsupported case remains: if the seed is reached only through a
  recursive alias that the evaluator must expand while the recursive box is
  still being formed, evaluation still loops. This is the family illustrated by
  `state = next ~ _; prev = state; grad = fad(loss(prev), prev);`.
- This limit is not currently a Rust-only regression: upstream C++ Faust also
  rejects the same recursive-alias motif with an endless evaluation cycle.

#### Corpus coverage validated

| fixture | description |
|---------|-------------|
| `fad_basic` | `fad(f : sin, f)` — one control, one explicit seed |
| `fad_product` | `fad(f * g, f)` — product rule with one selected seed |
| `fad_trig_composition` | nested trig chain with explicit seed |
| `fad_triple_chain` | long chain differentiated against one chosen factor |
| `fad_delay` | fixed delay with explicit seed |
| `fad_recursive` | feedback loop differentiated with respect to one control |
| `fad_recursive_branch` | `fad` inside one recursive branch |
| `fad_recursive_deep_right` | deeply nested recursive branch with explicit seed |
| `fad_recursive_delay` | recursive delay with explicit seed |
| `fad_gradient_host` | host-side extraction of primal/tangent pair |
| `rad_parse_only` | parse/eval preserved, propagate intentionally unsupported |

These fixtures pass through `crates/compiler/tests/signal_pipeline.rs`.
`fad_delay` is additionally validated end-to-end through the `signal_fir_lane`
fast-lane test (C++ and interpreter bytecode output checked).

#### `rad(expr)` remains out of scope

Reverse-mode AD is not implemented. `rad(expr)` returns
`PropagateError::UnsupportedBox`.

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
- `fad(exp, x)` programs propagate correctly and produce the expected
  primal/tangent bundle when the seed is an ordinary explicit signal or a
  structured recursive use already covered by the corpus. End-to-end backend
  compilation succeeds whenever the underlying tangent signal content stays
  within the supported fast-lane subset (same as for primal-only programs).
- The main currently unsupported `fad(exp, x)` family is "differentiate with
  respect to a recursive state alias currently being formed". `rad(expr)` is
  not supported.

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
