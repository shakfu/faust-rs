// Minimal test: counter with modulo, outputs 1.0 at beat (every 10 samples)
process = (counter == 0) : float
with {
    counter = (+(1)) ~ %(10);
};
