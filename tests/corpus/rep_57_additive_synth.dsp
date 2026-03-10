sr = fconstant(int fSamplingFreq, <math.h>);
twopi = 6.283185307179586;

freq = hslider("freq", 220.0, 27.5, 1760.0, 0.1);
gain = hslider("gain", 0.2, 0.0, 1.0, 0.01);
h2(i) = hslider("harmonic%i", 0.5, 0.0, 1.0, 0.01);
h3(i) = hslider("harmonic%i", 0.25, 0.0, 1.0, 0.01);
h4(i) = hslider("harmonic%i", 0.125, 0.0, 1.0, 0.01);

phasor(step) = +(step) ~ _;
partial(mult, amp) = amp * sin(twopi * phasor((freq * mult) / sr));

voice = gain * (
    partial(1.0, 1.0) +
    partial(2.0, h2(2)) +
    partial(3.0, h3(3)) +
    partial(4.0, h4(4))
);

process = voice;
