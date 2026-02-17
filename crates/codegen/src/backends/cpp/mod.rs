//! C++ backend generation from FIR `Module` roots.
//!
//! # Source provenance (C++)
//! - `compiler/generator/instructions.hh` (`ModuleInst`)
//! - `compiler/generator/cpp/cpp_instructions.hh` (`CPPInstVisitor::visit(ModuleInst*)`)
//! - `compiler/generator/text_instructions.hh`
//!
//! # Current slice
//! This backend follows a module-first contract:
//! input must be a FIR module node and code generation walks FIR through
//! `match_fir` only.

use std::fmt::Write as _;

use fir::{FirBinOp, FirId, FirMatch, FirStore, FirType, NamedType, match_fir};

pub const BACKEND_NAME: &str = "cpp";

/// C++ backend options for module-first emission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CppOptions {
    /// Optional namespace wrapping generated code.
    pub namespace: Option<String>,
    /// Optional class name override for the FIR module name.
    pub class_name: Option<String>,
    /// C++ spelling used for FIR `Quad` values.
    ///
    /// C++ uses target-dependent `quad` spellings; Rust backend keeps this
    /// configurable to preserve parity when exact 1:1 naming is unavailable.
    pub quad_type_name: String,
    /// C++ spelling used for FIR `FixedPoint` values.
    ///
    /// C++ fixed-point support may be backend-specific; Rust backend keeps this
    /// configurable to document/adapt non-1:1 mappings explicitly.
    pub fixed_type_name: String,
}

impl Default for CppOptions {
    fn default() -> Self {
        Self {
            namespace: None,
            class_name: None,
            quad_type_name: "quad".to_owned(),
            fixed_type_name: "fixed".to_owned(),
        }
    }
}

/// Stable backend error codes for C++ code generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodegenErrorCode {
    /// Root FIR node is not a module (`FirMatch::Module`).
    RootNotModule,
    /// Module section is not a FIR block shape.
    InvalidModuleSection,
    /// One FIR node is not yet supported by the C++ emitter slice.
    UnsupportedNode,
}

impl CodegenErrorCode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RootNotModule => "FRS-CGEN-CPP-0001",
            Self::InvalidModuleSection => "FRS-CGEN-CPP-0002",
            Self::UnsupportedNode => "FRS-CGEN-CPP-0003",
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

struct DeclareFunView<'a> {
    name: &'a str,
    typ: &'a FirType,
    named_args: &'a [NamedType],
    body: FirId,
    is_inline: bool,
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
    emit_section(store, &mut out, options, "dsp_struct", module.dsp_struct, 1)?;
    emit_section(store, &mut out, options, "globals", module.globals, 1)?;
    let _ = writeln!(out, "public:");
    emit_section(
        store,
        &mut out,
        options,
        "functions",
        module.declarations,
        1,
    )?;
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

fn emit_section(
    store: &FirStore,
    out: &mut String,
    options: &CppOptions,
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
    for item in items {
        emit_stmt(store, out, options, item, indent)?;
    }
    Ok(())
}

