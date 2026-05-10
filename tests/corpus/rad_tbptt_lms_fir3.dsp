// TBPTT 3-tap LMS adaptive FIR — classic adaptive filtering.
//
// Target: y_target[n] = h0* * x[n] + h1* * x[n-1] + h2* * x[n-2]
// Model:  y_pred[n]   = h0  * x[n] + h1  * x[n-1] + h2  * x[n-2]
//
// The three tap weights (h0, h1, h2) are maintained as a joint 3-wire
// SYMREC (`loop ~ (_, _, _)`) updated every sample.  Each weight has its
// own BRA (seed h_i, shared body `loss`).  CSE ensures `y_pred` and
// `loss` are computed once despite three separate BRA groups.
//
// Classic LMS gradients (e = y_target - y_pred):
//   d(loss)/dh0 = -2 * e * x[n]
//   d(loss)/dh1 = -2 * e * x[n-1]
//   d(loss)/dh2 = -2 * e * x[n-2]
//
// All BRA bodies are feedforward (Mul and Add over delayed inputs).
// No IIR inside the BRA body — no Delay1 carry.
//
// Convergence: (h0, h1, h2) → (h0*, h1*, h2*); residual → 0.
//
// Outputs: [residual_L, residual_R]

h0_star = 0.5;    // hidden target tap 0  (gain on x[n])
h1_star = 0.3;    // hidden target tap 1  (gain on x[n-1])
h2_star = -0.2;   // hidden target tap 2  (gain on x[n-2])
lr      = 0.02;   // learning rate

// Inline LCG white noise
noise = lcg * 4.656612873077393e-10
with { lcg = +(12345) ~ *(1103515245); };

x        = noise;
x1       = x  : mem;   // x[n-1]
x2       = x1 : mem;   // x[n-2]
y_target = h0_star * x + h1_star * x1 + h2_star * x2;

// 3-wire SYMREC: (h0, h1, h2) jointly updated each sample.
// The three outputs are extracted by individual channel routing.
taps = loop ~ (_, _, _)
with {
    loop(h0, h1, h2) = h0n, h1n, h2n
    with {
        y_pred = h0 * x + h1 * x1 + h2 * x2;
        loss   = (y_target - y_pred) * (y_target - y_pred);
        g0     = rad(loss, h0) : !, _;   // BRA for tap 0
        g1     = rad(loss, h1) : !, _;   // BRA for tap 1
        g2     = rad(loss, h2) : !, _;   // BRA for tap 2
        h0n    = max(-4.0, min(4.0, h0 - lr * g0));
        h1n    = max(-4.0, min(4.0, h1 - lr * g1));
        h2n    = max(-4.0, min(4.0, h2 - lr * g2));
    };
};

h0_l = taps : _, !, !;
h1_l = taps : !, _, !;
h2_l = taps : !, !, _;

process = (y_target - (h0_l * x + h1_l * x1 + h2_l * x2)) <: _, _;
