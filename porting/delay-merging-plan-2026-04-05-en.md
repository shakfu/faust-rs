# Plan: General `occMarkup`-Equivalent Pass + Delay/Recursion Buffer Merging

Date: 2026-04-05

## 1. Problem Statement

For `process = + ~ @(10);`, C++ Faust generates **one** buffer (`fRec0[12]`),
while faust-rs generates **two**: `fRec33[2]` (recursion) + `fVec17[16]` (delay).

The signal structure after propagation:
```
Proj(0, Rec(body = [BinOp(Add, Delay(Delay1(Proj(0, self)), 10), Input(0))]))
```

`Delay1` comes from `~` (implicit 1-sample feedback), `Delay(..., 10)` from
`@(10)`.  Total history needed from the recursion output: 11 samples.  A single
`fRec[16]` circular buffer suffices — reading at `(fIOTA - 11) & 15` replaces
the separate `fVec`.

More generally, any delay chain applied to a signal that already lives in a
stateful array (recursion output, delay1 state) can be served from that same
array if it is sized large enough.

## 2. How C++ Faust Handles This

### 2.1 The `occMarkup` Pre-Pass

C++ Faust runs an **occurrence-markup** pass (`compiler/transform/occMarkup.cpp`)
before code generation.  It walks the signal DAG top-down and tracks, for each
signal node:

- **`fOcc`** (occurrence count): how many times the signal is referenced.
- **`fMaxDelay`**: the maximum number of samples by which any consumer delays
  this signal.

When the walker encounters `SIGDELAY(x, n)`, it adds `n` to the accumulated
delay and recurses into `x`.  Similarly, `SIGDELAY1(x)` adds 1.  The delay
is propagated transitively: for `Delay(Delay1(Proj), 10)`, the projection
receives `fMaxDelay = 11` (= 10 + 1).

### 2.2 How `fMaxDelay` Drives Codegen

In `signalFIRCompiler`, when lowering a recursion group:

1. Call `getMaxDelay(proj)` on the recursion output signal.
2. Allocate a single delay buffer sized to `maxDelay + 1`:
   - If `maxDelay + 1 <= gMaxCopyDelay` (default 16): **linear shift buffer**
     with a copy loop (`for (j=N; j>0; j--) fRec[j] = fRec[j-1]`).
   - If `maxDelay + 1 > gMaxCopyDelay`: **circular buffer** with `fIOTA` masking.
3. All reads at different offsets (Delay1 → offset 1, @(10) → offset 11) share
   the **same buffer**, just indexed at different offsets.

### 2.3 `fOutDelayOcc` / `fCountDelay` Optimization

The C++ compiler also tracks `fOutDelayOcc` (number of uses at delay > 0) and
`fCountDelay` (total delay uses).  When a signal is **only** read at offset 0,
the delay buffer is skipped entirely (scalar state).  This is a code-quality
optimization, not a correctness requirement.

## 3. Current faust-rs Architecture

All code in `crates/transform/src/signal_fir/module.rs`.

### 3.1 Buffer Types

| Type | Prefix | Size | Mechanism |
|------|--------|------|-----------|
| Recursion array | `fRec`/`iRec` | **always 2** (hardcoded) | `ensure_recursion_array_for_group` (line 3421) |
| Delay1 state slot | `fRec`/`iRec` | **always 2** (hardcoded) | `ensure_state_slot` (line 1757) |
| Fixed SIGDELAY | `fVec`/`iVec` | `pow2(delay + 1)` | `ensure_delay_line_decl` (line 1783) |

`RecArrayInfo { name, typ }` has no `size` field.
`DelayLineInfo { name, size }` carries its size.

### 3.2 Pre-Scan Phase

`prepare_delay_lines` (line 962) runs before any lowering:

1. `scan_delay_lines` walks the DAG, matching `SigMatch::Delay(value, amount)`.
2. Records `max_delays: HashMap<SigId, i32>` keyed by the **carried signal**.
3. Calls `ensure_delay_line_decl(carried, max_delay)` for each entry → `fVec`.

This only sees **immediate** SIGDELAY nodes.  It does **not** propagate delays
transitively through Delay1, other Delays, or back into recursion projections.

### 3.3 Lowering Phase

