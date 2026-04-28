// Mixed AD: a temporal violation in the inner RAD must still be
// surfaced as a structured RAD diagnostic when the outer pass is FAD.
// fad(rad(x', x), x) is rejected because the inner rad would require a
// non-causal transpose of `x'`. Without this guarantee, wrapping `rad`
// inside `fad` could mask the missing gradient.
x = hslider("x", 0.0, -1.0, 1.0, 0.01);
process = fad(rad(x', x), x);
