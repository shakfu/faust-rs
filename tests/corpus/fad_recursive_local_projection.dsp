process = step ~ _
with {
    target = hslider("Target", 0, -1, 1, 0.01);
    lr = hslider("LR", 0.05, 0.0001, 0.5, 0.0001);
    step(prev) = prev - lr * grad
    with {
        loss = (prev - target) ^ 2;
        grad = fad(loss, prev) : !, _;
    };
};
