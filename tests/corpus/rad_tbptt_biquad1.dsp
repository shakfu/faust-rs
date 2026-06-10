// TBPTT online identification of a self-contained five-parameter recursive
// filter.  This is intentionally library-free so mandatory CI tests do not
// depend on `stdfaust.lib` / `filters.lib` availability.
//
// Target:
//   y_target[n] = (b0*x[n] + b1*x[n-1] + b2*x[n-2])
//                 : + ~ *(a1*) : + ~ *(a2*)
//
// Model:
//   y_pred[n]   = (b0*x[n] + b1*x[n-1] + b2*x[n-2])
//                 : + ~ *(a1) : + ~ *(a2)
//
// The five coefficients (b0,b1,b2,a1,a2) are maintained in a 5-wire SYMREC
// and updated jointly each sample via five separate BRA sweeps.  This keeps
// the original regression shape: multi-parameter TBPTT with feed-forward taps,
// nested recursive poles, fixed input delays, and shared RAD body CSE.
//
// Outputs: [residual_L, residual_R]

b0_star =  0.5;
b1_star =  0.3;
b2_star =  0.0;
a1_star =  0.6;
a2_star =  0.3;

lr_b = 0.01;
lr_a = 0.001;

// Inline LCG white noise excitation.
noise = lcg * 4.656612873077393e-10
with { lcg = +(12345) ~ *(1103515245); };

x = noise;
ff3(b0, b1, b2, sig) = b0 * sig + b1 * sig' + b2 * sig'';
filter5(b0, b1, b2, a1, a2, sig) = ff3(b0, b1, b2, sig) : + ~ *(a1) : + ~ *(a2);
y_target = filter5(b0_star, b1_star, b2_star, a1_star, a2_star, x);

coeffs = loop ~ (_, _, _, _, _)
with {
    loop(b0, b1, b2, a1, a2) = b0n, b1n, b2n, a1n, a2n
    with {
        y_pred = filter5(b0, b1, b2, a1, a2, x);
        loss   = (y_target - y_pred) * (y_target - y_pred);

        gb0 = rad(loss, b0) : !, _;
        gb1 = rad(loss, b1) : !, _;
        gb2 = rad(loss, b2) : !, _;
        ga1 = rad(loss, a1) : !, _;
        ga2 = rad(loss, a2) : !, _;

        b0n = max(-2.0, min(2.0, b0 - lr_b * gb0));
        b1n = max(-2.0, min(2.0, b1 - lr_b * gb1));
        b2n = max(-2.0, min(2.0, b2 - lr_b * gb2));
        a1n = max(-0.99, min(0.99, a1 - lr_a * ga1));
        a2n = max(-0.99, min(0.99, a2 - lr_a * ga2));
    };
};

b0_l = coeffs : _, !, !, !, !;
b1_l = coeffs : !, _, !, !, !;
b2_l = coeffs : !, !, _, !, !;
a1_l = coeffs : !, !, !, _, !;
a2_l = coeffs : !, !, !, !, _;

y_ia = filter5(b0_l, b1_l, b2_l, a1_l, a2_l, x);
process = (y_target - y_ia) <: _, _;
