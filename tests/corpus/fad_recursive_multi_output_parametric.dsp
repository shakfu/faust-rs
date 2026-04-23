import("stdfaust.lib");

p = hslider("p", 0.2, -0.9, 0.9, 0.001);

// Two independent recursive lanes sharing one differentiation seed.
// Expected output layout:
//   [lane0, d(lane0)/dp, lane1, d(lane1)/dp]
process = fad(si.bus(2) ~ (*(p), *(0.25)), p);
