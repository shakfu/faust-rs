// Forward-mode AD through an external tanh call.
// tanh is defined in math.lib as ffunction(float tanhf|tanh|tanhl (float), <math.h>, "")
// so it reaches the FAD pass as SigMatch::FFun.
//
// d/dp tanh(p) = 1 - tanh²(p)   (= sech²(p))
tanh = ffunction(float tanhf|tanh|tanhl (float), <math.h>, "");
p = hslider("p", 0, -3, 3, 0.01);
process = fad(tanh(p), p);
