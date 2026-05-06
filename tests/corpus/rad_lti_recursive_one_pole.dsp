// Accepted RAD E1 fixture: one-pole strict-LTI recursion with one audio input.
//
// The primal is:
//
//   y[n] = x[n] + p * y[n-1]
//
// `p` is a literal block-invariant seed, so the recursive group is accepted by
// the E1 block-local transpose. Output bundle: [y, dp], where `dp[n]` is the
// per-sample contribution `lambda[n] * y[n-1]`, not a block-reduced scalar.
p = 0.5;
process = rad((_ : + ~ *(p)), p);
