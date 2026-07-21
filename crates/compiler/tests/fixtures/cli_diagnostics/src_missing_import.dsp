// Fixture for the SRC-family CLI diagnostics channel test
// (crates/compiler/tests/cli_diagnostics_channel.rs).
//
// No corpus DSP exercises source/import resolution failure, so this fixture
// drives FRS-SRC-0002 end to end: code, a span on the import directive, and
// the list of searched directories.
//
// Keep the line-comment glob on the next line. It is a regression guard for
// advance_block_comment_state: a "//" comment containing a slash-star used to
// read as opening a block comment, which hid this very import from the source
// reader and cost the diagnostic its span. See tests/corpus/*.dsp.
import("this_library_does_not_exist_frs_src_test.lib");
process = _;
