import("stdfaust.lib");

process = no.noise * (hslider("gain [style:knob]", 0.5, 0, 1, 0.01) : si.smoo);
