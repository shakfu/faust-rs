freq1 = hslider("freq1", 440.0, 20.0, 3000.0, 1.0);
freq2 = hslider("freq2", 440.0, 20.0, 3000.0, 1.0);
gain = hslider("gain", 0.2, 0.0, 1.0, 0.01);
feedback = hslider("feedback", 0.35, 0.0, 0.95, 0.01);

phase1 = +(freq1 / 48000.0) ~ _;
phase2 = +(freq2 / 48000.0) ~ _;

echo(feedback_gain) = + ~ (@(10000) : *(feedback_gain));

voice1 = gain * sin(6.283185307179586 * phase1);
voice2 = gain * sin(6.283185307179586 * phase2);

process = (voice1 : echo(feedback)), (voice2 : echo(feedback));
