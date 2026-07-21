// Fixture for `division_by_zero_is_a_clean_diagnostic_not_a_panic`
// (crates/compiler/tests/cli_diagnostics_channel.rs).
//
// The divisor is a *folded* constant zero, not a literal: `0 : *(0)` only
// becomes the constant 0 during algebraic simplification. That is why the
// check cannot move to a pre-normalization scan, and why C++ detects it at
// the same point (`mterm::operator/=`, compiler/normalize/mterm.cpp:172).
//
// Reference C++ behavior, verified directly:
//   $ faust fir_constant_zero_division.dsp
//   ERROR : division by 0 in IN[0] / 0.0f      (exit 1, same with -wall)
//
// faust-rs must likewise reject it -- with a structured diagnostic and no
// panic text on stderr.
//
// The file name is historical: an earlier revision used this DSP to reach the
// FIR verifier family. It no longer gets that far.
process = _ / (0 : *(0));
