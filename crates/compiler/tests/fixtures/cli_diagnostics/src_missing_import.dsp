// Minimal fixture for the SRC-family CLI diagnostics channel test
// (crates/compiler/tests/cli_diagnostics_channel.rs).
//
// No `tests/corpus/*.dsp` file exercises source/import resolution failure,
// and the `FRS-SRC-*` codes themselves are unused dead code (see
// docs/diagnostics-codes-en.md) -- a real unresolved import surfaces as
// `CompilerError::Import`, which carries no `DiagnosticBundle` at all. This
// fixture exists to exercise that real fallback path end to end through the
// CLI's `--error-format json` channel.
import("this_library_does_not_exist_frs_src_test.lib");
process = _;
