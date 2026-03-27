//! Generic JSON description builder from FIR metadata/UI instructions.
//!
//! # Source provenance (C++)
//! - `architecture/faust/gui/JSONUI.h`
//! - `compiler/generator/json_instructions.hh`
//! - `compiler/generator/code_container.hh`
//!
//! # Role
//! - Reconstruct backend-agnostic Faust JSON payloads from canonical FIR
//!   `metadata` and `buildUserInterface` bodies.
//! - Keep the JSON description separate from any single backend so it can be
//!   reused by a future global `-json` CLI path and by backends such as WASM.

use std::fmt::Write as _;

use fir::{FirId, FirMatch, FirStore, match_fir};

/// Backend-agnostic Faust JSON description reconstructed from FIR.
///
/// This mirrors the logical payload produced by the C++ JSON pipeline
/// (`JSONUI`, `json_instructions.hh`) after the UI and metadata visitor passes
/// have run. It is intentionally detached from any single backend so the same
/// structure can back:
///
/// - strict `-json` output, where widget [`JsonWidget::index`] is absent,
/// - companion backend JSON such as WASM, where runtime-specific fields like
///   [`JsonDescription::size`], [`JsonDescription::sr_index`], and widget
///   indexes are supplied by the caller.
///
/// The rendering order in [`JsonDescription::render`] is stable on purpose so
/// differential tests can compare Rust output against C++ snapshots.
#[derive(Clone, Debug, PartialEq)]
pub struct JsonDescription {
    /// Root DSP name. FIR/UI metadata may override the initially requested
    /// module name through `declare name`.
    pub name: String,
    /// Optional source filename emitted by the CLI/compiler facade.
    pub filename: Option<String>,
    /// Optional compiler version string.
    pub version: Option<String>,
    /// Backend-aware compile flags string exposed for runtime consumers.
    pub compile_options: Option<String>,
    /// Imported logical library names seen during compilation.
    pub library_list: Vec<String>,
    /// Include roots preserved for parity with the C++ JSON schema.
    pub include_pathnames: Vec<String>,
    /// Runtime prefix size when a backend needs one, notably WASM companion
    /// JSON. Strict `-json` leaves this unset.
    pub size: Option<u32>,
    /// DSP input arity.
    pub inputs: usize,
    /// DSP output arity.
    pub outputs: usize,
    /// Optional sample-rate slot offset for WASM-style runtime ABIs.
    pub sr_index: Option<u32>,
    /// Root metadata declarations after top-level/compiler metadata and FIR
    /// metadata have been merged.
    pub meta: Vec<JsonMetaEntry>,
    /// Hierarchical UI description reconstructed from `buildUserInterface`.
    pub ui: Vec<JsonUiItem>,
}

impl JsonDescription {
    /// Render the description as a compact JSON string.
    ///
    /// The serializer is intentionally local and deterministic rather than
    /// delegating to `serde_json`, because parity work here cares about field
    /// presence, ordering, and omission rules that mirror the existing Faust
    /// JSON payloads.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push('{');
        push_json_field_string(&mut out, "name", &self.name);
        if let Some(filename) = &self.filename {
            out.push(',');
            push_json_field_string(&mut out, "filename", filename);
        }
        if let Some(version) = &self.version {
            out.push(',');
            push_json_field_string(&mut out, "version", version);
        }
        if let Some(compile_options) = &self.compile_options {
            out.push(',');
            push_json_field_string(&mut out, "compile_options", compile_options);
        }
        if !self.library_list.is_empty() {
            out.push(',');
            push_json_field_string_array(&mut out, "library_list", &self.library_list);
        }
        if !self.include_pathnames.is_empty() {
            out.push(',');
            push_json_field_string_array(&mut out, "include_pathnames", &self.include_pathnames);
        }
        if let Some(size) = self.size {
            out.push(',');
            push_json_field_u32(&mut out, "size", size);
        }
        out.push(',');
        push_json_field_usize(&mut out, "inputs", self.inputs);
        out.push(',');
        push_json_field_usize(&mut out, "outputs", self.outputs);
        if let Some(sr_index) = self.sr_index {
            out.push(',');
            push_json_field_u32(&mut out, "sr_index", sr_index);
        }
        if !self.meta.is_empty() {
            out.push(',');
            push_json_field_meta_array(&mut out, "meta", &self.meta);
        }
        out.push(',');
        push_json_field_ui_array(&mut out, "ui", &self.ui);
        out.push('}');
        out
    }
}

