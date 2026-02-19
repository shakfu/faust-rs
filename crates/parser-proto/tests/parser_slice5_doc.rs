//! Integration tests for `parser_slice5_doc`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

use parser_proto::parse_program;

#[test]
fn supports_doc_statement_with_notice_listing_metadata_equation_diagram() {
    let src = concat!(
        "<mdoc>",
        "<notice/>",
        "<listingdependencies=\"true\"/>",
        "<metadata>author</metadata>",
        "<equation>_</equation>",
        "<diagram>_</diagram>",
        "</mdoc>",
        "process = _;"
    );

    let output = parse_program(src, "slice5_doc.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );
    assert!(output.root.is_some(), "root should be present");

    let ctx = &output.state.ctx;
    assert_eq!(ctx.doc_block_count(), 1);
    assert_eq!(ctx.doc_notice_count(), 1);
    assert_eq!(ctx.doc_listing_count(), 1);
    assert_eq!(ctx.lst_dependencies(), Some(true));
    assert_eq!(ctx.doc_metadata_tags().len(), 1);
    assert_eq!(ctx.doc_metadata_tags()[0].as_ref(), "author");
}

#[test]
fn supports_listing_switches_mdoctags_and_distributed() {
    let src = concat!(
        "<mdoc>",
        "<listingmdoctags=\"false\"distributed=\"true\"/>",
        "</mdoc>",
        "process = _;"
    );

    let output = parse_program(src, "slice5_listing_switches.dsp");
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let ctx = &output.state.ctx;
    assert_eq!(ctx.doc_block_count(), 1);
    assert_eq!(ctx.doc_listing_count(), 1);
    assert_eq!(ctx.lst_mdoctags(), Some(false));
    assert_eq!(ctx.lst_distributed(), Some(true));
}
