// Genuine mutual-recursion regression.
//
// Unlike rep_79_multi_output_recursion, this one is not just a multi-output
// recursive group with one self-feedback lane per output. The feedback path
// crosses the two signals, so each output depends on the previous sample of
// the other output:
//
//   output0[n] = 0.25 * output1[n-1]
//   output1[n] = 0.5  * output0[n-1]

import("stdfaust.lib");

process = si.bus(2) ~ ((*(0.5), *(0.25)) : ro.cross(2));
