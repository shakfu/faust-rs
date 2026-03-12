# C++ Interval Library Port Plan

Date: 2026-03-12

Status: proposed

## 1. Goal

Port the C++ interval subsystem under `compiler/interval/` into the Rust
workspace as a first-class library, then integrate it into the active signal
typing and transform pipeline.

This plan targets the **full** interval library, not only the minimum slice
needed for fast-lane variable delays.

The outcome must let Rust compute interval facts with the same operational
contract as the C++ compiler for the supported signal/type pipeline, so that
downstream passes can rely on:

- boundedness checks,
- min/max value propagation,
- integer-cast interval behavior,
- delay-bound sizing contracts,
- UI-derived interval ranges,
- arithmetic and logic interval transfer functions.

## 2. Why This Must Be Ported First

Current `faust-rs` state:

- [crates/interval/src/lib.rs](/Users/letz/Developpements/RUST/faust-rs/crates/interval/src/lib.rs)
  is only a scaffold.
- [crates/transform/src/signal_prepare.rs](/Users/letz/Developpements/RUST/faust-rs/crates/transform/src/signal_prepare.rs)
  keeps only reduced types `Int | Real | Sound`.
- [crates/transform/src/signal_fir/module.rs](/Users/letz/Developpements/RUST/faust-rs/crates/transform/src/signal_fir/module.rs)
  rejects variable `SIGDELAY` amounts because no interval-bound contract exists.

Reference C++ behavior:

- [sigtyperules.cpp](/Users/letz/Developpements/RUST/faust/compiler/signals/sigtyperules.cpp:626)
  validates that `SIGDELAY` amounts have valid, bounded, non-negative
  intervals.
- [signalFIRCompiler.hh](/Users/letz/Developpements/RUST/faust/compiler/transform/signalFIRCompiler.hh:519)
  sizes delay lines from `it.hi()`.

Therefore, variable delays, several interval-sensitive simplifications, and a
class of parity-critical safety checks remain blocked until the interval system
itself exists in Rust.

## 3. Scope

### 3.1 In scope

Port the full C++ subsystem under:

- [compiler/interval/interval_def.hh](/Users/letz/Developpements/RUST/faust/compiler/interval/interval_def.hh)
- [compiler/interval/interval_algebra.hh](/Users/letz/Developpements/RUST/faust/compiler/interval/interval_algebra.hh)
- all concrete operator files under
  [compiler/interval/](/Users/letz/Developpements/RUST/faust/compiler/interval)

Porting includes:

- the core `interval` value model,
- all set/predicate helpers,
- the interval algebra API,
- all unary, binary, ternary, quaternary, quinary, and variadic interval
  operators currently implemented in C++,
- the missing-operation placeholders and their current semantics,
- the existing algebra self-tests as Rust tests,
- the integration boundary needed by signal typing and transforms.

### 3.2 Out of scope for this plan

- Full Rust port of the entire C++ type-inference pipeline.
- Full signal normalization or simplification parity.
- Changing interval semantics during the port.
- Opportunistic redesign of the whole algebra/dispatch architecture beyond what
  is needed to keep the port maintainable and testable.

## 4. C++ Reference Surface

### 4.1 Core data model

- [interval_def.hh](/Users/letz/Developpements/RUST/faust/compiler/interval/interval_def.hh)
  - `itv::interval`
  - constructors
  - `isEmpty`, `isValid`, `isBounded`, `isconst`, `ispowerof2`, `isbitmask`
  - `lo`, `hi`, `lsb`, `msb`
  - `empty`, `intersection`, `reunion`, `singleton`
  - interval comparison predicates

### 4.2 Algebra interface

- [interval_algebra.hh](/Users/letz/Developpements/RUST/faust/compiler/interval/interval_algebra.hh)
  - full operator surface
  - missing-op contract
  - test entrypoints

### 4.3 Dispatch layer

- [FaustAlgebra.hh](/Users/letz/Developpements/RUST/faust/compiler/FaustAlgebra/FaustAlgebra.hh)

Rust does not need to reproduce this class hierarchy literally, but it must
preserve:

