// rad through a read-only table index. The table contents are constant
// data, so RAD differentiates only through the read index using the same
// symmetric finite-difference slope as FAD:
//   slope(k) ≈ (rdtable(T, k+1) - rdtable(T, k-1)) / 2
// Expected output bundle: [rdtable(T, k), slope(k)]
k = hslider("k", 3.0, 1, 6, 1);
process = rad(rdtable(waveform{0, 1, 4, 9, 16, 25, 36, 49}, k), k);
