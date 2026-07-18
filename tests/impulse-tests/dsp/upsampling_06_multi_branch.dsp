fast = 1 : (+ ~ _);
slow = 10 : (+ ~ *(0.75));
delayed = fast : @(3);

process = ((4, (_ : !)) : upsampling(fast, slow, delayed)) :> _;
