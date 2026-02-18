//! C backend generation from FIR `Module` roots.
//!
//! # Source provenance (C++)
//! - `compiler/generator/c/c_code_container.cpp`
//! - `compiler/generator/c/c_instructions.hh`
//!
//! # Current slice
//! - Module-first emission from FIR `Module`.
//! - C API style output (`typedef struct`, `new/delete/init/buildUI/compute`).
//! - `compute` emits a sample loop and writes signal outputs to `outputs[]`.
//!
//! # Output contract
//! - Emits C header-style unit with include guard + `extern "C"` section.
//! - Emits `typedef struct { ... } <name>;` as DSP state container.
//! - Emits Faust C-style exported functions:
//!   `new*`, `delete*`, `metadata*`, `getNum*`, `init*`, `buildUserInterface*`,
//!   `compute*`.
//! - `instanceConstants*` always writes `dsp->fSampleRate = sample_rate` before
//!   section body statements, keeping lifecycle parity with Faust C++ init flow.
//! - Emits `compute*(..., int count, FAUSTFLOAT** RESTRICT, FAUSTFLOAT** RESTRICT)`
//!   with a per-sample loop and channel writes.
//!
//! # Limitations
//! Unsupported FIR nodes currently fail fast with `FRS-CGEN-C-0003`.

use std::fmt::Write as _;

use fir::{AccessType, FirBinOp, FirId, FirMatch, FirStore, FirType, NamedType, match_fir};

pub const BACKEND_NAME: &str = "c";

/// C backend options for module-first emission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct COptions {
    /// Optional C struct name override for the FIR module name.
    pub class_name: Option<String>,
    /// Number of audio inputs reported by `getNumInputs*`.
    pub num_inputs: usize,
    /// Number of audio outputs reported by `getNumOutputs*`.
    pub num_outputs: usize,
    /// C spelling used for FIR `Quad` values.
    ///
    /// Kept configurable because C targets can differ on extended precision
    /// support and naming.
    pub quad_type_name: String,
    /// C spelling used for FIR `FixedPoint` values.
    ///
    /// Kept configurable because fixed-point backends may require a project
    /// specific typedef or include.
    pub fixed_type_name: String,
}

