// This test demonstrates isomorphic fusion of nested recursive groups.
// 0-input nested loops automatically form structurally identical ASTs 
// in Faust when duplicated.
// Both `gen` invocations should be mapped back to a single shared C++ compute block 
// with a consolidated feedback variable hierarchy.

gen = (+ ~ *(0.5)) ~ *(0.5);
process = gen, gen;