- `lower_proj` (line 3280): allocates `fRec[2]`, writes at `fIOTA & 1`, reads
  at `fIOTA & 1` (fast path) or `(fIOTA - 1) & 1` (feedback via Delay1).
- `lower_delay_state` (line 1645): for `Delay1(Proj(i, group))` in active
  recursion, reuses the recursion array and reads at `(fIOTA - 1) & 1`.
  For other Delay1, allocates a separate `fRec[2]`.
- `lower_fixed_delay` (line 1600): gets/creates `fVec`, lowers carried signal,
  writes to `fVec`, reads at offset.

For `+ ~ @(10)`:
1. `scan_delay_lines` sees `SIGDELAY(Delay1(Proj), 10)` → records
   `max_delays[Delay1_node] = 10` → allocates `fVec17[16]`.
2. `lower_proj` allocates `fRec33[2]` for the recursion.
3. `lower_fixed_delay` lowers `Delay1(Proj)` (via recursion feedback fast path
   → `fRec33[(fIOTA-1) & 1]`), copies that value to `fVec17`, reads from
   `fVec17` at offset 10.

Result: two buffers, one copy per sample.

### 3.4 All Hardcoded Size-2 References

| Site | Code |
|------|------|
| `ensure_recursion_array_for_group` line 3443 | `FirType::Array(…, 2)` |
| `ensure_state_slot` line 1768 | `FirType::Array(…, 2)` |
| `register_clear_recursion_array` line 1947 | `b.int32(2)` |
| `lower_proj` fast path line 3299 | `masked_delay_index(iota, 2)` |
| `lower_proj` body write line 3373 | `masked_delay_index(iota, 2)` |
| `lower_proj` result return line 3393 | `masked_delay_index(iota, 2)` |
| `lower_delay_state` feedback line 1660 | `delayed_iota_index(one, 2)` |
| `lower_delay_state` non-feedback line 1674 | `delayed_iota_index(one, 2)` |
| `lower_delay_state` write line 1684 | `masked_delay_index(iota, 2)` |

## 4. General `occMarkup`-Equivalent Design

### 4.1 Overview

Add a **max-delay propagation pass** (`compute_max_delays`) that runs alongside
the existing `scan_delay_lines` in `prepare_delay_lines`.  It computes, for
every signal node that contributes to a stateful carrier (recursion projection
or delay1 state), the maximum total delay at which any consumer accesses it.

This is the Rust equivalent of C++ `occMarkup::incOcc` with delay propagation.

### 4.2 Data Structure

```rust
/// Maximum delay at which each signal is accessed by any consumer.
/// Key: SigId of the signal being delayed (e.g. Proj node, Delay1 node).
/// Value: max total delay in samples.
sig_max_delay: HashMap<SigId, i32>,
```

Field on `SignalToFirLower`, populated during `prepare_delay_lines`, consulted
during `ensure_recursion_array_for_group` and `ensure_state_slot`.

### 4.3 Algorithm: `compute_max_delays`

Walk the signal DAG top-down with a `(sig, accumulated_delay)` work stack.
`accumulated_delay` starts at 0 for each output signal.

```
fn compute_max_delays(outputs) -> HashMap<SigId, i32>:
    result = HashMap::new()
    visited = HashMap::new()   // SigId -> max accumulated_delay seen so far
    stack = [(output, 0) for output in outputs]

    while stack not empty:
        (sig, acc) = stack.pop()

        // Prune: if we've visited this node with >= acc, skip
        if visited[sig] >= acc:
            continue
        visited[sig] = acc

        // Record: if acc > 0, this signal is accessed at a delay
        if acc > 0:
            result[sig] = max(result[sig], acc)

        match match_sig(sig):
            Delay(value, amount):
                n = constant_delay_amount(amount)  // or interval-based
                if n is Some:
                    // The carried signal is accessed at acc + n
                    stack.push((value, acc + n))
                    // The amount signal is accessed at acc + 0 (no extra delay)
                    stack.push((amount, acc))
                else:
                    // Variable delay — cannot propagate further
                    // Record carried signal at acc level, fall through
                    stack.push((value, acc))
                    stack.push((amount, acc))

            Delay1(value):
                // Adds 1 to the delay chain
                stack.push((value, acc + 1))

            Prefix(init, value):
                // Prefix is semantically a Delay1 variant
                stack.push((value, acc + 1))
                stack.push((init, 0))

            Proj(i, group):
                // Record the delay on the projection
                // Then recurse into the recursion body (with acc=0 from the
                // body's perspective — the delay is on the *output*, not
                // propagated into the body's own signal graph)
                recurse_into_group_bodies(group, stack, acc=0)

            other:
                // Recurse into children with acc=0 (delay does NOT propagate
                // through arithmetic, math, select2, etc.)
                for child in children(sig):
                    stack.push((child, 0))
```

