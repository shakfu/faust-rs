// rad over a binary product against two independent seeds.
// Expected output bundle: [a*b, b, a]
//   d/da (a*b) = b
//   d/db (a*b) = a
a = hslider("a", 1.0, -2.0, 2.0, 0.001);
b = hslider("b", 2.0, -2.0, 2.0, 0.001);
process = rad(a*b, (a, b));
