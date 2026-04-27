// Reverse-AD parse-only fixture: `rad(expr, seeds)` reaches propagate, where
// reverse-mode expansion is still phase-gated and surfaces a structured
// `UnsupportedBox` diagnostic. Once propagation support lands this fixture
// will be promoted into a runtime corpus.
f = hslider("f", 1, 0, 10, 0.1);
p = hslider("p", 0, -1, 1, 0.01);
process = rad(f : sin, p);
