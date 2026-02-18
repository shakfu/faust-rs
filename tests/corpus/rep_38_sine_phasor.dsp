freq = hslider("freq", 440.0, 20.0, 3000.0, 1.0);
gain = hslider("gain", 0.2, 0.0, 1.0, 0.01);

phase = +(freq / 48000.0) ~ _;
process = gain * sin(6.283185307179586 * phase);
