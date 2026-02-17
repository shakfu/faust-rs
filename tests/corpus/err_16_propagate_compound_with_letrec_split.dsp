foo = _,_ <: _,_,_;
bar = foo with {
  recw = _ letrec {
    'r = _;
  };
};
baz = bar;
process = baz,baz;
