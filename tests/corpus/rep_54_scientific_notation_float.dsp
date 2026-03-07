// rep_54 — scientific notation float literals and precision qualifiers
//
// Exercises the full set of float literal forms (negative exponent, positive
// exponent, unsigned exponent, dot-leading) and precision-qualified
// definitions.  Without the lexer alternation reorder, all e-notation forms
// failed because the shorter [0-9]+\.[0-9]*f? alternative matched first.
//
// In single-precision mode (default) only the `singleprecision` binding is
// active; the others are silently filtered by the parser.

singleprecision  TINY = 1.192092896e-07;
doubleprecision  TINY = 2.2204460492503131e-016;
quadprecision    TINY = 1.084202172485504434007452e-019;
fixedpointprecision TINY = 2.2204460492503131e-016;

EXP_NEG  = 1.5e-3;
EXP_POS  = 1.5e+3;
EXP_BARE = 1.5e3;
DOT_EXP  = .75e-2;

process = TINY + EXP_NEG + EXP_POS + EXP_BARE + DOT_EXP;
