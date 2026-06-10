// Accepted RAD E1 fixture: coupled two-state strict-LTI recursion.
//
// With inputs (x0, x1), the primal recurrence is:
//
//   y0[n] = x0[n] + q * y1[n-1]
//   y1[n] = x1[n] + p * y0[n-1]
//
// `p` and `q` are literal block-invariant seeds. Output bundle:
// [y0, y1, dp, dq], with per-sample gradient contribution lanes.

p = 0.5;
q = 0.25;
interleave22 = _,_,_,_ <: _,!,!,!, !,!,_,!, !,_,!,!, !,!,!,_;
cross2 = _,_ <: !,_,_,!;
core = (interleave22 : (+, +)) ~ ((*(p), *(q)) : cross2);
process = rad((_, _) : core, (p, q));
