# FAD Through Table and Waveform Reads — Plan

**Date:** 2026-04-23  
**Status:** Proposed  
**Scope:** Extend forward-mode AD (`fad`) so read-only table and waveform
accesses can emit meaningful tangents instead of falling back to zero.

## 1. Goal

Today `crates/propagate/src/forward_ad.rs` treats table/waveform/soundfile
families as outside the differentiable subset and preserves the primal while
emitting zero tangents.

That is safe, but too weak for a number of practically interesting DSP forms:

- wavetable oscillators whose phase/index is differentiated,
- lookup-driven nonlinearities and transfer curves,
- read-only generator tables used as differentiable envelopes or filters,
- waveform-backed recurrence systems where the differentiated variable drives
  the read index.

This plan targets the narrowest useful step:

- **support `fad` through read-only table/waveform reads first**
- keep writable tables and soundfiles out of the initial implementation
- preserve existing output ordering and recursive FAD machinery

## 2. Non-goals

- No reverse-mode (`rad`) work.
- No differentiation through writable table contents (`wrtbl`, `rwtable`).
- No differentiation through soundfile contents in the first phase.
- No change to parser/eval surface syntax.
- No promise of C++ parity: this is expected to be a Rust-side extension.

## 3. Current State

### 3.1 Signal forms already available

The signal layer already models the relevant primitives:

- `SIGWAVEFORM`
- `SIGRDTBL`
- `SIGWRTBL`
- `SIGSOUNDFILE*`

Propagation and FIR lowering already support these families as ordinary
signals/tables for non-AD compilation.

### 3.2 Current FAD boundary

`crates/propagate/src/forward_ad.rs` currently falls back to:

- primal preserved
- tangent = zero

for:

- tables
- soundfiles
- waveforms
- other unmatched families

This boundary is now explicitly documented in Rustdoc and in
`porting/faust-rs-supported-faust-subset-en.md`.

## 4. Problem Statement

The key unresolved semantic question is:

> What does it mean to differentiate a table read with respect to its index?

For a literal waveform/table access, the underlying function is discrete.
So there is no unique exact derivative unless the language/runtime defines an
interpolation model. Faust source itself does not expose “the derivative of the
lookup table” as a first-class primitive.

Therefore the first implementation must choose and document one of a small
number of explicit models.

## 5. Candidate Semantics

### Option A — Zero-order / status quo

```text
d/dp rdtable(table, idx(p)) = 0
```

Pros:

- trivially safe
- matches the current implementation

Cons:

- not useful
- defeats most practical AD use-cases involving lookup tables

### Option B — Backward finite difference on the table

For a read-only table access:

```text
value = rdtable(table, idx)
grad_idx ≈ rdtable(table, idx) - rdtable(table, idx - 1)
d/dp value ≈ grad_idx * idx'
```

Pros:

- easy to express in the current signal graph
- compatible with integer/discrete indexing
- no need to redefine the runtime table model

Cons:

- only an approximation
- asymmetrical and discontinuous at wrap/boundary points
- may diverge from a future interpolation-based design

### Option C — Forward finite difference on the table

```text
grad_idx ≈ rdtable(table, idx + 1) - rdtable(table, idx)
```

Pros:

- same implementation complexity as Option B

Cons:

- same approximation issues
- different boundary bias

### Option D — Symmetric finite difference on the table

```text
grad_idx ≈ (rdtable(table, idx + 1) - rdtable(table, idx - 1)) / 2
```

Pros:

- better local approximation for smooth lookup data

Cons:

- doubles the number of extra reads
- still an approximation
- more sensitive to boundary behavior choices

### Option E — Explicit interpolation model

Treat `rdtable(table, idx)` as if it were a continuous linear interpolation
between adjacent samples, then differentiate that interpolant exactly.

Pros:

- mathematically cleaner
- most useful long term

Cons:

- this is not obviously the current Faust signal semantics
- likely larger parity risk
- requires clear agreement on how integer indexing, wrap, and clamping interact

## 6. Recommendation

### Phase 1 recommendation

Adopt **Option D (symmetric finite difference)** for:

- `SIGRDTBL`
- `SIGWAVEFORM` when used through a read-table path

but only when differentiating with respect to the **read index** or an
expression that flows into the read index.

Rationale:

- best practical tradeoff between usefulness and implementation cost
- remains local to the signal graph
- avoids pretending that the table contents themselves are differentiable state
- gives a documented approximation model instead of silent zero tangents

### Explicit boundary

Even with Option D:

- table contents are still treated as constants
- writable table updates remain outside the differentiable subset
- soundfile contents remain outside the differentiable subset

## 7. Proposed Semantics

For a read-only lookup:

```text
y = rdtable(T, i)
```

the transform emits:

```text
y' = dT_di(i) * i'
```

with:

```text
dT_di(i) := (rdtable(T, i + 1) - rdtable(T, i - 1)) / 2
```

where:

