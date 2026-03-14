# Signal Simplification / Normalize Port Plan

**Date**: 2026-03-14
**Source**: `/Users/letz/Developpements/RUST/faust/compiler/normalize/`
**Target**: `crates/normalize/src/`
**Status**: scaffold → full parity

---

## Context

The C++ `normalize/` directory implements the algebraic simplification of Faust signals before code generation. Without it, the compiler cannot fold constants, factor shared sub-expressions, simplify delay patterns, or produce the canonical normal form required by the FIR lowering pass.

The Rust crate `crates/normalize/` already exists but contains only a scaffold (`pub fn crate_id()`). This plan details the five-step port to full C++ parity.

---

## C++ Architecture (5 files → 5 Rust modules)

```
normalform.cpp        ← main pipeline (simplifyToNormalForm)
    └── simplify.cpp  ← rewrite engine + memoised graph traversal (sigMap)
        └── normalize.cpp ← add-term and delay normalization
            ├── aterm.cpp ← additive term  (sum of mterms)
            └── mterm.cpp ← multiplicative term (k·x^n·y^m / …)
```

---

## Target Rust modules

All under `crates/normalize/src/`:

| Rust file | C++ source | Role |
|---|---|---|
| `mterm.rs` | `mterm.hh/.cpp` (14 KB) | Multiplicative algebraic term |
| `aterm.rs` | `aterm.hh/.cpp` (8 KB) | Additive algebraic term |
| `normalize.rs` | `normalize.hh/.cpp` (5 KB) | Add-term + delay normalization |
| `simplify.rs` | `simplify.hh/.cpp` (13 KB) | Rewrite engine + sigMap |
| `normalform.rs` | `normalform.hh/.cpp` (6 KB) | **Phase 1 scope only** (see Step 5) |
| `lib.rs` | — | Public re-exports |

**Add to `crates/normalize/Cargo.toml`**:
```toml
[dependencies]
tlib     = { path = "../tlib" }
signals  = { path = "../signals" }
sigtype  = { path = "../sigtype" }
interval = { path = "../interval" }
```

---

## Step 1 — `mterm.rs`: multiplicative term

**C++ key members**:
```cpp
Tree fCoef;                  // numeric constant coefficient
std::map<Tree, int> fFactors;  // signal base → integer exponent
```

**Rust**:
```rust
pub struct Mterm {
    coef: SigId,                    // SigInt or SigReal node
    factors: BTreeMap<SigId, i32>,  // deterministic order like std::map
}
```

`SigId = TreeId = u32`. Use `BTreeMap` to match `std::map` ordering determinism.

### Functions to port

| C++ | Rust signature | Notes |
|---|---|---|
| `mterm()` | `Mterm::zero()` | coefficient = int(0) |
| `mterm(int k)` | `Mterm::from_int(arena, k)` | |
| `mterm(double k)` | `Mterm::from_real(arena, k)` | |
| `mterm(Tree t)` | `Mterm::from_sig(arena, t)` | recursive decomposition of mul/div |
| `operator*=(Tree)` | `fn mul_sig(&mut self, arena, t)` | |
| `operator/=(Tree)` | `fn div_sig(&mut self, arena, t)` | |
| `operator*=(mterm)` | `fn mul_mterm(&mut self, arena, m)` | |
| `operator/=(mterm)` | `fn div_mterm(&mut self, arena, m)` | |
| `operator+=(mterm)` | `fn add_mterm(&mut self, arena, m)` | same signature required |
| `operator-=(mterm)` | `fn sub_mterm(&mut self, arena, m)` | |
| `cleanup()` | `fn cleanup(&mut self)` | remove factors with exponent 0 |
| `isNotZero()` | `fn is_not_zero(&self, arena)` | |
| `isNegative()` | `fn is_negative(&self, arena)` | |
| `normalizedTree(sign, neg)` | `fn normalized_tree(arena, sign, neg)` | canonical reconstruction |
| `signatureTree()` | `fn signature_tree(arena)` | without coefficient |
| `hasDivisor(d)` | `fn has_divisor(&self, d: &Mterm)` | |
| `complexity()` | `fn complexity(&self, arena)` | order-weighted metric |
| `gcd(m1, m2)` | `fn gcd(arena, m1, m2) -> Mterm` | free function |
| `isSigPow(t, x, n)` | `fn is_sig_pow(arena, t) -> Option<(SigId, i32)>` | |
| `sigPow(x, p)` | `fn sig_pow(arena, x, p) -> SigId` | |

