// Variable delay whose amount involves `ma.SR` (= min(192000, max(1, fSamplingFreq))).
// `fSamplingFreq` carries interval [f64::MIN, f64::MAX] (the C++ `interval()` default),
// so the interval algebra produces a concrete bound:
//   max(1, fSamplingFreq)          ∈ [1, MAX]
//   min(192000, …)                 ∈ [1, 192000]
//   hslider[0,1000] * … / 1000    ∈ [0, 192000]
//   int_cast − 1 → max(0,…)       ∈ [0, 191999]
//   min(65536, …)                  ∈ [0, 65536]   ← check_delay_interval returns 65536
// Delay line size = next_pow2(65537) = 131072, matching C++: `fVec[IOTA & 131071]`.
import("stdfaust.lib");
process = de.delay(65536, int(hslider("ms", 0, 0, 1000, 1) * ma.SR / 1000.0));
