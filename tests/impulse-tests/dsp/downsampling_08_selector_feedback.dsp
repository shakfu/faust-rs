counter = 1 : (+ ~ _);
direct = counter;
delayed = counter : @(2);
body = select2(counter % 2, delayed, direct);

process = (3, (_ : !)) : downsampling(body);
