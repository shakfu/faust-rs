// TBPTT online learning — simplest case: feedforward scalar gain.
//
// Target system: y_target[n] = g_star * x[n]
// Learned model: y_pred[n]   = g[n]   * x[n]
//
// The outer `loop ~ _` updates g every sample (BS=1 TBPTT).
// The BRA body `g * x` is purely feedforward — no recursive state,
// no Delay1 carry.  This is the minimal pattern that exercises the
// `classify_reverse_time_outputs` SYMREC-boundary fix: the outer
// SYMREC is classified as forward-time even though its body contains
// a BRA gradient projection.
//
// Gradient:  d(loss)/dg = -2 * (y_target - y_pred) * x
// Update:    g[n+1] = clip(g[n] - lr * d(loss)/dg, -4, 4)
//
// Convergence: g[n] → g_star; residual (= y_target - y_pred) → 0.
//
// Outputs: [residual_L, residual_R]

g_star = 0.5;   // hidden target gain  (range -4 to 4 is valid)
lr     = 0.05;  // learning rate

// Inline LCG white noise: x[n] ∈ [-0.5, 0.5]
noise = lcg * 4.656612873077393e-10
with { lcg = +(12345) ~ *(1103515245); };

x = noise;
y_target = g_star * x;

g_learned = loop ~ _
with {
    loop(g) = g_next
    with {
        y_pred = g * x;
        loss   = (y_target - y_pred) * (y_target - y_pred);
        grad   = rad(loss, g) : !, _;
        g_next = max(-4.0, min(4.0, g - lr * grad));
    };
};

process = (y_target - g_learned * x) <: _, _;
