// Float literal pattern matching — canonicalization regression.
//
// # The bug
//
// Faust allows functions defined by pattern matching on float literals:
//
//   foo2(1.0) = 456;
//
// This creates a `case` automaton with a `Constant(float_bits(1.0))` transition.
// When `foo2(1.0)` is called, `apply_pattern_matcher` simplifies the argument
// through `simplify_pattern`.  The buggy version converted any integer-valued
// `Real(x)` to `Int(i)` in the fast-path literal branch:
//
//   BoxMatch::Real(x) => {
//       let i = x as i32;
//       if (i as f64) == x { return BoxBuilder::new(arena).int(i); }  // BUG
//       return box_id;
//   }
//
// `1.0` satisfied `(1 as f64) == 1.0`, so it was coerced to `int(1)`.
// The TreeId equality check `int(1) == float_bits(1.0)` then failed:
// "no case rule matches arguments".
//
// # Why C++ does not have this bug
//
// The C++ `simplifyPattern` returns any `boxInt` / `boxReal` literal unchanged —
// no float→int coercion.  For non-literal arithmetic expressions the return type
// is governed by the signal type (`SigInt` vs `SigReal`), not by whether the
// numeric value happens to be integral.
//
// # This file
//
// Four pattern-matching functions exercising all combinations:
//   foo1(1)       — integer literal pattern, integer argument  → 123
//   foo2(1.0)     — float literal pattern, float argument      → 456
//   foo(4/2)      — integer arithmetic pattern (4/2 = Real(2.0)), argument 4/2 → 789
//   foo4(4.0/2.0) — float arithmetic pattern (Real(2.0)), argument 4.0/2.0    → 101112
//
// All four must compile and evaluate to the correct constants.

foo1(1)       = 123;
foo2(1.0)     = 456;
foo(4/2)      = 789;
foo4(4.0/2.0) = 101112;

process = foo1(1), foo2(1.0), foo(4/2), foo4(4.0/2.0);
