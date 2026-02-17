//! C++ backend generation from FIR `Module` roots.
//!
//! # Source provenance (C++)
//! - `compiler/generator/instructions.hh` (`ModuleInst`)
//! - `compiler/generator/cpp/cpp_instructions.hh` (`CPPInstVisitor::visit(ModuleInst*)`)
//! - `compiler/generator/text_instructions.hh`
//!
//! # Current slice
//! This initial slice validates the module-first backend contract:
//! input must be a FIR module node and generation fails with a typed
//! backend error otherwise.

use std::fmt::Write as _;

use fir::{FirId, FirMatch, FirStore, match_fir};

pub const BACKEND_NAME: &str = "cpp";

/// C++ backend options for module-first emission.
///
/// This type is intentionally small in the first slice and will be
/// extended as parity grows (`namespace`, virtual/final policy, etc.).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CppOptions {
    /// Optional namespace wrapping generated code.
    pub namespace: Option<String>,
    /// Optional class name override for the FIR module name.
    pub class_name: Option<String>,
}

/// Stable backend error codes for C++ code generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodegenErrorCode {
    /// Root FIR node is not a module (`FirMatch::Module`).
    RootNotModule,
    /// Module section is not a FIR block shape.
    InvalidModuleSection,
}

impl CodegenErrorCode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RootNotModule => "FRS-CGEN-CPP-0001",
            Self::InvalidModuleSection => "FRS-CGEN-CPP-0002",
        }
    }
}

/// Typed backend error for C++ generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenError {
    code: CodegenErrorCode,
    message: String,
}

impl CodegenError {
    #[must_use]
    pub fn new(code: CodegenErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn code(&self) -> CodegenErrorCode {
        self.code
    }
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for CodegenError {}

#[derive(Debug, Clone)]
struct ModuleView {
    name: String,
    dsp_struct: FirId,
    globals: FirId,
    declarations: FirId,
}

/// Generates C++ code from a FIR module root.
///
/// # C++ parity mapping
/// This is the Rust module-first entrypoint corresponding to C++ `ModuleInst`
/// visitor-based emission.
///
/// # Errors
/// Returns [`CodegenError`] with code `FRS-CGEN-CPP-0001` when `module`
/// does not decode to `FirMatch::Module`.
pub fn generate_cpp_module(
    store: &FirStore,
    module: FirId,
    options: &CppOptions,
) -> Result<String, CodegenError> {
    let module = decode_module(store, module)?;
    let class_name = options
        .class_name
        .as_deref()
        .unwrap_or(module.name.as_str());

    let mut out = String::new();
    emit_cpp_header(&mut out, class_name);
    if let Some(namespace) = options.namespace.as_deref() {
        let _ = writeln!(out, "namespace {namespace} {{");
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "class {class_name} : public dsp {{");
    let _ = writeln!(out, "private:");
    emit_section_shell(store, &mut out, "dsp_struct", module.dsp_struct, 1)?;
    emit_section_shell(store, &mut out, "globals", module.globals, 1)?;
    let _ = writeln!(out, "public:");
    emit_section_shell(store, &mut out, "functions", module.declarations, 1)?;
    let _ = writeln!(out, "}};");

    if let Some(namespace) = options.namespace.as_deref() {
        let _ = writeln!(out);
        let _ = writeln!(out, "}} // namespace {namespace}");
    }
    Ok(out)
}

fn emit_cpp_header(out: &mut String, class_name: &str) {
    let _ = writeln!(out, "#ifndef FAUSTCLASS");
    let _ = writeln!(out, "#define FAUSTCLASS {class_name}");
    let _ = writeln!(out, "#endif");
    let _ = writeln!(out);
    let _ = writeln!(out, "#ifdef __APPLE__");
    let _ = writeln!(out, "#define exp10f __exp10f");
    let _ = writeln!(out, "#define exp10 __exp10");
    let _ = writeln!(out, "#endif");
    let _ = writeln!(out);
    let _ = writeln!(out, "#if defined(_WIN32)");
    let _ = writeln!(out, "#define RESTRICT __restrict");
    let _ = writeln!(out, "#else");
    let _ = writeln!(out, "#define RESTRICT __restrict__");
    let _ = writeln!(out, "#endif");
    let _ = writeln!(out);
}

fn emit_section_shell(
    store: &FirStore,
    out: &mut String,
    section_name: &str,
    section_id: FirId,
    indent: usize,
) -> Result<(), CodegenError> {
    let tab = "    ".repeat(indent);
    let FirMatch::Block(items) = match_fir(store, section_id) else {
        return Err(CodegenError::new(
            CodegenErrorCode::InvalidModuleSection,
            format!(
                "section '{section_name}' must be a FIR block, got {:?} at node {}",
                match_fir(store, section_id),
                section_id.as_u32()
            ),
        ));
    };
    let _ = writeln!(
        out,
        "{tab}// section: {section_name} ({} items)",
        items.len()
    );
    Ok(())
}

fn decode_module(store: &FirStore, module: FirId) -> Result<ModuleView, CodegenError> {
    match match_fir(store, module) {
        FirMatch::Module {
            name,
            dsp_struct,
            globals,
            declarations,
        } => Ok(ModuleView {
            name,
            dsp_struct,
            globals,
            declarations,
        }),
        _ => Err(CodegenError::new(
            CodegenErrorCode::RootNotModule,
            format!(
                "expected FIR module root, got {:?} at node {}",
                match_fir(store, module),
                module.as_u32()
            ),
        )),
    }
}

#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}

#[cfg(test)]
mod tests {
    use super::*;
    use fir::FirBuilder;

    #[test]
    fn rejects_non_module_root() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let not_module = b.int32(7);
        let err = generate_cpp_module(&store, not_module, &CppOptions::default())
            .expect_err("non-module root must fail");
        assert_eq!(err.code(), CodegenErrorCode::RootNotModule);
        assert!(err.to_string().contains("FRS-CGEN-CPP-0001"));
    }

    #[test]
    fn accepts_module_root() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = b.block(&[]);
        let globals = b.block(&[]);
        let declarations = b.block(&[]);
        let module = b.module("mydsp", dsp_struct, globals, declarations);

        let out = generate_cpp_module(&store, module, &CppOptions::default())
            .expect("module root should generate");
        assert!(out.contains("#define FAUSTCLASS mydsp"));
        assert!(out.contains("class mydsp : public dsp"));
        assert!(out.contains("// section: dsp_struct (0 items)"));
        assert!(out.contains("// section: functions (0 items)"));
    }

    #[test]
    fn rejects_non_block_module_section() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let dsp_struct = b.int32(1);
        let globals = b.block(&[]);
        let declarations = b.block(&[]);
        let module = b.module("mydsp", dsp_struct, globals, declarations);
        let err = generate_cpp_module(&store, module, &CppOptions::default())
            .expect_err("non-block section must fail");
        assert_eq!(err.code(), CodegenErrorCode::InvalidModuleSection);
        assert!(err.to_string().contains("FRS-CGEN-CPP-0002"));
    }
}