/// One Faust metadata declaration (`declare key "value"`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonMetaEntry {
    pub key: String,
    pub value: String,
}

/// One item in the JSON `ui` tree.
///
/// Groups preserve the nested `open*box`/`closeBox` structure found in
/// `buildUserInterface`, while widgets carry the leaf control/bargraph payload.
#[derive(Clone, Debug, PartialEq)]
pub enum JsonUiItem {
    Group {
        typ: &'static str,
        label: String,
        meta: Vec<JsonMetaEntry>,
        items: Vec<JsonUiItem>,
    },
    Widget(JsonWidget),
}

/// Leaf widget payload in the Faust JSON schema.
///
/// `index` is optional because only some backends expose a runtime memory ABI
/// through the JSON. Strict `-json` keeps it unset, while the WASM companion
/// JSON uses it as the public control address consumed by
/// `getParamValue`/`setParamValue`.
#[derive(Clone, Debug, PartialEq)]
pub struct JsonWidget {
    pub typ: &'static str,
    pub label: String,
    pub varname: String,
    pub shortname: String,
    pub address: String,
    pub index: Option<u32>,
    pub meta: Vec<JsonMetaEntry>,
    pub range: Option<JsonRange>,
    pub soundfile_url: Option<String>,
}

/// Numeric range metadata for sliders, numeric entries, and bargraphs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JsonRange {
    pub init: Option<f64>,
    pub min: f64,
    pub max: f64,
    pub step: Option<f64>,
}

/// Extra context required to turn FIR `metadata` and `buildUserInterface`
/// bodies into a complete JSON description.
///
/// This structure carries the fields that do not live in FIR itself, such as
/// CLI/compiler provenance and backend ABI data. The caller chooses whether to
/// populate backend-specific fields like [`JsonBuildOptions::size`] and
/// [`JsonBuildOptions::sr_index`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonBuildOptions {
    /// Requested module name before any `declare name` override.
    pub name: String,
    /// Optional source filename attached by the compiler facade.
    pub filename: Option<String>,
    /// Optional compiler version string.
    pub version: Option<String>,
    /// Backend-aware compile flags string.
    pub compile_options: Option<String>,
    /// Imported logical library names.
    pub library_list: Vec<String>,
    /// Include roots retained in the final JSON.
    pub include_pathnames: Vec<String>,
    /// Compiler-provided metadata that sits alongside FIR metadata.
    pub top_level_meta: Vec<JsonMetaEntry>,
    /// Backend-specific runtime prefix size.
    pub size: Option<u32>,
    /// DSP input arity.
    pub inputs: usize,
    /// DSP output arity.
    pub outputs: usize,
    /// Backend-specific sample-rate slot offset.
    pub sr_index: Option<u32>,
}

/// FIR-to-JSON reconstruction error.
///
/// This stays intentionally narrow: unsupported node shapes are surfaced with
/// the offending FIR context so parity gaps remain visible instead of being
/// silently dropped from the emitted JSON.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonBuildError {
    UnsupportedFirNode(String),
}

