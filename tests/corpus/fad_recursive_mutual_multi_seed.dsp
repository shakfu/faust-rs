import("stdfaust.lib");

p = hslider("p", 0.2, -0.9, 0.9, 0.001);
q = hslider("q", 0.35, -0.9, 0.9, 0.001);

// Two-state mutual-recursion regression with two explicit seeds:
//   y0[n] = p * y1[n-1]
//   y1[n] = q * y0[n-1]
//
// Expected output layout:
//   [y0, d(y0)/dp, d(y0)/dq, y1, d(y1)/dp, d(y1)/dq]
process = fad(si.bus(2) ~ ((*(p), *(q)) : ro.cross(2)), (p, q));
