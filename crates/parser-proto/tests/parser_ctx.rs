use parser_proto::{DiagnosticSeverity, ParserCtx};
use tlib::TreeArena;

#[test]
fn def_and_use_properties_follow_cxx_contract_shape() {
    let mut arena = TreeArena::new();
    let sym = arena.symbol("gain");

    let mut ctx = ParserCtx::new();
    ctx.set_def_prop(sym, "test.dsp", 12);
    ctx.set_use_prop(sym, "test.dsp", 24);

    assert!(ctx.has_def_prop(sym));
    assert_eq!(ctx.def_file_prop(sym), Some("test.dsp"));
    assert_eq!(ctx.def_line_prop(sym), Some(12));
    assert_eq!(ctx.use_file_prop(sym), Some("test.dsp"));
    assert_eq!(ctx.use_line_prop(sym), Some(24));
}

#[test]
fn cursor_hooks_waveform_result_and_diagnostics_are_parser_local() {
    let mut arena = TreeArena::new();
    let a = arena.int(1);
    let b = arena.int(2);
    let root = arena.tag("Root");
    let sym = arena.symbol("x");

    let mut ctx = ParserCtx::new();
    ctx.set_cursor("unit.dsp", 7);
    ctx.set_def_prop_at_cursor(sym);
    assert_eq!(ctx.def_file_prop(sym), Some("unit.dsp"));
    assert_eq!(ctx.def_line_prop(sym), Some(7));

    ctx.push_waveform_value(a);
    ctx.push_waveform_value(b);
    assert_eq!(ctx.waveform(), &[a, b]);
    assert_eq!(ctx.take_waveform(), vec![a, b]);
    assert!(ctx.waveform().is_empty());

    ctx.set_parse_result(root);
    assert_eq!(ctx.parse_result(), Some(root));
    ctx.clear_parse_result();
    assert_eq!(ctx.parse_result(), None);

    assert!(ctx.diagnostics_is_empty());
    ctx.error("syntax error");
    ctx.warning("suspicious token");
    ctx.remark("prototype note");
    ctx.note_recovery();

    assert_eq!(ctx.parse_error_count(), 1);
    assert_eq!(ctx.recovery_count(), 1);
    assert_eq!(ctx.diagnostics().len(), 3);
    assert_eq!(ctx.diagnostics()[0].severity, DiagnosticSeverity::Error);
}
