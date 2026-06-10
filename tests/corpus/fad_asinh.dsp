// d/dp asinh(p) = 1 / sqrt(1 + p²)
asinh = ffunction(float asinhf|asinh|asinhl (float), <math.h>, "");
p = hslider("p", 0, -3, 3, 0.01);
process = fad(asinh(p), p);
