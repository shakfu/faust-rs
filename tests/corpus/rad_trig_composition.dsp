// rad over a trig composition. Exercises the unary chain rule (sin/cos)
// alongside the multiplicative inner expression.
// Expected output bundle: [sin(a*b), cos(a*b)*b, cos(a*b)*a]
a = hslider("a", 0.6, -2.0, 2.0, 0.001);
b = hslider("b", 0.4, -2.0, 2.0, 0.001);
process = rad(sin(a*b), (a, b));
