//! Integration tests for `parser_smoke`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

use parser_proto::parse_minimal;

#[test]
fn minimal_lrpar_lrlex_pipeline_accepts_process_wire() {
    assert!(parse_minimal("process = _;"));
}

#[test]
fn minimal_lrpar_lrlex_pipeline_recovers_invalid_sentence() {
    assert!(parse_minimal("process = ;"));
}
