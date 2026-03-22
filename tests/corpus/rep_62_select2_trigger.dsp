import("stdfaust.lib");
// Minimal test: Select2 with counter trigger and noise excitation
// Outputs noise for 100 samples after every 256-sample beat
process = noise_out
with {
    N = 256;
    counter = (+(1)) ~ %(N);
    beat = counter == 0;
    // Envelope: fires when beat, decays
    env = max(0.0, _-0.01) ~ _ + beat : float;
    noise = (+(12345)) ~ *(1103515245) : float / 2147483647.0;
    noise_out = noise * (env > 0.0);
};
