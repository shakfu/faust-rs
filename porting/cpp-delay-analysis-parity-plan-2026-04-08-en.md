# Plan: Reproduce C++ Delay Analysis in Rust

**Date**: 2026-04-08
**Scope**: `crates/transform/src/signal_fir/delay.rs`, `module.rs`, signal preparation / analysis support
**Status**: Plan — not yet implemented
**Goal**: reproduce in Rust the C++ compiler's delay-analysis model (`maxDelay`,
`delayCount`, and simple-recursion detection) so delay-line sizing and strategy
selection are driven by the same global information as Faust C++ rather than by
local special-case pattern matching.

---

## 1. Problem Statement

The current Rust fast lane decides delay resources from a local pre-scan:

- immediate `SIGDELAY(value, amount)` nodes allocate one shared delay line per
  carried `value`
- standalone `Delay1(value)` can be pre-scanned when the shift strategy is enabled
- one special recursion merge pattern is recognized:
  `SIGDELAY(Delay1(Proj(i, group)), N)`

This is enough for many cases, but it is not the same model as C++ Faust.

For `APF.dsp`, Faust C++ emits one compact recursion carrier:

- [APF_faust.cpp](/tmp/APF_faust.cpp#L97) declares `float fRec0[3];`
- [APF_faust.cpp](/tmp/APF_faust.cpp#L138) reads `fRec0[1]` and `fRec0[2]`

The Rust fast lane emits several independent 2-slot buffers:

- [APF_faustrs.cpp](/tmp/APF_faustrs.cpp#L35) declares `fRec182[2]`,
  `fVec56[2]`, `fVec183[2]`, `fVec185[2]`

The semantic output is correct, but the resource planning is weaker because the
Rust pre-scan does not yet compute the same transitive delay information as the
C++ compiler.

---

## 2. How C++ Faust Reasons About Delays

### 2.1 Core observation

The C++ compiler does not rely only on local pattern matching for delay sizing.
It computes occurrence metadata for signals and uses that metadata during code
generation.

Relevant C++ references:

- `compiler/transform/occMarkup.cpp` / `occMarkup.hh`
- `compiler/generator/compile_scal.cpp`
- `compiler/generator/dag_instructions_compiler.cpp`
- `compiler/generator/compile.hh`

### 2.2 Key properties produced by C++

For a signal node, the C++ pipeline tracks at least:

- `maxDelay`: maximum delayed access observed on that signal
- `delayCount`: number of delayed accesses
- occurrence / sharing information used to distinguish scalar, mono-delay,
  copy-delay, dense-delay, and ring-buffer cases

This information is then consumed by codegen.

Examples in C++:

- [compile_scal.cpp](/Users/letz/faust/compiler/generator/compile_scal.cpp#L1404)
  computes the delay implementation type from `mxd` and `count`
- [compile_scal.cpp](/Users/letz/faust/compiler/generator/compile_scal.cpp#L1371)
  detects `isSigSimpleRec`
- [dag_instructions_compiler.cpp](/Users/letz/faust/compiler/generator/dag_instructions_compiler.cpp#L435)
  uses `getMaxDelay()` on the carried expression to access one canonical delay line

### 2.3 Consequence for recursion

When a recursion output is consumed through chained delays, C++ attributes the
total delay to the recursion output carrier itself. That is why a form like:

`Delay1(Delay1(Proj(...)))`

can still lower to one recursion array sized to 3 instead of a recursion array
plus two auxiliary 2-slot delay buffers.

---

## 3. Current Rust Gap

The current Rust design in [`delay.rs`](/Users/letz/Developpements/RUST/faust-rs/crates/transform/src/signal_fir/delay.rs) and [`module.rs`](/Users/letz/Developpements/RUST/faust-rs/crates/transform/src/signal_fir/module.rs):

- tracks maximum delay only for immediate carried signals discovered during
  `scan_signals`
- recognizes one explicit merged recursion pattern
- does not maintain a general, reusable "delay metadata per signal" map
- does not currently distinguish the C++ notions of:
  - total delayed accesses to a carrier
  - simple recursion with one delayed use
  - general chained-delay access on recursion outputs

This leads to two classes of drift:

1. structural drift
   - same semantics, more small buffers and copies than C++
2. planning drift
   - delay strategy is selected from local syntax rather than from the
     signal's globally observed delay requirements

---

## 4. Objective

Introduce a Rust delay-analysis pass that is the functional equivalent of the
C++ occurrence-delay analysis for the fast-lane slice.

The pass must provide canonical metadata per signal carrier before FIR lowering,
so that:

- recursion arrays can be sized from the true maximum observed delay
- chained `Delay1` access can reuse the same carrier instead of allocating extra
  `fVec` buffers
- delay strategy selection remains centralized and happens once
- lowering becomes a pure consumer of precomputed metadata

The first target is parity of analysis behavior, not immediate full parity of
all backend code shape.

---

## 5. Proposed Rust Analysis Model

### 5.1 New metadata structure

Add an explicit delay-analysis map keyed by canonical signal carriers.

Suggested shape:

```rust
struct DelayAnalysisEntry {
    max_delay: i32,
    delay_count: u32,
    out_delay_count: u32,
    is_simple_rec_candidate: bool,
}
```

And:

```rust
type DelayAnalysisMap = HashMap<SigId, DelayAnalysisEntry>;
```

Notes:

- `max_delay` is the main driver for buffer sizing
- `delay_count` is needed for C++-style distinctions such as simple recursion
  vs general 1-sample delay
- `out_delay_count` is optional for phase 1 but should be kept in scope because
  C++ uses delayed-use density and occurrence information to refine code shape
- `is_simple_rec_candidate` should be derived from metadata plus recursion shape,
  not guessed ad hoc during lowering

### 5.2 Canonical carrier identity

The analysis must decide which signal node "owns" the delay history.

For the fast-lane scope, the canonical carriers are:

- general delayed expressions: the carried `value` of `SIGDELAY(value, amount)`
- recursion outputs: `Proj(i, group)` for active symbolic recursion groups
- standalone non-recursive delay1 state: the carried `value` of `Delay1(value)`

This must be explicit and documented. The same signal should never end up with
two competing owners for the same history requirement.

---

## 6. Required Traversal Semantics

### 6.1 Delay accumulation

The new pass should propagate accumulated delay through:

- `SIGDELAY(value, amount)` by adding the proven maximum delay bound of `amount`
- `SIGDELAY1(value)` by adding 1
- `SIGPREFIX(init, value)` when used as a one-sample state edge

### 6.2 Delay reset at non-delay computation nodes

The accumulated delay must not propagate through arbitrary arithmetic or other
computation nodes.

Example:

- `Delay(Add(x, y), 2)` means the delayed carrier is `Add(x, y)`, not `x` or `y`

So for non-delay nodes the traversal should recurse into children with a fresh
accumulator of 0 unless the node itself is the designated carrier.

### 6.3 DAG-aware maximum propagation

The same signal can be reached through multiple paths with different accumulated
delays. The analysis must keep the maximum observed accumulated delay and only
revisit a node when a larger delay value is discovered.

This is the Rust equivalent of C++ delay propagation through shared signal DAGs.

---

## 7. Simple Recursion Parity

The C++ scalar path distinguishes:

- `kMonoDelay`
- `kSingleDelay`

The boundary depends on both structure and occurrence metadata, not just on the
fact that the delay is 1.

Reference:

- [compile_scal.cpp](/Users/letz/faust/compiler/generator/compile_scal.cpp#L1371)

For the Rust fast lane, the immediate parity goal is narrower:

- keep the current direct 2-slot recursion lowering for simple recursion
- stop hardcoding the decision from local lowering shape alone
- derive the "simple recursion" decision from precomputed analysis metadata

This allows the same analysis infrastructure to serve both:

- compact `fRec[2]` direct recursions
- upsized recursion carriers such as `fRec[3]`, `fRec[12]`, etc.

---

## 8. Integration Plan

### Step 1. Introduce a read-only delay-analysis pass

Add one analysis pass in `signal_fir` preparation that:

- walks the prepared signal DAG
- computes `DelayAnalysisMap`
- has no FIR side effects
- does not allocate delay lines

Acceptance:

- unit tests can query computed `max_delay` and `delay_count` for hand-built
  signal DAGs
- APF-like chained recursion cases report delay 2 on the recursion projection

### Step 2. Define canonical carrier mapping

Document and implement how analysis entries are keyed:

- recursion projection carrier
- general delayed expression carrier
- delay1 standalone carrier

Acceptance:

- no ambiguous duplication for the same effective history requirement
- the analysis map can answer "which carrier owns this delayed read?"

### Step 3. Replace ad hoc recursion-delay merge detection

Retire the narrow special-case merge model:

- current `try_record_rec_delay` style pattern matching becomes a compatibility
  shim or disappears

Replace it with:

- general accumulated-delay attribution to the recursion carrier

Acceptance:

- `SIGDELAY(Delay1(Proj), N)` still works
- `Delay1(Delay1(Proj))` also attributes delay 2 to the same recursion carrier

### Step 4. Make `prepare_delay_lines` consume analysis output

`prepare_delay_lines` should become a planning phase, not a discover-on-the-fly
pattern collector.

Responsibilities:

- inspect the precomputed analysis map
- size recursion arrays from carrier `max_delay`
- size non-recursive delay lines from carrier `max_delay`
- choose delay strategy once

Acceptance:

- delay geometry decisions are centralized
- lowering no longer decides resource sizes opportunistically

### Step 5. Simplify lowering to lookup-only behavior

Update `module.rs` paths so they consume preplanned metadata:

- `lower_fixed_delay`
- `lower_shift_delay1`
- `lower_delay_state`
- `lower_proj`

Acceptance:

- lowering obtains precomputed carrier info
- missing carrier metadata is treated as an internal planning error
- no new delay line is allocated during lowering

### Step 6. Add APF-class parity tests

Add structural tests that lock the intended analysis and resulting FIR/C++ shape
for representative cases:

- `Delay1(Proj)` simple recursion
- `Delay1(Delay1(Proj))`
- `Delay(Delay1(Proj), N)`
- APF / biquad-style two-pole recurrence

Acceptance:

- the analysis reports the expected `max_delay`
- the emitted C++ shape uses one recursion carrier where C++ Faust does

---

## 9. Validation Matrix

### 9.1 Unit-level analysis tests

Add tests in `crates/transform/src/signal_fir/tests.rs` for:

- non-recursive standalone `Delay1`
- nested `Delay1`
- fixed delay over `Delay1`
- shared delayed users of the same carried signal
- recursion output reached at multiple delayed depths

### 9.2 Structural lowering tests

Lock FIR/C++ shape for:

- `process = + ~ _;`
- `process = + ~ @(10);`
- `process = APF(x, F, G, Q);` equivalent lowered fixture

These tests should assert:

- number of recursion arrays
- number of auxiliary delay lines
- presence or absence of `fIOTA`
- expected recursion array sizes

### 9.3 Differential reference checks

Use the reference Faust checkout in `/Users/letz/faust` for:

- generated C++ comparison on targeted DSPs
- impulse-test spot checks where delay compaction matters

Priority fixtures:

- `APF.dsp`
- `par_fir_32.dsp`
- representative delay/recursion impulse tests

---

## 10. Non-Goals for the First Iteration

This plan does not require, in the first implementation:

- full port of all `occMarkup` fields unrelated to delays
- immediate parity with every scalar/vector/DAG backend heuristic in C++
- dense-delay heuristics parity
- backend-specific loop-layout parity

The first milestone is:

- same effective delay metadata for the fast-lane-relevant signal shapes
- enough parity to drive the same carrier sizing decisions

---

## 11. Risks

### 11.1 Wrong carrier identity

If analysis keys the wrong node as the carrier, the result will either:

- duplicate buffers
- or incorrectly alias unrelated state

Mitigation:

- document carrier ownership rules explicitly
- add structural non-regression tests for alias-sensitive recursion cases

### 11.2 Over-propagating delay through computation

If accumulated delay incorrectly flows through arbitrary operators, the compiler
may attribute delay history to child signals that do not own the delayed value.

Mitigation:

- keep propagation semantics narrow and explicit
- test `Delay(Add(x, y), N)` and similar shapes

### 11.3 Analysis/lowering split drift

If lowering still contains hidden fallback allocation paths, parity will remain
fragile.

Mitigation:

- make lowering lookup-only after the planning phase is in place
- turn missing-planning cases into internal errors in tests

---

## 12. Deliverables

1. One documented Rust delay-analysis pass with C++ provenance notes.
2. One canonical metadata map for delay planning.
3. `prepare_delay_lines` rewritten to consume analysis output.
4. Removal or reduction of local special-case recursion-delay merge logic.
5. New analysis and structural tests covering APF-class chained recursion.

---

## 13. Pass Criteria

This plan is complete when:

- Rust computes the same effective maximum delay as C++ for the targeted fast-lane fixtures
- recursion carrier sizing is driven by that analysis rather than by narrow
  local patterns
- `APF.dsp` lowers with one recursion carrier instead of multiple auxiliary
  delay1 buffers
- the relevant unit, structural, and differential tests pass
