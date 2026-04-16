process = step ~ _
with {
    target = hslider("Target", 0, -1, 1, 0.01);
    lr = hslider("LR", 0.05, 0.0001, 0.5, 0.0001);
    step(prev) = prev - lr * (g1 + g2)
    with {
        g1 = fad((prev - target) ^ 2, prev) : !, _;
        g2 = fad((prev + target) ^ 2, prev) : !, _;
    };
};