Key semantics:
- **Delay adds to accumulator**: `SIGDELAY(x, N)` pushes `(x, acc + N)`.
- **Delay1 adds 1**: `SIGDELAY1(x)` pushes `(x, acc + 1)`.
- **Non-delay nodes reset accumulator**: `BinOp(Add, a, b)` pushes
  `(a, 0), (b, 0)`.  Delay does not propagate through computation.
- **Recursion bodies**: when visiting `Proj(i, group)`, the delay is recorded
  on the Proj node.  The body itself is walked with `acc = 0` (the body's
  internal delay chains are independent).
- **DAG sharing**: a node can be reached at different accumulated delays.  We
  record the **maximum** and only re-walk if a higher delay is found.

### 4.4 Why Delay Doesn't Propagate Through Computation

Consider `Delay(BinOp(Add, x, y), 10)`.  The delay applies to the
**result** of the Add, not to `x` and `y` individually.  You cannot serve
the delayed Add from `x`'s buffer — you need the Add result itself to
be stored.  So the accumulator resets to 0 at non-delay nodes.

The important chain is: `Delay → Delay1 → Delay → ... → Proj`.  As long as
the path from the outer Delay to the Proj goes exclusively through delay
nodes (Delay, Delay1, Prefix), the delays accumulate and the Proj's buffer
can serve all of them.

### 4.5 Integration with Existing `scan_delay_lines`

`prepare_delay_lines` becomes:

```rust
fn prepare_delay_lines(&mut self, outputs: &[SigId]) -> Result<(), …> {
    // Phase 1: compute transitive max delays
    self.compute_max_delays(outputs)?;

    // Phase 2: collect immediate SIGDELAY carriers (existing logic)
    let mut max_delays: HashMap<SigId, i32> = HashMap::new();
    let mut seen = HashSet::new();
    for output in outputs {
        self.scan_delay_lines(*output, &mut seen, &mut max_delays)?;
    }

    // Phase 3: remove carriers that will be merged into recursion/state arrays
    max_delays.retain(|carried, _| !self.is_merged_into_stateful_array(*carried));

    // Phase 4: allocate fVec delay lines only for non-merged carriers
    for (carried, delay) in max_delays {
        self.ensure_delay_line_decl(carried, delay)?;
    }
    Ok(())
}
```

`is_merged_into_stateful_array(carried)` returns true when:
- `carried` matches `Delay1(inner)` or `Prefix(_, inner)` AND
- `inner` matches `Proj(i, SYMREF(var))` AND
- `sig_max_delay[inner]` exists (meaning the Proj is accessed at delay > 0,
  so the recursion array will be sized to accommodate it)

### 4.6 Using `sig_max_delay` for Buffer Sizing

#### Recursion arrays (`ensure_recursion_array_for_group`)

When allocating for group output `(group, index)`:

1. Find the `Proj(index, group)` node (the projection SigId).
2. Look up `sig_max_delay[proj_sig]`.
3. If found: `size = pow2limit(max_delay + 1)` (the +1 accounts for the
   current write slot).
4. If not found: `size = 2` (only used for Delay1 feedback at offset 1).

The Proj SigId is available because it was the `node` argument to `lower_proj`
which called `ensure_recursion_array_for_group`.  We can store it:

```rust
fn ensure_recursion_array_for_group(
    &mut self, group: SigId, index: usize, typ: FirType, init: FirId,
    proj_sig: Option<SigId>,  // NEW: the Proj node, for max-delay lookup
) -> Result<RecArrayInfo, …>
```

#### Delay1 state slots (`ensure_state_slot`)

