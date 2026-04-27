// rad with a zero-output body must surface a structured `RadBodyArity`
// diagnostic, not a generic UnsupportedBox. `environment { }` evaluates
// to a closed scope with zero outputs in the box layer.
x = hslider("x", 0.0, -1.0, 1.0, 0.01);
process = rad(environment { }, x);