- operator naming coverage,
- arity coverage,
- deterministic mapping from signal primitive to interval transfer function.

### 4.4 Operator implementation files

Representative examples:

- [intervalAdd.cpp](/Users/letz/Developpements/RUST/faust/compiler/interval/intervalAdd.cpp)
- [intervalMul.cpp](/Users/letz/Developpements/RUST/faust/compiler/interval/intervalMul.cpp)
- [intervalDiv.cpp](/Users/letz/Developpements/RUST/faust/compiler/interval/intervalDiv.cpp)
- [intervalIntCast.cpp](/Users/letz/Developpements/RUST/faust/compiler/interval/intervalIntCast.cpp)
- [intervalDelay.cpp](/Users/letz/Developpements/RUST/faust/compiler/interval/intervalDelay.cpp)
- [intervalMin.cpp](/Users/letz/Developpements/RUST/faust/compiler/interval/intervalMin.cpp)
- [intervalMax.cpp](/Users/letz/Developpements/RUST/faust/compiler/interval/intervalMax.cpp)
- [intervalSelect2.cpp] is not present as a dedicated file because `Select2`
  is declared in the algebra surface and currently falls under missing-op
  placeholder behavior in
  [intervalMissing.cpp](/Users/letz/Developpements/RUST/faust/compiler/interval/intervalMissing.cpp)

### 4.5 Test harness and utilities

- [check.hh](/Users/letz/Developpements/RUST/faust/compiler/interval/check.hh)
- [check.cpp](/Users/letz/Developpements/RUST/faust/compiler/interval/check.cpp)
- [precision_utils.hh](/Users/letz/Developpements/RUST/faust/compiler/interval/precision_utils.hh)
- [utils.hh](/Users/letz/Developpements/RUST/faust/compiler/interval/utils.hh)
- [bitwiseOperations.hh](/Users/letz/Developpements/RUST/faust/compiler/interval/bitwiseOperations.hh)
- [bitwiseOperations.cpp](/Users/letz/Developpements/RUST/faust/compiler/interval/bitwiseOperations.cpp)

## 5. Current Rust Gap Analysis

### 5.1 What exists

- a workspace crate name already reserved for interval analysis:
  [crates/interval/](/Users/letz/Developpements/RUST/faust-rs/crates/interval)
- fast-lane and preparation code that already expose the need for interval facts:
  - [signal_prepare.rs](/Users/letz/Developpements/RUST/faust-rs/crates/transform/src/signal_prepare.rs)
  - [module.rs](/Users/letz/Developpements/RUST/faust-rs/crates/transform/src/signal_fir/module.rs)

### 5.2 What is missing

- no `Interval` value type in Rust,
- no interval transfer functions,
- no interval test matrix,
- no annotation form richer than `SimpleSigType`,
- no bridge from signal typing to interval facts,
- no consumer-side API for transforms needing boundedness and max-delay data.

### 5.3 Consequences

- variable delays cannot be sized safely,
- non-constant interval-based checks cannot be expressed,
- C++-style delay rejection rules are absent,
- future parity work on tables, selectors, and UI-sensitive logic stays blocked.

## 6. Porting Strategy

### 6.1 Port semantics first, not architecture first

The Rust port should preserve the C++ interval semantics before attempting any
larger Rust-specific redesign.

That means:

- keep the same interval representation fields,
- keep the same special cases for emptiness, boundedness, and integer-cast
  saturation,
- keep the same result-shape behavior for arithmetic and logic operators,
- keep the same placeholder behavior for operations that are still stubbed in
  C++.

### 6.2 Port in two layers

Layer 1: standalone interval library

- independent of the rest of signal typing,
- no dependency on the signal arena,
- pure value operations plus tests,
- enough API stability for downstream crates.

Layer 2: compiler integration

- attach intervals to prepared/type-annotated signals,
- expose accessors for transforms,
- migrate consumers incrementally.

### 6.3 Do not block the library port on full type parity

The interval crate should be complete enough to stand on its own before the
signal typing integration is finished.

