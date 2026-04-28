// Mixed AD: outer RAD over inner FAD on a quadratic.
// inner fad(x*x, x)         → [x*x, 2x]
// outer rad([x*x, 2x], x)   → [x*x, 2x, d/dx(x*x + 2x) = 2x + 2]   (3 lanes)
// The last lane is the sum-cotangent gradient = f'(x) + f''(x).
x = hslider("x", 1.5, -2.0, 2.0, 0.001);
process = rad(fad(x*x, x), x);
