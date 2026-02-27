//! `.clif` container serialization helpers for `cranelift-ffi`.
//!
//! This module defines the V1 textual container used by Cranelift factory
//! `*Bitcode*` APIs. The container is intentionally deterministic and
//! self-describing so it can be validated before deserialization.
//!
//! Note: in this step, only write-side serialization is wired. Read-side
//! parsing is completed in a follow-up step.

use crate::types::CraneliftDspFactory;

/// Magic header used to identify Cranelift `.clif` container payloads.
pub(crate) const CLIF_MAGIC: &str = "FAUST_CLIF_V1";

/// Escapes one textual field for key/value line serialization.
fn esc_field(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\n', "\\n")
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
    out.push_str("clif_text=deferred\n");
    Ok(out)
}