impl std::fmt::Display for JsonBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedFirNode(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for JsonBuildError {}

/// Rebuild a Faust JSON description from FIR function bodies.
///
/// The expected inputs are the top-level FIR items for one lowered module.
/// This function looks for the canonical `metadata` and `buildUserInterface`
/// functions, decodes their instruction bodies, merges compiler-supplied
/// metadata, and asks `resolve_index` for any backend-specific widget index.
///
/// The callback receives each widget `var` name and may return a runtime index.
/// Callers that want strict backend-agnostic JSON can simply return `None`.
pub fn build_json_description_from_fir<F>(
    store: &FirStore,
    function_items: &[FirId],
    options: JsonBuildOptions,
    mut resolve_index: F,
) -> Result<JsonDescription, JsonBuildError>
where
    F: FnMut(&str) -> Option<u32>,
{
    let metadata = parse_metadata(store, find_function_body(store, function_items, "metadata"))?;
    let merged_meta = merge_top_level_and_fir_meta(options.top_level_meta, metadata.entries);
    let declared_name = merged_meta
        .iter()
        .find(|entry| entry.key == "name")
        .map(|entry| entry.value.clone())
        .or(metadata.declared_name);
    let declared_filename = merged_meta
        .iter()
        .find(|entry| entry.key == "filename")
        .map(|entry| entry.value.clone())
        .or(metadata.declared_filename);
    Ok(JsonDescription {
        name: declared_name.unwrap_or(options.name),
        filename: declared_filename.or(options.filename),
        version: options.version,
        compile_options: options.compile_options,
        library_list: options.library_list,
        include_pathnames: options.include_pathnames,
        size: options.size,
        inputs: options.inputs,
        outputs: options.outputs,
        sr_index: options.sr_index,
        meta: merged_meta,
        ui: parse_ui(
            store,
            find_function_body(store, function_items, "buildUserInterface"),
            &mut resolve_index,
        )?,
    })
}

/// Escape one string for inclusion in the hand-written JSON renderer.
///
/// This intentionally only covers the characters needed by the Faust JSON
/// payloads generated here; it is not exposed as a general-purpose serializer.
pub fn escape_json_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// Merge compiler-supplied top-level metadata with metadata reconstructed from
/// the FIR `metadata` body.
///
/// Ordering is preserved so the caller can inject provenance entries first,
/// while later FIR entries still remain visible in the final `meta` array.
fn merge_top_level_and_fir_meta(
    top_level_meta: Vec<JsonMetaEntry>,
    fir_meta: Vec<JsonMetaEntry>,
) -> Vec<JsonMetaEntry> {
    let mut merged = top_level_meta;
    merged.extend(fir_meta);
    merged
}

fn push_json_field_string(out: &mut String, key: &str, value: &str) {
    let _ = write!(
        out,
        "\"{}\":\"{}\"",
        escape_json_string(key),
        escape_json_string(value)
    );
}

fn push_json_field_usize(out: &mut String, key: &str, value: usize) {
    let _ = write!(out, "\"{}\":{}", escape_json_string(key), value);
}

fn push_json_field_u32(out: &mut String, key: &str, value: u32) {
    let _ = write!(out, "\"{}\":{}", escape_json_string(key), value);
}

fn push_json_field_f64(out: &mut String, key: &str, value: f64) {
    let _ = write!(out, "\"{}\":{}", escape_json_string(key), value);
}

fn push_json_field_string_array(out: &mut String, key: &str, values: &[String]) {
    let _ = write!(out, "\"{}\":[", escape_json_string(key));
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let _ = write!(out, "\"{}\"", escape_json_string(value));
    }
    out.push(']');
}

fn push_json_field_meta_array(out: &mut String, key: &str, values: &[JsonMetaEntry]) {
    let _ = write!(out, "\"{}\":[", escape_json_string(key));
    for (index, entry) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('{');
        push_json_field_string(out, &entry.key, &entry.value);
        out.push('}');
    }
    out.push(']');
}

/// Serialize the JSON `ui` array.
fn push_json_field_ui_array(out: &mut String, key: &str, values: &[JsonUiItem]) {
    let _ = write!(out, "\"{}\":[", escape_json_string(key));
    for (index, item) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_json_ui_item(out, item);
    }
    out.push(']');
}

