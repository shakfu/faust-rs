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

#[derive(Clone, Debug, PartialEq)]
pub struct JsonDescription {
    pub name: String,
    pub filename: Option<String>,
    pub version: Option<String>,
    pub compile_options: Option<String>,
    pub library_list: Vec<String>,
    pub include_pathnames: Vec<String>,
    pub size: Option<u32>,
    pub inputs: usize,
    pub outputs: usize,
    pub sr_index: Option<u32>,
    pub meta: Vec<JsonMetaEntry>,
    pub ui: Vec<JsonUiItem>,
}

impl JsonDescription {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonMetaEntry {
    pub key: String,
    pub value: String,
}

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

#[derive(Clone, Debug, PartialEq)]
pub struct JsonWidget {
    pub typ: &'static str,
    pub label: String,
    pub varname: String,
    pub shortname: String,
    pub address: String,
    pub index: u32,
    pub meta: Vec<JsonMetaEntry>,
    pub range: Option<JsonRange>,
    pub soundfile_url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JsonRange {
    pub init: Option<f64>,
    pub min: f64,
    pub max: f64,
    pub step: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonBuildOptions {
    pub name: String,
    pub filename: Option<String>,
    pub version: Option<String>,
    pub compile_options: Option<String>,
    pub library_list: Vec<String>,
    pub include_pathnames: Vec<String>,
    pub size: Option<u32>,
    pub inputs: usize,
    pub outputs: usize,
    pub sr_index: Option<u32>,
}

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

pub fn build_json_description_from_fir<F>(
    store: &FirStore,
    function_items: &[FirId],
    options: JsonBuildOptions,
    mut resolve_index: F,
) -> Result<JsonDescription, JsonBuildError>
where
    F: FnMut(&str) -> Option<u32>,
{
    Ok(JsonDescription {
        name: options.name,
        filename: options.filename,
        version: options.version,
        compile_options: options.compile_options,
        library_list: options.library_list,
        include_pathnames: options.include_pathnames,
        size: options.size,
        inputs: options.inputs,
        outputs: options.outputs,
        sr_index: options.sr_index,
        meta: parse_metadata(store, find_function_body(store, function_items, "metadata"))?,
        ui: parse_ui(
            store,
            find_function_body(store, function_items, "buildUserInterface"),
            &mut resolve_index,
        )?,
    })
}

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
            out.push(',');
            push_json_field_u32(out, "index", widget.index);
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

fn parse_metadata(
    store: &FirStore,
    body: Option<FirId>,
) -> Result<Vec<JsonMetaEntry>, JsonBuildError> {
    let Some(body) = body else {
        return Ok(Vec::new());
    };
    let FirMatch::Block(items) = match_fir(store, body) else {
        return Err(JsonBuildError::UnsupportedFirNode(
            "JSON metadata function body must be a FIR Block".to_owned(),
        ));
    };
    let mut meta = Vec::with_capacity(items.len());
    for item in items {
        match match_fir(store, item) {
            FirMatch::AddMetaDeclare { key, value, .. } => {
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
    Ok(meta)
}

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
    let index = resolve_index(&var).ok_or_else(|| {
        JsonBuildError::UnsupportedFirNode(format!("missing JSON field offset for `{var}`"))
    })?;
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

fn ui_box_type_name(typ: fir::UiBoxType) -> &'static str {
    match typ {
        fir::UiBoxType::Vertical => "vgroup",
        fir::UiBoxType::Horizontal => "hgroup",
        fir::UiBoxType::Tab => "tgroup",
    }
}

fn button_type_name(typ: fir::ButtonType) -> &'static str {
    match typ {
        fir::ButtonType::Button => "button",
        fir::ButtonType::Checkbox => "checkbox",
    }
}

fn slider_type_name(typ: fir::SliderType) -> &'static str {
    match typ {
        fir::SliderType::Horizontal => "hslider",
        fir::SliderType::Vertical => "vslider",
        fir::SliderType::NumEntry => "nentry",
    }
}

fn bargraph_type_name(typ: fir::BargraphType) -> &'static str {
    match typ {
        fir::BargraphType::Horizontal => "hbargraph",
        fir::BargraphType::Vertical => "vbargraph",
    }
}

#[cfg(test)]
mod tests;