**Numeric helpers**: use `match_sig(arena, id)` from `signals` to test `SigMatch::Int(n)` / `SigMatch::Real(r)`. Reconstruct result nodes via `SigBuilder`.

**Signal order**: the C++ `getSigOrder()` comes from `sigorderrules.hh`. Check `sigtype`; if absent, add `order.rs` in `normalize`:
```rust
pub fn sig_order(type_annotator: &TypeAnnotator, sig: SigId) -> i32 {
    match type_annotator.get_variability(sig) {
        Variability::KSamp  => 0,   // constant
        Variability::KBlock => 1,   // block-rate
        Variability::KSamp  => 2,   // control-rate (samp)
        _                   => 3,   // audio-rate
    }
}
```

---

## Step 2 — `aterm.rs`: additive term

**C++ key member**:
```cpp
std::map<Tree, mterm> fSig2MTerms;  // signature → mterm
```

**Rust**:
```rust
pub struct Aterm {
    terms: BTreeMap<SigId, Mterm>,  // signature_tree(m) → m
}
```

### Functions to port

| C++ | Rust signature |
|---|---|
| `aterm()` | `Aterm::zero()` |
| `aterm(Tree t)` | `Aterm::from_sig(arena, t)` — decomposes add/sub chains |
| `operator+=(Tree)` | `fn add_sig(&mut self, arena, t)` |
| `operator-=(Tree)` | `fn sub_sig(&mut self, arena, t)` |
| `operator+=(mterm)` | `fn add_mterm(&mut self, arena, m)` |
| `operator-=(mterm)` | `fn sub_mterm(&mut self, arena, m)` |
| `normalizedTree()` | `fn normalized_tree(arena) -> SigId` |
| `greatestDivisor()` | `fn greatest_divisor(arena) -> Mterm` |
| `factorize(d)` | `fn factorize(arena, d: &Mterm) -> Aterm` |
| `simplifyingAdd(t1,t2)` | `fn simplifying_add(arena, t1, t2) -> SigId` — free fn, numeric fold |

**Reconstruction strategy**: four orders (0=const, 1=block, 2=ctrl, 3=audio). Separate positive/negative mterms per order, combine within each order, then fold orders from highest to lowest.

---

## Step 3 — `normalize.rs`: add-term and delay normalization

**Public API**:
```rust
pub fn normalize_add_term(arena: &mut TreeArena, t: SigId) -> SigId;
pub fn normalize_delay1_term(arena: &mut TreeArena, s: SigId) -> SigId;
pub fn normalize_delay_term(arena: &mut TreeArena, s: SigId, d: SigId) -> SigId;
pub fn clock_normalize_delay_term(
    arena: &mut TreeArena, clock: SigId, s: SigId, d: SigId
) -> SigId;
```

### `normalize_add_term` algorithm

1. `let mut a = Aterm::from_sig(arena, t)`
2. Loop: `d = a.greatest_divisor()` — if not identity → `a = a.factorize(d)`, accumulate `d*quotient + remainder`, repeat
3. Return `a.normalized_tree(arena)`

### `normalize_delay_term` rules

| Pattern | Result |
|---|---|
| `s @ 0` | `s` |
| `0 @ d` | `0` |
| `(k*s) @ d` where `order(k) < 2` | `k * (s @ d)` |
| `(s/k) @ d` where `order(k) < 2` | `(s @ d) / k` |
| `(x @ n) @ m` where `n` is constant | `x @ (n + m)` |

---

## Step 4 — `simplify.rs`: rewrite engine  *(most complex step)*

**Context struct**:
```rust
pub struct Simplifier<'a> {
    arena: &'a mut TreeArena,
    cache: HashMap<SigId, SigId>,   // replaces C++ setProperty(SIMPLIFIED)
}
```

