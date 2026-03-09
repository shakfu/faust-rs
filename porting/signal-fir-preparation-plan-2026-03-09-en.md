# Signal -> FIR Preparation Parity Plan

Date: 2026-03-09

Status: completed

## Follow-up note (2026-03-09)

The preparation pipeline in this plan is implemented, but one `signalFIRCompiler`
parity slice remains open in the fast-lane:

- `SIGDELAY` should eventually lower like C++ `compileSigDelay(...)`, using a
  dedicated circular delay line, a persistent `IOTA` counter, and
  power-of-two masked indexing.

Current implementation status:

- `SIGDELAY1` and `SIGPREFIX` are lowered through typed scalar state slots.
- general `SIGDELAY` now supports constant integer amounts through typed
  fixed-size circular delay lines.
- variable `SIGDELAY` amounts are still deferred until Rust grows a proper
  static delay-bound analysis.

Explicit scope decision for the next slice:

- implement parity first for **statically bounded simple delays only**
  (constant integer delay amount after preparation/promotion),
- explicitly defer **variable delays** until Rust has a proper delay-bound
  analysis comparable to the C++ interval-driven sizing contract.

## Progress

- [x] Step 1: clone the whole output forest into a staging arena and run list-wide
  `de_bruijn_to_sym`
- [x] Step 2: add simple signal typing for the pre-FIR subset
- [x] Step 3: port the reduced `SignalPromotion` cast insertion rules
- [x] Step 4: make `signal_fir` consume prepared types for delay/recursion/table lowering
- [x] Step 5: run differential validation on parity-critical DSP families
- [x] Step 6: add fixed-size FIR delay lines for constant `SIGDELAY`

## 1. Goal

Bring the Rust fast-lane closer to C++ `signalFIRCompiler` preparation semantics by inserting a
pre-lowering preparation stage after `propagate` and before `transform::signal_fir` FIR emission:

`propagate -> de_bruijn_to_sym -> simple signal typing -> signal promotion -> signal_fir`

This plan intentionally does **not** add full signal simplification or normalization yet.

The fast-lane must continue to treat the following families as parity-critical, using
`signalFIRCompiler` as the semantic reference or close inspiration where direct 1:1 reuse is not
appropriate:

- delays and prefixes
- recursive signals
- tables (`waveform`, `rdtbl`, `wrtbl`)

Additional restriction for the active delay slice:

- `SIGDELAY` parity is currently targeted only for statically bounded simple
  delays
- variable delay amounts remain out of scope until a separate bound-analysis
  step exists

## 2. C++ reference points

Primary C++ references:

- `compiler/normalize/normalform.cpp`
  - `deBruijn2Sym(...)`
  - `typeAnnotation(...)`
  - `signalPromote(...)`
- `compiler/transform/sigPromotion.cpp`
- `compiler/transform/sigPromotion.hh`
- `compiler/transform/signalFIRCompiler.cpp`
- `compiler/transform/signalFIRCompiler.hh`
- `compiler/box_signal_api.cpp`
  - `boxesToSignalsMLIR(...)` is especially relevant because it already uses the reduced flow:
    `propagate -> deBruijn2Sym -> typeAnnotation`, without full simplification

## 3. Current Rust state

Current fast-lane structure:

- `propagate` produces signal trees containing de Bruijn recursion payloads.
- `transform::signal_fir::compile_signals_to_fir_fastlane(...)` consumes those signals directly.
- `transform::signal_fir` currently decodes recursion in `module.rs` from:
  - `DEBRUIJN(body)`
  - `DEBRUIJNREF(level)`
  - partially `SIGREC(body)`
- there is no dedicated signal typing phase before FIR lowering
- there is no `SignalPromotion`-style cast insertion phase before FIR lowering

Current semantic consequence:

- many FIR lowering decisions are still driven by a global `real_ty` default instead of by the
  actual signal nature
- integer-vs-real parity is therefore only partial today
- recursion representation in the fast-lane intentionally differs from the C++ symbolic recursion
  expectation
- existing delay/recursion/table support is partially implemented, but not yet anchored on the same
  preparation contract as C++ `signalFIRCompiler`

Examples of current drift in `crates/transform/src/signal_fir/module.rs`:

- arithmetic binops default to `real_ty`
- `select2` result defaults to `real_ty`
- delay state slots default to `real_ty`
- table element types default to `real_ty`
- waveform element typing is not driven by a prior signal typing pass

## 4. Target contract

### 4.1 Preparation boundary

The Rust fast-lane should stop consuming raw propagated signals directly.

Instead it should consume a prepared package:

- recursion converted from de Bruijn form to symbolic form
- each signal annotated with a simple type
- explicit `SIGINTCAST` / `SIGFLOATCAST` inserted where required by C++ `SignalPromotion`

### 4.2 Reduced parity objective

The target is **not** full `simplifyToNormalForm(...)` parity.

The target is the reduced preparation subset:

1. `deBruijn2Sym`
2. simple type annotation
3. first `signalPromote`
4. FIR lowering

Explicitly out of scope for this stage:

- `simplify(...)`
- the second `typeAnnotation(...)`
- the second `signalPromote(...)`
- `signalTablePromote(...)`
- `signalIntCastPromote(...)`
- UI freeze/range promotion
- FTZ promotion
- full warning/interval diagnostics parity

### 4.3 Parity-critical signal families

Even though this plan is limited to preparation before FIR lowering, the implementation must be
validated specifically against `signalFIRCompiler` behavior for these families:

- `SIGDELAY1`, `SIGDELAY`, `SIGPREFIX`
- recursion groups and projections
- `SIGWAVEFORM`, `SIGRDTBL`, `SIGWRTBL`

This means the preparation phase cannot be designed in isolation. It must feed the FIR lowerer with
enough information to preserve:

- correct carried type for state slots
- correct recursive-group identity and projection semantics
- correct table element type and index typing
- correct initialization/update ordering for stateful nodes
- correct delay-line element type and fixed-size circular buffer behavior for
  statically bounded delays

## 5. Key design decisions

### 5.1 Convert the full output forest, not each output independently

`deBruijn2Sym` must be applied to the **whole output forest with shared memoization**, not one
output at a time.

Reason:

- multi-output recursive groups must keep one coherent symbolic identity
- per-output conversion would allocate fresh symbolic variables independently and break
  cross-output sharing/parity

This is consistent with C++ using list-level roots:

- `simplifyToNormalForm(outputs)`
- `deBruijn2Sym(outputs)`

### 5.2 Use a private mutable staging arena

`de_bruijn_to_sym` and signal promotion insert new tree nodes.

Current Rust fast-lane API consumes `&TreeArena`, which is read-only.

Therefore the preparation stage should work on a private mutable staging arena, not on the
original parse/eval arena.

Recommended approach:

- add a `TreeArena` forest-clone helper with shared memoization
- clone the full output forest into a fresh arena
- run all preparation passes in that staging arena
- pass the prepared arena and prepared output roots to `signal_fir`

This avoids intrusive mutation of `ParseOutput` / compiler-owned parse state.

### 5.3 Use a deliberately small type system

We do not need the full C++ `sigtyperules` lattice yet.

For this stage, a minimal value-type system is sufficient:

```rust
enum SimpleSigType {
    Int,
    Real,
    Sound,
}
```

Optional escape hatch if needed:

```rust
enum SimpleSigType {
    Int,
    Real,
    Sound,
    Opaque,
}
```

This typing phase should answer only the questions needed by:

- `de_bruijn_to_sym` consumers in `signal_fir`
- `SignalPromotion` subset rules
- FIR type selection in `signal_fir`

It should not try to reproduce:

- interval analysis
- variability
- computability
- vector/scalar distinctions
- detailed UI algebra typing

Consequence for delays:

- the preparation phase may promote delay amounts to `Int`,
- but it does **not** yet compute general delay bounds,
- so only constant integer delay amounts can currently drive C++-style delay
  line allocation in the fast-lane.

## 6. Proposed Rust architecture

### 6.1 New preparation module

Add a dedicated preparation layer under `transform`, for example:

- `crates/transform/src/signal_prepare/mod.rs`
- `crates/transform/src/signal_prepare/typing.rs`
- `crates/transform/src/signal_prepare/promotion.rs`
- `crates/transform/src/signal_prepare/error.rs`

Alternative acceptable placement:

- `crates/transform/src/signal_fir/prepare.rs`

Recommendation:

