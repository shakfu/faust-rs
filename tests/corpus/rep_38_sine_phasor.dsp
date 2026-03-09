freq1 = hslider("freq1", 440.0, 20.0, 3000.0, 1.0);
freq2 = hslider("freq2", 440.0, 20.0, 3000.0, 1.0);
gain = hslider("gain", 0.2, 0.0, 1.0, 0.01);

phase1 = +(freq1 / 48000.0) ~ _;
phase2 = +(freq2 / 48000.0) ~ _;

process = gain * sin(6.283185307179586 * phase1), gain * sin(6.283185307179586 * phase2) ;
