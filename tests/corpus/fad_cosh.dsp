// d/dp cosh(p) = sinh(p) = (exp(p) - exp(-p)) / 2
cosh = ffunction(float coshf|cosh|coshl (float), <math.h>, "");
p = hslider("p", 0, -3, 3, 0.01);
process = fad(cosh(p), p);