- prefer a separate `signal_prepare` module
- keep `signal_fir` focused on FIR lowering, not tree rewriting

### 6.2 Proposed preparation output

```rust
pub struct PreparedSignals {
    pub arena: TreeArena,
    pub outputs: Vec<SigId>,
    pub types: ahash::AHashMap<SigId, SimpleSigType>,
}
```

Suggested API:

```rust
pub fn prepare_signals_for_fir(
    src_arena: &TreeArena,
    outputs: &[SigId],
) -> Result<PreparedSignals, SignalPrepareError>;
```

### 6.3 Fast-lane API integration

Two acceptable integration shapes:

1. Keep `compile_signals_to_fir_fastlane(...)` public and make it call the preparation stage
   internally.
2. Expose a typed internal boundary:
   - `prepare_signals_for_fir(...)`
   - `compile_prepared_signals_to_fir(...)`

Recommendation:

- keep the existing public API stable
- split the internal implementation into:
  - preparation
  - FIR lowering

## 7. Simple typing scope

### 7.1 First-pass rules to implement

The simple typer should cover the currently active fast-lane slice first.

Core rules:

- `Int`, `IntCast`, comparison/binop boolean result, bitwise result, shift result:
  `Int`
- `Real`, `Input`, `Output`, `FloatCast`, unary math, `Pow/Min/Max`, division:
  `Real`
- `Delay1`, `Delay`, `Prefix`, `Attach`, `Control`:
  type of carried DSP value
- `Select2`:
  join of branch types, selector forced to `Int`
- `RdTbl`:
  table element type, index forced to `Int`
- `WrTbl`:
  generator element type, write index forced to `Int`, written signal cast to generator element type
- `Waveform`:
  all values unified to one element type
- `Button`, `Checkbox`, `VSlider`, `HSlider`, `NumEntry`:
  `Real` for current FIR-lowering purposes
- `HBargraph`, `VBargraph`:
  carried signal type, but promoted to `Real` at UI write boundary if needed
- `Soundfile`:
  `Sound`
- `SoundfileLength`, `SoundfileRate`:
  `Int`
- `SoundfileBuffer`:
  `Real`

### 7.2 Explicitly deferred typing details

Deferred from the first implementation:

- interval propagation
- warnings on possible division by zero
- warnings on out-of-range shifts or casts
- full `ffun` signature typing parity
- full `xtended` inference parity
- UI variability/computability details

## 8. SignalPromotion subset to port now

The reduced promotion pass should follow the C++ rules that matter directly for FIR lowering.

### 8.1 Required subset

- `Delay(sig, amount)`:
  cast `amount` to `Int`
- arithmetic/comparison binops:
  - `Add/Sub/Mul/GT/LT/GE/LE/EQ/NE`
    - if operand natures differ, promote both sides to `Real`
- `Div`
  - always promote both sides to `Real`
- `Rem`
  - keep integer form for `Int/Int`
  - otherwise promote to `Real`
- bitwise ops and shifts:
  - cast both sides to `Int`
- `Prefix`
  - unify both branches
- `Select2`
  - cast selector to `Int`
  - unify both branches
- `IntCast`
  - keep only if source is not already `Int`
- `FloatCast`
  - keep only if source is not already `Real`
- `RdTbl`
  - cast read index to `Int`
- `WrTbl`
  - cast write index to `Int`
  - cast write signal to generator/table element type
- `SoundfileLength`, `SoundfileRate`, `SoundfileBuffer`
  - cast `part` / `ri` arguments to `Int`
- `HBargraph`, `VBargraph`
  - cast input value to `Real`
- `Waveform`
  - unify all element values to one common type

### 8.2 Deferred subset

Defer initially:

- full `FFun` cast insertion
- `xtended` inference-based casting
- `SignalTablePromotion`
- `SignalIntCastPromotion`
- clocked-wrapper special propagation for `OD/US/DS`

These can be added later once the base typed preparation path is stable.

Note:

- table support itself is **not** deferred
- only the broader C++ table-safety promotion layer is deferred
- the fast-lane still has to preserve `signalFIRCompiler`-style typing and lowering semantics for
  the currently supported table shapes

## 9. Required FIR-lowering changes

Adding preparation is not sufficient by itself.

`signal_fir` must also stop assuming "internal real unless explicitly integer".

