//! Parser context for the `lrpar/lrlex` migration prototype.
//!
//! # Source provenance (C++)
//! - `compiler/parser/faustparser.y`:
//!   - parser cursor (`FAUSTfilename`, `FAUSTlineno`)
//!   - waveform accumulator (`gGlobal->gWaveForm`)
//!   - parse root storage (`gGlobal->gResult`)
//! - `compiler/errors/errormsg.cpp`:
//!   - definition/use properties (`setDefProp`, `setUseProp`)
//!
//! # Parity invariants
//! - Definition/use properties are attached to `TreeId` symbols with source file + line payload.
//! - Waveform values are accumulated in parse order then drained by the corresponding action.
//! - Parser diagnostics are explicitly scoped to one parser context (no global mutable singleton).

use tlib::{PropertyKey, PropertyStore, TreeId};

/// Parser source location equivalent to `(filename, lineno)` in C++ parser globals.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLocation {
    file: Box<str>,
    line: u32,
}

impl SourceLocation {
    /// Creates a source location.
    #[must_use]
    pub fn new(file: &str, line: u32) -> Self {
        Self {
            file: file.into(),
            line,
        }
    }

    /// Source file path/name.
    #[must_use]
    pub fn file(&self) -> &str {
        &self.file
    }

    /// 1-based line number.
    #[must_use]
    pub fn line(&self) -> u32 {
        self.line
    }
}

/// Diagnostic severity levels used during parsing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
    Remark,
}

/// Parser diagnostic message with optional source location.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParserDiagnostic {
    pub severity: DiagnosticSeverity,
    pub message: Box<str>,
    pub location: Option<SourceLocation>,
}

/// Parser-local context replacing the parser-relevant subset of `gGlobal`.
#[derive(Debug)]
pub struct ParserCtx {
    cursor: SourceLocation,
    diagnostics: Vec<ParserDiagnostic>,
    parse_error_count: u32,
    recovery_count: u32,
    waveform: Vec<TreeId>,
    parse_result: Option<TreeId>,
    imports: Vec<Box<str>>,
    declared_metadata: Vec<(Box<str>, Box<str>)>,
    declared_definition_metadata: Vec<(Box<str>, Box<str>, Box<str>)>,
    doc_block_count: u32,
    doc_notice_count: u32,
    doc_listing_count: u32,
    doc_char_count: u32,
    doc_metadata_tags: Vec<Box<str>>,
    lst_dependencies: Option<bool>,
    lst_mdoctags: Option<bool>,
    lst_distributed: Option<bool>,
    props: PropertyStore<SourceLocation>,
    def_prop_key: PropertyKey,
    use_prop_key: PropertyKey,
}

impl Default for ParserCtx {
    fn default() -> Self {
        Self::new()
    }
}

impl ParserCtx {
    /// Creates a fresh parser context.
    ///
    /// Cursor defaults follow C++ parser defaults (`FAUSTfilename="????"`).
    #[must_use]
    pub fn new() -> Self {
        let mut props = PropertyStore::new();
        let def_prop_key = props.key("DEFLINEPROP");
        let use_prop_key = props.key("USELINEPROP");
        Self {
            cursor: SourceLocation::new("????", 1),
            diagnostics: Vec::new(),
            parse_error_count: 0,
            recovery_count: 0,
            waveform: Vec::new(),
            parse_result: None,
            imports: Vec::new(),
            declared_metadata: Vec::new(),
            declared_definition_metadata: Vec::new(),
            doc_block_count: 0,
            doc_notice_count: 0,
            doc_listing_count: 0,
            doc_char_count: 0,
            doc_metadata_tags: Vec::new(),
            lst_dependencies: None,
            lst_mdoctags: None,
            lst_distributed: None,
            props,
            def_prop_key,
            use_prop_key,
        }
    }

    /// Sets parser cursor location (equivalent to lexer-maintained file/line globals).
    pub fn set_cursor(&mut self, file: &str, line: u32) {
        self.cursor = SourceLocation::new(file, line);
    }

    /// Returns current parser cursor.
    #[must_use]
    pub fn cursor(&self) -> &SourceLocation {
        &self.cursor
    }

    /// Appends one waveform value in parse order.
    pub fn push_waveform_value(&mut self, value: TreeId) {
        self.waveform.push(value);
    }

    /// Returns current waveform buffer.
    #[must_use]
    pub fn waveform(&self) -> &[TreeId] {
        &self.waveform
    }

