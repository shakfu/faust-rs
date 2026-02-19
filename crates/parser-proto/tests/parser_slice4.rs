//! Integration tests for `parser_slice4`.
//!
//! Scope:
//! - Exercises public APIs and structural invariants for the targeted module.
//! - Guards regression/parity behavior on representative fixtures and corpus cases.

use parser_proto::parse_program;

#[test]
fn supports_import_statement_and_records_import_path() {
    let output = parse_program(
        r#"import("stdfaust.lib"); process = _;"#,
        "slice4_import.dsp",
    );
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );
    assert!(output.root.is_some(), "root should be present");
    assert_eq!(output.state.ctx.imports().len(), 1);
    assert_eq!(output.state.ctx.imports()[0].as_ref(), "stdfaust.lib");
}

#[test]
fn supports_declare_metadata_forms() {
    let output = parse_program(
        r#"declare author "letz"; declare proc category "ui"; process = _;"#,
        "slice4_declare.dsp",
    );
    assert!(
        output.errors.is_empty(),
        "unexpected parse errors: {:?}",
        output.errors
    );

    let declared = output.state.ctx.declared_metadata();
    assert_eq!(declared.len(), 1);
    assert_eq!(declared[0].0.as_ref(), "author");
    assert_eq!(declared[0].1.as_ref(), "letz");

    let def_declared = output.state.ctx.declared_definition_metadata();
    assert_eq!(def_declared.len(), 1);
    assert_eq!(def_declared[0].0.as_ref(), "proc");
    assert_eq!(def_declared[0].1.as_ref(), "category");
    assert_eq!(def_declared[0].2.as_ref(), "ui");
}
