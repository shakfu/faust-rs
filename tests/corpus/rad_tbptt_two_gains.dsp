// TBPTT online learning with two parameters in a shared 2-wire SYMREC.
//
// Target: y_target[n] = a_star * x[n] + b_star * x[n-1]
//           (2-tap FIR, entirely feedforward)
// Model:  y_pred[n]   = a[n] * x[n]   + b[n] * x[n-1]
//
// Both (a, b) are maintained as a 2-output `loop ~ (_, _)` SYMREC.
// Each parameter has its own BRA: rad(loss, a) and rad(loss, b).
// The two BRAs share the same body signals via CSE.
//
// Gradients:
//   d(loss)/da = -2 * (y_target - y_pred) * x
//   d(loss)/db = -2 * (y_target - y_pred) * x[n-1]
//
// Update (per sample):
//   a[n+1] = clip(a[n] - lr * da, -4, 4)
//   b[n+1] = clip(b[n] - lr * db, -4, 4)
//
// Convergence: (a, b) → (a_star, b_star); residual → 0.
//
// This demonstrates the 2-wire `loop ~ (_, _)` TBPTT pattern: both
// parameters share a single SYMREC group so their state advances in
// lockstep.  No IIR in either BRA body — no Delay1 carry.
//
// Outputs: [residual_L, residual_R]

a_star = 0.6;  // hidden target coefficient for x[n]
b_star = 0.3;  // hidden target coefficient for x[n-1]
lr     = 0.03; // shared learning rate

// Inline LCG white noise
noise = lcg * 4.656612873077393e-10
with { lcg = +(12345) ~ *(1103515245); };

x        = noise;
x1       = x : mem;          // x[n-1]
y_target = a_star * x + b_star * x1;

// 2-wire SYMREC: (a, b) updated jointly each sample.
// Faust does not support multi-output LHS destructuring;
// extract each output via routing after defining the process.
params = loop ~ (_, _)
with {
    loop(a, b) = a_next, b_next
    with {
        y_pred = a * x + b * x1;
        loss   = (y_target - y_pred) * (y_target - y_pred);
        grad_a = rad(loss, a) : !, _;   // separate BRA for a
        grad_b = rad(loss, b) : !, _;   // separate BRA for b
        a_next = max(-4.0, min(4.0, a - lr * grad_a));
        b_next = max(-4.0, min(4.0, b - lr * grad_b));
    };
};

a_learned = params : _, !;
b_learned = params : !, _;

process = (y_target - (a_learned * x + b_learned * x1)) <: _, _;