### 9.1 Lowerer changes required for parity

`crates/transform/src/signal_fir/module.rs` should be updated so that:

- arithmetic result FIR types come from `SimpleSigType`
- `select2` result type comes from the prepared branch type
- delay state slots use the carried signal type
- statically bounded `SIGDELAY` nodes lower to typed circular delay lines
  instead of scalar state slots
- `SIGDELAY` amount `0` keeps the C++ zero-delay fast path
- a persistent struct `fIOTA: Int32` is added and incremented once per sample
- delay-line access uses masked indices:
  - write at `fIOTA & (size - 1)`
  - read at `(fIOTA - amount) & (size - 1)`
- delay-line struct arrays are sized with `pow2limit(delay + 1)` for constant
  integer delays
- table element types use the prepared generator/waveform type
- waveform declarations use the prepared element type
- recursion state slots use the prepared carried type
- delay/update ordering remains compatible with the current `signalFIRCompiler` model
- recursion lowering keeps one coherent state identity across multi-output groups
- table initialization and runtime writes stay compatible with the current `signalFIRCompiler`
  resource model
- output casts remain:
  - internal `Int` or `Real` -> external `FaustFloat`
- input loads remain:
  - external `FaustFloat` -> internal carried type

Explicitly deferred from this slice:

- non-constant `SIGDELAY` amounts
- interval-driven buffer sizing parity
- any attempt to emulate variable-delay sizing without a proved static bound

### 9.2 Recursion decoding update

After activating `de_bruijn_to_sym`, `lower_proj(...)` must no longer rely on de Bruijn payloads.

It should instead decode symbolic recursion payloads using:

- `tlib::match_sym_rec(...)`
- `tlib::match_sym_ref(...)`

This is the direct semantic consequence of the requested pipeline change.

## 10. Files likely to change

Primary Rust files:

- `crates/transform/src/lib.rs`
- `crates/transform/src/signal_fir/mod.rs`
- `crates/transform/src/signal_fir/module.rs`
- `crates/transform/src/signal_fir/error.rs`
- new preparation files under `crates/transform/src/signal_prepare/`
- `crates/compiler/src/lib.rs`
- `crates/box-ffi/src/lib.rs`
- `crates/tlib/src/lib.rs`
- `crates/tlib/src/arena.rs` if a forest-clone helper is added

Documentation files to update during implementation:

- `porting/phases/phase-5-recursive-trees-debruijn2sym-en.md`
  - current fast-lane "forbidden" policy will become obsolete
- `crates/tlib/src/lib.rs`
  - current pipeline note says fast-lane still consumes de Bruijn recursion directly
- `crates/transform/README.md`
- `crates/transform/src/signal_fir/mod.rs` RustDoc

## 11. Test plan

### 11.1 Preparation-unit tests

Add new focused tests for:

- list-wide `de_bruijn_to_sym` on multi-output recursive groups
- simple typing of:
  - `Add(Int, Int)`
  - `Add(Int, Real)`
  - `Div(Int, Int)`
  - `Select2(IntSelector, Int, Real)`
  - `Delay(value, real_delay)` -> promoted delay amount
  - `RdTbl/WrTbl` index casting
  - waveform mixed `Int`/`Real` value promotion

### 11.2 FIR-lowering tests

Extend `crates/transform/src/signal_fir/mod.rs` tests to assert:

- integer arithmetic lowers to FIR `Int32` when promotion keeps it integer
- mixed arithmetic lowers to FIR `Float32/Float64` only after explicit float promotion
- delay/prefix state slots use integer type when the carried signal is integer
- constant `SIGDELAY(n)` allocates a struct delay-line array with the carried
  FIR element type
- constant `SIGDELAY(n)` reads/writes through masked circular indices derived
  from `fIOTA`
- `fIOTA` is declared in the DSP struct, cleared to zero, and incremented in
  the sample loop
- `SIGDELAY(0)` lowers through the zero-delay fast path with no delay-line
  allocation
- non-constant `SIGDELAY` is still rejected with an explicit deferred-parity
  error
- table element types follow prepared waveform/generator type
- symbolic recursion payloads are accepted by `lower_proj(...)`
- recursive multi-output groups preserve coherent state/projection behavior
- readonly and runtime-written tables preserve current initialization/update semantics
- delay and recursion updates still appear in the same compute-order model as before

