// Regression: fad(expr, seed) inside a lambda whose argument is a recursive
// signal (here `phi_gen`). The seed reference and every occurrence of the
// slot inside `body` share one de Bruijn recursion sub-term. Before the fix,
// the two independent `de_bruijn_to_sym` calls (one for the outputs, one for
// the seed) allocated divergent fresh recursion names (`W0` vs. `W1`), so
// the FAD transform could no longer recognise the seed inside the output
// graph, producing a spurious second recursion slot in the generated code
// and a tangent that silently differentiated with respect to the inner
// `frac` accumulator (leaking a factor of `2 * PI`).
import("stdfaust.lib");
kin(phi) = eq, vel with {
    eq = sin(phi) + 0.3 * cos(2.5 * phi);
    vel = fad(eq, phi) : !, _;
};
phi_gen = (+(0.5 / ma.SR) : ma.frac) ~ _ : *(2 * ma.PI);
process = phi_gen : kin;
