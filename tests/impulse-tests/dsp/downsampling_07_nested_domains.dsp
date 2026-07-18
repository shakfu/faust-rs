inner = (3, (0 : !)) : downsampling(1 : (+ ~ _));
outerBody = inner : (+ ~ *(0.25));

process = (2, (_ : !)) : downsampling(outerBody);
