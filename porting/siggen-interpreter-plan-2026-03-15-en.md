# SIGGEN Computed Table Init — Compile-time Signal Interpreter

**Date**: 2026-03-15
**Status**: Ready for implementation
**Blocks**: `osc.dsp` and all DSP using `os.osc` (table-based oscillators)

## Context

`osc.dsp` (and any DSP using `os.osc`) fails with FRS-SFIR-0004 because
`expand_generator_values()` only supports constant generators (waveform,
int, real). The `os.osc` oscillator uses a recursive phasor `+(1)~_` inside
`sin()` for table init — a computed generator that is rejected.

This is **pre-existing** (not a regression from boxClosure). The C++ compiler
handles this with a runtime sub-container (`signal2Container`) that executes
the generator DSP at init time. Instead of that complexity, we can **interpret
the generator signal at compile time** since SIGGEN generators are always
0-input deterministic DSP.

## C++ Reference

```cpp
// instructions_compiler.cpp — C++ creates a runtime sub-container
ValueInst* InstructionsCompiler::generateSigGen(Tree sig, Tree content) {
    string cname = "SIG<ID>";
    CodeContainer* subcontainer = signal2Container(cname, content);
    fContainer->addSubContainer(subcontainer);
    // Instantiate subcontainer at init time, fill table by running DSP
}
```

## Approach: Compile-time Signal Interpreter

Add a function `interpret_generator(arena, sig, size) -> Result<Vec<f64>>`
that runs the generator signal for `size` steps, collecting output values.
This is called from `expand_generator_values()` as a fallback when the
generator is not a simple constant/waveform.

The interpreter maintains:
- A `HashMap<SigId, (Vec<f64>, Vec<f64>)>` for recursion group state
  (current and previous values per group)
- Evaluates the signal tree recursively for each step

### Signal nodes to interpret

For `os.osc` and common table generators:

| Node | Interpretation |
|------|---------------|
| `Int(v)` | `v as f64` |
| `Real(v)` | `v` |
| `BinOp(op, x, y)` | `eval(x) op eval(y)` |
| `FloatCast(x)` | `eval(x)` (already f64) |
| `IntCast(x)` | `eval(x) as i32 as f64` |
| `Sin(x)` | `eval(x).sin()` |
| `Cos(x)` | `eval(x).cos()` |
| `Sqrt/Exp/Log/...` | Standard math |
| `Proj(idx, group)` | Evaluate recursion group, return output `idx` |
| `Delay1(x)` | Read previous-step value of x |

### Recursion handling

For `Proj(idx, group)` where group is `Rec(body)`:
1. On first encounter, register the group in the state map (zeros)
2. Each step: evaluate body elements with current state → update state
3. `Delay1(Proj(idx, ref))` reads the **previous step** value

The phasor `+(1)~_` is: `Rec(cons(add(delay1(proj(0, ref)), 1), nil))`
- Step 0: prev=0, output = 0+1 = 1
- Step 1: prev=1, output = 1+1 = 2
- Step n: prev=n, output = n+1

Then: `sin(2*PI * float(output - 1) / 65536)` produces the sine table.

## Implementation Steps

### Step 1 — Add `GeneratorInterpreter` struct

**File**: `crates/transform/src/signal_fir/module.rs`

```rust
struct GeneratorInterpreter<'a> {
    arena: &'a TreeArena,
    /// Recursion group state: (current_values, prev_values)
    rec_state: HashMap<SigId, (Vec<f64>, Vec<f64>)>,
    /// Groups currently being evaluated (prevent infinite recursion)
    evaluating: HashSet<SigId>,
}

impl<'a> GeneratorInterpreter<'a> {
    fn new(arena: &'a TreeArena) -> Self { ... }
    fn eval(&mut self, sig: SigId) -> Result<f64, SignalFirError> { ... }
    fn advance(&mut self) { /* swap current → prev for all groups */ }
}
```

### Step 2 — Implement `eval` method

Pattern-match on `SigMatch` variants:
- Constants: `Int`, `Real` → direct conversion
- Arithmetic: `BinOp` → evaluate both operands, apply op
- Math functions: `Sin`, `Cos`, `Sqrt`, `Exp`, `Log`, etc. → Rust std math
- Casts: `FloatCast`, `IntCast` → type conversion
- Recursion: `Proj(idx, group)` → evaluate group body, return output `idx`
- State: `Delay1(x)` → read previous-step value from recursion state
- Unsupported: `Input`, `HSlider`, etc. → error (should not appear in generators)

### Step 3 — Update `expand_generator_values`

Replace the catch-all error with:

```rust
_ => {
    let values = interpret_generator(self.arena, init_sig, size)?;
    let mut out = Vec::with_capacity(size);
    for v in values {
        let mut b = FirBuilder::new(&mut self.store);
        out.push(if v == (v as i32) as f64 {
            b.int32(v as i32)  // Keep integers as int for type parity
        } else {
            b.float64(v)
        });
    }
    Ok(out)
}
```

### Step 4 — Handle cons-list bodies for multi-output recursion

For `Rec(body)` where body is a cons-list:
- Extract body signals via `arena.hd()`/`arena.tl()` iteration
- Each element is one output of the recursion group
- Evaluate all elements per step, store in current_values

## Files Modified

| File | Changes |
|------|---------|
| `crates/transform/src/signal_fir/module.rs` | `GeneratorInterpreter`, `interpret_generator`, update `expand_generator_values` fallback |

## Verification

1. `cargo test` — all existing tests pass
2. `faust-rs osc.dsp` — compiles to C++ (currently fails with FRS-SFIR-0004)
3. `faust-rs --dump-sig osc.dsp` — signals already OK (sanity check)
4. Compare C++ output with reference: `faust -lang cpp osc.dsp`
5. Test other table-based DSP files from impulse-tests

## Risk assessment

| Risk | Mitigation |
|------|-----------|
| Generator uses UI controls | Reject with clear error — UI controls don't appear in SIGGEN |
| Generator uses inputs | Reject — SIGGEN is 0-input by definition |
| Very large tables (>100k) | Same as current waveform path — just more values |
| Floating-point divergence vs C++ | Acceptable — same math functions, negligible difference |
