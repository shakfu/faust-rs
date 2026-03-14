// Route whose ins/outs/spec contain arithmetic sub-expressions.
// C++ eval.cpp resolves them at evaluation time via boxPropagateSig:
//   ins  = 1+1 → 2
//   outs = 1+1 → 2
//   spec = (1,1, 2,2) — already literals, but tested alongside arithmetic params
// The result is a canonical route(2,2,...) with all Int-literal children.
// Without eval-time resolution the Rust propagate crate fails because
// usize_from_int_node / flatten_route_ints require literal Int nodes.
process = route(1+1, 1+1, 1,1, 2,2);
