// TBPTT online system identification: learn the pole of a 1-pole IIR.
//
// Target (black box): y_target[n] = x[n] + p_star * y_target[n-1]
// Learned model:      y_pred[n]   = x[n] + p[n]   * y_pred[n-1]
//
// The outer `loop ~ _` updates p every sample (BS=1 TBPTT).  The inner
// IIR `+ ~ *(p)` is a primal recursive state inside the BRA body, which
// introduces a Delay1 carry in the backward sweep.  With BS=1 the carry
// approximates the future adjoint as zero within each compute() call,
// making the effective gradient:
//
//   d(loss)/dp ≈ -2 * (y_target - y_pred) * y_pred[n-1]   (direct term)
//
// plus a carry term from the previous compute() call boundary.
//
// Update:      p[n+1] = clip(p[n] - lr * d(loss)/dp, -0.99, 0.99)
// Convergence: p[n] → p_star; residual → 0.
//
// This is the self-contained distillation of rad_filter1.dsp (which uses
// stdfaust.lib for si.smoo and no.noise).
//
// Outputs: [residual_L, residual_R]

p_star = 0.7;   // hidden target pole  (|p_star| < 1 for stability)
lr     = 0.002; // learning rate (smaller than feedforward case — IIR is sensitive)

// Inline LCG white noise excitation
noise = lcg * 4.656612873077393e-10
with { lcg = +(12345) ~ *(1103515245); };

x        = noise;
y_target = x : + ~ *(p_star);

p_learned = loop ~ _
with {
    loop(p) = p_next
    with {
        y_pred = x : + ~ *(p);
        loss   = (y_target - y_pred) * (y_target - y_pred);
        grad   = rad(loss, p) : !, _;
        p_next = max(-0.99, min(0.99, p - lr * grad));
    };
};

// y_ia uses the learned pole — residual converges to silence
y_ia    = x : + ~ *(p_learned);
process = (y_target - y_ia) <: _, _;
