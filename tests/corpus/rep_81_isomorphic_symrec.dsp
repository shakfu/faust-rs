// By copying the input to both instances of `foo`, they share the same tree root 
// and thus their structurally equivalent recursion is detected and merged.
// The generated C++ code will use a single `fRec` variable instead of two.
foo = +~_;
process = _ <: foo, foo;
