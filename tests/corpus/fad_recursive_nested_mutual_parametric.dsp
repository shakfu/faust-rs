import("stdfaust.lib");

p = hslider("p", 0.2, -0.9, 0.9, 0.001);

// Nested mutual-recursion regression:
//   core0[n] = p * core1[n-1]
//   core1[n] = 0.25 * core0[n-1]
//   mix[n]   = (core0[n] + core1[n]) * mix[n-1] + 1
core = si.bus(2) ~ ((*(p), *(0.25)) : ro.cross(2));
mix = 1 : + ~ *(core : +);

process = fad(mix, p);
