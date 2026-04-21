// d/dp sinh(p) = cosh(p) = sqrt(1 + sinh²(p))
import("stdfaust.lib");
p = hslider("p", 0, -3, 3, 0.01);
process = fad(ma.sinh(p), p);
