// TBPTT online identification of a nonlinear drive parameter.
//
// Soft-clipper model:  f(drive, x) = drive * x / (1 + |drive * x|)
//   — an algebraic saturator (Doidic-style) with gain `drive`.
//
// Target: y_target[n] = f(d_star, x[n])
// Model:  y_pred[n]   = f(d[n],   x[n])
//
// The BRA body is purely feedforward (Div, Abs, Mul, Add).  No recursive
// state inside the BRA, so no Delay1 carry — cleanest nonlinear TBPTT.
//
// Gradient (by chain rule through f):
//   Let u = d*x, then f = u/(1+|u|), df/dd = x/(1+|u|)^2
//   d(loss)/dd = -2*(y_target - y_pred) * x / (1 + |d*x|)^2
//
// Update:      d[n+1] = clip(d[n] - lr * d(loss)/dd, 0.01, 10.0)
// Convergence: d[n] → d_star; residual → 0.
//
// Tests Div, Abs and Mul backward rules in the TBPTT context.
//
// Outputs: [residual_L, residual_R]

d_star = 3.0;   // hidden target drive (> 1 produces audible saturation)
lr     = 0.02;  // learning rate

// Inline LCG white noise
noise = lcg * 4.656612873077393e-10
with { lcg = +(12345) ~ *(1103515245); };

x = noise;

// Algebraic soft-clipper: f(d, x) = d*x / (1 + |d*x|)
softclip(d, sig) = u / (1.0 + abs(u))
with { u = d * sig; };

y_target = softclip(d_star, x);

d_learned = loop ~ _
with {
    loop(d) = d_next
    with {
        y_pred = softclip(d, x);
        loss   = (y_target - y_pred) * (y_target - y_pred);
        grad   = rad(loss, d) : !, _;
        d_next = max(0.01, min(10.0, d - lr * grad));
    };
};

process = (y_target - softclip(d_learned, x)) <: _, _;
