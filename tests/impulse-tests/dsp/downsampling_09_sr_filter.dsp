import("stdfaust.lib");

rate = 3;
carrier = 0.2 * (os.osc(440) + 0.5 * os.osc(660));
body(x) = (x + carrier) : fi.lowpass(2, 3000);

process = (rate, _) : downsampling(body);
