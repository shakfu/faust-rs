//! `.clif` container serialization helpers for `cranelift-ffi`.
//!
//! This module defines the V1 textual container used by Cranelift factory
//! `*Bitcode*` APIs. The container is intentionally deterministic and
//! self-describing so it can be validated before deserialization.
//!
use crate::types::CraneliftDspFactory;

/// Magic header used to identify Cranelift `.clif` container payloads.
pub(crate) const CLIF_MAGIC: &str = "FAUST_CLIF_V1";

/// Escapes one textual field for key/value line serialization.
///
/// The format is intentionally tiny and line-oriented, so only backslashes and
/// newlines are escaped.
fn esc_field(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
}

/// Unescapes one textual field from key/value line serialization.
///
/// Unknown escape sequences are preserved literally so the parser remains
/// forward-compatible with future extensions.
fn unesc_field(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(ch) = it.next() {
        if ch == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Parsed `.clif` payload fields needed to rebuild a runnable factory.
///
/// This is the decoded intermediate form used by the FFI bitcode read path
/// before recompiling the embedded Faust source into a fresh Cranelift module.
pub(crate) struct DecodedClifPayload {
    pub(crate) name: String,
    pub(crate) expected_sha: String,
    pub(crate) expected_compile_options: String,
    pub(crate) opt_level: i32,
    pub(crate) num_inputs: usize,
    pub(crate) num_outputs: usize,
    pub(crate) argv: Vec<String>,
    pub(crate) source_fallback: String,
    pub(crate) clif_functions: Vec<(String, String)>,
}

/// Encodes one factory into a textual `.clif` container payload.
///
/// The current container remains source-fallback-backed for rebuilding while
/// carrying Cranelift-oriented metadata and a stable magic header.
pub(crate) fn encode_factory_clif(factory: &CraneliftDspFactory) -> Result<String, String> {
    if !factory.source_is_faust {
        return Err(
            "bitcode write is currently supported only for source-backed factories".to_owned(),
        );
    }

    let mut out = String::new();
    out.push_str(CLIF_MAGIC);
    out.push('\n');
    out.push_str("format_version=1\n");
    out.push_str(&format!(
        "faust_rs_version={}\n",
        esc_field(env!("CARGO_PKG_VERSION"))
    ));
    out.push_str("cranelift_codegen_version=unknown\n");
    out.push_str(&format!(
        "target_triple={}-{}\n",
        esc_field(std::env::consts::ARCH),
        esc_field(std::env::consts::OS)
    ));
    out.push_str(&format!(
        "pointer_width={}\n",
        std::mem::size_of::<usize>() * 8
    ));
    out.push_str(&format!(
        "endianness={}\n",
        if cfg!(target_endian = "little") {
            "little"
        } else {
            "big"
        }
    ));
    out.push_str(&format!("name={}\n", esc_field(&factory.source_name)));
    out.push_str(&format!("sha={}\n", esc_field(&factory.sha_key)));
    out.push_str(&format!(
        "compile_options={}\n",
        esc_field(&factory.compile_options)
    ));
    out.push_str(&format!("opt_level={}\n", factory.opt_level));
    out.push_str(&format!("argc={}\n", factory.compile_argv.len()));
    for (idx, arg) in factory.compile_argv.iter().enumerate() {
        out.push_str(&format!("arg{idx}={}\n", esc_field(arg)));
    }
    out.push_str(&format!("num_inputs={}\n", factory.num_inputs));
    out.push_str(&format!("num_outputs={}\n", factory.num_outputs));
    out.push_str(&format!(
        "compute_body_lowered={}\n",
        if factory.compute_body_lowered { 1 } else { 0 }
    ));
    out.push_str(&format!(
        "source_fallback={}\n",
        esc_field(&factory.dsp_code)
    ));
    let clif_functions = factory
        .compiled_jit
        .as_ref()
        .map(|jit| jit.generated_functions_clif())
        .ok_or_else(|| "bitcode write requires compiled Cranelift JIT module".to_owned())?;
    out.push_str(&format!("clif_func_count={}\n", clif_functions.len()));
    for (idx, (name, clif_text)) in clif_functions.iter().enumerate() {
        out.push_str(&format!("clif_func_name_{idx}={}\n", esc_field(name)));
        out.push_str(&format!("clif_func_body_{idx}={}\n", esc_field(clif_text)));
    }
    Ok(out)
}

/// Decodes one textual `.clif` container payload.
///
/// Validation is intentionally strict for host-dependent fields such as
/// pointer width and endianness so incompatible payloads fail early with a
/// plain string diagnostic rather than later during JIT/runtime use.
pub(crate) fn decode_factory_clif(text: &str) -> Result<DecodedClifPayload, String> {
    let mut lines = text.lines();
    match lines.next() {
        Some(CLIF_MAGIC) => {}
        Some(_) => return Err("unsupported cranelift bitcode format".to_owned()),
        None => return Err("empty bitcode payload".to_owned()),
    }

    let mut fields = std::collections::HashMap::<String, String>::new();
    for line in lines {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        fields.insert(k.to_owned(), unesc_field(v));
    }

    let version = fields
        .remove("format_version")
        .ok_or_else(|| "missing 'format_version' field".to_owned())?
        .parse::<u32>()
        .map_err(|e| format!("invalid 'format_version' field: {e}"))?;
    if version != 1 {
        return Err(format!(
            "unsupported FAUST_CLIF_V1 format_version '{version}'"
        ));
    }

    let payload_pointer_width = fields
        .remove("pointer_width")
        .ok_or_else(|| "missing 'pointer_width' field".to_owned())?
        .parse::<usize>()
        .map_err(|e| format!("invalid 'pointer_width' field: {e}"))?;
    let host_pointer_width = std::mem::size_of::<usize>() * 8;
    if payload_pointer_width != host_pointer_width {
        return Err(format!(
            "incompatible pointer width: payload {payload_pointer_width}, host {host_pointer_width}"
        ));
    }

    let payload_endianness = fields
        .remove("endianness")
        .ok_or_else(|| "missing 'endianness' field".to_owned())?;
    let host_endianness = if cfg!(target_endian = "little") {
        "little"
    } else {
        "big"
    };
    if payload_endianness != host_endianness {
        return Err(format!(
            "incompatible endianness: payload {payload_endianness}, host {host_endianness}"
        ));
    }

    let name = fields
        .remove("name")
        .ok_or_else(|| "missing 'name' field".to_owned())?;
    let expected_sha = fields
        .remove("sha")
        .ok_or_else(|| "missing 'sha' field".to_owned())?;
    let expected_compile_options = fields
        .remove("compile_options")
        .ok_or_else(|| "missing 'compile_options' field".to_owned())?;
    let source_fallback = fields
        .remove("source_fallback")
        .ok_or_else(|| "missing 'source_fallback' field".to_owned())?;
    let clif_count = fields
        .remove("clif_func_count")
        .ok_or_else(|| "missing 'clif_func_count' field".to_owned())?
        .parse::<usize>()
        .map_err(|e| format!("invalid 'clif_func_count' field: {e}"))?;
    let mut clif_functions = Vec::with_capacity(clif_count);
    for idx in 0..clif_count {
        let name_key = format!("clif_func_name_{idx}");
        let body_key = format!("clif_func_body_{idx}");
        let func_name = fields
            .remove(&name_key)
            .ok_or_else(|| format!("missing '{name_key}' field"))?;
        let func_body = fields
            .remove(&body_key)
            .ok_or_else(|| format!("missing '{body_key}' field"))?;
        clif_functions.push((func_name, func_body));
    }
    let opt_level = fields
        .remove("opt_level")
        .ok_or_else(|| "missing 'opt_level' field".to_owned())?
        .parse::<i32>()
        .map_err(|e| format!("invalid 'opt_level' field: {e}"))?;
    let argc = fields
        .remove("argc")
        .ok_or_else(|| "missing 'argc' field".to_owned())?
        .parse::<usize>()
        .map_err(|e| format!("invalid 'argc' field: {e}"))?;
    let num_inputs = fields
        .remove("num_inputs")
        .ok_or_else(|| "missing 'num_inputs' field".to_owned())?
        .parse::<usize>()
        .map_err(|e| format!("invalid 'num_inputs' field: {e}"))?;
    let num_outputs = fields
        .remove("num_outputs")
        .ok_or_else(|| "missing 'num_outputs' field".to_owned())?
        .parse::<usize>()
        .map_err(|e| format!("invalid 'num_outputs' field: {e}"))?;
    let mut argv = Vec::with_capacity(argc);
    for idx in 0..argc {
        let key = format!("arg{idx}");
        let arg = fields
            .remove(&key)
            .ok_or_else(|| format!("missing '{key}' field"))?;
        argv.push(arg);
    }

    Ok(DecodedClifPayload {
        name,
        expected_sha,
        expected_compile_options,
        opt_level,
        num_inputs,
        num_outputs,
        argv,
        source_fallback,
        clif_functions,
    })
}
