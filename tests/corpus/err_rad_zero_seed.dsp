// rad with a zero-output seed expression must surface a structured
// `RadSeedArity` diagnostic. The seed expression has no outputs that
// could carry a partial-derivative lane.
x = hslider("x", 0.0, -1.0, 1.0, 0.01);
process = rad(sin(x), environment { });
