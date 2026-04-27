// rad with a seed list containing the same `SigId` twice. Phase-1 RAD
// preserves both lanes verbatim instead of deduplicating: lane 0 and
// lane 1 must both equal the gradient w.r.t. `a`.
// Expected output bundle: [a*b, b, b]
a = hslider("a", 0.5, -2.0, 2.0, 0.001);
b = hslider("b", 0.7, -2.0, 2.0, 0.001);
process = rad(a*b, (a, a));
