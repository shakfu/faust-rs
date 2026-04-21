// d/dp atanh(p) = 1 / (1 - p²)
import("stdfaust.lib");
p = hslider("p", 0, -0.99, 0.99, 0.01);
process = fad(ma.atanh(p), p);
