k = hslider("k", 3, 1, 6, 1);

process = fad(rdtable(8, 1 : + ~ _, k), k);