**Public functions**:
```rust
pub fn simplify(arena: &mut TreeArena, sig: SigId) -> SigId;
pub fn doc_table_conversion(arena: &mut TreeArena, sig: SigId) -> SigId;
```

### `sig_map` — memoised recursive graph traversal

```rust
fn sig_map(
    arena: &mut TreeArena,
    cache: &mut HashMap<SigId, SigId>,
    sig: SigId,
    f: &dyn Fn(&mut TreeArena, SigId) -> SigId,
) -> SigId
```

**Algorithm**:
1. Cache hit → return cached result
2. Recursive nodes (`sym_rec`): insert placeholder in cache before descending (cycle breaking)
3. Recursively apply `sig_map` on each child
4. Apply `f` on the transformed node
5. Store result in cache, return

### `simplification` — rewrite rules

Implement via `match_sig(arena, sig)` exhaustive match:

| Category | Key rules |
|---|---|
| **Constant folding** | `n op m → compute(n,m)` for all 17 BinOp variants |
| **Neutral elements** | `0+x=x`, `x+0=x`, `1*x=x`, `x*1=x`, `x-0=x` |
| **Absorbing elements** | `0*x=0`, `x*0=0`; `x/0` → panic/assert |
| **Self-operations** | `x AND x = x`, `x OR x = x`, `x >= x = 1`, `x < x = 0`, `x == x = 1`, etc. |
| **Negation folding** | `(-1)*(x-y) → y-x`; `n*(-1) → -n` |
| **Nested mul** | `n*(m*x) → (n*m)*x` when n,m are numeric |
| **Type casts on constants** | `int_cast(3.7) → 3`, `float_cast(2) → 2.0` |
| **Select2** | `select2(0,t,_)=t`, `select2(n≠0,_,t)=t`, `select2(_,t,t)=t` |
| **Enable/Control** | `enable(t,0)=0`, `enable(t,1)=t`, `control(t,0)=0`, `control(t,1)=t` |
| **Lowest/Highest** | Extract interval bounds via `sigtype` interval annotation |
| **Delay1** | Call `normalize_delay1_term()` |
| **Delay** | Call `normalize_delay_term()` |
| **Add-terms** | Call `normalize_add_term()` |
| **Math on constants** | `sin(k) → sin(k as f64)` as `SigBuilder::real()`, etc. |

**Numeric helpers**: define `fold_binop(arena, op, l, r) -> Option<SigId>` that matches both operands as Int/Real and returns the computed result node.

**Math primitives on constants**: for `SigMatch::Sin(SigMatch::Real(k))` etc., compute directly using `f64` and return `arena.sig().real(result)`.

---

## Step 5 — `normalform.rs`: Phase 1 scope (suspended — deferred to later)

> **Status: SUSPENDED for Phase 1.**
> The full `simplifyToNormalForm` C++ pipeline (UI promotion, FTZ wrapping, auto-diff, double promotion pass, causality check) is **deferred**. Only the three sub-steps currently performed in `crates/transform/signal_prepare` are in scope for Phase 1.

### Phase 1 — only what `signal_prepare` currently does

These three operations are already implemented in `crates/transform/src/signal_prepare.rs` and are kept there. `normalform.rs` in Phase 1 is a thin coordinator that calls them in order:

| Step | Operation | Already in |
|---|---|---|
| 1 | de Bruijn → symbolic | `tlib::de_bruijn_to_sym` (used by `signal_prepare`) |
| 2 | Type annotation | `sigtype::TypeAnnotator::annotate` |
| 3 | Signal promotion + cast | `sigtype` / `transform::signal_prepare` |

**Phase 1 public API** (minimal):
```rust
pub fn prepare_signals(
    arena: &mut TreeArena,
    annotator: &mut TypeAnnotator,
    sigs: &[SigId],
) -> Vec<SigId>;
```

This wraps steps 1–3 and replaces the ad-hoc calls scattered in `signal_prepare`.

### Deferred to Phase 2 (full C++ parity)

The following `normalform.cpp` steps are **not** implemented in Phase 1:

- UI range promotion (`gRangeUI`)
- UI freeze to init (`gFreezeUI`)
- FTZ wrapping (`gFTZMode`)
- Auto-differentiation
- Double signal-promotion pass
- Causality check

