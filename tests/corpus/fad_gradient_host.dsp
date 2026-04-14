target_gain = 0.7;
g = hslider("g", 0.1, 0, 1, 0.001);
error = _ <: (*(g) - *(target_gain)) : ^(2);
process = fad(error);
