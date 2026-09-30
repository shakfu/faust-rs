declare name "noise";
declare author "faust-rs demos";

// 32-bit linear congruential generator, scaled to [-1, 1].
random = +(12345) ~ *(1103515245);
process = random / 2147483647.0 * hslider("volume", 0.5, 0, 1, 0.01);