/// Serialize one UI item, omitting backend-specific optional fields when they
/// are absent.
///
/// Notably, widget `index` is only emitted when the caller provided one through
/// [`build_json_description_from_fir`]. This keeps strict `-json` output free
/// from WASM-only ABI fields while still reusing the same builder.
fn push_json_ui_item(out: &mut String, item: &JsonUiItem) {
    match item {
        JsonUiItem::Group {
            typ,
            label,
            meta,
            items,
        } => {
            out.push('{');
            push_json_field_string(out, "type", typ);
            out.push(',');
            push_json_field_string(out, "label", label);
            if !meta.is_empty() {
                out.push(',');
                push_json_field_meta_array(out, "meta", meta);
            }
            out.push(',');
            push_json_field_ui_array(out, "items", items);
            out.push('}');
        }
        JsonUiItem::Widget(widget) => {
            out.push('{');
            push_json_field_string(out, "type", widget.typ);
            out.push(',');
            push_json_field_string(out, "label", &widget.label);
            out.push(',');
            push_json_field_string(out, "varname", &widget.varname);
            out.push(',');
            push_json_field_string(out, "shortname", &widget.shortname);
            out.push(',');
            push_json_field_string(out, "address", &widget.address);
            if let Some(index) = widget.index {
                out.push(',');
                push_json_field_u32(out, "index", index);
            }
            if !widget.meta.is_empty() {
                out.push(',');
                push_json_field_meta_array(out, "meta", &widget.meta);
            }
            if let Some(url) = &widget.soundfile_url {
                out.push(',');
                push_json_field_string(out, "url", url);
            }
            if let Some(range) = widget.range {
                if let Some(init) = range.init {
                    out.push(',');
                    push_json_field_f64(out, "init", init);
                }
                out.push(',');
                push_json_field_f64(out, "min", range.min);
                out.push(',');
                push_json_field_f64(out, "max", range.max);
                if let Some(step) = range.step {
                    out.push(',');
                    push_json_field_f64(out, "step", step);
                }
            }
            out.push('}');
        }
    }
}

struct ParsedMetadata {
    entries: Vec<JsonMetaEntry>,
    declared_name: Option<String>,
    declared_filename: Option<String>,
}

/// Parse the canonical FIR `metadata` function body.
///
/// The body is expected to be a `Block` containing `AddMetaDeclare` nodes and
/// optional labels. `declare name` and `declare filename` are additionally
/// lifted into dedicated return fields because they may override root JSON
/// fields in the same way as the C++ JSON pipeline.
fn parse_metadata(store: &FirStore, body: Option<FirId>) -> Result<ParsedMetadata, JsonBuildError> {
    let Some(body) = body else {
        return Ok(ParsedMetadata {
            entries: Vec::new(),
            declared_name: None,
            declared_filename: None,
        });
    };
    let FirMatch::Block(items) = match_fir(store, body) else {
        return Err(JsonBuildError::UnsupportedFirNode(
            "JSON metadata function body must be a FIR Block".to_owned(),
        ));
    };
    let mut meta = Vec::with_capacity(items.len());
    let mut declared_name = None;
    let mut declared_filename = None;
    for item in items {
        match match_fir(store, item) {
            FirMatch::AddMetaDeclare { key, value, .. } => {
                if key == "name" && declared_name.is_none() {
                    declared_name = Some(value.clone());
                }
                if key == "filename" && declared_filename.is_none() {
                    declared_filename = Some(value.clone());
                }
                meta.push(JsonMetaEntry { key, value });
            }
            FirMatch::Label(_) => {}
            other => {
                return Err(JsonBuildError::UnsupportedFirNode(format!(
                    "unsupported FIR node in JSON metadata body: {other:?}"
                )));
            }
        }
    }
    Ok(ParsedMetadata {
        entries: meta,
        declared_name,
        declared_filename,
    })
}