impl Default for COptions {
    /// Default backend options.
    ///
    /// Uses `class_name = Some("mydsp")` to match the current workspace
    /// convention for deterministic generated type names.
    fn default() -> Self {
        Self {
            class_name: Some("mydsp".to_owned()),
            num_inputs: 0,
            num_outputs: 0,
            quad_type_name: "quad".to_owned(),
            fixed_type_name: "fixed".to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodegenErrorCode {
    /// Root FIR node is not a module (`FirMatch::Module`).
    RootNotModule,
    /// One module section is not a FIR block.
    InvalidModuleSection,
    /// The C emitter slice does not yet support this FIR node.
    UnsupportedNode,
}

impl CodegenErrorCode {
    /// Stable textual code used in diagnostics and tests.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RootNotModule => "FRS-CGEN-C-0001",
            Self::InvalidModuleSection => "FRS-CGEN-C-0002",
            Self::UnsupportedNode => "FRS-CGEN-C-0003",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodegenError {
    code: CodegenErrorCode,
    message: String,
}

impl CodegenError {
    /// Creates a typed C backend code generation error.
    #[must_use]
    pub fn new(code: CodegenErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Returns the stable backend error code.
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

#[derive(Debug, Clone)]
struct StructInit {
    name: String,
    typ: FirType,
    init: FirId,
}

#[derive(Clone, Debug)]
struct TableInit {
    name: String,
    access: AccessType,
    elem_type: FirType,
    values: Vec<FirId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmitMode {
    Default,
    Compute {
        drop_index: usize,
        output_channels: usize,
    },
}

#[derive(Debug, Clone)]
struct DeclareFunView {
    name: String,
    typ: FirType,
    named_args: Vec<NamedType>,
    body: FirId,
}

#[must_use]
/// Returns the stable backend identifier (`"c"`).
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}

/// Generates C code from a FIR module root.
///
/// Generated code follows Faust C backend conventions:
/// - header guard + `extern "C"` block
/// - `typedef struct { ... } <class_name>;`
/// - C API entrypoints:
///   `new*`, `delete*`, `metadata*`, `init*`, `buildUserInterface*`, `compute*`
/// - `compute*` signature:
///   `(<class>* dsp, int count, FAUSTFLOAT** RESTRICT inputs, FAUSTFLOAT** RESTRICT outputs)`
///   with a per-sample loop and output writes.
///
/// # Errors
/// Returns [`CodegenError`] if the root is not a FIR module or if the module
/// contains unsupported FIR nodes for the current C emitter slice.
///
/// # Options behavior
/// - `class_name`: overrides FIR module name.
/// - `num_inputs`/`num_outputs`: drive `getNumInputs*`/`getNumOutputs*`; when
///   `num_outputs == 0`, output arity is inferred from `compute` drops.
pub fn generate_c_module(
    store: &FirStore,
    module: FirId,
    options: &COptions,
) -> Result<String, CodegenError> {
    let module = decode_module(store, module)?;
    let class_name = options
        .class_name
        .as_deref()
        .unwrap_or(module.name.as_str())
        .to_owned();
    let mut effective_options = options.clone();
    if effective_options.num_outputs == 0 {
        effective_options.num_outputs =
            infer_module_compute_output_arity(store, module.declarations);
    }

    let declared_functions = collect_declared_functions(store, module.declarations)?;
    let struct_inits = collect_struct_initializers(store, module.dsp_struct, module.globals)?;
    let table_inits = collect_table_initializers(store, module.dsp_struct, module.globals)?;
    let mut out = String::new();
    emit_c_header(&mut out, &class_name);
    emit_struct_definition(
        store,
        &mut out,
        &effective_options,
        &class_name,
        module.dsp_struct,
        module.globals,
    )?;
    emit_c_api(
        store,
        &mut out,
        &effective_options,
        &class_name,
        &declared_functions,
        &struct_inits,
        &table_inits,
    )?;
    emit_c_footer(&mut out);
    Ok(out)
}

fn emit_c_header(out: &mut String, class_name: &str) {
    let guard = format!("__{}_H__", class_name);
    let _ = writeln!(out, "#ifndef  {guard}");
    let _ = writeln!(out, "#define  {guard}");
    let _ = writeln!(out);
    let _ = writeln!(out, "#ifndef FAUSTFLOAT");
    let _ = writeln!(out, "#define FAUSTFLOAT float");
    let _ = writeln!(out, "#endif");
    let _ = writeln!(out);
    let _ = writeln!(out, "#ifdef __cplusplus");
    let _ = writeln!(out, "extern \"C\" {{");
    let _ = writeln!(out, "#endif");
    let _ = writeln!(out);
    let _ = writeln!(out, "#if defined(_WIN32)");
    let _ = writeln!(out, "#define RESTRICT __restrict");
    let _ = writeln!(out, "#else");
    let _ = writeln!(out, "#define RESTRICT __restrict__");
    let _ = writeln!(out, "#endif");
    let _ = writeln!(out);
    let _ = writeln!(out, "#include <math.h>");
    let _ = writeln!(out, "#include <stdint.h>");
    let _ = writeln!(out, "#include <stdlib.h>");
    let _ = writeln!(out);
    let _ = writeln!(out, "#ifndef FAUSTCLASS");
    let _ = writeln!(out, "#define FAUSTCLASS {class_name}");
    let _ = writeln!(out, "#endif");
    let _ = writeln!(out);
    let _ = writeln!(out, "#ifdef __APPLE__");
    let _ = writeln!(out, "#define exp10f __exp10f");
    let _ = writeln!(out, "#define exp10 __exp10");
    let _ = writeln!(out, "#endif");
    let _ = writeln!(out);
}

fn emit_c_footer(out: &mut String) {
    let _ = writeln!(out);
    let _ = writeln!(out, "#ifdef __cplusplus");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "#endif");
    let _ = writeln!(out);
    let _ = writeln!(out, "#endif");
}

fn emit_struct_definition(
    store: &FirStore,
    out: &mut String,
    options: &COptions,
    class_name: &str,
    dsp_struct: FirId,
    globals: FirId,
) -> Result<(), CodegenError> {
    let _ = writeln!(out, "typedef struct {{");
    emit_struct_fields(store, out, options, dsp_struct)?;
    emit_struct_fields(store, out, options, globals)?;
    let _ = writeln!(out, "    int fSampleRate;");
    let _ = writeln!(out, "}} {class_name};");
    let _ = writeln!(out);
    Ok(())
}

fn emit_struct_fields(
    store: &FirStore,
    out: &mut String,
    options: &COptions,
    block_id: FirId,
) -> Result<(), CodegenError> {
    let FirMatch::Block(items) = match_fir(store, block_id) else {
        return Err(CodegenError::new(
            CodegenErrorCode::InvalidModuleSection,
            format!(
                "struct section must be a FIR block, got {:?} at node {}",
                match_fir(store, block_id),
                block_id.as_u32()
            ),
        ));
    };

    for item in items {
        match match_fir(store, item) {
            FirMatch::DeclareVar { name, typ, .. } => {
                let _ = write!(out, "    {} {name}", emit_type(&typ, options));
                let _ = writeln!(out, ";");
            }
            FirMatch::DeclareTable {
                name,
                elem_type,
                values,
                ..
            } => {
                let _ = writeln!(
                    out,
                    "    {} {}[{}];",
                    emit_type(&elem_type, options),
                    name,
                    values.len()
                );
            }
            _ => {}
        }
    }
    Ok(())
}

fn emit_c_api(
    store: &FirStore,
    out: &mut String,
    options: &COptions,
    class_name: &str,
    declared_functions: &[DeclareFunView],
    struct_inits: &[StructInit],
    table_inits: &[TableInit],
) -> Result<(), CodegenError> {
    let names: Vec<&str> = declared_functions.iter().map(|f| f.name.as_str()).collect();

    let _ = writeln!(out, "{class_name}* new{class_name}() {{");
    let _ = writeln!(
        out,
        "    {class_name}* dsp = ({class_name}*)calloc(1, sizeof({class_name}));"
    );
    let _ = writeln!(out, "    return dsp;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);

    let _ = writeln!(out, "void delete{class_name}({class_name}* dsp) {{");
    let _ = writeln!(out, "    free(dsp);");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);

    emit_metadata(store, out, options, class_name, declared_functions)?;

    let _ = writeln!(
        out,
        "int getSampleRate{class_name}({class_name}* RESTRICT dsp) {{"
    );
    let _ = writeln!(out, "    return dsp->fSampleRate;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);

    let _ = writeln!(
        out,
        "int getNumInputs{class_name}({class_name}* RESTRICT dsp) {{"
    );
    let _ = writeln!(out, "    (void)dsp;");
    let _ = writeln!(out, "    return {};", options.num_inputs);
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);

    let _ = writeln!(
        out,
        "int getNumOutputs{class_name}({class_name}* RESTRICT dsp) {{"
    );
    let _ = writeln!(out, "    (void)dsp;");
    let _ = writeln!(out, "    return {};", options.num_outputs);
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);

    let _ = writeln!(out, "void classInit{class_name}(int sample_rate) {{");
    let _ = writeln!(out, "    (void)sample_rate;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);

    if let Some(f) = declared_functions
        .iter()
        .find(|f| f.name == "instanceConstants")
    {
        emit_named_fun(store, out, options, class_name, f)?;
    } else {
        let _ = writeln!(
            out,
            "void instanceConstants{class_name}({class_name}* dsp, int sample_rate) {{"
        );
        let _ = writeln!(out, "    dsp->fSampleRate = sample_rate;");
        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
    }

    if let Some(f) = declared_functions
        .iter()
        .find(|f| f.name == "instanceResetUserInterface")
    {
        emit_named_fun(store, out, options, class_name, f)?;
    } else {
        let _ = writeln!(
            out,
            "void instanceResetUserInterface{class_name}({class_name}* dsp) {{"
        );
        if struct_inits.is_empty() && table_inits.is_empty() {
            let _ = writeln!(out, "    (void)dsp;");
        } else {
            for init in struct_inits {
                let value = emit_value(store, options, init.init)?;
                let _ = writeln!(
                    out,
                    "    dsp->{} = ({})({value});",
                    init.name,
                    emit_type(&init.typ, options)
                );
            }
            for init in table_inits {
                for (index, value_id) in init.values.iter().copied().enumerate() {
                    let value = emit_value(store, options, value_id)?;
                    let table_ref = emit_var_ref(&init.name, init.access);
                    let _ = writeln!(
                        out,
                        "    {table_ref}[{index}] = ({})({value});",
                        emit_type(&init.elem_type, options)
                    );
                }
            }
        }
        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
    }

    if let Some(f) = declared_functions
        .iter()
        .find(|f| f.name == "instanceClear")
    {
        emit_named_fun(store, out, options, class_name, f)?;
    } else {
        let _ = writeln!(out, "void instanceClear{class_name}({class_name}* dsp) {{");
        let _ = writeln!(out, "    (void)dsp;");
        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
    }

    let _ = writeln!(
        out,
        "void instanceInit{class_name}({class_name}* dsp, int sample_rate) {{"
    );
    let _ = writeln!(out, "    instanceConstants{class_name}(dsp, sample_rate);");
    let _ = writeln!(out, "    instanceResetUserInterface{class_name}(dsp);");
    let _ = writeln!(out, "    instanceClear{class_name}(dsp);");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);

    let _ = writeln!(
        out,
        "void init{class_name}({class_name}* dsp, int sample_rate) {{"
    );
    let _ = writeln!(out, "    classInit{class_name}(sample_rate);");
    let _ = writeln!(out, "    instanceInit{class_name}(dsp, sample_rate);");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);

    if let Some(f) = declared_functions
        .iter()
        .find(|f| f.name == "buildUserInterface")
    {
        emit_named_fun(store, out, options, class_name, f)?;
    } else {
        let _ = writeln!(
            out,
            "void buildUserInterface{class_name}({class_name}* dsp, UIGlue* ui_interface) {{"
        );
        let _ = writeln!(out, "    (void)dsp;");
        let _ = writeln!(out, "    (void)ui_interface;");
        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
    }

    if let Some(f) = declared_functions.iter().find(|f| f.name == "compute") {
        emit_named_fun(store, out, options, class_name, f)?;
    } else {
        let _ = writeln!(
            out,
            "void compute{class_name}({class_name}* dsp, int count, FAUSTFLOAT** RESTRICT inputs, FAUSTFLOAT** RESTRICT outputs) {{"
        );
        let _ = writeln!(out, "    (void)dsp;");
        let _ = writeln!(out, "    (void)count;");
        let _ = writeln!(out, "    (void)inputs;");
        let _ = writeln!(out, "    (void)outputs;");
        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
    }

    for f in declared_functions {
        if matches!(
            f.name.as_str(),
            "metadata"
                | "instanceConstants"
                | "instanceResetUserInterface"
                | "instanceClear"
                | "buildUserInterface"
                | "compute"
        ) {
            continue;
        }
        if names.contains(&f.name.as_str()) {
            emit_helper_function(store, out, options, f)?;
        }
    }

    Ok(())
}

fn emit_metadata(
    store: &FirStore,
    out: &mut String,
    options: &COptions,
    class_name: &str,
    declared_functions: &[DeclareFunView],
) -> Result<(), CodegenError> {
    if let Some(f) = declared_functions.iter().find(|f| f.name == "metadata") {
        emit_named_fun(store, out, options, class_name, f)
    } else {
        let _ = writeln!(out, "void metadata{class_name}(MetaGlue* m) {{");
        let _ = writeln!(
            out,
            "    m->declare(m->metaInterface, \"faust-rs\", \"module-first c backend prototype\");"
        );
        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
        Ok(())
    }
}

fn emit_named_fun(
    store: &FirStore,
    out: &mut String,
    options: &COptions,
    class_name: &str,
    decl: &DeclareFunView,
) -> Result<(), CodegenError> {
    validate_faust_api_signature(decl)?;
    let signature = match decl.name.as_str() {
        "metadata" => format!("void metadata{class_name}(MetaGlue* m)"),
        "instanceConstants" => {
            format!("void instanceConstants{class_name}({class_name}* dsp, int sample_rate)")
        }
        "instanceResetUserInterface" => {
            format!("void instanceResetUserInterface{class_name}({class_name}* dsp)")
        }
        "instanceClear" => format!("void instanceClear{class_name}({class_name}* dsp)"),
        "buildUserInterface" => {
            format!("void buildUserInterface{class_name}({class_name}* dsp, UIGlue* ui_interface)")
        }
        "compute" => format!(
            "void compute{class_name}({class_name}* dsp, int count, FAUSTFLOAT** RESTRICT inputs, FAUSTFLOAT** RESTRICT outputs)"
        ),
        _ => format!(
            "{} {}{class_name}({class_name}* dsp)",
            emit_return_type(&decl.typ, options),
            decl.name
        ),
    };
    let _ = writeln!(out, "{signature} {{");
    if decl.name == "instanceConstants" {
        let _ = writeln!(out, "    dsp->fSampleRate = sample_rate;");
    }
    if decl.name == "compute" {
        emit_compute_body(store, out, options, decl.body, 1)?;
    } else {
        emit_block(store, out, options, decl.body, 1)?;
    }
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);
    Ok(())
}

fn validate_faust_api_signature(decl: &DeclareFunView) -> Result<(), CodegenError> {
    let (expected_args, expected_ret, api_sig) = match decl.name.as_str() {
        "metadata" => (vec![FirType::Meta], FirType::Void, "void metadata(Meta*)"),
        "instanceConstants" => (
            vec![FirType::Int32],
            FirType::Void,
            "void instanceConstants(int)",
        ),
        "instanceResetUserInterface" => (
            Vec::new(),
            FirType::Void,
            "void instanceResetUserInterface()",
        ),
        "instanceClear" => (Vec::new(), FirType::Void, "void instanceClear()"),
        "buildUserInterface" => (
            vec![FirType::UI],
            FirType::Void,
            "void buildUserInterface(UI*)",
        ),
        "compute" => (
            vec![
                FirType::Int32,
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            ],
            FirType::Void,
            "void compute(int, FAUSTFLOAT**, FAUSTFLOAT**)",
        ),
        _ => return Ok(()),
    };

    let FirType::Fun { args, ret } = &decl.typ else {
        return Err(CodegenError::new(
            CodegenErrorCode::InvalidModuleSection,
            format!(
                "invalid FIR signature for {}: expected {api_sig}, got non-function type {:?}",
                decl.name, decl.typ
            ),
        ));
    };

    if *args != expected_args || ret.as_ref() != &expected_ret {
        return Err(CodegenError::new(
            CodegenErrorCode::InvalidModuleSection,
            format!(
                "invalid FIR signature for {}: expected {api_sig}, got {:?}",
                decl.name, decl.typ
            ),
        ));
    }

    if decl.named_args.len() != expected_args.len()
        || decl
            .named_args
            .iter()
            .zip(expected_args.iter())
            .any(|(named, expected)| named.typ != *expected)
    {
        return Err(CodegenError::new(
            CodegenErrorCode::InvalidModuleSection,
            format!(
                "invalid FIR named args for {}: expected types {:?}, got {:?}",
                decl.name, expected_args, decl.named_args
            ),
        ));
    }

    Ok(())
}

fn emit_helper_function(
    store: &FirStore,
    out: &mut String,
    options: &COptions,
    decl: &DeclareFunView,
) -> Result<(), CodegenError> {
    let (ret, params) = match &decl.typ {
        FirType::Fun {
            args: typed_args,
            ret,
        } => {
            let ret = emit_type(ret, options);
            let mut rendered = Vec::with_capacity(typed_args.len());
            for (index, arg_type) in typed_args.iter().enumerate() {
                let name = decl
                    .named_args
                    .get(index)
                    .map_or_else(|| format!("arg{index}"), |named| named.name.clone());
                rendered.push(format!("{} {}", emit_type(arg_type, options), name));
            }
            (ret, rendered.join(", "))
        }
        other => (emit_type(other, options), String::new()),
    };
    let _ = writeln!(out, "static {ret} {}({params}) {{", decl.name);
    emit_block(store, out, options, decl.body, 1)?;
    let _ = writeln!(out, "}}");
    let _ = writeln!(out);
    Ok(())
}

fn emit_return_type(typ: &FirType, options: &COptions) -> String {
    match typ {
        FirType::Fun { ret, .. } => emit_type(ret, options),
        _ => emit_type(typ, options),
    }
}

fn emit_compute_body(
    store: &FirStore,
    out: &mut String,
    options: &COptions,
    body: FirId,
    indent: usize,
) -> Result<(), CodegenError> {
    let tab = "    ".repeat(indent);
    let output_channels = if options.num_outputs > 0 {
        options.num_outputs
    } else {
        infer_compute_output_arity(store, body)
    };
    for index in 0..options.num_inputs {
        let _ = writeln!(out, "{tab}FAUSTFLOAT* input{index} = inputs[{index}];");
    }
    for index in 0..output_channels {
        let _ = writeln!(out, "{tab}FAUSTFLOAT* output{index} = outputs[{index}];");
    }
    let _ = writeln!(out, "{tab}{{");
    let _ = writeln!(out, "{tab}    int i0;");
    let _ = writeln!(out, "{tab}    for (i0 = 0; i0 < count; i0 = i0 + 1) {{");
    let mut mode = EmitMode::Compute {
        drop_index: 0,
        output_channels,
    };
    emit_block_with_mode(store, out, options, body, indent + 2, &mut mode)?;
    let _ = writeln!(out, "{tab}    }}");
    let _ = writeln!(out, "{tab}}}");
    Ok(())
}

fn infer_module_compute_output_arity(store: &FirStore, declarations: FirId) -> usize {
    let FirMatch::Block(items) = match_fir(store, declarations) else {
        return 0;
    };
    for item in items {
        if let FirMatch::DeclareFun { name, body, .. } = match_fir(store, item)
            && name == "compute"
        {
            return infer_compute_output_arity(store, body);
        }
    }
    0
}

fn collect_struct_initializers(
    store: &FirStore,
    dsp_struct: FirId,
    globals: FirId,
) -> Result<Vec<StructInit>, CodegenError> {
    let mut out = Vec::new();
    for section in [dsp_struct, globals] {
        let FirMatch::Block(items) = match_fir(store, section) else {
            return Err(CodegenError::new(
                CodegenErrorCode::InvalidModuleSection,
                format!(
                    "struct section must be a FIR block, got {:?} at node {}",
                    match_fir(store, section),
                    section.as_u32()
                ),
            ));
        };
        for item in items {
            if let FirMatch::DeclareVar {
                name,
                typ,
                init: Some(init),
                ..
            } = match_fir(store, item)
            {
                out.push(StructInit { name, typ, init });
            }
        }
    }
    Ok(out)
}

fn collect_table_initializers(
    store: &FirStore,
    dsp_struct: FirId,
    globals: FirId,
) -> Result<Vec<TableInit>, CodegenError> {
    let mut out = Vec::new();
    for section in [dsp_struct, globals] {
        let FirMatch::Block(items) = match_fir(store, section) else {
            return Err(CodegenError::new(
                CodegenErrorCode::InvalidModuleSection,
                format!(
                    "struct section must be a FIR block, got {:?} at node {}",
                    match_fir(store, section),
                    section.as_u32()
                ),
            ));
        };
        for item in items {
            if let FirMatch::DeclareTable {
                name,
                access,
                elem_type,
                values,
            } = match_fir(store, item)
            {
                out.push(TableInit {
                    name,
                    access,
                    elem_type,
                    values,
                });
            }
        }
    }
    Ok(out)
}

fn infer_compute_output_arity(store: &FirStore, body: FirId) -> usize {
    let FirMatch::Block(items) = match_fir(store, body) else {
        return 0;
    };
    items
        .iter()
        .filter(|stmt| matches!(match_fir(store, **stmt), FirMatch::Drop(_)))
        .count()
}

fn collect_declared_functions(
    store: &FirStore,
    declarations: FirId,
) -> Result<Vec<DeclareFunView>, CodegenError> {
    let FirMatch::Block(items) = match_fir(store, declarations) else {
        return Err(CodegenError::new(
            CodegenErrorCode::InvalidModuleSection,
            format!(
                "section 'functions' must be a FIR block, got {:?} at node {}",
                match_fir(store, declarations),
                declarations.as_u32()
            ),
        ));
    };

    let mut names = Vec::new();
    for item in items {
        if let FirMatch::DeclareFun {
            name,
            typ,
            args,
            body,
            ..
        } = match_fir(store, item)
        {
            names.push(DeclareFunView {
                name,
                typ,
                named_args: args,
                body,
            });
        }
    }
    Ok(names)
}

fn emit_block(
    store: &FirStore,
    out: &mut String,
    options: &COptions,
    block: FirId,
    indent: usize,
) -> Result<(), CodegenError> {
    let mut mode = EmitMode::Default;
    emit_block_with_mode(store, out, options, block, indent, &mut mode)
}

fn emit_block_with_mode(
    store: &FirStore,
    out: &mut String,
    options: &COptions,
    block: FirId,
    indent: usize,
    mode: &mut EmitMode,
) -> Result<(), CodegenError> {
    let FirMatch::Block(items) = match_fir(store, block) else {
        return Err(unsupported_node("expected block", block, store));
    };
    for stmt in items {
        emit_stmt(store, out, options, stmt, indent, mode)?;
    }
    Ok(())
}

fn emit_stmt(
    store: &FirStore,
    out: &mut String,
    options: &COptions,
    stmt: FirId,
    indent: usize,
    mode: &mut EmitMode,
) -> Result<(), CodegenError> {
    let tab = "    ".repeat(indent);
    match match_fir(store, stmt) {
        FirMatch::DeclareVar {
            name,
            typ,
            access: _,
            init,
        } => {
            let _ = write!(out, "{tab}{} {name}", emit_type(&typ, options));
            if let Some(init) = init {
                let init = emit_value(store, options, init)?;
                let _ = write!(out, " = {init}");
            }
            let _ = writeln!(out, ";");
            Ok(())
        }
        FirMatch::DeclareTable {
            name,
            elem_type,
            values,
            ..
        } => {
            let mut rendered = Vec::with_capacity(values.len());
            for value in &values {
                rendered.push(emit_value(store, options, *value)?);
            }
            let _ = writeln!(
                out,
                "{tab}{} {}[{}] = {{{}}};",
                emit_type(&elem_type, options),
                name,
                values.len(),
                rendered.join(", ")
            );
            Ok(())
        }
        FirMatch::StoreVar {
            name,
            access,
            value,
        } => {
            let value = emit_value(store, options, value)?;
            let target = emit_var_ref(&name, access);
            let _ = writeln!(out, "{tab}{target} = {value};");
            Ok(())
        }
        FirMatch::StoreTable {
            name,
            access,
            index,
            value,
        } => {
            let index = emit_value(store, options, index)?;
            let value = emit_value(store, options, value)?;
            let target = emit_var_ref(&name, access);
            let _ = writeln!(out, "{tab}{target}[{index}] = {value};");
            Ok(())
        }
        FirMatch::Drop(value) => {
            let value = emit_value(store, options, value)?;
            if let EmitMode::Compute {
                drop_index,
                output_channels,
            } = mode
                && *drop_index < *output_channels
            {
                let output_index = *drop_index;
                let _ = writeln!(
                    out,
                    "{tab}output{output_index}[i0] = (FAUSTFLOAT)({value});"
                );
                *drop_index += 1;
                return Ok(());
            }
            let _ = writeln!(out, "{tab}(void)({value});");
            Ok(())
        }
        FirMatch::Return(value) => {
            if let Some(value) = value {
                let value = emit_value(store, options, value)?;
                let _ = writeln!(out, "{tab}return {value};");
            } else {
                let _ = writeln!(out, "{tab}return;");
            }
            Ok(())
        }
        FirMatch::Block(_) => emit_block_with_mode(store, out, options, stmt, indent, mode),
        FirMatch::If {
            cond,
            then_block,
            else_block,
        } => {
            let cond = emit_value(store, options, cond)?;
            let _ = writeln!(out, "{tab}if ({cond}) {{");
            emit_block_with_mode(store, out, options, then_block, indent + 1, mode)?;
            let _ = writeln!(out, "{tab}}}");
            if let Some(else_block) = else_block {
                let _ = writeln!(out, "{tab}else {{");
                emit_block_with_mode(store, out, options, else_block, indent + 1, mode)?;
                let _ = writeln!(out, "{tab}}}");
            }
            Ok(())
        }
        FirMatch::ForLoop {
            var,
            init,
            end,
            step,
            body,
            ..
        } => {
            let init = emit_value(store, options, init)?;
            let end = emit_value(store, options, end)?;
            let step = emit_value(store, options, step)?;
            let _ = writeln!(
                out,
                "{tab}for (int {var} = {init}; {var} < {end}; {var} = {var} + {step}) {{"
            );
            emit_block_with_mode(store, out, options, body, indent + 1, mode)?;
            let _ = writeln!(out, "{tab}}}");
            Ok(())
        }
        FirMatch::OpenBox { typ, label } => {
            let api = match typ {
                fir::UiBoxType::Vertical => "openVerticalBox",
                fir::UiBoxType::Horizontal => "openHorizontalBox",
                fir::UiBoxType::Tab => "openTabBox",
            };
            let _ = writeln!(
                out,
                "{tab}ui_interface->{api}(ui_interface->uiInterface, {});",
                c_string_literal(&label)
            );
            Ok(())
        }
        FirMatch::CloseBox => {
            let _ = writeln!(
                out,
                "{tab}ui_interface->closeBox(ui_interface->uiInterface);"
            );
            Ok(())
        }
        FirMatch::AddButton { typ, label, var } => {
            let api = match typ {
                fir::ButtonType::Button => "addButton",
                fir::ButtonType::Checkbox => "addCheckButton",
            };
            let _ = writeln!(
                out,
                "{tab}ui_interface->{api}(ui_interface->uiInterface, {}, &dsp->{var});",
                c_string_literal(&label)
            );
            Ok(())
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
            let api = match typ {
                fir::SliderType::Horizontal => "addHorizontalSlider",
                fir::SliderType::Vertical => "addVerticalSlider",
                fir::SliderType::NumEntry => "addNumEntry",
            };
            let _ = writeln!(
                out,
                "{tab}ui_interface->{api}(ui_interface->uiInterface, {}, &dsp->{var}, (FAUSTFLOAT){}, (FAUSTFLOAT){}, (FAUSTFLOAT){}, (FAUSTFLOAT){});",
                c_string_literal(&label),
                trim_float(init),
                trim_float(lo),
                trim_float(hi),
                trim_float(step),
            );
            Ok(())
        }
        FirMatch::AddBargraph {
            typ,
            label,
            var,
            lo,
            hi,
        } => {
            let api = match typ {
                fir::BargraphType::Horizontal => "addHorizontalBargraph",
                fir::BargraphType::Vertical => "addVerticalBargraph",
            };
            let _ = writeln!(
                out,
                "{tab}ui_interface->{api}(ui_interface->uiInterface, {}, &dsp->{var}, (FAUSTFLOAT){}, (FAUSTFLOAT){});",
                c_string_literal(&label),
                trim_float(lo),
                trim_float(hi)
            );
            Ok(())
        }
        FirMatch::AddMetaDeclare { var, key, value } => {
            let zone = if var == "0" {
                "0".to_owned()
            } else {
                format!("&dsp->{var}")
            };
            let _ = writeln!(
                out,
                "{tab}m->declare(m->metaInterface, {zone}, {}, {});",
                c_string_literal(&key),
                c_string_literal(&value)
            );
            Ok(())
        }
        FirMatch::NullStatement => {
            let _ = writeln!(out, "{tab};");
            Ok(())
        }
        FirMatch::Label(label) => {
            let _ = writeln!(out, "{tab}// {label}");
            Ok(())
        }
        _ => Err(unsupported_node("statement", stmt, store)),
    }
}

fn emit_value(store: &FirStore, options: &COptions, value: FirId) -> Result<String, CodegenError> {
    match match_fir(store, value) {
        FirMatch::Int32 { value, .. } => Ok(value.to_string()),
        FirMatch::Int64 { value, .. } => Ok(value.to_string()),
        FirMatch::Float32 { value, .. } => Ok(format!("{}f", trim_float(f64::from(value)))),
        FirMatch::Float64 { value, .. } => Ok(trim_float(value)),
        FirMatch::Bool { value, .. } => Ok(if value { "1" } else { "0" }.to_owned()),
        FirMatch::LoadVar { name, access, .. } | FirMatch::LoadVarAddress { name, access, .. } => {
            Ok(emit_var_ref(&name, access))
        }
        FirMatch::LoadTable {
            name,
            access,
            index,
            ..
        } => {
            let index = emit_value(store, options, index)?;
            Ok(format!("{}[{index}]", emit_var_ref(&name, access)))
        }
        FirMatch::TeeVar {
            name,
            access,
            value,
            ..
        } => {
            let value = emit_value(store, options, value)?;
            Ok(format!("({} = {value})", emit_var_ref(&name, access)))
        }
        FirMatch::BinOp { op, lhs, rhs, .. } => {
            let lhs = emit_value(store, options, lhs)?;
            let rhs = emit_value(store, options, rhs)?;
            Ok(format!("({lhs} {} {rhs})", emit_binop(op)))
        }
        FirMatch::Neg { value, .. } => {
            let value = emit_value(store, options, value)?;
            Ok(format!("(-{value})"))
        }
        FirMatch::Cast { typ, value } => {
            let value = emit_value(store, options, value)?;
            Ok(format!("(({})({value}))", emit_type(&typ, options)))
        }
        FirMatch::Select2 {
            cond,
            then_value,
            else_value,
            ..
        } => {
            let cond = emit_value(store, options, cond)?;
            let then_value = emit_value(store, options, then_value)?;
            let else_value = emit_value(store, options, else_value)?;
            Ok(format!("({cond} ? {then_value} : {else_value})"))
        }
        FirMatch::FunCall { name, args, .. } => {
            let mut rendered = Vec::with_capacity(args.len());
            for arg in args {
                rendered.push(emit_value(store, options, arg)?);
            }
            let c_name = name.strip_prefix("std::").unwrap_or(name.as_str());
            Ok(format!("{c_name}({})", rendered.join(", ")))
        }
        FirMatch::NullValue { .. } => Ok("NULL".to_owned()),
        _ => Err(unsupported_node("value", value, store)),
    }
}

fn emit_binop(op: FirBinOp) -> &'static str {
    match op {
        FirBinOp::Add => "+",
        FirBinOp::Sub => "-",
        FirBinOp::Mul => "*",
        FirBinOp::Div => "/",
        FirBinOp::Rem => "%",
        FirBinOp::And => "&",
        FirBinOp::Or => "|",
        FirBinOp::Xor => "^",
        FirBinOp::Eq => "==",
        FirBinOp::Ne => "!=",
        FirBinOp::Lt => "<",
        FirBinOp::Le => "<=",
        FirBinOp::Gt => ">",
        FirBinOp::Ge => ">=",
    }
}

fn emit_var_ref(name: &str, access: AccessType) -> String {
    match access {
        AccessType::Struct => format!("dsp->{name}"),
        _ => name.to_owned(),
    }
}

fn emit_type(typ: &FirType, options: &COptions) -> String {
    match typ {
        FirType::Int32 => "int".to_owned(),
        FirType::Int64 => "long long".to_owned(),
        FirType::Float32 => "float".to_owned(),
        FirType::Float64 => "double".to_owned(),
        FirType::FaustFloat => "FAUSTFLOAT".to_owned(),
        FirType::Quad => options.quad_type_name.clone(),
        FirType::FixedPoint => options.fixed_type_name.clone(),
        FirType::Bool => "int".to_owned(),
        FirType::Void => "void".to_owned(),
        FirType::Obj => "void*".to_owned(),
        FirType::Sound => "Soundfile*".to_owned(),
        FirType::UI => "UIGlue*".to_owned(),
        FirType::Meta => "MetaGlue*".to_owned(),
        FirType::Ptr(inner) => format!("{}*", emit_type(inner, options)),
        FirType::Array(inner, size) => format!("{}[{size}]", emit_type(inner, options)),
        FirType::Vector(inner, lanes) => format!("Vec<{},{lanes}>", emit_type(inner, options)),
        FirType::Struct(name) => name.clone(),
        FirType::Fun { args, ret } => {
            let args = args
                .iter()
                .map(|arg| emit_type(arg, options))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{}({args})", emit_type(ret, options))
        }
    }
}

fn decode_module(store: &FirStore, module: FirId) -> Result<ModuleView, CodegenError> {
    if let FirMatch::Module {
        name,
        dsp_struct,
        globals,
        declarations,
    } = match_fir(store, module)
    {
        Ok(ModuleView {
            name,
            dsp_struct,
            globals,
            declarations,
        })
    } else {
        Err(CodegenError::new(
            CodegenErrorCode::RootNotModule,
            format!(
                "expected FIR module root, got {:?} at node {}",
                match_fir(store, module),
                module.as_u32()
            ),
        ))
    }
}

fn unsupported_node(kind: &str, node: FirId, store: &FirStore) -> CodegenError {
    CodegenError::new(
        CodegenErrorCode::UnsupportedNode,
        format!(
            "unsupported FIR {kind} node {:?} at {}",
            match_fir(store, node),
            node.as_u32()
        ),
    )
}

fn trim_float(value: f64) -> String {
    let mut s = format!("{value:.15}");
    while s.contains('.') && s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.push('0');
    }
    if s == "-0.0" { "0.0".to_owned() } else { s }
}

