p = hslider("p", 0.2, -0.9, 0.9, 0.001);
bus2 = _,_;
cross2 = _,_ <: !,_,_,!;

// Nested mutual-recursion regression:
//   core0[n] = p * core1[n-1]
//   core1[n] = 0.25 * core0[n-1]
//   mix[n]   = (core0[n] + core1[n]) * mix[n-1] + 1
core = bus2 ~ ((*(p), *(0.25)) : cross2);
mix = 1 : + ~ *(core : +);

process = fad(mix, p);
