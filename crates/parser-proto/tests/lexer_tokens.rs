//! Integration tests for lexer_tokens.rs.

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
fn doc_listing_and_equation_states_are_tokenized() {
    assert_eq!(
        names(r#"<mdoc><notice/><listingtrue=false"/></mdoc>"#),
        vec![
            "BDOC", "NOTICE", "BLST", "LSTTRUE", "LSTEQ", "LSTFALSE", "LSTQ", "ELST", "EDOC",
        ]
    );

    assert_eq!(
        names("<mdoc><equation>process = _;</equation></mdoc>"),
        vec![
            "BDOC", "BEQN", "PROCESS", "DEF", "WIRE", "ENDDEF", "EEQN", "EDOC",
        ]
    );
}

#[test]
fn extended_keyword_matrix_matches_cpp_lexer_surface() {
    let cases = [
        ("prefix", "PREFIX"),
        ("int", "INTCAST"),
        ("float", "FLOATCAST"),
        ("any", "NOTYPECAST"),
        ("rdtable", "RDTBL"),
        ("rwtable", "RWTBL"),
        ("select2", "SELECT2"),
        ("select3", "SELECT3"),
        ("ffunction", "FFUNCTION"),
        ("fconstant", "FCONSTANT"),
        ("fvariable", "FVARIABLE"),
        ("vgroup", "VGROUP"),
        ("hgroup", "HGROUP"),
        ("tgroup", "TGROUP"),
        ("soundfile", "SOUNDFILE"),
        ("attach", "ATTACH"),
        ("minput", "MODULATE"),
        ("acos", "ACOS"),
        ("asin", "ASIN"),
        ("atan", "ATAN"),
        ("atan2", "ATAN2"),
        ("cos", "COS"),
        ("sin", "SIN"),
        ("tan", "TAN"),
        ("exp", "EXP"),
        ("log", "LOG"),
        ("log10", "LOG10"),
        ("pow", "POWFUN"),
        ("sqrt", "SQRT"),
        ("abs", "ABS"),
        ("fmod", "FMOD"),
        ("remainder", "REMAINDER"),
        ("floor", "FLOOR"),
        ("ceil", "CEIL"),
        ("rint", "RINT"),
        ("round", "ROUND"),
        ("inputs", "INPUTS"),
        ("outputs", "OUTPUTS"),
        ("ondemand", "ONDEMAND"),
        ("upsampling", "UPSAMPLING"),
        ("downsampling", "DOWNSAMPLING"),
        ("import", "IMPORT"),
        ("component", "COMPONENT"),
        ("library", "LIBRARY"),
        ("environment", "ENVIRONMENT"),
        ("waveform", "WAVEFORM"),
        ("route", "ROUTE"),
        ("enable", "ENABLE"),
        ("control", "CONTROL"),
        ("declare", "DECLARE"),
        ("case", "CASE"),
        ("assertbounds", "ASSERTBOUNDS"),
        ("lowest", "LOWEST"),
        ("highest", "HIGHEST"),
        ("singleprecision", "FLOATMODE"),
        ("doubleprecision", "DOUBLEMODE"),
        ("quadprecision", "QUADMODE"),
        ("fixedpointprecision", "FIXEDPOINTMODE"),
    ];

    for (lexeme, token_name) in cases {
        assert_eq!(
            names(lexeme),
            vec![token_name],
            "keyword `{lexeme}` mismatch"
        );
    }
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
