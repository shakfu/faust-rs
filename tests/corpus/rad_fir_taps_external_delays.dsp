// rad on a 4-tap FIR whose delayed inputs are provided by the host as
// separate audio channels (x[n], x[n-1], x[n-2], x[n-3]).
//
// Phase-1 RAD refuses temporal feedback inside `rad(...)` (`delay1` is
// non-causal under transpose without a tape). This fixture works around
// that by lifting the delay line OUT of the differentiated expression:
// the host buffers the input stream and feeds the four taps as four
// audio channels. The differentiated body becomes a pure feed-forward
// linear combination.
//
//   y = c0 * x_n + c1 * x_n_minus_1 + c2 * x_n_minus_2 + c3 * x_n_minus_3
//
// Seeds (c0, c1, c2, c3): the host trains them by descending against a
// target signal recorded at the same four input lanes.
//
// Output bundle: [y, ∂y/∂c0=x_n, ∂y/∂c1=x_{n-1}, ∂y/∂c2=x_{n-2}, ∂y/∂c3=x_{n-3}]
c0 = hslider("c0", 0.25, -2.0, 2.0, 0.001);
c1 = hslider("c1", 0.25, -2.0, 2.0, 0.001);
c2 = hslider("c2", 0.25, -2.0, 2.0, 0.001);
c3 = hslider("c3", 0.25, -2.0, 2.0, 0.001);

// kernel: take four inputs in order (x_n, x_{n-1}, x_{n-2}, x_{n-3]).
kernel(x0, x1, x2, x3) = c0 * x0 + c1 * x1 + c2 * x2 + c3 * x3;
process = rad(kernel, (c0, c1, c2, c3));
