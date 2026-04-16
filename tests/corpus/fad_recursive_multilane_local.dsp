process = step ~ (_, _)
with {
    target = hslider("Target", 0, -1, 1, 0.01);
    step(a, b) = (fad((a - target) ^ 2, a) : !, _, b);
};
