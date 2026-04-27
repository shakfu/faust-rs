// rad with a multi-output `expr`. Phase-1 RAD applies an implicit
// all-ones cotangent on every primal output, so the emitted gradients are
// those of `sum(primals)`.
//
// Expected output bundle: [a*b, sin(a), b + cos(a), a]
//   d/da (a*b + sin(a)) = b + cos(a)
//   d/db (a*b + sin(a)) = a
a = hslider("a", 0.4, -2.0, 2.0, 0.001);
b = hslider("b", 0.3, -2.0, 2.0, 0.001);
process = rad((a*b, sin(a)), (a, b));
