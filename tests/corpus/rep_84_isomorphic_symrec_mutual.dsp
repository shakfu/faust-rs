// This test demonstrates the isomorphic fusion of complex mutual recursions.
// `osc` initializes a 2-variable cycle (coupled oscillator) tied together by `~`.
// Because the two oscillators are spawned from the same input parameter stream `_`
// they are computationally isomorphic and should generate a single C++ mutual loop 
// producing `iRec/fRec` pairs instead of two disjoint loops.

osc(f) = (_,_) ~ (\(x, y).(x * cos(f) - y * sin(f), x * sin(f) + y * cos(f)));
process = _ <: osc, osc;
