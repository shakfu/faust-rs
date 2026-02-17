#![doc = "Structured diagnostics model for the `faust-rs` workspace."]

use std::path::PathBuf;

pub mod codes;

pub use codes::all_codes;

pub const CRATE_NAME: &str = "errors";

#[must_use]
pub fn crate_id() -> &'static str {
    CRATE_NAME
}

/// Diagnostic severity level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Severity {
    Error,
    Warning,
    Remark,
}

/// Compiler stage producing one diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    SourceReader,
    Lexer,
    Parser,
    Eval,
    Propagate,
    Normalize,
    Transform,
    Fir,
    Codegen,
    Compiler,
}

/// Stable diagnostic code identifier (for tests, CI, and tooling integrations).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DiagnosticCode(pub &'static str);

/// File-local source span.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub file: PathBuf,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

impl SourceSpan {
    /// Creates a source span.
    #[must_use]
    pub fn new(file: impl Into<PathBuf>, line: u32, col: u32, end_line: u32, end_col: u32) -> Self {
        Self {
            file: file.into(),
            line,
            col,
            end_line,
            end_col,
        }
    }
}

/// Source label style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LabelStyle {
    Primary,
    Secondary,
}

/// One labeled source span attached to a diagnostic.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Label {
    pub style: LabelStyle,
    pub span: SourceSpan,
    pub message: Box<str>,
}

impl Label {
    /// Creates a label.
    #[must_use]
    pub fn new(style: LabelStyle, span: SourceSpan, message: impl Into<Box<str>>) -> Self {
        Self {
            style,
            span,
            message: message.into(),
        }
    }
}

/// Structured diagnostic payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub stage: Stage,
    pub code: DiagnosticCode,
    pub message: Box<str>,
    pub labels: Vec<Label>,
    pub notes: Vec<Box<str>>,
    pub help: Vec<Box<str>>,
}

impl Diagnostic {
    /// Creates a diagnostic with empty labels/notes/help.
    #[must_use]
    pub fn new(
        severity: Severity,
        stage: Stage,
        code: DiagnosticCode,
        message: impl Into<Box<str>>,
    ) -> Self {
        Self {
            severity,
            stage,
            code,
            message: message.into(),
            labels: Vec::new(),
            notes: Vec::new(),
            help: Vec::new(),
        }
    }

    /// Adds one source label and returns the updated diagnostic.
    #[must_use]
    pub fn with_label(mut self, label: Label) -> Self {
        self.labels.push(label);
        self
    }

    /// Adds one note and returns the updated diagnostic.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<Box<str>>) -> Self {
        self.notes.push(note.into());
        self
    }

    /// Adds one help entry and returns the updated diagnostic.
    #[must_use]
    pub fn with_help(mut self, help: impl Into<Box<str>>) -> Self {
        self.help.push(help.into());
        self
    }
}

/// Aggregated diagnostics for one stage/session outcome.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagnosticBundle {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticBundle {
    /// Creates an empty bundle.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one diagnostic.
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    /// Extends this bundle with another sequence of diagnostics.
    pub fn extend(&mut self, diagnostics: impl IntoIterator<Item = Diagnostic>) {
        self.diagnostics.extend(diagnostics);
    }

    /// Returns all diagnostics as a read-only slice.
    #[must_use]
    pub fn as_slice(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Number of diagnostics stored in this bundle.
    #[must_use]
    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    /// Returns `true` when no diagnostics are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    /// Counts diagnostics with [`Severity::Error`].
    #[must_use]
    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count()
    }
}

impl From<Vec<Diagnostic>> for DiagnosticBundle {
    fn from(diagnostics: Vec<Diagnostic>) -> Self {
        Self { diagnostics }
    }
}

/// Conversion contract for phase-local errors to diagnostics.
pub trait IntoDiagnostic {
    fn into_diagnostic(self) -> Diagnostic;
}

#[cfg(test)]
mod tests {
    use super::{
        Diagnostic, DiagnosticBundle, DiagnosticCode, Label, LabelStyle, Severity, SourceSpan, Stage,
    };

    #[test]
    fn diagnostic_builder_keeps_fields_and_payloads() {
        let span = SourceSpan::new("foo.dsp", 3, 5, 3, 9);
        let diag = Diagnostic::new(
            Severity::Error,
            Stage::Parser,
            DiagnosticCode("FRS-PARSE-0001"),
            "unexpected token",
        )
        .with_label(Label::new(LabelStyle::Primary, span.clone(), "here"))
        .with_note("while parsing process definition")
        .with_help("check missing ';'");

        assert_eq!(diag.severity, Severity::Error);
        assert_eq!(diag.stage, Stage::Parser);
        assert_eq!(diag.code, DiagnosticCode("FRS-PARSE-0001"));
        assert_eq!(diag.message.as_ref(), "unexpected token");
        assert_eq!(diag.labels.len(), 1);
        assert_eq!(diag.labels[0].span, span);
        assert_eq!(diag.notes.len(), 1);
        assert_eq!(diag.help.len(), 1);
    }

    #[test]
    fn bundle_counts_error_severity_only() {
        let mut bundle = DiagnosticBundle::new();
        bundle.push(Diagnostic::new(
            Severity::Warning,
            Stage::Eval,
            DiagnosticCode("FRS-EVAL-0100"),
            "non-fatal warning",
        ));
        bundle.push(Diagnostic::new(
            Severity::Error,
            Stage::Eval,
            DiagnosticCode("FRS-EVAL-0001"),
            "undefined symbol",
        ));

        assert_eq!(bundle.len(), 2);
        assert_eq!(bundle.error_count(), 1);
        assert!(!bundle.is_empty());
    }
}
