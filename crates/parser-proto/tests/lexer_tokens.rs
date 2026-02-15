use parser_proto::{ParserCtx, lex_tokens, set_use_prop_from_token};
use tlib::TreeArena;

fn names(input: &str) -> Vec<String> {
    lex_tokens(input)
        .expect("lexing should succeed")
        .into_iter()
        .map(|t| t.name.into())
        .collect()
}

#[test]
fn keywords_take_priority_over_identifiers() {
    assert_eq!(
        names("with withhold letrec where"),
        vec!["WITH", "IDENT", "LETREC", "WHERE"]
    );
}

#[test]
fn operators_follow_cxx_priority_ordering() {
    assert_eq!(
        names("<: <= < :> +> : , ~ -> => >> >"),
        vec![
            "SPLIT", "LE", "LT", "MIX", "MIX", "SEQ", "PAR", "REC", "LAPPLY", "ARROW", "RSH", "GT"
        ]
    );
}

#[test]
fn numerics_and_strings_are_tokenized_with_expected_kinds() {
    assert_eq!(
        names(r#"42 42f 3.14 .5 1e-3 "abc" <foo> <foo.bar>"#),
        vec![
            "INT", "FLOAT", "FLOAT", "FLOAT", "FLOAT", "STRING", "FSTRING", "FSTRING"
        ]
    );
}

#[test]
fn comments_and_whitespace_are_skipped() {
    assert_eq!(
        names(
            r#"
            // line comment
            process = _;
            /* block
               comment */
            "#,
        ),
        vec!["PROCESS", "DEF", "WIRE", "ENDDEF"]
    );
}

#[test]
fn lexer_positions_bridge_to_parser_ctx_use_props() {
    let tokens = lex_tokens("foo\nprocess = _;").expect("lexing should succeed");
    let process_tok = tokens
        .iter()
        .find(|tok| tok.name.as_ref() == "PROCESS")
        .expect("PROCESS token should be present");
    assert_eq!(process_tok.start_line, 2);

    let mut arena = TreeArena::new();
    let sym = arena.symbol("process");
    let mut ctx = ParserCtx::new();
    set_use_prop_from_token(&mut ctx, sym, "unit.dsp", process_tok);

    assert_eq!(ctx.use_file_prop(sym), Some("unit.dsp"));
    assert_eq!(ctx.use_line_prop(sym), Some(2));
}
