// Variable delay whose amount involves `ma.SR` (= min(192000, max(1, fSamplingFreq))).
// `fSamplingFreq` carries an empty interval in the type system, which poisons
// the entire `hslider * SR / 1000` chain via interval algebra.
// The delay amount is `min(65536, max(0, int(hslider * SR/1000) - 1))`.
// Since interval analysis yields empty for this expression, the compiler must
// fall back to the structural `SIGMIN(const, _)` upper bound (65536) to size
// the delay line — matching the C++ output: `fVec[IOTA & 131071]`.
import("stdfaust.lib");
process = de.delay(65536, int(hslider("ms", 0, 0, 1000, 1) * ma.SR / 1000.0));