/// Parse the canonical FIR `buildUserInterface` function body into a JSON UI
/// tree.
///
/// `resolve_index` is threaded through to leaf widget construction so callers
/// can inject backend runtime offsets without baking ABI policy into this
/// generic parser.
fn parse_ui<F>(
    store: &FirStore,
    body: Option<FirId>,
    resolve_index: &mut F,
) -> Result<Vec<JsonUiItem>, JsonBuildError>
where
    F: FnMut(&str) -> Option<u32>,
{
    let Some(body) = body else {
        return Ok(Vec::new());
    };
    let FirMatch::Block(items) = match_fir(store, body) else {
        return Err(JsonBuildError::UnsupportedFirNode(
            "JSON buildUserInterface body must be a FIR Block".to_owned(),
        ));
    };
    let mut cursor = 0;
    let mut pending_meta = Vec::new();
    parse_ui_items(
        store,
        &items,
        &mut cursor,
        &mut pending_meta,
        false,
        Vec::new(),
        resolve_index,
    )
}

/// Recursive descent parser for the flattened FIR UI instruction stream.
///
/// FIR stores UI instructions in one linear `Block`; groups are delimited by
/// `OpenBox` / `CloseBox`. `pending_meta` accumulates `AddMetaDeclare`
/// instructions until they are attached to the next group or widget, matching
/// the Faust UI builder convention.
#[allow(clippy::too_many_arguments)]
fn parse_ui_items<F>(
    store: &FirStore,
    items: &[FirId],
    cursor: &mut usize,
    pending_meta: &mut Vec<JsonMetaEntry>,
    stop_on_close: bool,
    path_stack: Vec<String>,
    resolve_index: &mut F,
) -> Result<Vec<JsonUiItem>, JsonBuildError>
where
    F: FnMut(&str) -> Option<u32>,
{
    let mut out = Vec::new();
    while *cursor < items.len() {
        match match_fir(store, items[*cursor]) {
            FirMatch::AddMetaDeclare { key, value, .. } => {
                pending_meta.push(JsonMetaEntry { key, value });
                *cursor += 1;
            }
            FirMatch::OpenBox { typ, label } => {
                let group_meta = std::mem::take(pending_meta);
                let mut child_path = path_stack.clone();
                child_path.push(label.clone());
                *cursor += 1;
                let children = parse_ui_items(
                    store,
                    items,
                    cursor,
                    pending_meta,
                    true,
                    child_path,
                    resolve_index,
                )?;
                out.push(JsonUiItem::Group {
                    typ: ui_box_type_name(typ),
                    label,
                    meta: group_meta,
                    items: children,
                });
            }
            FirMatch::CloseBox => {
                *cursor += 1;
                if stop_on_close {
                    return Ok(out);
                }
                return Err(JsonBuildError::UnsupportedFirNode(
                    "unexpected CloseBox at top-level JSON UI body".to_owned(),
                ));
            }
            FirMatch::AddButton { typ, label, var } => {
                let widget_meta = std::mem::take(pending_meta);
                out.push(JsonUiItem::Widget(build_widget(
                    button_type_name(typ),
                    label,
                    var,
                    widget_meta,
                    None,
                    None,
                    &path_stack,
                    resolve_index,
                )?));
                *cursor += 1;
            }
            FirMatch::AddSlider {
                typ,
                label,
                var,
                init,
                lo,
                hi,
                step,
            } => {
                let widget_meta = std::mem::take(pending_meta);
                out.push(JsonUiItem::Widget(build_widget(
                    slider_type_name(typ),
                    label,
                    var,
                    widget_meta,
                    Some(JsonRange {
                        init: Some(init),
                        min: lo,
                        max: hi,
                        step: Some(step),
                    }),
                    None,
                    &path_stack,
                    resolve_index,
                )?));
                *cursor += 1;
            }
            FirMatch::AddBargraph {
                typ,
                label,
                var,
                lo,
                hi,
            } => {
                let widget_meta = std::mem::take(pending_meta);
                out.push(JsonUiItem::Widget(build_widget(
                    bargraph_type_name(typ),
                    label,
                    var,
                    widget_meta,
                    Some(JsonRange {
                        init: None,
                        min: lo,
                        max: hi,
                        step: None,
                    }),
                    None,
                    &path_stack,
                    resolve_index,
                )?));
                *cursor += 1;
            }
            FirMatch::AddSoundfile { label, url, var } => {
                let widget_meta = std::mem::take(pending_meta);
                out.push(JsonUiItem::Widget(build_widget(
                    "soundfile",
                    label,
                    var,
                    widget_meta,
                    None,
                    Some(url),
                    &path_stack,
                    resolve_index,
                )?));
                *cursor += 1;
            }
            FirMatch::Label(_) => {
                *cursor += 1;
            }
            other => {
                return Err(JsonBuildError::UnsupportedFirNode(format!(
                    "unsupported FIR node in JSON UI body: {other:?}"
                )));
            }
        }
    }
    if stop_on_close {
        return Err(JsonBuildError::UnsupportedFirNode(
            "missing CloseBox in JSON UI body".to_owned(),
        ));
    }
    Ok(out)
}

