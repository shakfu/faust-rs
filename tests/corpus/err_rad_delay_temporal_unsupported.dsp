// Phase D contract: differentiating a temporal node (delay1) through `rad`
// must surface a structured `RadUnsupportedNode` diagnostic rather than
// silently producing a wrong gradient.
x = hslider("x", 0.0, -1.0, 1.0, 0.01);
process = rad(x', x);
