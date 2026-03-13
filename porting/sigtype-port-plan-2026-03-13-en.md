# Plan — Port of the Faust Signal Type System to Rust

Date: 2026-03-13

## Context

The previous conversation established that `SimpleSigType { Int, Real, Sound }` in
`signal_prepare.rs` is an excessive reduction relative to the C++ compiler.
In C++, `AudioType` carries `fInterval` directly: type and interval are inseparable.
To enable interval-bound-driven delay line sizing (`SIGDELAY` with variable amounts),
and to serve as the foundation for all subsequent analysis, the full system must be ported:
- `sigtype.hh / sigtype.cpp` → type structures, factories, operators
- `sigtyperules.cpp` → inference rules + recursive fixed-point loop

## C++ Reference Files

| C++ File | Contents |
|---|---|
| `compiler/signals/sigtype.hh` | AudioType / SimpleType / TableType / TupletType hierarchy, enums, res, factories, cast helpers |
| `compiler/signals/sigtype.cpp` | Constructors, equality, memoization, union/product operators |
| `compiler/signals/sigtyperules.cpp` | `inferSigType`, `typeAnnotation`, fixed-point loop |
| `compiler/interval/interval_def.hh` | Interval — already ported in `crates/interval` |

## Target Architecture — new crate `crates/sigtype`

```
tlib ─────────────────┐
                       ↓
signals ──────────────→ sigtype ──────────────→ transform
interval ─────────────┘
```

`sigtype` depends on `signals` (for `SigId`, `SigMatch`, `TreeArena`)
and `interval` (for `Interval`). No circular dependency.

## Step 0 — Crate Scaffold

Create `crates/sigtype/` with `Cargo.toml` and `src/lib.rs`.
Add to workspace `Cargo.toml` and to `transform/Cargo.toml`.

## Step 1 — Enums and `Res` struct (`src/enums.rs`)

```rust
#[repr(u8)]
pub enum Nature        { Int = 0, Real = 1, Any = 2 }
#[repr(u8)]
pub enum Variability   { Konst = 0, Block = 1, Samp = 3 }
#[repr(u8)]
pub enum Computability { Comp = 0, Init = 1, Exec = 3 }
#[repr(u8)]
pub enum Vectorability { Vect = 0, Scal = 1, TrueScal = 3 }
#[repr(u8)]
pub enum Boolean       { Num = 0, Bool = 1 }
```

Each enum implements `join(self, other) -> Self` via `(a as u8) | (b as u8)`.
Inverse conversion via `from_u8`.

Note: gaps in the integer sequences (Variability uses 0,1,3 not 0,1,2,3) are
intentional — bitwise OR of valid values always yields a valid value.

```rust
pub struct Res { pub valid: bool, pub index: i32 }
impl Default for Res { fn default() -> Self { Res { valid: false, index: 0 } } }
```

## Step 2 — Concrete Types (`src/types.rs`)

```rust
pub struct SimpleType {
    pub nature: Nature,
    pub variability: Variability,
    pub computability: Computability,
    pub vectorability: Vectorability,
    pub boolean: Boolean,
    pub interval: Interval,
    pub res: Res,
}

pub struct TableType {
    pub content: Box<SigType>,
    pub nature: Nature,
    pub variability: Variability,
    pub computability: Computability,
    pub vectorability: Vectorability,
    pub boolean: Boolean,
    pub interval: Interval,
}

pub struct TupletType {
    pub components: Vec<SigType>,
    // Aggregated from components:
    pub nature: Nature,
    pub variability: Variability,
    pub computability: Computability,
    pub vectorability: Vectorability,
    pub boolean: Boolean,
    pub interval: Interval,
}

pub enum SigType {
    Simple(SimpleType),
    Table(TableType),
    Tuplet(TupletType),
}
```

`SigType` is `Clone` (value semantics). No smart pointers — value comparison
replaces C++ pointer-identity memoization.

`PartialEq` implementation **ignores `res`** (matches C++ behaviour, ensures
fixed-point convergence).

## Step 3 — Common API (`src/api.rs`)

