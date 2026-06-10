// d/dp atanh(p) = 1 / (1 - p²)
atanh = ffunction(float atanhf|atanh|atanhl (float), <math.h>, "");
p = hslider("p", 0, -0.99, 0.99, 0.01);
process = fad(atanh(p), p);