- `T` is treated as constant data,
- `i'` is the tangent of the index expression,
- table boundary behavior reuses the existing runtime table-read semantics
  (wrapping/clamping behavior must not be silently invented by FAD).

In other words, FAD differentiates **through the read address**, not through
the table payload itself.

## 8. Scope Slices

### Slice 1 — `rdtable` on read-only tables

Support:

- `fad(rdtable(table, idx), idx_seed)`
- `fad(expr(rdtable(...)), seed)`

when the table source is:

- `waveform(...)`
- read-only generated table

### Slice 2 — waveform-origin tables

Ensure the same rule applies when the read-table source was originally a
waveform literal.

### Slice 3 — recursive read-index cases

Revalidate that the new table-read rule composes with existing recursive FAD:

- scalar recursive phase/index
- nested recursive phase/index
- mutual-recursive index if representable with current corpus style

### Deferred slices

- `wrtbl` / mutable tables
- `soundfile_buffer`
- differentiation through table generator contents

## 9. Impact on Parity

### With Faust C++

This work is expected to be a **deliberate Rust extension**:

- Faust C++ already accepts the table programs themselves,
- but the current Rust FAD model returns zero tangents for these families,
- the proposed extension would produce non-zero tangents in cases the C++
  compiler does not currently model via a dedicated forward-table rule.

Therefore:

- this should be documented as `adapted`, not `1:1`
- it must be called out in the supported-subset document
- tests should verify internal consistency and numeric usefulness, not claim
  byte-for-byte parity with the C++ compiler

## 10. Implementation Sketch

Target file:

- `crates/propagate/src/forward_ad.rs`

Likely shape:

1. detect `SigMatch::RDTbl(table, idx)` or equivalent signal family
2. transform `idx` normally to get `(idx_primal, idx_tangents)`
3. synthesize:
   - `idx_plus = idx_primal + 1`
   - `idx_minus = idx_primal - 1`
   - `sample_plus = rdtbl(table, idx_plus)`
   - `sample_minus = rdtbl(table, idx_minus)`
   - `table_slope = (sample_plus - sample_minus) / 2`
4. emit one tangent lane:
   - `table_slope * idx_tangent`
5. preserve the primal as the original table read

Important invariant:

- the table operand itself stays primal-only unless and until a later design
  explicitly allows differentiating table contents

## 11. Risks

- **Semantic surprise:** users may assume exact differentiation, while the
  implementation is a local approximation.
- **Boundary behavior:** `idx ± 1` inherits whatever wrapping/clamping semantics
  the existing table read has; that must be stated explicitly in docs/tests.
- **Code growth:** each differentiated read adds extra table accesses.
- **Recursive amplification:** recursive table-index systems may magnify local
  approximation error quickly.
- **Future lock-in:** once published, the approximation model becomes part of
  the observable Rust behavior and later changes would need migration notes.

## 12. Tests

### Structural tests

Add to `crates/compiler/tests/signal_pipeline.rs`:

- `fad_table_read_index_compiles_through_full_signal_pipeline`
- `fad_waveform_read_index_compiles_through_full_signal_pipeline`
- `fad_recursive_table_index_compiles_through_full_signal_pipeline`

Corpus candidates:

- `tests/corpus/fad_rdtbl_index_basic.dsp`
- `tests/corpus/fad_waveform_index_basic.dsp`
- `tests/corpus/fad_recursive_waveform_index.dsp`

### Numeric tests

Add to `crates/compiler/tests/fad_recursive_runtime.rs` or a sibling file:

- compare FAD against central finite differences on:
  - read-only table lookup with parameterized index
  - waveform lookup with parameterized index
  - recursive phase accumulator driving a table read

The reference should be:

- finite difference on the **whole DSP output**
- not a reimplementation of the chosen local slope rule in the test itself

### Non-regression tests

- confirm existing zero-tangent behavior remains unchanged for:
  - `wrtbl`
  - `soundfile_buffer`
  - unmatched table-like families

## 13. Documentation Updates Required

If implemented, update:

- `crates/propagate/src/forward_ad.rs`
- `porting/faust-rs-supported-faust-subset-en.md`
- `porting/journal/YYYY-MM-DD.md`

The Rustdoc must explicitly say:

- the rule is an approximation over the read index,
- table contents are still treated as constants,
- writable tables and soundfiles remain out of scope.

## 14. Recommended Execution Order

1. Add this plan and secure sign-off on the semantic choice.
2. Implement read-only `rdtable` differentiation.
3. Add waveform-origin coverage.
4. Add recursive-index coverage.
5. Re-document the support boundary.

## 15. Decision Needed Before Implementation

The only real design decision is the slope model:

- `Option B` backward difference
- `Option C` forward difference
- `Option D` symmetric difference
- or `Option E` explicit interpolation model

My recommendation is still:

- **Option D** first,
- document it as an approximation,
- keep writable tables and soundfiles out of scope until that read-only slice
  is stable.
