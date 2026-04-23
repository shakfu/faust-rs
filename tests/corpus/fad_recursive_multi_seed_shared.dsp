a = hslider("a", 0.2, -0.9, 0.9, 0.001);
b = hslider("b", 1.0, -2.0, 2.0, 0.001);

// Shared-recursion regression for the unified multi-seed transform:
//   y[n] = a * y[n-1] + b
// Output layout:
//   [y, dy/da, dy/db]
process = fad((b : + ~ *(a)), (a, b));