fn emit_stmt(
    store: &FirStore,
    out: &mut String,
    options: &CppOptions,
    stmt: FirId,
    indent: usize,
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
        FirMatch::DeclareFun {
            name,
            typ,
            args,
            body,
            is_inline,
        } => emit_declare_fun(
            store,
            out,
            options,
            DeclareFunView {
                name: &name,
                typ: &typ,
                named_args: &args,
                body,
                is_inline,
            },
            indent,
        ),
        FirMatch::NullDeclareVar => {
            let _ = writeln!(out, "{tab}/* null declare */");
            Ok(())
        }
        FirMatch::DeclareStructType { typ } => {
            let _ = writeln!(
                out,
                "{tab}// struct type declaration: {}",
                emit_type(&typ, options)
            );
            Ok(())
        }
        FirMatch::DeclareBufferIterators {
            name1,
            name2,
            channels,
            typ,
            mutable,
            chunk,
        } => {
            let _ = writeln!(
                out,
                "{tab}// buffer iterators: {name1}, {name2}, channels={channels}, type={}, mutable={mutable}, chunk={chunk}",
                emit_type(&typ, options)
            );
            Ok(())
        }
        FirMatch::StoreVar {
            name,
            access: _,
            value,
        } => {
            let value = emit_value(store, options, value)?;
            let _ = writeln!(out, "{tab}{name} = {value};");
            Ok(())
        }
        FirMatch::ShiftArrayVar {
            name,
            access: _,
            delay,
        } => {
            let _ = writeln!(out, "{tab}// shift array {name} by {delay}");
            Ok(())
        }
        FirMatch::Drop(value) => {
            let value = emit_value(store, options, value)?;
            let _ = writeln!(out, "{tab}(void)({value});");
            Ok(())
        }
        FirMatch::NullStatement => {
            let _ = writeln!(out, "{tab};");
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
        FirMatch::Block(_) => emit_block(store, out, options, stmt, indent),
        FirMatch::If {
            cond,
            then_block,
            else_block,
        } => {
            let cond = emit_value(store, options, cond)?;
            let _ = writeln!(out, "{tab}if ({cond}) {{");
            emit_block(store, out, options, then_block, indent + 1)?;
            let _ = writeln!(out, "{tab}}}");
            if let Some(else_block) = else_block {
                let _ = writeln!(out, "{tab}else {{");
                emit_block(store, out, options, else_block, indent + 1)?;
                let _ = writeln!(out, "{tab}}}");
            }
            Ok(())
        }
        FirMatch::Control { cond, stmt } => {
            let cond = emit_value(store, options, cond)?;
            let _ = writeln!(out, "{tab}if ({cond}) {{");
            emit_stmt(store, out, options, stmt, indent + 1)?;
            let _ = writeln!(out, "{tab}}}");
            Ok(())
        }
        FirMatch::ForLoop {
            var,
            init,
            end,
            step,
            body,
            is_reverse: _,
        } => {
            let init = emit_value(store, options, init)?;
            let end = emit_value(store, options, end)?;
            let step = emit_value(store, options, step)?;
            let _ = writeln!(
                out,
                "{tab}for (int {var} = {init}; {var} < {end}; {var} += {step}) {{"
            );
            emit_block(store, out, options, body, indent + 1)?;
            let _ = writeln!(out, "{tab}}}");
            Ok(())
        }
        FirMatch::SimpleForLoop {
            var,
            upper,
            body,
            is_reverse: _,
        } => {
            let upper = emit_value(store, options, upper)?;
            let _ = writeln!(out, "{tab}for (int {var} = 0; {var} < {upper}; ++{var}) {{");
            emit_block(store, out, options, body, indent + 1)?;
            let _ = writeln!(out, "{tab}}}");
            Ok(())
        }
        FirMatch::IteratorForLoop {
            iterators,
            is_reverse: _,
            body,
        } => {
            let joined = iterators.join(", ");
            let _ = writeln!(out, "{tab}// iterator-for over [{joined}]");
            emit_block(store, out, options, body, indent + 1)?;
            Ok(())
        }
        FirMatch::WhileLoop { cond, body } => {
            let cond = emit_value(store, options, cond)?;
            let _ = writeln!(out, "{tab}while ({cond}) {{");
            emit_block(store, out, options, body, indent + 1)?;
            let _ = writeln!(out, "{tab}}}");
            Ok(())
        }
        FirMatch::Switch {
            cond,
            cases,
            default,
        } => {
            let cond = emit_value(store, options, cond)?;
            let _ = writeln!(out, "{tab}switch ({cond}) {{");
            for (value, block) in cases {
                let _ = writeln!(out, "{tab}case {value}: {{");
                emit_block(store, out, options, block, indent + 1)?;
                let _ = writeln!(out, "{tab}    break;");
                let _ = writeln!(out, "{tab}}}");
            }
            if let Some(default) = default {
                let _ = writeln!(out, "{tab}default: {{");
                emit_block(store, out, options, default, indent + 1)?;
                let _ = writeln!(out, "{tab}}}");
            }
            let _ = writeln!(out, "{tab}}}");
            Ok(())
        }
        FirMatch::Label(label) => {
            let _ = writeln!(out, "{tab}// {label}");
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
                "{tab}ui_interface->{api}({});",
                cpp_string_literal(&label)
            );
            Ok(())
        }
        FirMatch::CloseBox => {
            let _ = writeln!(out, "{tab}ui_interface->closeBox();");
            Ok(())
        }
        FirMatch::AddButton { typ, label, var } => {
            let api = match typ {
                fir::ButtonType::Button => "addButton",
                fir::ButtonType::Checkbox => "addCheckButton",
            };
            let _ = writeln!(
                out,
                "{tab}ui_interface->{api}({}, &{var});",
                cpp_string_literal(&label)
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
                "{tab}ui_interface->{api}({}, &{var}, {}, {}, {}, {});",
                cpp_string_literal(&label),
                trim_float(init),
                trim_float(lo),
                trim_float(hi),
                trim_float(step)
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
                "{tab}ui_interface->{api}({}, &{var}, {}, {});",
                cpp_string_literal(&label),
                trim_float(lo),
                trim_float(hi)
            );
            Ok(())
        }
        FirMatch::AddSoundfile { label, url, var } => {
            let _ = writeln!(
                out,
                "{tab}ui_interface->addSoundfile({}, {}, &{var});",
                cpp_string_literal(&label),
                cpp_string_literal(&url)
            );
            Ok(())
        }
        FirMatch::AddMetaDeclare { var, key, value } => {
            let _ = writeln!(
                out,
                "{tab}m->declare(&{var}, {}, {});",
                cpp_string_literal(&key),
                cpp_string_literal(&value)
            );
            Ok(())
        }
        _ => Err(unsupported_node("statement", stmt, store)),
    }
}

