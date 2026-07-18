rate = int(hslider("rate", 3, 2, 8, 1));

feedback(x) = x : (+ ~ *(0.5));
delayed(x) = x : @(4);
body(x) = x <: feedback, delayed :> _;

process = (rate, _) : upsampling(body);