Currently always size 2.  With `sig_max_delay`, a Delay1 node whose output
is further delayed (e.g. `Delay(Delay1(x), 10)`) could be sized larger.
However, the recursion-feedback case is the primary target; non-recursion
Delay1 state already uses size 2 correctly.  **Defer** general Delay1 state
upsizing to a follow-up.

### 4.7 Delay Folding During Lowering

#### In `lower_fixed_delay` (line 1600)

Add an early-exit before `ensure_delay_line_decl`:

```rust
// Merged recursion delay: Delay(Delay1(Proj(i, active_group)), N)
// → single read from recursion array at offset N+1.
if let SigMatch::Delay1(inner) = match_sig(self.arena, value) {
    if let Some(rec_info) = self.recursion_feedback_info(inner)? {
        // The recursion array is already sized for max_delay.
        self.ensure_iota_state();
        let total_offset = self.lower_int32_const(delay + 1);
        let read_index = self.delayed_iota_index(total_offset, rec_info.size);
        let read_ty = self.signal_fir_type(node)?;
        let mut b = FirBuilder::new(&mut self.store);
        return Ok(b.load_table(
            rec_info.name, AccessType::Struct, read_index, read_ty,
        ));
    }
}
```

This handles the primary pattern: `Delay(Delay1(Proj(i, self)), N)`.

#### Chained delays: `Delay(Delay(Delay1(Proj), M), N)`

If `signal_prepare` doesn't merge nested delays, the inner Delay would create
a `fVec` and the outer Delay would reference it.  To handle this, the folding
could unwrap delay chains recursively:

```rust
fn unwrap_delay_chain(&self, sig: SigId) -> (SigId, i32) {
    // Unwrap Delay/Delay1 chain, accumulating total offset.
    let mut current = sig;
    let mut total = 0i32;
    loop {
        match match_sig(self.arena, current) {
            SigMatch::Delay1(inner) => { total += 1; current = inner; }
            SigMatch::Delay(inner, amount) => {
                match self.delay_size_for_amount(amount) {
                    Ok(Some(n)) => { total += n; current = inner; }
                    _ => break,
                }
            }
            _ => break,
        }
    }
    (current, total)
}
```

Then in `lower_fixed_delay`:

```rust
let (root, total_offset) = self.unwrap_delay_chain_from(value, delay);
if let Some(rec_info) = self.try_recursion_feedback(root)? {
    // Read directly from recursion array at total_offset
    …
}
```

**Note**: this general unwrapping is an enhancement.  The initial
implementation should focus on the single `Delay(Delay1(Proj), N)` pattern,
which covers the `~ @(N)` case.  The chain unwrapping can be added later if
patterns like `~ (@(M) : @(N))` need it.

## 5. Implementation Steps

All in `crates/transform/src/signal_fir/module.rs`:

### Step 1: Add `size` to `RecArrayInfo`

```rust
struct RecArrayInfo {
    name: String,
    typ: FirType,
    size: usize,  // NEW
}
```

Initialize `size: 2` everywhere.  No behavioral change.

### Step 2: Replace all hardcoded size-2 masks

Use `info.size` / `rec_info.size` instead of literal `2` at all 9 sites listed
in Section 3.4.  Since size is still 2, all tests pass unchanged.

For `register_clear_recursion_array`, add a `size: usize` parameter:
```rust
fn register_clear_recursion_array(&mut self, name: String, init: FirId, size: usize)
```
Replace `b.int32(2)` with `b.int32(size as i32)`.

### Step 3: Add `sig_max_delay` field and `compute_max_delays`

Add field:
```rust
sig_max_delay: HashMap<SigId, i32>,
```

Implement the top-down DAG walk described in Section 4.3.  Call it at the
start of `prepare_delay_lines`.

### Step 4: Wire `sig_max_delay` into recursion array sizing

In `ensure_recursion_array_for_group`, look up the Proj node's max delay:
- If `sig_max_delay[proj]` exists: `size = pow2limit(max_delay + 1)`
- Otherwise: `size = 2`

This requires passing the Proj SigId to `ensure_recursion_array_for_group`.
Add an optional `proj_sig: Option<SigId>` parameter, used when called from
`lower_proj` (which has the Proj node).

