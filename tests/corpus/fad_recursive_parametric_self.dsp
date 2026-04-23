p = hslider("p", 0.2, -0.9, 0.9, 0.001);

// Single-state recursive regression:
//   y[n] = p * y[n-1] + 2
//   dy/dp[n] = y[n-1] + p * dy/dp[n-1]
process = fad((2 : + ~ *(p)), p);