### 11.3 Compiler integration tests

Extend `crates/compiler/tests/signal_fir_lane.rs` with corpus-style checks that compare:

- current fast-lane C/C++ output before and after the change
- integer-heavy or mixed-type fixtures
- recursive fixtures where symbolic conversion affects lowering shape

### 11.4 Differential tests against C++

Add targeted differential tests against the C++ reference for representative cases:

- recursive feedback
- mixed int/real arithmetic
- select/cast cases
- table index casting cases
- waveform mixed-element cases

## 12. Suggested implementation sequence

### Step 1 - Introduce the preparation boundary

- add `PreparedSignals`
- add a forest-clone helper or equivalent staging-arena builder
- keep existing FIR lowering unchanged for the moment

Pass criterion:

- fast-lane compiles from a staging arena without semantic change

### Step 2 - Activate list-wide `de_bruijn_to_sym`

- convert the full output forest in the staging arena
- update recursion decoding expectations in tests
- do not add promotion yet

Pass criterion:

- recursive corpus cases still compile
- symbolic recursion is visible in prepared trees

### Step 3 - Add the simple typing pass

- compute `SimpleSigType` map for the supported subset
- add typed errors for unsupported typing situations

Pass criterion:

- typing tests cover the required subset
- no FIR lowering change yet beyond storing the type map

### Step 4 - Port the reduced `SignalPromotion` subset

- rewrite prepared signals with explicit casts
- keep the pass purely structural, no simplification

Pass criterion:

- inserted casts match expected C++ subset behavior on unit fixtures

### Step 5 - Make `signal_fir` consume prepared types

- replace blanket `real_ty` assumptions with per-node type decisions
- keep DSP boundary casts (`FaustFloat <-> internal`) explicit
- validate delay, recursion, and table lowering against current `signalFIRCompiler` semantics

Pass criterion:

- integer-vs-real FIR node typing is correct on unit and integration tests

### Step 6 - Add constant-delay FIR delay lines

- add a typed delay-line resource map keyed by the carried delayed signal
- allocate struct arrays for constant integer delay amounts only
- add persistent `fIOTA` state mirroring the C++ delay-line access model
- keep `SIGDELAY1`/`SIGPREFIX` on scalar state slots unless/until unified later
- reject non-constant delay amounts with an explicit deferred-parity error

Pass criterion:

- constant-delay fixtures lower with C++-style circular buffers and pass local
  FIR, compiler, and corpus checks
- variable-delay fixtures still fail explicitly instead of silently compiling
- delay/recursion/table fixtures still lower correctly and keep their current behavioral shape

### Step 7 - Differential closure and doc cleanup

- update outdated recursion-policy docs
- add targeted C++ differential tests
- re-run workspace and golden gates

Pass criterion:

- documented pipeline matches implementation
- C++ differentials shrink on type/cast-related cases

## 13. Acceptance criteria

This plan is complete when:

1. the fast-lane preparation boundary is explicit and uses a mutable staging arena
2. `de_bruijn_to_sym` is applied to the full propagated output forest before FIR lowering
3. a simple `Int/Real/Sound` signal typing phase exists and is covered by tests
4. a reduced `SignalPromotion` subset inserts the required explicit casts before FIR lowering
5. `signal_fir` uses prepared signal types instead of assuming `real_ty` for most nodes
6. recursive fast-lane lowering accepts symbolic recursion payloads
7. delay, recursion, and table lowering remain semantically aligned with `signalFIRCompiler`
8. workspace gates and `cargo run -p xtask -- golden-check` remain green

## 14. Non-goals for this stage

- full `simplifyToNormalForm(...)` parity
- full `sigtyperules` interval/variability/computability parity
- full `sigPromotion.cpp` feature coverage
- global signal simplification or canonicalization
- making the fast-lane the primary production pipeline

## 15. Immediate next implementation question

Before implementation starts, one architectural choice should be frozen:

- add a `TreeArena` forest-clone helper with shared memoization
- or make the compiler-side fast-lane preparation mutate an owned arena earlier in the pipeline

Recommendation:

- use a staging arena + forest-clone helper
- keep parse/eval arenas immutable at the signal->FIR boundary
