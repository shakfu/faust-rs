// d/dp cosh(p) = sinh(p) = (exp(p) - exp(-p)) / 2
import("stdfaust.lib");
p = hslider("p", 0, -3, 3, 0.01);
process = fad(ma.cosh(p), p);
