make(x) = case {
  (0) => x;
};
process = make(1)(0) + make(2)(0);
