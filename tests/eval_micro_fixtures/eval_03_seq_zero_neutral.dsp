foo(O,N) = bar(O,N) with {
  bar(N) = baz(1), _, *(-1.0);
  baz(i) = seq(j,N-1-i,_);
};
process = foo(3,2);
