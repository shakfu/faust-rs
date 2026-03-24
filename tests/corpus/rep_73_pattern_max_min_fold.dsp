// rep_73 — pattern_simplification folds max/min via full signal propagation
//
// Tests that `patternSimplification` (called from `evalPattern` during automaton
// construction) reduces `max(1, min(6, 4))` to the integer `4` so that the
// case rule `f(4) = 40` is matched correctly.
//
// C++ reference: `isBoxNumeric` propagates the box to a signal then calls
// `simplify()`, reducing `max(1, min(6, 4))` → `sigInt(4)` → `boxInt(4)`.

import("stdfaust.lib");

// Simple case: pattern constant folded by arithmetic
f(1) = 10;
f(2) = 20;
f(3) = 30;
f(4) = 40;

// max(1, min(6, 4)) must fold to 4 at construction time
process = f(max(1, min(6, 4)));
