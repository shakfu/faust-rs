// Adaptive 3-tap FIR notch filter with the notch radian frequency
// `omega` exposed as a `rad(...)` seed.
//
//   H(z) = 1 - 2·cos(omega)·z⁻¹ + z⁻²
//
// This places a pair of zeros on the unit circle at e^(±j·omega), so a
// pure sinusoid at that frequency is suppressed to zero. The host
// buffers x[n], x[n-1], x[n-2] and feeds them as three audio channels;
// keeping the filter strictly feed-forward (no Faust `~`) is what lets
// phase-1 RAD differentiate it.
//
// Seeds: (omega).
// Output bundle: [y, ∂y/∂omega] where ∂y/∂omega = 2·sin(omega)·x_n_1.
//
// LMS adaptation: descending the mean output power
//   loss = E[y²]
// drives `omega` to the strongest input frequency (classical adaptive
// notch).
omega = hslider("omega", 1.0, 0.01, 3.0, 0.0001);

notch(xn, xn1, xn2) = xn - 2.0 * cos(omega) * xn1 + xn2;
process = rad(notch, omega);
