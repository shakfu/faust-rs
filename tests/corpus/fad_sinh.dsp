// d/dp sinh(p) = cosh(p) = sqrt(1 + sinh²(p))
sinh = ffunction(float sinhf|sinh|sinhl (float), <math.h>, "");
p = hslider("p", 0, -3, 3, 0.01);
process = fad(sinh(p), p);