    /// Drains waveform buffer in FIFO parse order.
    pub fn take_waveform(&mut self) -> Vec<TreeId> {
        std::mem::take(&mut self.waveform)
    }

    /// Sets parse root result.
    pub fn set_parse_result(&mut self, root: TreeId) {
        self.parse_result = Some(root);
    }

    /// Returns parse root result if set.
    #[must_use]
    pub fn parse_result(&self) -> Option<TreeId> {
        self.parse_result
    }

    /// Clears parse root result.
    pub fn clear_parse_result(&mut self) {
        self.parse_result = None;
    }

    /// Records one `import("...")` statement payload.
    pub fn note_import(&mut self, path: &str) {
        self.imports.push(path.into());
    }

    /// Recorded import paths in parse order.
    #[must_use]
    pub fn imports(&self) -> &[Box<str>] {
        &self.imports
    }

    /// Records `declare key value;`.
    pub fn note_declared_metadata(&mut self, key: &str, value: &str) {
        self.declared_metadata.push((key.into(), value.into()));
    }

    /// Records `declare def key value;`.
    pub fn note_declared_definition_metadata(&mut self, def: &str, key: &str, value: &str) {
        self.declared_definition_metadata
            .push((def.into(), key.into(), value.into()));
    }

    /// Recorded `declare key value;` entries.
    #[must_use]
    pub fn declared_metadata(&self) -> &[(Box<str>, Box<str>)] {
        &self.declared_metadata
    }

    /// Recorded `declare def key value;` entries.
    #[must_use]
    pub fn declared_definition_metadata(&self) -> &[(Box<str>, Box<str>, Box<str>)] {
        &self.declared_definition_metadata
    }

    /// Records one parsed doc block.
    pub fn note_doc_block(&mut self) {
        self.doc_block_count = self.doc_block_count.saturating_add(1);
    }

    /// Number of parsed doc blocks.
    #[must_use]
    pub fn doc_block_count(&self) -> u32 {
        self.doc_block_count
    }

    /// Records one parsed doc notice.
    pub fn note_doc_notice(&mut self) {
        self.doc_notice_count = self.doc_notice_count.saturating_add(1);
    }

    /// Number of parsed doc notices.
    #[must_use]
    pub fn doc_notice_count(&self) -> u32 {
        self.doc_notice_count
    }

    /// Records one parsed listing block.
    pub fn note_doc_listing(&mut self) {
        self.doc_listing_count = self.doc_listing_count.saturating_add(1);
    }

    /// Number of parsed listing blocks.
    #[must_use]
    pub fn doc_listing_count(&self) -> u32 {
        self.doc_listing_count
    }

    /// Records one doc character token consumed by the parser.
    pub fn note_doc_char(&mut self) {
        self.doc_char_count = self.doc_char_count.saturating_add(1);
    }

    /// Number of `DOCCHAR` tokens consumed by the parser.
    #[must_use]
    pub fn doc_char_count(&self) -> u32 {
        self.doc_char_count
    }

    /// Records one metadata tag name found in `<metadata>...</metadata>`.
    pub fn note_doc_metadata_tag(&mut self, tag: &str) {
        self.doc_metadata_tags.push(tag.into());
    }

    /// Metadata tag names parsed in documentation sections.
    #[must_use]
    pub fn doc_metadata_tags(&self) -> &[Box<str>] {
        &self.doc_metadata_tags
    }

    /// Equivalent to C++ listing switch update for dependencies.
    pub fn set_lst_dependencies(&mut self, value: bool) {
        self.lst_dependencies = Some(value);
    }

    /// Equivalent to C++ listing switch update for mdoctags.
    pub fn set_lst_mdoctags(&mut self, value: bool) {
        self.lst_mdoctags = Some(value);
    }

    /// Equivalent to C++ listing switch update for distributed.
    pub fn set_lst_distributed(&mut self, value: bool) {
        self.lst_distributed = Some(value);
    }

    /// Last seen dependencies listing switch value.
    #[must_use]
    pub fn lst_dependencies(&self) -> Option<bool> {
        self.lst_dependencies
    }

    /// Last seen mdoctags listing switch value.
    #[must_use]
    pub fn lst_mdoctags(&self) -> Option<bool> {
        self.lst_mdoctags
    }

    /// Last seen distributed listing switch value.
    #[must_use]
    pub fn lst_distributed(&self) -> Option<bool> {
        self.lst_distributed
    }

