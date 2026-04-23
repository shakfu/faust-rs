step = hslider("step", 0, -1, 1, 1);
phase = step : + ~ _;

process = fad(rdtable(waveform{0, 1, 4, 9, 16, 25, 36, 49}, phase), step);
