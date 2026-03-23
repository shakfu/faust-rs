# interval

Interval analysis library — ported from `compiler/interval/` in the Faust C++ compiler.

## C++ provenance

| C++ file | Role |
|---|---|
| `compiler/interval/interval_def.hh` | Core `Interval` type and set operations |
| `compiler/interval/interval_algebra.hh` | Algebra surface (operator dispatch) |
| `compiler/interval/interval*.cpp` | Concrete operator implementations |

## What this crate does

Provides a closed-interval type `Interval` carrying `[lo, hi]` bounds and a
least-significant-bit precision (`lsb`). Implements the full Faust interval
algebra: arithmetic, bitwise, logic, trigonometric, math, delay/table, and
UI-relevant operators — all mirroring the C++ `itv::interval` lattice.

## Public API

### Core type

| Item | Description |
|---|---|
| `Interval` | Closed interval `[lo, hi]` with `lsb` precision |
| `Interval::new(lo, hi, lsb)` | Construct an interval |
| `Interval::from_scalar(v)` | Construct a degenerate point interval |
| `Interval::lo()` / `hi()` / `lsb()` | Bound and precision accessors |
| `Interval::is_valid()` / `is_empty()` / `is_unbounded()` / `is_bounded()` | State predicates |
| `Interval::has(v)` / `is(v)` / `has_zero()` / `is_zero()` / `is_const()` | Value predicates |
| `Interval::is_power_of2()` / `is_bitmask()` | Structural predicates |

### Set operations (re-exported from `ops`)

| Item | Description |
|---|---|
| `ops::arithmetic` | `+`, `-`, `*`, `/`, `%`, `abs`, `floor`, `ceil`, … |
| `ops::bitwise` | Signed/unsigned `&`, `\|`, `^`, `~`, shifts |
| `ops::logic` | Boolean algebra on intervals |
| `ops::math` | `pow`, `sqrt`, `exp`, `log`, … |
| `ops::trig` | `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2` |
| `ops::delay_table` | Delay and table access interval propagation |
| `ops::ui` | UI-widget interval constraints |
| `ops::casts` | Type-cast interval operations |

### Utilities

| Item | Description |
|---|---|
| `utils::saturated_int_cast(i)` | Saturating cast from `Interval` to integer range |
| `utils::saturated_precision_add(a, b)` | Saturating precision addition |
| `utils::saturated_precision_sub(a, b)` | Saturating precision subtraction |
| `bitwise::*` | Raw bitwise interval operations (used by `ops::bitwise`) |

## Position in the pipeline

```
signals  →  normalize  →  [interval inference]  →  transform
```
