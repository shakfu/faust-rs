// d/dp asinh(p) = 1 / sqrt(1 + p²)
import("stdfaust.lib");
p = hslider("p", 0, -3, 3, 0.01);
process = fad(ma.asinh(p), p);
