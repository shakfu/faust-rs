// d/dp acosh(p) = 1 / sqrt(p² - 1)   (domain: p > 1)
acosh = ffunction(float acoshf|acosh|acoshl (float), <math.h>, "");
p = hslider("p", 1.5, 1.01, 5, 0.01);
process = fad(acosh(p), p);
