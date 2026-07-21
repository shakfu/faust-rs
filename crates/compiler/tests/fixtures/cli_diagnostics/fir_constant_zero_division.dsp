// Minimal fixture for the FIR-family CLI diagnostics channel test
// (crates/compiler/tests/cli_diagnostics_channel.rs).
//
// No `tests/corpus/*.dsp` file naturally trips the FIR verifier: front-end
// output is valid FIR by construction unless a DSP happens to trigger a
// verifier *warning* (as opposed to a compiler-bug verifier *error*, which
// is not reachable from any known DSP input -- see docs/diagnostics-codes-en.md
// for FRS-FIR-0001). This constant-zero division lowers to a `BinOp` the FIR
// checker flags (`fir_code=FIR-B04`), which becomes fatal (FRS-FIR-0002,
// severity=warning promoted by `--fir-verify-strict`) and is exactly the
// natural way to reach the FIR family through `--check`/`--error-format json`.
process = _ / (0 : *(0));
