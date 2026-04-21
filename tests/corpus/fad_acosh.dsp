// d/dp acosh(p) = 1 / sqrt(p² - 1)   (domain: p > 1)
import("stdfaust.lib");
p = hslider("p", 1.5, 1.01, 5, 0.01);
process = fad(ma.acosh(p), p);