fn c_string_literal(input: &str) -> String {
    let escaped = input
        .chars()
        .flat_map(|c| match c {
            '\\' => "\\\\".chars().collect::<Vec<_>>(),
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            '\r' => "\\r".chars().collect::<Vec<_>>(),
            '\t' => "\\t".chars().collect::<Vec<_>>(),
            _ => vec![c],
        })
        .collect::<String>();
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::{COptions, generate_c_module};
    use crate::fixtures::build_sine_phasor_test_module;
    use fir::{FirBuilder, FirStore, FirType, NamedType};

    #[test]
    fn emits_c_module_with_dsp_struct_ui_and_compute_loop() {
        let (store, module) = build_sine_phasor_test_module();
        let out = generate_c_module(
            &store,
            module,
            &COptions {
                num_outputs: 1,
                ..COptions::default()
            },
        )
        .expect("c module generation should succeed");

        assert!(out.contains("typedef struct {"));
        assert!(out.contains("FAUSTFLOAT fFreq;"));
        assert!(out.contains("FAUSTFLOAT fGain;"));
        assert!(out.contains("double fPhase;"));
        assert!(out.contains("dsp->fFreq = (FAUSTFLOAT)(440.0);"));
        assert!(out.contains("dsp->fGain = (FAUSTFLOAT)(0.2);"));
        assert!(out.contains("void buildUserInterfacemydsp(mydsp* dsp, UIGlue* ui_interface)"));
        assert!(out.contains(
            "ui_interface->addHorizontalSlider(ui_interface->uiInterface, \"freq\", &dsp->fFreq, (FAUSTFLOAT)440.0, (FAUSTFLOAT)20.0, (FAUSTFLOAT)3000.0, (FAUSTFLOAT)1.0);"
        ));
        assert!(out.contains("void computemydsp(mydsp* dsp, int count, FAUSTFLOAT** RESTRICT inputs, FAUSTFLOAT** RESTRICT outputs)"));
        assert!(out.contains("for (i0 = 0; i0 < count; i0 = i0 + 1)"));
        assert!(out.contains("output0[i0] = (FAUSTFLOAT)("));
        assert!(out.contains("sin("));
        assert!(out.contains("void instanceConstantsmydsp(mydsp* dsp, int sample_rate) {"));
        assert!(out.contains("dsp->fSampleRate = sample_rate;"));
        let instance_init_i = out
            .find("void instanceInitmydsp(mydsp* dsp, int sample_rate) {")
            .expect("instanceInit should be emitted");
        let constants_call_i = out
            .find("instanceConstantsmydsp(dsp, sample_rate);")
            .expect("instanceConstants call should be emitted");
        let reset_call_i = out
            .find("instanceResetUserInterfacemydsp(dsp);")
            .expect("instanceResetUserInterface call should be emitted");
        let clear_call_i = out
            .find("instanceClearmydsp(dsp);")
            .expect("instanceClear call should be emitted");
        assert!(
            instance_init_i < constants_call_i
                && constants_call_i < reset_call_i
                && reset_call_i < clear_call_i,
            "instanceInit should call constants -> resetUI -> clear in order"
        );
    }

    #[test]
    fn rejects_invalid_canonical_metadata_signature() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let body = b.block(&[]);
        let bad_ty = FirType::Fun {
            args: vec![FirType::Int32],
            ret: Box::new(FirType::Void),
        };
        let bad_args = vec![NamedType {
            name: "x".to_string(),
            typ: FirType::Int32,
        }];
        let metadata = b.declare_fun("metadata", bad_ty, &bad_args, body, false);
        let dsp_struct = b.block(&[]);
        let globals = b.block(&[]);
        let declarations = b.block(&[metadata]);
        let module = b.module("mydsp", dsp_struct, globals, declarations);

        let err = generate_c_module(&store, module, &COptions::default())
            .expect_err("invalid canonical metadata signature must fail");
        assert_eq!(err.code(), super::CodegenErrorCode::InvalidModuleSection);
        assert!(
            err.to_string()
                .contains("invalid FIR signature for metadata")
        );
    }
}
