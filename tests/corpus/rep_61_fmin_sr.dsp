import("stdfaust.lib");
// Test using sample rate via min/max (exercises fmin/fmax host calls in compute loop)
process = min(ma.SR, 192000.0) / 48000.0 : float;
