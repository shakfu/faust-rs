x = 10;
foo = case {
  (y) => x + y;
};
process = foo(2);
