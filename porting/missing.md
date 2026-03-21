# Missing Features / Known Gaps

This file documents features or passes that exist in the C++ reference compiler but are
not yet implemented in faust-rs, along with the concrete symptoms they produce.

Each entry points to the relevant porting plan (when one exists) and lists affected test
cases so that progress can be verified once the feature is implemented.

---

## 1. Constant-folding pass (`normalize/simplify.cpp`)

### C++ location
`compiler/normalize/simplify.cpp` — the `simplify(Tree sig)` function, called as part of
`simplifyToNormalForm()` before any backend sees the signal tree.

### What it does
A bottom-up memoised walk of the entire signal tree that rewrites:
- `BinOp(op, Int(a), Int(b))` → `Int(op(a,b))` (and the float equivalent)
- Various algebraic identities: `n*(m*x)` → `(n*m)*x`, `-1*(x-y)` → `y-x`, etc.
- Calls `computeSigOutput` on `xtended` (primitive math) nodes with constant arguments.

The net effect: **all constant arithmetic is fully reduced to a single `Int` or `Real`
node before code generation.**

### Why faust-rs does not have it yet
`signal_prepare.rs` handles domain promotion (float/int), delay normalisation, and a few
signal-level rewrites, but does not perform algebraic constant folding.
See porting plan: `porting/normalize-simplify-port-plan-2026-03-14-en.md`

### Concrete symptom — `tabulateNd_test`

**Test**: `ba.tabulateNd(1, powSin, (8,8, 2.0,2.0, 8.0,8.0, 3.0,4.0)).lin`
**Error**: `[FRS-SFIR-0004] SIGWRTBL currently requires constant integer size in Step 2H`

**Root cause**: `tabulateNd` computes the total table size as `tableSize = size(N)` where
`size(2) = _ * size(1) = _ * _` — a multiplication of the two dimension sizes.  With
inputs `(8, 8)` this becomes `BinOp(Mul, Int(8), Int(8))` in the signal tree.  The C++
`simplify` pass folds this to `Int(64)` before `generateTable` is called; faust-rs does
not, so `table_size_from_sig` sees a non-`Int` node and rejects it.

The reference C++ output confirms a `float ftbl0mydspSIG0[64]` static array — the size
is a compile-time constant `64`.

**Affected tests** (basics_tests.dsp):
- `tabulateNd_test`

**Fix scope**: implement the `simplify` constant-folding pass in `signal_prepare.rs` (or
a dedicated `crates/normalize/` crate), replacing all `BinOp(Int(a), op, Int(b))` nodes
with `Int(op(a,b))` across the whole signal tree before FIR lowering.  A purely local
workaround (teaching `table_size_from_sig` to evaluate constant expressions) would fix
this one call site but leave the same class of bug latent elsewhere (delay amounts, route
parameters, loop bounds, etc.).

