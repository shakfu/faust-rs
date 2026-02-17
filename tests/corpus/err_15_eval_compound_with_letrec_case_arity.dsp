recdef = _ letrec {
  'x = _;
};
foo = case { (x, y) => x; };
bar(u) = foo(u) with {
  tmp = recdef;
};
process = bar(1);
