// fad(body, seed) with a seed that has several outputs.
// The seed `(f, g)` is a par of two sliders, so fad bundles two
// independent differentiation variables through a single node.
// Expected outputs: [sin(f*g), cos(f*g)*g, cos(f*g)*f].
f = hslider("f", 1.0, 0.0, 10.0, 0.1);
g = hslider("g", 1.0, 0.0, 10.0, 0.1);
process = fad(f * g : sin, (f, g));
