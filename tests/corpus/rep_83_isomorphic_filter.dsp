// This test demonstrates how fusioning REC groups works and simplifies generated C++ code.
// Without fusion, `smooth` on both parallel paths would allocate independent state variables (fRec0, fRec1)
// and duplicate the recursive compute loop.
// With isomorphic REC group fusion, both applications of `smooth(0.999)` operate on the same input
// and have the exact same recursive shape, causing them to be merged into a single state variable.
// The generated C++ will show only one loop relying on a single fRec state.

smooth(c) = *(1-c) : +~*(c);
process = _ <: smooth(0.999), smooth(0.999);
