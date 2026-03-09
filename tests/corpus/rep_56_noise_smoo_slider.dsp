SR = fconstant(int fSamplingFreq, <math.h>);
gain = hslider("gain [style:knob]", 0.5, 0.0, 1.0, 0.01);
noise = +(12345) ~ *(1103515245);

process = noise * gain / SR;