That avoids turning the interval port into a multi-subsystem all-or-nothing
change.

## 7. Target Rust Architecture

### 7.1 `crates/interval`

Target responsibilities:

- own the `Interval` model,
- own the interval algebra operations,
- own interval-specific helper math and bitwise support,
- expose deterministic, side-effect-free APIs,
- contain the ported unit tests.

Recommended modules:

- `interval.rs`
- `ops/arithmetic.rs`
- `ops/logic.rs`
- `ops/ui.rs`
- `ops/delay_table.rs`
- `ops/casts.rs`
- `ops/missing.rs`
- `bitwise.rs`
- `test_support.rs`

The exact split may differ, but the crate should remain centered on parity and
clarity rather than mirroring every C++ file 1:1.

### 7.2 Integration contract for downstream crates

Downstream users should be able to ask for:

- `is_valid`
- `is_bounded`
- `lo`
- `hi`
- `lsb`
- integer-cast interval conversion
- operator transfer functions

without depending on C++-style dispatch tables.

### 7.3 Prepared signal annotation

After the library exists, `transform::signal_prepare` should evolve from:

```rust
enum SimpleSigType {
    Int,
    Real,
    Sound,
}
```

to something like:

```rust
struct PreparedSigInfo {
    kind: PreparedValueKind,
    interval: Option<Interval>,
}
```

This exact data model can be refined during implementation, but the plan must
end with prepared/type passes carrying interval facts explicitly.

## 8. Required Semantic Inventory

The port must account for all operators declared in
[interval_algebra.hh](/Users/letz/Developpements/RUST/faust/compiler/interval/interval_algebra.hh),
including:

- injected values: `Nil`, `IntNum`, `Int64Num`, `FloatNum`, `Label`
- fixpoint/input/output family
- UI family: `Button`, `Checkbox`, `VSlider`, `HSlider`, `HBargraph`,
  `VBargraph`, `NumEntry`, `Attach`
- numeric family: `Abs`, `Highest`, `Lowest`, `Add`, `Sub`, `Mul`, `Div`,
  `Inv`, `Neg`, `Mod`, `Acos`, `Acosh`, `And`, `Asin`, `Asinh`, `Atan`,
  `Atan2`, `Atanh`, `Ceil`, `Cos`, `Cosh`, `Eq`, `Exp`, `FloatCast`,
  `BitCast`, `Floor`, `Ge`, `Gt`, `IntCast`, `Le`, `Log`, `Log10`, `Lsh`,
  `Lt`, `Max`, `Min`, `Ne`, `Not`, `Or`, `Pow`, `Remainder`, `Rint`,
  `Round`, `Rsh`, `Select2`, `Sin`, `Sinh`, `Sqrt`, `Tan`, `Tanh`, `Xor`
- state/table/soundfile family: `Mem`, `Delay`, `Prefix`, `RDTbl`, `WRTbl`,
  `Gen`, `SoundFile`, `SoundFileRate`, `SoundFileLength`, `SoundFileBuffer`,
  `Waveform`
- foreign-function family: `ForeignFunction`, `ForeignVar`, `ForeignConst`

Important note:

Several of these operations are currently placeholders in C++. The Rust port
must preserve that status faithfully first, then future work can improve the
semantics once parity is locked.

## 9. Implementation Steps

## Step 1. Port the core interval type

Deliverables:

- Rust `Interval` type with `lo`, `hi`, `lsb`
- constructors and invariants matching C++
- empty/valid/bounded helpers
- set operations and comparisons

Exit criteria:

- Rust tests cover all behaviors from `interval_def.hh`
- integer/float edge cases are reproduced

## Step 2. Port support utilities

Deliverables:

- saturation helpers
- precision helpers
- bitwise helper module
- shared test support replacing `check.hh` usage

Exit criteria:

- all operator ports can reuse common helpers instead of re-encoding local math

## Step 3. Port all concrete operators

Deliverables:

- one Rust implementation for each operation exposed in the algebra surface
- explicit parity comments referencing the C++ source when semantics are subtle
- placeholder implementations where C++ is still placeholder-only

