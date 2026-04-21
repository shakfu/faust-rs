// Regression: nested fad(fad(eq, phi), phi) where phi comes from a recursive
// accumulator. The inner fad differentiates eq wrt phi → vel. The outer fad
// differentiates vel wrt phi → acc. All three lanes must share one recursion
// slot for phi; a prior bug forked phi into two or three distinct SYMREC
// nodes (via fresh-name drift in de_bruijn_to_sym), emitting phantom
// recursion slots and multiplying the trigger by a stuck-at-zero iRec.
//
// Self-contained — no stdfaust import. phi_gen is an unbounded ramp (linear
// accumulator without floor-wrap) so the fixture compiles without library
// dependencies. The AD identity is preserved regardless of wrapping.
//
// Expected outputs:
//   out[0] = sin(phi) + 0.3*cos(2.5*phi)            (position)
//   out[1] = cos(phi) - 0.75*sin(2.5*phi)           (velocity  = d pos/d phi)
//   out[2] = -sin(phi) - 1.875*cos(2.5*phi)         (accel     = d vel/d phi)
// All three reference exactly one recursion slot (fRec for phi).

SR = 44100;
TWO_PI = 6.283185307179586;

phi_gen = (+(0.5 / SR)) ~ _ : *(TWO_PI);

kin(phi) = pos, vel, acc
with {
    eq  = sin(phi) + 0.3 * cos(2.5 * phi);
    pos = eq;
    vel = fad(eq, phi) : !, _;
    acc = fad(vel, phi) : !, _;
};

process = phi_gen : kin;
