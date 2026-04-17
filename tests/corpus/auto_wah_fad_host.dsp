import("stdfaust.lib");

// Compileable adaptation of auto_wah_origin for the current forward-AD subset.
// The DSP computes the loss and FAD gradients, but host code is expected to
// update the `Wah Freq` and `Wah Q` controls externally instead of closing the
// gradient-descent loop inside the DSP graph.

sq(x) = x * x;
clamp(lo, hi, x) = min(hi, max(lo, x));

smoothEnergy(x) = step ~ _
with {
    k = 0.001;
    step(prev) = k * (x * x) + (1.0 - k) * prev;
};

envelope(x) = step ~ _
with {
    k = 0.01;
    step(prev) = k * abs(x) + (1.0 - k) * prev;
};

wahFilter(fc, q, x) = fi.resonbp(fc, q, 1.0, x);

process = auto_wah;

auto_wah(input) = out,
                  freq_monitor,
                  q_monitor,
                  env_monitor,
                  energy_monitor,
                  loss_monitor,
                  dloss_df_monitor,
                  dloss_dq_monitor
with {
    mix = hslider("Mix", 1.0, 0.0, 1.0, 0.01);
    envDepth = hslider("Envelope Depth", 800.0, 0.0, 3000.0, 1.0);
    minFreq = hslider("Min Freq", 300.0, 50.0, 2000.0, 1.0);
    maxFreq = hslider("Max Freq", 2500.0, 500.0, 5000.0, 1.0);
    minQ = hslider("Min Q", 0.5, 0.1, 10.0, 0.01);
    maxQ = hslider("Max Q", 12.0, 1.0, 30.0, 0.01);
    wahFreqRaw = hslider("Wah Freq", 900.0, 50.0, 2500.0, 1.0);
    wahQRaw = hslider("Wah Q", 5.0, 0.1, 30.0, 0.01);
    qTarget = hslider("Q Soft Target", 6.0, 0.5, 20.0, 0.1);
    qPenalty = hslider("Q Penalty", 0.0005, 0.0, 0.01, 0.00001);

    env = envelope(input);
    env_monitor = env;

    wahFreq = clamp(minFreq, maxFreq, wahFreqRaw + envDepth * env);
    wahQ = clamp(minQ, maxQ, wahQRaw);

    freq_monitor = wahFreq;
    q_monitor = wahQ;

    wahSig = wahFilter(wahFreq, wahQ, input);
    energy = smoothEnergy(wahSig);
    energy_monitor = energy;

    lossExpr = -energy + qPenalty * sq(wahQ - qTarget);
    loss_monitor = lossExpr;

    dloss_df = fad(lossExpr, wahFreqRaw) : !, _;
    dloss_dq = fad(lossExpr, wahQRaw) : !, _;

    dloss_df_monitor = dloss_df;
    dloss_dq_monitor = dloss_dq;

    out = (1.0 - mix) * input + mix * wahSig;
};
