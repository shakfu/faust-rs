p = hslider("p", 0.2, -0.9, 0.9, 0.001);

// Triple nested recursive regression:
// inner[n]  = p * inner[n-1] + 2
// middle[n] = inner[n] * middle[n-1] + 1
// outer[n]  = middle[n] * outer[n-1] + 0.5
inner = 2 : + ~ *(p);
middle = 1 : + ~ *(inner);
outer = 0.5 : + ~ *(middle);

process = fad(outer, p);
