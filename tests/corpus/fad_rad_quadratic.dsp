// Mixed AD: outer FAD over inner RAD on a quadratic.
// f(x) = x*x  ⇒  f'(x) = 2x, f''(x) = 2
// inner rad(x*x, x)         → [x*x, 2x]
// outer fad([x*x, 2x], x)   → [x*x, 2x, 2x, 2]               (4 lanes)
// The last lane is the second derivative.
x = hslider("x", 1.5, -2.0, 2.0, 0.001);
process = fad(rad(x*x, x), x);