/// Build one JSON widget payload from a FIR UI leaf.
///
/// The runtime `address` is derived from the current group stack plus the
/// widget label, matching the public Faust JSON UI convention. `varname` keeps
/// the lowered FIR symbol used for backend-specific index resolution.
#[allow(clippy::too_many_arguments)]
fn build_widget<F>(
    typ: &'static str,
    label: String,
    var: String,
    meta: Vec<JsonMetaEntry>,
    range: Option<JsonRange>,
    soundfile_url: Option<String>,
    path_stack: &[String],
    resolve_index: &mut F,
) -> Result<JsonWidget, JsonBuildError>
where
    F: FnMut(&str) -> Option<u32>,
{
    let index = resolve_index(&var);
    let mut address = String::new();
    for segment in path_stack {
        address.push('/');
        address.push_str(segment);
    }
    address.push('/');
    address.push_str(&label);
    Ok(JsonWidget {
        typ,
        shortname: label.clone(),
        label,
        varname: var,
        address,
        index,
        meta,
        range,
        soundfile_url,
    })
}

/// Find the body of one named helper function in the lowered FIR item list.
///
/// The JSON builder only depends on the canonical `metadata` and
/// `buildUserInterface` functions; missing helpers are treated as “no metadata”
/// or “no UI” rather than as hard errors.
fn find_function_body(store: &FirStore, function_items: &[FirId], name: &str) -> Option<FirId> {
    function_items
        .iter()
        .copied()
        .find_map(|id| match match_fir(store, id) {
            FirMatch::DeclareFun {
                name: ref fun_name,
                body: Some(body),
                ..
            } if fun_name == name => Some(body),
            _ => None,
        })
}

/// Map FIR UI group kinds to the JSON schema names used by Faust runtimes.
fn ui_box_type_name(typ: fir::UiBoxType) -> &'static str {
    match typ {
        fir::UiBoxType::Vertical => "vgroup",
        fir::UiBoxType::Horizontal => "hgroup",
        fir::UiBoxType::Tab => "tgroup",
    }
}

/// Map FIR button kinds to Faust JSON widget type strings.
fn button_type_name(typ: fir::ButtonType) -> &'static str {
    match typ {
        fir::ButtonType::Button => "button",
        fir::ButtonType::Checkbox => "checkbox",
    }
}

/// Map FIR slider kinds to Faust JSON widget type strings.
fn slider_type_name(typ: fir::SliderType) -> &'static str {
    match typ {
        fir::SliderType::Horizontal => "hslider",
        fir::SliderType::Vertical => "vslider",
        fir::SliderType::NumEntry => "nentry",
    }
}

/// Map FIR bargraph kinds to Faust JSON widget type strings.
fn bargraph_type_name(typ: fir::BargraphType) -> &'static str {
    match typ {
        fir::BargraphType::Horizontal => "hbargraph",
        fir::BargraphType::Vertical => "vbargraph",
    }
}

#[cfg(test)]
mod tests;
