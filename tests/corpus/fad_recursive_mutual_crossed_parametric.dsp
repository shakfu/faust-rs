import("stdfaust.lib");

p = hslider("p", 0.2, -0.9, 0.9, 0.001);

// Genuine mutual-recursion regression with one parameterized edge:
//   y0[n] = 0.25 * y1[n-1]
//   y1[n] = p * y0[n-1]
//
// Expected output layout:
//   [y0, d(y0)/dp, y1, d(y1)/dp]
process = fad(si.bus(2) ~ ((*(p), *(0.25)) : ro.cross(2)), p);