Uniform accessors on `SigType`:
```rust
impl SigType {
    pub fn nature(&self) -> Nature
    pub fn variability(&self) -> Variability
    pub fn computability(&self) -> Computability
    pub fn vectorability(&self) -> Vectorability
    pub fn boolean(&self) -> Boolean
    pub fn interval(&self) -> Interval
    pub fn is_maximal(&self) -> bool

    // promote* : return a new SigType with one field modified
    pub fn promote_nature(self, n: Nature) -> SigType
    pub fn promote_variability(self, v: Variability) -> SigType
    pub fn promote_computability(self, c: Computability) -> SigType
    pub fn promote_vectorability(self, v: Vectorability) -> SigType
    pub fn promote_boolean(self, b: Boolean) -> SigType
    pub fn promote_interval(self, i: Interval) -> SigType
}
```

## Step 4 — Operators and Cast Helpers (`src/ops.rs`)

```rust
// Union (| in C++)
pub fn union_types(a: SigType, b: SigType) -> SigType

// Cartesian product (* in C++) → flattened TupletType
pub fn product_types(a: SigType, b: SigType) -> SigType

// table() wrapper
pub fn make_table(content: SigType) -> SigType

// Cast helpers (port of sigtype.hh inline functions)
pub fn int_cast(t: SigType) -> SigType      // nature→Int, interval→cast2int
pub fn bit_cast(t: SigType) -> SigType      // nature→Int, interval unchanged
pub fn float_cast(t: SigType) -> SigType    // nature→Real
pub fn samp_cast(t: SigType) -> SigType     // variability→Samp
pub fn bool_cast(t: SigType) -> SigType     // boolean→Bool, nature→Int
pub fn num_cast(t: SigType) -> SigType      // boolean→Num
pub fn cast_interval(t: SigType, i: Interval) -> SigType

// Merge functions for TupletType construction
pub fn merge_nature(types: &[SigType]) -> Nature
pub fn merge_variability(types: &[SigType]) -> Variability
pub fn merge_computability(types: &[SigType]) -> Computability
pub fn merge_vectorability(types: &[SigType]) -> Vectorability
pub fn merge_boolean(types: &[SigType]) -> Boolean
pub fn merge_interval(types: &[SigType]) -> Interval

// Check assertions
pub fn check_int(t: &SigType) -> Result<(), TypeError>
pub fn check_konst(t: &SigType) -> Result<(), TypeError>
pub fn check_init(t: &SigType) -> Result<(), TypeError>
pub fn check_int_param(t: &SigType) -> Result<(), TypeError>
pub fn check_delay_interval(t: &SigType) -> Result<i32, TypeError>
```

`cast2int` delegates to `interval::ops::casts::int_cast` (already implemented).

## Step 5 — Factories (`src/factory.rs`)

```rust
pub fn make_simple(
    nature: Nature, variability: Variability,
    computability: Computability, vectorability: Vectorability,
    boolean: Boolean, interval: Interval,
) -> SigType

pub fn make_simple_with_res(
    nature: Nature, variability: Variability,
    computability: Computability, vectorability: Vectorability,
    boolean: Boolean, interval: Interval, res: Res,
) -> SigType

pub fn make_table_type(content: SigType) -> SigType
pub fn make_tuplet(components: Vec<SigType>) -> SigType
```

No global memoization — Rust types are compared by value.

## Step 6 — Type Inference (`src/rules.rs`)

Port of `sigtyperules.cpp` → `TypeAnnotator`.

```rust
pub struct TypeAnnotator<'a> {
    arena: &'a TreeArena,
    env: HashMap<SigId, SigType>,   // memoized results
    in_progress: HashSet<SigId>,    // cycle detection
}

impl<'a> TypeAnnotator<'a> {
    pub fn new(arena: &'a TreeArena) -> Self

    // Public entry point — annotates the whole signal forest
    pub fn annotate(&mut self, outputs: &[SigId]) -> Result<HashMap<SigId, SigType>, TypeError>

    // Main dispatch (port of inferSigType)
    fn infer(&mut self, sig: SigId) -> Result<SigType, TypeError>
}
```

Key inference rules (dispatch on `SigMatch`):

