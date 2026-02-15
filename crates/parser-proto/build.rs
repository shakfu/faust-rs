use cfgrammar::yacc::YaccKind;
use lrlex::CTLexerBuilder;

fn main() {
    CTLexerBuilder::new()
        .lrpar_config(|ctp| {
            ctp.yacckind(YaccKind::Grmtools)
                .warnings_are_errors(false)
                .show_warnings(false)
                .grammar_in_src_dir("grammar/faustparser.y")
                .expect("invalid parser grammar path")
        })
        .lexer_in_src_dir("grammar/faustlexer.l")
        .expect("invalid lexer grammar path")
        .build()
        .expect("failed to generate parser/lexer");
}
