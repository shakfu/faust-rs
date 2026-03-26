//! Build script for production parser grammar/lexer generation.

use cfgrammar::yacc::YaccKind;
use lrlex::CTLexerBuilder;
use lrpar::RecoveryKind;

fn main() {
    // `lrpar`'s CPCT+ recovery uses `Instant::now()` to enforce a repair budget.
    // That works on native targets but traps on the bare `wasm32-unknown-unknown`
    // target used by the embedded faustwasm compiler module. Keep native builds on
    // the richer recoverer and switch wasm builds to `None` so the generated parser
    // remains loadable in the browser/worker environment.
    let recoverer = if std::env::var("TARGET")
        .map(|target| target.starts_with("wasm32-unknown-unknown"))
        .unwrap_or(false)
    {
        RecoveryKind::None
    } else {
        RecoveryKind::CPCTPlus
    };

    CTLexerBuilder::new()
        .lrpar_config(move |ctp| {
            ctp.yacckind(YaccKind::Grmtools)
                .recoverer(recoverer)
                .warnings_are_errors(true)
                .show_warnings(true)
                .grammar_in_src_dir("grammar/faustparser.y")
                .expect("invalid parser grammar path")
        })
        .lexer_in_src_dir("grammar/faustlexer.l")
        .expect("invalid lexer grammar path")
        .build()
        .expect("failed to generate parser/lexer");
}
