foo = _,_ <: _,_,_;
bar = foo;
baz = bar with {
  local = _;
};
process = baz,baz;