Exit criteria:

- all C++ operator files under `compiler/interval/` have a Rust counterpart or
  an explicit documented reason when a dedicated file is folded into a shared
  Rust module

## Step 4. Port the algebra test matrix

Deliverables:

- Rust unit tests for every ported operator
- coverage for integer wrapping-sensitive cases
- coverage for empty, unbounded, and mixed-sign intervals

Exit criteria:

- all C++ `test*()` families have Rust equivalents
- the new crate can be validated independently with `cargo test -p interval`

## Step 5. Add a stable public crate API

Deliverables:

- clear public exports for `Interval` and operator functions
- documentation on parity status and placeholder semantics

Exit criteria:

- downstream crates can use interval operations without reaching into internal
  modules

## Step 6. Integrate interval facts into signal preparation

Deliverables:

- replace or extend reduced prepared typing so interval facts are preserved
- integer casts update interval facts through the ported `IntCast`
- UI nodes and arithmetic nodes propagate interval facts during preparation

Exit criteria:

- prepared signals expose interval data for all nodes in the currently prepared
  subset

## Step 7. Migrate first consumers

Priority consumers:

- fast-lane `SIGDELAY`
- any existing preparation checks that currently only know `Int | Real | Sound`

Exit criteria:

- variable `SIGDELAY` can distinguish exact, bounded, and unknown cases
- constant-delay current behavior stays unchanged

## Step 8. Differential validation against C++

Deliverables:

- Rust/C++ comparison tests on representative interval expressions
- corpus cases for bounded/non-bounded delay amounts
- documented remaining mismatches

Exit criteria:

- parity-critical interval behaviors are demonstrated, not just assumed

## 10. Validation Matrix

### 10.1 Crate-level validation

- `cargo test -p interval`
- exhaustive unit coverage for all ported operator families

### 10.2 Integration validation

- `cargo test -p transform`
- targeted tests for interval-aware preparation
- targeted tests for interval-aware `SIGDELAY`

### 10.3 Differential validation

Compare Rust and C++ for:

- `IntCast`
- arithmetic ranges
- min/max propagation
- UI-bounded ranges
- delay boundedness acceptance/rejection
- placeholder-op behavior where applicable

## 11. Risks and Failure Modes

### 11.1 Porting the interface without the semantics

Risk:

- a superficially complete Rust API that diverges on corner cases

Mitigation:

- port tests at the same time as implementations
- keep source provenance comments on subtle operators

### 11.2 Over-eager Rust redesign

Risk:

- semantic drift introduced by “improving” the C++ model during the port

Mitigation:

- parity-first implementation
- defer ergonomic redesign until after the port is complete and validated

### 11.3 Placeholder semantics hidden by cleanup

Risk:

- accidentally replacing a C++ placeholder with invented Rust behavior

Mitigation:

- preserve placeholder behavior explicitly
- document these cases in the crate and in `JOURNAL.md` when touched

### 11.4 Large-bang integration

Risk:

- porting the library and all consumers in one step makes failures hard to
  localize

Mitigation:

- land the standalone interval crate first
- integrate consumers in small follow-up steps

## 12. Concrete First Deliverable

The first implementation milestone should be:

- a real `crates/interval` library with the full core data model and operator
  surface ported from C++,
- Rust tests mirroring the C++ interval tests,
- no consumer integration required yet.

This keeps the port measurable and lets later work reuse a stable interval
library instead of rebuilding ad hoc bound logic inside transforms.

## 13. Success Criteria

This plan is complete only when all of the following are true:

- `crates/interval` is no longer a scaffold and covers the full C++ interval
  library surface,
- every operator declared in `interval_algebra.hh` has a Rust implementation or
  a documented parity-preserving placeholder,
- interval tests exist in Rust for the whole ported surface,
- `transform` can attach interval facts to prepared signals,
- at least one parity-critical consumer, starting with variable `SIGDELAY`,
  uses those interval facts successfully,
- remaining gaps versus C++ are documented explicitly rather than hidden behind
  fallbacks.
