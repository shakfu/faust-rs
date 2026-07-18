slow = 1 : (+ ~ _);
decay = 10 : (+ ~ *(0.75));
delayed = slow : @(3);

process = ((4, (_ : !)) : downsampling(slow, decay, delayed)) :> _;
