inner = (3, (0 : !)) : upsampling(1 : (+ ~ _));
outerBody = inner : (+ ~ *(0.25));

process = (2, (_ : !)) : upsampling(outerBody);
