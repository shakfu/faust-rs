import("stdfaust.lib");

// Compileable adaptation of auto_chorus_stereo for the current forward-AD subset.
// The chorus stays stereo and emits FAD gradients, but host code is expected to
// update the delay controls externally instead of closing the gradient loop in DSP.

sq(x) = x * x;
clamp(lo, hi, x) = min(hi, max(lo, x));
instCorr(x, y) = (x * y) / (0.0001 + sqrt(0.0001 + sq(x) * sq(y)));

chorusVoice(input, mix, delay, lfoSign, minDelay, maxDelay, lfoDepth, lfo) = out
with {
    modDelay = clamp(minDelay, maxDelay, delay + lfoSign * lfoDepth * lfo);
    voice = de.delay(256, modDelay, input);
    wet = 0.7 * input + 0.3 * voice;
    out = (1.0 - mix) * input + mix * wet;
};

process = outL, outR,
          delayL_monitor,
          delayR_monitor,
          corr_monitor,
          loss_monitor,
          gradL_monitor,
          gradR_monitor
with {
    input = _;

    mix = hslider("Mix", 0.7, 0.0, 1.0, 0.01);
    minDelay = hslider("Min Delay (samples)", 2.0, 0.0, 20.0, 0.1);
    maxDelay = hslider("Max Delay (samples)", 25.0, 5.0, 100.0, 0.1);
    delayL = hslider("Delay L", 6.0, 0.0, 40.0, 0.1);
    delayR = hslider("Delay R", 12.0, 0.0, 40.0, 0.1);
    delayTargetL = hslider("Soft Target Delay L", 7.0, 0.0, 40.0, 0.1);
    delayTargetR = hslider("Soft Target Delay R", 13.0, 0.0, 40.0, 0.1);
    delayPenalty = hslider("Delay Penalty", 0.0001, 0.0, 0.01, 0.00001);
    spreadPenalty = hslider("Spread Penalty", 0.00005, 0.0, 0.01, 0.00001);
    corrPenalty = hslider("Correlation Penalty", 1.0, 0.0, 10.0, 0.01);
    lfoRate = hslider("LFO Rate", 0.2, 0.01, 5.0, 0.01);
    lfoDepth = hslider("LFO Depth (samples)", 2.0, 0.0, 10.0, 0.1);

    lfo = os.osc(lfoRate);

    outL = chorusVoice(input, mix, delayL, 1.0, minDelay, maxDelay, lfoDepth, lfo);
    outR = chorusVoice(input, mix, delayR, -1.0, minDelay, maxDelay, lfoDepth, lfo);

    corr = instCorr(outL, outR);
    regDelays = delayPenalty * sq(delayL - delayTargetL)
              + delayPenalty * sq(delayR - delayTargetR);
    regSpread = spreadPenalty * sq((delayR - delayL) - 6.0);
    lossExpr = corrPenalty * sq(corr) + regDelays + regSpread;

    rawGradL = fad(lossExpr, delayL) : !, _;
    rawGradR = fad(lossExpr, delayR) : !, _;

    delayL_monitor = delayL;
    delayR_monitor = delayR;
    corr_monitor = corr;
    loss_monitor = lossExpr;
    gradL_monitor = rawGradL;
    gradR_monitor = rawGradR;
};