| Node | Rule |
|---|---|
| `Int(n)` | `Simple(Int, Konst, Comp, Vect, Num, singleton(n))` |
| `Real(r)` | `Simple(Real, Konst, Comp, Vect, Num, singleton(r))` |
| `Input(i)` | `Simple(Real, Samp, Exec, Vect, Num, default())` |
| `BinOp(op, l, r)` | dispatch to `arithmetic()` helper |
| `IntCast(x)` | `int_cast(infer(x))` |
| `FloatCast(x)` | `float_cast(infer(x))` |
| `BitCast(x)` | `bit_cast(infer(x))` |
| `Delay1(x)` | `samp_cast(infer(x))` |
| `Delay(x, n)` | `check_delay_interval(infer(n))?; samp_cast(infer(x))` |
| `VSlider/HSlider/NumEntry` | `Simple(Real, Block, Init, Vect, Num, vslider_interval(...))` |
| `Button/Checkbox` | `Simple(Int, Block, Exec, Scal, Num, [0,1])` |
| `HBargraph/VBargraph` | `cast_interval(infer(x), [lo,hi])` |
| `RdTbl(tbl, idx)` | `inferReadTableType(tbl, idx)` |
| `WrTbl(...)` | `inferWriteTableType(...)` |
| `SymRec` / `Proj` | fixed-point loop (see below) |
| `FFun/FConst/FVar` | from type annotation node |
| everything else | `Simple(Real, Samp, Exec, Scal, Num, default())` (conservative) |

**Recursive fixed-point loop** (port of `typeAnnotation` / `updateRecTypes`):
1. Initialise recursive types with `initialRecType` (maximal = Real, Samp, Exec)
2. Iterate `infer` on the body until stabilisation (`types[g] == prev[g]`)
3. Convergence guaranteed by the finite lattice (join is monotone)

## Step 7 — Migration of `signal_prepare.rs`

- Replace `SimpleSigType` with `SigType` (from `sigtype` crate)
- `PreparedSignals::types: HashMap<SigId, SimpleSigType>` → `HashMap<SigId, SigType>`
- `infer_simple_types()` → delegates to `TypeAnnotator::annotate()`
- Delete `SimpleTyper`, `TypeSlot`, `SimpleSigType` (no longer needed)
- Adapt `signal_fir/module.rs`: `simple_type()` returns `SigType`, use `.nature()` for dispatch

## Step 8 — Tests

Unit tests in `crates/sigtype/src/` at each step.

Key tests:
- `nature_join`: `Int.join(Real) == Real`, `Real.join(Int) == Real`, symmetry
- `variability_join`: `Konst.join(Samp) == Samp`, gap values respected
- `union_types`: interval reunion, variability promotion
- `int_cast`: `cast2int` applied to interval correctly
- `check_delay_interval`: bounded interval OK, unbounded → error
- Inference on `Int(42)` → nature=Int, variability=Konst, interval=[42,42]
- Inference on `VSlider("n", 0.0, 1000.0, 1.0, ...)` → interval=[0,1000], variability=Block
- Inference on `SIGDELAY(x, vslider(...))` → passes `check_delay_interval`, hi=1000

## Execution Order

1. Scaffold crate `sigtype` (Cargo.toml, lib.rs)
2. `src/enums.rs` — Nature, Variability, Computability, Vectorability, Boolean, Res
3. `src/types.rs` — SimpleType, TableType, TupletType, SigType enum
4. `src/api.rs` — accessors + promote*
5. `src/ops.rs` — union, product, cast helpers, merge, check*
6. `src/factory.rs` — make_simple, make_table_type, make_tuplet
7. `src/rules.rs` — TypeAnnotator + infer (non-recursive nodes first)
8. `src/rules.rs` — add recursive fixed-point loop
9. Migrate `signal_prepare.rs`
10. Adapt `signal_fir/module.rs`
11. `cargo test -p sigtype`

## Files Created / Modified

| File | Action |
|---|---|
| `crates/sigtype/Cargo.toml` | Create |
| `crates/sigtype/src/lib.rs` | Create |
| `crates/sigtype/src/enums.rs` | Create |
| `crates/sigtype/src/types.rs` | Create |
| `crates/sigtype/src/api.rs` | Create |
| `crates/sigtype/src/ops.rs` | Create |
| `crates/sigtype/src/factory.rs` | Create |
| `crates/sigtype/src/rules.rs` | Create |
| `Cargo.toml` (workspace) | Add member `sigtype` |
| `crates/transform/Cargo.toml` | Add dep `sigtype` |
| `crates/transform/src/signal_prepare.rs` | Migrate SimpleSigType → SigType |
| `crates/transform/src/signal_fir/module.rs` | Adapt `.nature()` / `.interval()` |
