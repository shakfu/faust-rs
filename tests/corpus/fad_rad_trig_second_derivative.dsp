// Mixed AD: outer FAD over inner RAD on a trig composition.
// f(x) = sin(x*x). The fourth output lane is f''(x), pinned in the
// runtime suite against a central finite difference on the first-order
// gradient.
x = hslider("x", 0.7, -2.0, 2.0, 0.001);
process = fad(rad(sin(x*x), x), x);
