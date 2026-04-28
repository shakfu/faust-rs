// rad on a 3rd-order polynomial waveshaper. A common building block for
// neural distortion modelling and analog-circuit emulation when the
// stage is feed-forward (no feedback through the waveshaper).
//
//   y = c0 + c1·x + c2·x² + c3·x³
//
// Seeds (c0, c1, c2, c3) — host fits them against a recorded target.
//
// Inputs: x, x², x³ are passed as three audio channels so the host
// pre-computes the powers once. This keeps the differentiated body
// strictly feed-forward and avoids re-using the `_` wire (which Faust
// would otherwise expand into three independent inputs).
//
// Output bundle: [y, ∂y/∂c0=1, ∂y/∂c1=x, ∂y/∂c2=x², ∂y/∂c3=x³]
c0 = hslider("c0", 0.0, -2.0, 2.0, 0.001);
c1 = hslider("c1", 1.0, -2.0, 2.0, 0.001);
c2 = hslider("c2", 0.0, -2.0, 2.0, 0.001);
c3 = hslider("c3", 0.0, -2.0, 2.0, 0.001);

shape(x, xx, xxx) = c0 + c1 * x + c2 * xx + c3 * xxx;
process = rad(shape, (c0, c1, c2, c3));
