process = step ~ _
with {
    target = hslider("Target", 0, -1, 1, 0.01);
    step(prev) = fad((prev - target) ^ 2, prev) : (_, _) : +;
};
