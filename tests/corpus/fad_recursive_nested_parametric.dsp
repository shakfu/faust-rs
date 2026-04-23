p = hslider("p", 0.2, -0.9, 0.9, 0.001);

// Nested recursive regression:
// inner[n] = p * inner[n-1] + 2
// outer[n] = inner[n] * outer[n-1] + 1
inner = 2 : + ~ *(p);
outer = 1 : + ~ *(inner);

process = fad(outer, p);
