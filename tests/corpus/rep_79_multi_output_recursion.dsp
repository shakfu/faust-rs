// True multi-output recursion group regression.
//
// This is distinct from the degenerate unary-projection case: both outputs
// remain live in the recursive group.

import("stdfaust.lib");

feedback = si.bus(2) ~ (*(0.5), *(0.25));
process = feedback;
