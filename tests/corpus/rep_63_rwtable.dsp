// Test rwtable: counter writes ascending values, read-back at index 0
N = 16;
counter = (+(1)) ~ %(N);
val = counter : float;
// Write val at position counter, read from position 0
process = rwtable(N, 0.0, counter, val, 0) : float;