These will be tackled once Steps 1–4 (mterm/aterm/normalize/simplify) are validated.

---

## Global state: C++ `gGlobal` → Rust equivalents

| C++ | Rust replacement |
|---|---|
| `gGlobal->SIMPLIFIED` (property key) | `HashMap<SigId, SigId>` local to each pass |
| `gGlobal->NORMALFORM` (property key) | `HashMap<SigId, SigId>` local to pipeline |
| `gGlobal->gRangeUI` etc. | `NormalFormOpts` fields |
| `gGlobal->TABBER` (debug indent) | local `depth: usize` in recursive calls |
| `gGlobal->nil` | `arena.nil()` |

---

## Development order and milestones

```
Step 1  mterm.rs      ~300 lines   → cargo test -p normalize (mterm tests)
Step 2  aterm.rs      ~200 lines   → cargo test -p normalize (aterm tests)
Step 3  normalize.rs  ~150 lines   → cargo test -p normalize (add/delay tests)
Step 4  simplify.rs   ~400 lines   → cargo test -p normalize (simplify tests)
Step 5  normalform.rs  ~80 lines   → Phase 1 only: de_bruijn_to_sym + annotate + promotion
        (full pipeline DEFERRED to Phase 2)
```

Each step must pass all prior tests before proceeding.

---

## Test plan (`crates/normalize/tests/`)

| Test | What it verifies |
|---|---|
| `test_mterm_from_int` | `Mterm::from_int(2).normalized_tree()` == `sig::int(2)` |
| `test_mterm_mul` | `2 * x` normalized correctly |
| `test_mterm_gcd` | GCD of `2*x*y` and `4*x` → `2*x` |
| `test_aterm_like_terms` | `x + x → 2*x` |
| `test_aterm_factorize` | `2*x + 2*y → 2*(x+y)` |
| `test_normalize_add` | Examples from C++ comments |
| `test_delay_zero` | `s@0 = s` |
| `test_delay_zero_sig` | `0@d = 0` |
| `test_delay_const_factor` | `(k*s)@d = k*(s@d)` |
| `test_delay_nested` | `(x@n)@m = x@(n+m)` |
| `test_simplify_fold_add` | `int(3) + int(4) = int(7)` |
| `test_simplify_neutral_add` | `0 + x = x`, `x + 0 = x` |
| `test_simplify_absorb_mul` | `0 * x = 0`, `x * 0 = 0` |
| `test_simplify_select2` | `select2(int(0), t1, t2) = t1` |
| `test_simplify_cast_int` | `int_cast(real(3.7)) = int(3)` |
| `test_simplify_sin_const` | `sin(real(0.0)) = real(0.0)` |
| `test_normalform_phase1` | Minimal signal through de_bruijn_to_sym + annotate + promotion (Phase 1 only) |

---

## Critical files reference

| File | Role |
|---|---|
| `crates/normalize/src/lib.rs` | Scaffold to fill |
| `crates/normalize/Cargo.toml` | Add dependencies |
| `crates/signals/src/lib.rs` | `SigBuilder`, `SigMatch`, `match_sig` |
| `crates/sigtype/src/rules.rs` | `TypeAnnotator` |
| `crates/tlib/src/lib.rs` | `de_bruijn_to_sym`, `TreeArena` |
| `crates/interval/src/lib.rs` | `Interval` — for Lowest/Highest simplification |
| C++ `mterm.cpp` | `/Users/letz/Developpements/RUST/faust/compiler/normalize/mterm.cpp` |
| C++ `aterm.cpp` | `/Users/letz/Developpements/RUST/faust/compiler/normalize/aterm.cpp` |
| C++ `simplify.cpp` | `/Users/letz/Developpements/RUST/faust/compiler/normalize/simplify.cpp` |
| C++ `normalform.cpp` | `/Users/letz/Developpements/RUST/faust/compiler/normalize/normalform.cpp` |

---

## Verification commands

```bash
# Unit tests for the normalize crate
cargo test -p normalize

# Integration tests (transform pipeline)
cargo test -p transform

# Lints
cargo clippy -p normalize -- -D warnings
```
