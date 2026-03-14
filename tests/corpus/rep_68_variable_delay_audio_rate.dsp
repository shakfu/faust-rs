// Variable delay where the amount is derived from an audio-rate input.
// `int(_+10)` has interval [9,11] (bounded, positive) so it is accepted
// as a delay amount and produces a delay line sized next_pow2(12) = 16.
// Matches C++ compiler behaviour: checkDelayInterval checks interval bounds
// only, not variability — audio-rate amounts with bounded intervals compile.
process = @(int(_+10));