### Step 5: Skip `fVec` allocation for merged carriers

In `prepare_delay_lines`, after collecting `max_delays`, filter out entries
whose carried signal will be served by a recursion array.  This prevents the
`fVec` from being allocated.

Detection: for each `(carried, delay)` in `max_delays`, check if `carried`
is `Delay1(Proj(i, SYMREF(var)))` and `sig_max_delay` has an entry for the
inner Proj.  If so, remove from `max_delays`.

### Step 6: Add delay folding in `lower_fixed_delay`

At the top of `lower_fixed_delay`, before `ensure_delay_line_decl`:

```rust
if let SigMatch::Delay1(inner) = match_sig(self.arena, value) {
    if let Some(rec_info) = self.recursion_feedback_info(inner)? {
        self.ensure_iota_state();
        let total_offset = self.lower_int32_const(delay + 1);
        let read_index = self.delayed_iota_index(total_offset, rec_info.size);
        let read_ty = self.signal_fir_type(node)?;
        let mut b = FirBuilder::new(&mut self.store);
        return Ok(b.load_table(
            rec_info.name, AccessType::Struct, read_index, read_ty,
        ));
    }
}
```

### Step 7: Tests

- Existing corpus tests must all pass.
- Add test for `process = + ~ @(10);`: verify single `fRec[16]`, no `fVec`.
- Add test for `process = + ~ @(100);`: verify `fRec[128]`, no `fVec`.
- Add test for `process = + ~ _;`: verify `fRec[2]`, unchanged.
- Differential test: compare faust-rs impulse output with C++ on these DSPs.

## 6. Expected Output

### `process = + ~ @(10);`

```cpp
int fIOTA;
float fRec33[16];    // single merged buffer
// NO fVec17

void compute(…) {
    for (int i0 = 0; i0 < count; ++i0) {
        int fTemp0 = (fIOTA & 15);
        fRec33[fTemp0] = (float(input0[i0]) + fRec33[((fIOTA - 11) & 15)]);
        output0[i0] = (FAUSTFLOAT)(fRec33[fTemp0]);
        fIOTA = (fIOTA + 1);
    }
}
```

### `process = + ~ _;` (unchanged)

```cpp
float fRec33[2];     // still size 2
```

## 7. Edge Cases

- **`+ ~ _`** (no explicit delay): `sig_max_delay[Proj]` has max=1 from
  Delay1.  `pow2limit(1+1) = 2`.  Size stays 2.  Unchanged behavior.

- **`+ ~ @(0)`**: zero delay, `scan_delay_lines` skips it (line 996).
  Only Delay1 remains → max=1 → size 2.  Unchanged.

- **Both Delay1 and @(N) on same output**: buffer sized for
  `pow2limit(N+1+1)`.  Delay1 reads at offset 1, @(N) reads at offset N+1.
  Both use same mask.  Correct.

- **Multiple projections with different delays**: each `(group, index)` has
  independent sizing via `sig_max_delay[Proj_i]`.

- **Nested recursion**: `recursion_feedback_info` walks the stack per group.
  Each group's Proj gets its own max-delay entry.

- **Non-recursion `Delay(Delay1(x))`**: `recursion_feedback_info(x)` returns
  None.  Falls through to normal `fVec` path.  Unchanged.

- **Variable delay `Delay(Delay1(Proj), slider)`**: `delay_size_for_amount`
  returns interval-based bound.  The `compute_max_delays` walk can use the
  same bound.  Works transparently.

## 8. Future Extensions

- **Delay chain unwrapping** for `~ (@(M) : @(N))` patterns:
  `unwrap_delay_chain` described in Section 4.7.
- **Non-recursion Delay1 upsizing**: when `Delay(Delay1(x), N)` and `x` is
  not a recursion projection, the Delay1 state slot could be upsized from 2
  to `pow2limit(N+2)` to merge the buffers.
- **`fOutDelayOcc` optimization**: skip delay buffer when a signal is only
  accessed at delay 0 (scalar state suffices).
- **Shift-buffer strategy** for small delays (matching C++ `gMaxCopyDelay`
  threshold): use a linear shift loop instead of circular buffer+mask when
  `maxDelay + 1 <= 16`.
