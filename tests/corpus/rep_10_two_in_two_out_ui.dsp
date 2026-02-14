gL = hslider("left/gain", 0.8, 0.0, 1.5, 0.01);
gR = hslider("right/gain", 0.8, 0.0, 1.5, 0.01);
process = _,_ : *(gL),*(gR);
