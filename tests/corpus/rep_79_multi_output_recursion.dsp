// True multi-output recursion group regression.
//
// This is distinct from both:
// - the degenerate unary-projection case, because both outputs remain live;
// - genuine mutual recursion, because each lane feeds back into itself rather
//   than into the other lane.

import("stdfaust.lib");

feedback = si.bus(2) ~ (*(0.5), *(0.25));
process = feedback;
