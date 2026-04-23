k = hslider("k", 3, 0, 7, 1);

process = fad(rwtable(8, 0.0, int(k), k, k), k);
