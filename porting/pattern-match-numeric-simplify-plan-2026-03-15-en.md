# Pattern Matcher Numeric Simplification — Bug Analysis & Fix Plan

**Date**: 2026-03-15
**Status**: Ready for implementation
**Affected**: 9 / 90 impulse-test DSP files fail with FRS-EVAL-0099

## Bug description

`carre_volterra.dsp` and 8 other files fail at evaluation with:

```
error [FRS-EVAL-0099] no case rule matches arguments
```

The root cause: the pattern matcher automaton does not constant-fold numeric
arguments before comparing them against literal pattern transitions.

## Reproduction

```bash
./target/release/faust-rs --dump-sig /Users/letz/faust/tests/impulse-tests/dsp/carre_volterra.dsp
# → "no case rule matches arguments"
```

The C++ Faust compiler compiles the same file successfully.

## Analysis

### The Faust code (oscillator.lib)

```faust
sawN(N,freq) = saw1l : poly(Nc) : D(Nc-1) : gate(Nc-1)
with {
  Nc = max(1, min(N, MAX_SAW_ORDER));  // MAX_SAW_ORDER = 4
  poly(1,x) = x;
  poly(2,x) = x*x;
  poly(3,x) = x*x*x - x;
  poly(4,x) = x*x*(x*x - 2.0);
  poly(5,x) = x*(7.0/3 + x*x*(-10.0/3.0 + x*x));
  poly(6,x) = x*x*(7.0 + x*x*(-5.0 + x*x));
  ...
};
```

When called as `sawN(6, freq)`:
- `N = 6`, `Nc = max(1, min(6, 4))` should evaluate to `4`
- `poly(Nc)` is a partial application — the first argument (an integer) is
  matched against the case patterns `1, 2, 3, 4, 5, 6`

### C++ evaluator behaviour

In `patternmatcher.cpp`, `apply_pattern_matcher_internal()`:

```cpp
if (A->state[s]->match_num) {
    X = simplifyPattern(X);
}
```

Where `simplifyPattern()` (in `eval.cpp`) calls `isBoxNumeric()` which does:
1. `boxPropagateSig(nil, box, [])` — propagates the box to a signal
2. `simplify(hd(lsignals))` — algebraically folds `max(1, min(6, 4))` → `4`
3. Returns `boxInt(4)`

This reduces the symbolic `max(...)` expression to the integer `4` before
comparison with pattern constant transitions.

### Rust evaluator behaviour (current)

In `pattern_matcher.rs`, `apply_pattern_matcher_internal()`:

```rust
fn apply_pattern_matcher_internal(
    arena: &TreeArena,       // ← immutable, cannot allocate
    automaton: &Automaton,
    s: usize,
    x: TreeId,               // ← NOT simplified
    substs: &mut [Subst],
) -> Option<usize> {
    let state = &automaton.states[s];
    // match_num flag is COMPUTED but NEVER USED
    for trans in &state.trans {
        if let Some(cst) = trans.is_const() {
            if x == cst {    // ← symbolic max(...) ≠ int(4) → FAILS
```

The `match_num` flag is correctly propagated during automaton construction
(`build_automaton_metadata`, line 671) but never consulted during matching.

### Existing infrastructure (ready to use)

All needed functions already exist in `eval/src/lib.rs` but are marked
`#[allow(dead_code)]`:

| Function | Line | Purpose |
|----------|------|---------|
| `is_box_numeric()` | 3022 | Propagate + simplify → `boxInt`/`boxReal` or `None` |
| `propagate_box_and_simplify()` | 3007 | Core: flatten box → propagate → simplify_const |
| `box_simplification()` | 3206 | Memoised recursive simplification |

## Fix plan

### Step 1: Activate dead code in `crates/eval/src/lib.rs`

- Remove `#[allow(dead_code)]` from `is_box_numeric`, `propagate_box_and_simplify`,
  `box_simplification`, `numeric_box_simplification`, `inside_box_simplification`.
- Change visibility of `is_box_numeric` to `pub(crate)`.
- Add a `pub(crate) simplify_pattern` function:

```rust
/// Mirrors C++ `simplifyPattern()` in `eval.cpp` line 136.
/// Tries to reduce a box to a numeric literal; returns original on failure.
pub(crate) fn simplify_pattern(arena: &mut TreeArena, x: TreeId) -> TreeId {
    is_box_numeric(arena, x).unwrap_or(x)
}
```

### Step 2: Wire simplification into `crates/eval/src/pattern_matcher.rs`

1. Change `apply_pattern_matcher_internal` signature:
   `arena: &TreeArena` → `arena: &mut TreeArena`

2. Add `match_num` guard at the top of the function body:
   ```rust
   let x = if state.match_num {
       crate::simplify_pattern(arena, x)
   } else {
       x
   };
   ```

3. Update all recursive calls (5 sites) to pass `arena` as `&mut`.

### Step 3: Verify

```bash
# Direct test
./target/release/faust-rs --dump-sig carre_volterra.dsp

# Full impulse suite
for f in /Users/letz/faust/tests/impulse-tests/dsp/*.dsp; do
  ./target/release/faust-rs --dump-sig --timeout 120 "$f" >/dev/null 2>&1 \
    || echo "FAIL: $(basename $f)"
done

# Unit tests
cargo test -p eval
```

## Files to modify

| File | Change |
|------|--------|
| `crates/eval/src/lib.rs` | Remove dead_code, add `pub(crate) simplify_pattern` |
| `crates/eval/src/pattern_matcher.rs` | `&mut TreeArena`, `match_num` simplification |

## Affected impulse-test files (all 9 expected to pass after fix)

All use `oscillator.lib` functions that rely on pattern-matched case rules
with computed integer arguments (`sawN`, `pulsetrainN`, etc.):

1. `carre_volterra.dsp`
2. `gate_compressor.dsp`
3. `parametric_eq.dsp`
4. `phaser_flanger.dsp`
5. `pitch_shifter.dsp`
6. `spectral_tilt.dsp`
7. `thru_zero_flanger.dsp`
8. `vcf_wah_pedals.dsp`
9. `virtual_analog_oscillators.dsp`