    /// Equivalent to C++ `setDefProp(sym, file, line)`.
    pub fn set_def_prop(&mut self, sym: TreeId, file: &str, line: u32) {
        let _ = self
            .props
            .set_with_key(sym, self.def_prop_key, SourceLocation::new(file, line));
    }

    /// Equivalent to C++ `setUseProp(sym, file, line)`.
    pub fn set_use_prop(&mut self, sym: TreeId, file: &str, line: u32) {
        let _ = self
            .props
            .set_with_key(sym, self.use_prop_key, SourceLocation::new(file, line));
    }

    /// Convenience hook: set definition property from current parser cursor.
    pub fn set_def_prop_at_cursor(&mut self, sym: TreeId) {
        let loc = self.cursor.clone();
        self.set_def_prop(sym, loc.file(), loc.line());
    }

    /// Convenience hook: set usage property from current parser cursor.
    pub fn set_use_prop_at_cursor(&mut self, sym: TreeId) {
        let loc = self.cursor.clone();
        self.set_use_prop(sym, loc.file(), loc.line());
    }

    /// Equivalent to C++ `hasDefProp(sym)`.
    #[must_use]
    pub fn has_def_prop(&self, sym: TreeId) -> bool {
        self.props.get_with_key(sym, self.def_prop_key).is_some()
    }

    /// Returns definition property when present.
    #[must_use]
    pub fn def_prop(&self, sym: TreeId) -> Option<&SourceLocation> {
        self.props.get_with_key(sym, self.def_prop_key)
    }

    /// Returns usage property when present.
    #[must_use]
    pub fn use_prop(&self, sym: TreeId) -> Option<&SourceLocation> {
        self.props.get_with_key(sym, self.use_prop_key)
    }

    /// Equivalent to C++ `getDefFileProp(sym)`.
    #[must_use]
    pub fn def_file_prop(&self, sym: TreeId) -> Option<&str> {
        self.def_prop(sym).map(SourceLocation::file)
    }

    /// Equivalent to C++ `getDefLineProp(sym)`.
    #[must_use]
    pub fn def_line_prop(&self, sym: TreeId) -> Option<u32> {
        self.def_prop(sym).map(SourceLocation::line)
    }

    /// Equivalent to C++ `getUseFileProp(sym)`.
    #[must_use]
    pub fn use_file_prop(&self, sym: TreeId) -> Option<&str> {
        self.use_prop(sym).map(SourceLocation::file)
    }

    /// Equivalent to C++ `getUseLineProp(sym)`.
    #[must_use]
    pub fn use_line_prop(&self, sym: TreeId) -> Option<u32> {
        self.use_prop(sym).map(SourceLocation::line)
    }

    /// Records a parser error at current cursor location.
    pub fn error(&mut self, message: &str) {
        self.parse_error_count = self.parse_error_count.saturating_add(1);
        self.push_diagnostic(
            DiagnosticSeverity::Error,
            message,
            Some(self.cursor.clone()),
        );
    }

    /// Records a parser warning at current cursor location.
    pub fn warning(&mut self, message: &str) {
        self.push_diagnostic(
            DiagnosticSeverity::Warning,
            message,
            Some(self.cursor.clone()),
        );
    }

    /// Records a parser remark at current cursor location.
    pub fn remark(&mut self, message: &str) {
        self.push_diagnostic(
            DiagnosticSeverity::Remark,
            message,
            Some(self.cursor.clone()),
        );
    }

    /// Records one parser recovery event (e.g. `error ENDDEF` path).
    pub fn note_recovery(&mut self) {
        self.recovery_count = self.recovery_count.saturating_add(1);
    }

    /// Number of parser errors recorded in this context.
    #[must_use]
    pub fn parse_error_count(&self) -> u32 {
        self.parse_error_count
    }

    /// Number of parser recovery events.
    #[must_use]
    pub fn recovery_count(&self) -> u32 {
        self.recovery_count
    }

    /// All recorded diagnostics.
    #[must_use]
    pub fn diagnostics(&self) -> &[ParserDiagnostic] {
        &self.diagnostics
    }

    /// Returns `true` when no diagnostics are currently recorded.
    #[must_use]
    pub fn diagnostics_is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    fn push_diagnostic(
        &mut self,
        severity: DiagnosticSeverity,
        message: &str,
        location: Option<SourceLocation>,
    ) {
        self.diagnostics.push(ParserDiagnostic {
            severity,
            message: message.into(),
            location,
        });
    }
}
