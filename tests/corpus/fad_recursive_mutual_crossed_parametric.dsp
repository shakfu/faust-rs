p = hslider("p", 0.2, -0.9, 0.9, 0.001);
bus2 = _,_;
cross2 = _,_ <: !,_,_,!;

// Genuine mutual-recursion regression with one parameterized edge:
//   y0[n] = 0.25 * y1[n-1]
//   y1[n] = p * y0[n-1]
//
// Expected output layout:
//   [y0, d(y0)/dp, y1, d(y1)/dp]
process = fad(bus2 ~ ((*(p), *(0.25)) : cross2), p);
