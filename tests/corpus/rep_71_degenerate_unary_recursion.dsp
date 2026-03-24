// Degenerate unary recursive group — canonicalization regression.
//
// # Recursive groups in the Faust signal tree
//
// Faust represents feedback circuits as de Bruijn recursion nodes.
// After `de_bruijn_to_sym`, an N-output recursive group looks like:
//
//   SigSymRec([body_0, body_1, …, body_{N-1}])  ← N physical bodies
//     SigProj(0, W)                               ← output slot 0
//     SigProj(1, W)                               ← output slot 1
//     …
//     SigProj(N-1, W)                             ← output slot N-1
//
// The FIR lowerer indexes recursive slots with a Vec, so projection
// indices must be dense: 0 ≤ proj_index < N.
//
// # What "degenerate" means
//
// A group is degenerate when it has one physical body (N = 1) but is
// still referenced via a non-zero projection index — for example
// proj(7, W).
//
// This arises from how the C++ compiler handles multi-channel feedback
// networks. Consider the N = 8 pattern below: the ~ combinator creates
// an 8-body de Bruijn group because all 8 channels are inside the loop.
// However, channels 0..6 drop their recursive input (!), so they are not
// genuinely self-recursive. The C++ pass `inlineDegenerateRecursions()`
// detects this, eliminates the 7 trivial bodies, and reduces the group
// to the single truly recursive body (channel 7). But the output
// projection for channel 7 keeps its original logical position index,
// yielding:
//
//   SigSymRec([body_7])   ← 1 physical body after C++ degeneracy elimination
//   SigProj(7, W)         ← logical index 7 — OUT OF BOUNDS (7 >= 1)!
//
// Without canonicalization, the FIR lowerer crashed:
//   "projection index 7 out of bounds for symbolic recursion group (arity = 1)"
//
// The real-world trigger was `re.zita_rev1_stereo(...)` (Birds.dsp), an
// 8-delay-line algorithmic reverb whose feedback matrix produced exactly
// this shape after evaluation and propagation.
//
// # Fix: `canonicalize_unary_rec_projections` in signal_prepare
//
// `signal_prepare` now rewrites any proj(k, group) where group has one
// physical body to proj(0, group), regardless of k. This is a narrower
// compatibility normalization, not a full port of the C++ degeneracy
// elimination machinery — the Rust fast-lane does not rebuild the
// recursive dependency graph or rewrite projection definitions through
// `hasProjDefinition`/`setProjDefinition`. It only canonicalizes the
// logical index once the physical arity is already known to be 1.
//
// # This file
//
// An 8-channel feedback bus (via ~) where channels 0..6 drop their
// recursive input (!) and only channel 7 truly feeds back through `gain`.
// This is the minimal structural pattern that, after C++ degeneracy
// elimination, produces a unary group still indexed via proj(7, ...).

import("stdfaust.lib");

N = 8;
gain = hslider("gain", 0.5, 0.0, 0.99, 0.01);

// N-channel feedback bus.
// Channels 0..N-2: recursive input is discarded (!), not self-recursive.
// Channel N-1:     recursive input is kept and multiplied by gain — the
//                  only genuinely self-recursive signal.
//
// Signal tree before C++ degeneracy elimination (N = 8 bodies):
//   SigSymRec([b0, b1, b2, b3, b4, b5, b6, b7])
//   b0..b6 = SigInput(i)          ← ignore feedback
//   b7     = SigProj(7,W) * gain  ← true recursion
//
// After C++ inlineDegenerateRecursions (1 body remains):
//   SigSymRec([b7])
//   output: SigProj(7, W)         ← index 7 >= arity 1 — canonicalized to 0

process = si.bus(N) ~ (!, !, !, !, !, !, !, *(gain));
