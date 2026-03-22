import("stdfaust.lib");
// Test fabs-based trigger: outputs 1 when abs(counter_f - 5.0) < 0.5
N = 10;
counter = (+(1)) ~ %(N) : float;
process = (abs(counter - 5.0) < 0.5) : float;