fn emit_block(
    store: &FirStore,
    out: &mut String,
    options: &CppOptions,
    block: FirId,
    indent: usize,
) -> Result<(), CodegenError> {
    let FirMatch::Block(items) = match_fir(store, block) else {
        return Err(unsupported_node("expected block", block, store));
    };
    for stmt in items {
        emit_stmt(store, out, options, stmt, indent)?;
    }
    Ok(())
}

fn emit_declare_fun(
    store: &FirStore,
    out: &mut String,
    options: &CppOptions,
    decl: DeclareFunView<'_>,
    indent: usize,
) -> Result<(), CodegenError> {
    let tab = "    ".repeat(indent);
    let (ret, params) = match decl.typ {
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
    let inline = if decl.is_inline { "inline " } else { "" };
    let _ = writeln!(out, "{tab}{inline}{ret} {}({params}) {{", decl.name);
    emit_block(store, out, options, decl.body, indent + 1)?;
    let _ = writeln!(out, "{tab}}}");
    Ok(())
}

fn emit_value(
    store: &FirStore,
    options: &CppOptions,
    value: FirId,
) -> Result<String, CodegenError> {
    match match_fir(store, value) {
        FirMatch::Int32 { value, .. } => Ok(value.to_string()),
        FirMatch::Int64 { value, .. } => Ok(value.to_string()),
        FirMatch::Float32 { value, .. } => Ok(format!("{}f", trim_float(f64::from(value)))),
        FirMatch::Float64 { value, .. } => Ok(trim_float(value)),
        FirMatch::Bool { value, .. } => Ok(if value { "true" } else { "false" }.to_owned()),
        FirMatch::Quad { value, .. } => Ok(trim_float(value)),
        FirMatch::FixedPoint { value, .. } => Ok(trim_float(value)),
        FirMatch::ValueArray { values, .. } => {
            let mut out = String::from("{");
            for (index, item) in values.iter().enumerate() {
                if index > 0 {
                    out.push_str(", ");
                }
                out.push_str(&emit_value(store, options, *item)?);
            }
            out.push('}');
            Ok(out)
        }
        FirMatch::Int32Array { values, .. } => {
            Ok(format_array(values.iter().map(ToString::to_string)))
        }
        FirMatch::Float32Array { values, .. } => Ok(format_array(
            values
                .iter()
                .map(|v| format!("{}f", trim_float(f64::from(*v)))),
        )),
        FirMatch::Float64Array { values, .. }
        | FirMatch::QuadArray { values, .. }
        | FirMatch::FixedPointArray { values, .. } => {
            Ok(format_array(values.iter().map(|v| trim_float(*v))))
        }
        FirMatch::LoadVar {
            name, access: _, ..
        }
        | FirMatch::LoadVarAddress {
            name, access: _, ..
        } => Ok(name),
        FirMatch::TeeVar {
            name,
            access: _,
            value,
            ..
        } => {
            let value = emit_value(store, options, value)?;
            Ok(format!("({name} = {value})"))
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
        FirMatch::Bitcast { typ, value } => {
            let value = emit_value(store, options, value)?;
            Ok(format!("bitcast<{}>({value})", emit_type(&typ, options)))
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
            Ok(format!("{name}({})", rendered.join(", ")))
        }
        FirMatch::NullValue { .. } => Ok("nullptr".to_owned()),
        FirMatch::NewDsp { name, .. } => Ok(format!("new {name}()")),
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

fn emit_type(typ: &FirType, options: &CppOptions) -> String {
    match typ {
        FirType::Int32 => "int".to_owned(),
        FirType::Int64 => "long long".to_owned(),
        FirType::Float32 => "float".to_owned(),
        FirType::Float64 => "double".to_owned(),
        FirType::Quad => options.quad_type_name.clone(),
        FirType::FixedPoint => options.fixed_type_name.clone(),
        FirType::Bool => "bool".to_owned(),
        FirType::Void => "void".to_owned(),
        FirType::Obj => "void*".to_owned(),
        FirType::Sound => "Soundfile*".to_owned(),
        FirType::UI => "UI*".to_owned(),
        FirType::Meta => "Meta*".to_owned(),
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
    let mut text = format!("{value}");
    if !text.contains(['.', 'e', 'E']) {
        text.push_str(".0");
    }
    text
}

fn format_array(values: impl Iterator<Item = String>) -> String {
    format!("{{{}}}", values.collect::<Vec<_>>().join(", "))
}

fn cpp_string_literal(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
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

    #[test]
    fn emits_core_statement_and_value_slice() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let one = b.int32(1);
        let two = b.int32(2);
        let sum = b.binop(FirBinOp::Add, one, two, FirType::Int32);
        let dec = b.declare_var("acc", FirType::Int32, fir::AccessType::Stack, Some(sum));
        let acc = b.load_var("acc", fir::AccessType::Stack, FirType::Int32);
        let sixteen = b.int32(16);
        let cond = b.binop(FirBinOp::Lt, acc, sixteen, FirType::Bool);
        let neg_acc = b.neg(acc, FirType::Int32);
        let then_store = b.store_var("acc", fir::AccessType::Stack, neg_acc);
        let then_block = b.block(&[then_store]);
        let branch = b.if_(cond, then_block, None);
        let loop_drop = b.drop_(acc);
        let loop_body = b.block(&[loop_drop]);
        let four = b.int32(4);
        let loop_ = b.simple_for_loop("i", four, loop_body, false);
        let while_drop = b.drop_(acc);
        let while_body = b.block(&[while_drop]);
        let while_ = b.while_loop(cond, while_body);
        let switch_drop = b.drop_(acc);
        let switch_case = b.block(&[switch_drop]);
        let switch_default = b.block(&[]);
        let switch_ = b.switch(acc, &[(0, switch_case)], Some(switch_default));
        let ret = b.ret(Some(acc));

        let body = b.block(&[dec, branch, loop_, while_, switch_, ret]);
        let fun_ty = FirType::Fun {
            args: vec![FirType::Int32],
            ret: Box::new(FirType::Int32),
        };
        let args = vec![NamedType {
            name: "x".to_owned(),
            typ: FirType::Int32,
        }];
        let fun = b.declare_fun("compute", fun_ty, &args, body, false);

        let dsp_struct = b.block(&[]);
        let globals = b.block(&[]);
        let declarations = b.block(&[fun]);
        let module = b.module("mydsp", dsp_struct, globals, declarations);
        let out = generate_cpp_module(&store, module, &CppOptions::default())
            .expect("core statement/value slice should generate");

        assert!(out.contains("int compute(int x)"));
        assert!(out.contains("if ((acc < 16))"));
        assert!(out.contains("for (int i = 0; i < 4; ++i)"));
        assert!(out.contains("while ((acc < 16))"));
        assert!(out.contains("switch (acc)"));
        assert!(out.contains("return acc;"));
    }

    #[test]
    fn emits_ui_and_metadata_nodes() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let open = b.open_box(fir::UiBoxType::Vertical, "group");
        let button = b.add_button(fir::ButtonType::Button, "gate", "fGate");
        let slider = b.add_slider(
            fir::SliderType::Horizontal,
            "gain",
            "fGain",
            fir::SliderRange {
                init: 0.5,
                lo: 0.0,
                hi: 1.0,
                step: 0.01,
            },
        );
        let bargraph = b.add_bargraph(fir::BargraphType::Horizontal, "level", "fLevel", -60.0, 6.0);
        let soundfile = b.add_soundfile_with_url("sample", "samples/piano.wav", "fSample");
        let meta = b.add_meta_declare("fGain", "unit", "dB");
        let close = b.close_box();
        let body = b.block(&[open, button, slider, bargraph, soundfile, meta, close]);
        let fun_ty = FirType::Fun {
            args: Vec::new(),
            ret: Box::new(FirType::Void),
        };
        let fun = b.declare_fun("ui", fun_ty, &[], body, false);
        let dsp_struct = b.block(&[]);
        let globals = b.block(&[]);
        let declarations = b.block(&[fun]);
        let module = b.module("mydsp", dsp_struct, globals, declarations);

        let out =
            generate_cpp_module(&store, module, &CppOptions::default()).expect("UI nodes emit");
        assert!(out.contains("ui_interface->openVerticalBox(\"group\");"));
        assert!(out.contains("ui_interface->addButton(\"gate\", &fGate);"));
        assert!(
            out.contains(
                "ui_interface->addHorizontalSlider(\"gain\", &fGain, 0.5, 0.0, 1.0, 0.01);"
            )
        );
        assert!(
            out.contains("ui_interface->addHorizontalBargraph(\"level\", &fLevel, -60.0, 6.0);")
        );
        assert!(
            out.contains(
                "ui_interface->addSoundfile(\"sample\", \"samples/piano.wav\", &fSample);"
            )
        );
        assert!(out.contains("m->declare(&fGain, \"unit\", \"dB\");"));
        assert!(out.contains("ui_interface->closeBox();"));
    }

    #[test]
    fn type_mapping_covers_pointer_array_vector_and_function_forms() {
        let options = CppOptions::default();
        assert_eq!(
            emit_type(&FirType::Ptr(Box::new(FirType::Int32)), &options),
            "int*"
        );
        assert_eq!(
            emit_type(&FirType::Array(Box::new(FirType::Float32), 8), &options),
            "float[8]"
        );
        assert_eq!(
            emit_type(&FirType::Vector(Box::new(FirType::Float64), 4), &options),
            "Vec<double,4>"
        );
        assert_eq!(
            emit_type(
                &FirType::Fun {
                    args: vec![FirType::Int32, FirType::Ptr(Box::new(FirType::Float32))],
                    ret: Box::new(FirType::Float64),
                },
                &options,
            ),
            "double(int, float*)"
        );
    }

    #[test]
    fn type_mapping_supports_quad_and_fixed_spelling_overrides() {
        let options = CppOptions {
            quad_type_name: "long double".to_owned(),
            fixed_type_name: "faustfixed".to_owned(),
            ..CppOptions::default()
        };
        assert_eq!(emit_type(&FirType::Quad, &options), "long double");
        assert_eq!(emit_type(&FirType::FixedPoint, &options), "faustfixed");
    }
}
