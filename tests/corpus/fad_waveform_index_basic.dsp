k = hslider("k", 3, 1, 6, 1);

process = fad(rdtable(waveform{0, 1, 4, 9, 16, 25, 36, 49}, k), k);
