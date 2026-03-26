//! WebAssembly backend generation from FIR `Module` roots.
//!
//! # Source provenance (C++)
//! - `compiler/generator/wasm/wasm_code_container.cpp`
//! - `compiler/generator/wasm/wasm_binary.hh`
//! - `compiler/generator/wasm/wasm_instructions.hh`
//!
//! # Current slice
//! - Step-1 scaffold for the WASM backend plan.
//! - Emits a valid `.wasm` module skeleton with the canonical Faust DSP export
//!   names, memory section/import, and JSON metadata data segment.
//! - Most function bodies started as trivial placeholders; the current step now
//!   lowers a narrow but real `compute` subset for mono passthrough-style FIR.

use fir::{AccessType, FirId, FirMatch, FirStore, FirType, match_fir};
use std::collections::HashMap;
use wasm_encoder::{
    BlockType, CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection,
    Function, FunctionSection, ImportSection, Instruction, MemArg, MemorySection, MemoryType,
    Module, TypeSection, ValType,
};

pub mod layout;

pub use layout::{FieldLayout, WasmMemoryLayout, WasmValType};

#[cfg(test)]
mod tests;

pub const BACKEND_NAME: &str = "wasm";

const DEFAULT_MEMORY_PAGES: u32 = 1;

/// WASM backend compilation options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmOptions {
    /// Emit `f64` (double) instead of `f32` for parameter/value APIs.
    pub double_precision: bool,
    /// Request companion WAT text. Deferred in the current scaffold.
    pub emit_wat: bool,
    /// Internal memory size in WASM pages (64 KiB each). `0` means auto-size.
    pub memory_pages: u32,
    /// Enable internal memory (`true`) or import memory from the host (`false`).
    pub internal_memory: bool,
}

impl Default for WasmOptions {
    fn default() -> Self {
        Self {
            double_precision: false,
            emit_wat: false,
            memory_pages: 0,
            internal_memory: true,
        }
    }
}

/// Compiled WASM module output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmModule {
    /// WASM binary (valid `.wasm` file).
    pub wasm_binary: Vec<u8>,
    /// Optional WAT text companion. Deferred in the current scaffold.
    pub wat_text: Option<String>,
    /// JSON metadata string consumed by higher-level runtimes.
    pub dsp_json: String,
    /// Current linear-memory layout descriptor.
    pub memory_layout: WasmMemoryLayout,
}

/// Stable machine-readable error codes for the WASM backend emitter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmBackendErrorCode {
    UnsupportedModuleShape,
    MissingCompute,
    UnsupportedFirNode,
    EncodingFailure,
    MemoryLayoutOverflow,
}

impl WasmBackendErrorCode {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedModuleShape => "FRS-CGEN-WASM-0001",
            Self::MissingCompute => "FRS-CGEN-WASM-0002",
            Self::UnsupportedFirNode => "FRS-CGEN-WASM-0003",
            Self::EncodingFailure => "FRS-CGEN-WASM-0004",
            Self::MemoryLayoutOverflow => "FRS-CGEN-WASM-0005",
        }
    }
}

/// Typed backend error returned by the WASM emitter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmBackendError {
    code: WasmBackendErrorCode,
    message: String,
}

impl WasmBackendError {
    #[must_use]
    pub fn new(code: WasmBackendErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    #[must_use]
    pub fn code(&self) -> WasmBackendErrorCode {
        self.code
    }
}

impl std::fmt::Display for WasmBackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for WasmBackendError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WasmFunc {
    ClassInit,
    Compute,
    GetNumInputs,
    GetNumOutputs,
    GetParamValue,
    GetSampleRate,
    Init,
    InstanceClear,
    InstanceConstants,
    InstanceInit,
    InstanceResetUserInterface,
    MaxI,
    MinI,
    SetParamValue,
}

impl WasmFunc {
    const ALL: [Self; 14] = [
        Self::ClassInit,
        Self::Compute,
        Self::GetNumInputs,
        Self::GetNumOutputs,
        Self::GetParamValue,
        Self::GetSampleRate,
        Self::Init,
        Self::InstanceClear,
        Self::InstanceConstants,
        Self::InstanceInit,
        Self::InstanceResetUserInterface,
        Self::MaxI,
        Self::MinI,
        Self::SetParamValue,
    ];

    #[must_use]
    fn signature(self, real_ty: ValType) -> (Vec<ValType>, Vec<ValType>) {
        match self {
            Self::ClassInit => (vec![ValType::I32, ValType::I32], vec![]),
            Self::Compute => (
                vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32],
                vec![],
            ),
            Self::GetNumInputs => (vec![ValType::I32], vec![ValType::I32]),
            Self::GetNumOutputs => (vec![ValType::I32], vec![ValType::I32]),
            Self::GetParamValue => (vec![ValType::I32, ValType::I32], vec![real_ty]),
            Self::GetSampleRate => (vec![ValType::I32], vec![ValType::I32]),
            Self::Init => (vec![ValType::I32, ValType::I32], vec![]),
            Self::InstanceClear => (vec![ValType::I32], vec![]),
            Self::InstanceConstants => (vec![ValType::I32, ValType::I32], vec![]),
            Self::InstanceInit => (vec![ValType::I32, ValType::I32], vec![]),
            Self::InstanceResetUserInterface => (vec![ValType::I32], vec![]),
            Self::MaxI => (vec![ValType::I32, ValType::I32], vec![ValType::I32]),
            Self::MinI => (vec![ValType::I32, ValType::I32], vec![ValType::I32]),
            Self::SetParamValue => (vec![ValType::I32, ValType::I32, real_ty], vec![]),
        }
    }
}

/// Emits one valid WASM scaffold module for a FIR `Module` root.
pub fn generate_wasm_module(
    store: &FirStore,
    module: FirId,
    options: &WasmOptions,
) -> Result<WasmModule, WasmBackendError> {
    let FirMatch::Module {
        num_inputs,
        num_outputs,
        ref name,
        functions,
        ..
    } = match_fir(store, module)
    else {
        return Err(WasmBackendError::new(
            WasmBackendErrorCode::UnsupportedModuleShape,
            "WASM backend expects a FIR Module root",
        ));
    };

    let FirMatch::Block(function_items) = match_fir(store, functions) else {
        return Err(WasmBackendError::new(
            WasmBackendErrorCode::UnsupportedFirNode,
            "WASM backend expects the functions section to be a FIR Block",
        ));
    };
    let has_compute = function_items.iter().copied().any(|id| {
        matches!(
            match_fir(store, id),
            FirMatch::DeclareFun { ref name, .. } if name == "compute"
        )
    });
    if !has_compute {
        return Err(WasmBackendError::new(
            WasmBackendErrorCode::MissingCompute,
            "WASM backend requires a compute function in the FIR module",
        ));
    }

    let real_ty = if options.double_precision {
        ValType::F64
    } else {
        ValType::F32
    };

    let dsp_json = render_scaffold_json(name, num_inputs, num_outputs, options);
    let mut memory_layout = WasmMemoryLayout::from_module(store, module, options, dsp_json.len())?;
    let pages = if options.memory_pages == 0 {
        memory_layout.pages.max(DEFAULT_MEMORY_PAGES)
    } else {
        options.memory_pages
    };
    memory_layout.pages = pages;

    let mut wasm = Module::new();

    let mut types = TypeSection::new();
    for func in WasmFunc::ALL {
        let (params, results) = func.signature(real_ty);
        types.ty().function(params, results);
    }
    wasm.section(&types);

    if !options.internal_memory {
        let mut imports = ImportSection::new();
        imports.import(
            "env",
            "memory",
            EntityType::Memory(MemoryType {
                minimum: u64::from(pages),
                maximum: None,
                memory64: false,
                shared: false,
                page_size_log2: None,
            }),
        );
        wasm.section(&imports);
    }

    let mut functions = FunctionSection::new();
    for type_index in 0..WasmFunc::ALL.len() {
        functions.function(type_index as u32);
    }
    wasm.section(&functions);

    if options.internal_memory {
        let mut memories = MemorySection::new();
        memories.memory(MemoryType {
            minimum: u64::from(pages),
            maximum: Some(u64::from(pages.saturating_add(1000))),
            memory64: false,
            shared: false,
            page_size_log2: None,
        });
        wasm.section(&memories);
    }

    let mut exports = ExportSection::new();
    exports.export(
        "compute",
        ExportKind::Func,
        function_index(options, WasmFunc::Compute),
    );
    exports.export(
        "getNumInputs",
        ExportKind::Func,
        function_index(options, WasmFunc::GetNumInputs),
    );
    exports.export(
        "getNumOutputs",
        ExportKind::Func,
        function_index(options, WasmFunc::GetNumOutputs),
    );
    exports.export(
        "getParamValue",
        ExportKind::Func,
        function_index(options, WasmFunc::GetParamValue),
    );
    exports.export(
        "getSampleRate",
        ExportKind::Func,
        function_index(options, WasmFunc::GetSampleRate),
    );
    exports.export(
        "init",
        ExportKind::Func,
        function_index(options, WasmFunc::Init),
    );
    exports.export(
        "instanceClear",
        ExportKind::Func,
        function_index(options, WasmFunc::InstanceClear),
    );
    exports.export(
        "instanceConstants",
        ExportKind::Func,
        function_index(options, WasmFunc::InstanceConstants),
    );
    exports.export(
        "instanceInit",
        ExportKind::Func,
        function_index(options, WasmFunc::InstanceInit),
    );
    exports.export(
        "instanceResetUserInterface",
        ExportKind::Func,
        function_index(options, WasmFunc::InstanceResetUserInterface),
    );
    exports.export(
        "setParamValue",
        ExportKind::Func,
        function_index(options, WasmFunc::SetParamValue),
    );
    if options.internal_memory {
        exports.export("memory", ExportKind::Memory, 0);
    }
    wasm.section(&exports);

    let import_count = u32::from(!options.internal_memory);
    let compute_body = function_items
        .iter()
        .copied()
        .find_map(|id| match match_fir(store, id) {
            FirMatch::DeclareFun {
                ref name,
                body: Some(body),
                ..
            } if name == "compute" => Some(body),
            _ => None,
        });
    let mut code = CodeSection::new();
    for func in WasmFunc::ALL {
        code.function(&scaffold_function_body(
            func,
            num_inputs as i32,
            num_outputs as i32,
            real_ty,
            memory_layout.field_offsets.get("fSampleRate"),
            &memory_layout,
            import_count,
            store,
            compute_body,
            options,
        ));
    }
    wasm.section(&code);

    let mut data = DataSection::new();
    data.active(
        0,
        &ConstExpr::i32_const(0),
        dsp_json.as_bytes().iter().copied(),
    );
    wasm.section(&data);

    Ok(WasmModule {
        wasm_binary: wasm.finish(),
        wat_text: None,
        dsp_json,
        memory_layout,
    })
}

fn function_index(options: &WasmOptions, func: WasmFunc) -> u32 {
    let import_count = u32::from(!options.internal_memory);
    import_count
        + WasmFunc::ALL
            .iter()
            .position(|item| *item == func)
            .expect("function present in static WASM function list") as u32
}

fn render_scaffold_json(
    module_name: &str,
    num_inputs: usize,
    num_outputs: usize,
    options: &WasmOptions,
) -> String {
    format!(
        concat!(
            "{{",
            "\"name\":\"{}\",",
            "\"backend\":\"wasm\",",
            "\"scaffold\":true,",
            "\"double_precision\":{},",
            "\"internal_memory\":{},",
            "\"inputs\":{},",
            "\"outputs\":{}",
            "}}"
        ),
        escape_json_string(module_name),
        options.double_precision,
        options.internal_memory,
        num_inputs,
        num_outputs
    )
}

fn escape_json_string(input: &str) -> String {
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

fn scaffold_function_body(
    func: WasmFunc,
    num_inputs: i32,
    num_outputs: i32,
    real_ty: ValType,
    sample_rate_field: Option<&FieldLayout>,
    memory_layout: &WasmMemoryLayout,
    _import_count: u32,
    store: &FirStore,
    compute_body: Option<FirId>,
    options: &WasmOptions,
) -> Function {
    let mut function = Function::new(Vec::new());
    match func {
        WasmFunc::ClassInit
        | WasmFunc::InstanceClear
        | WasmFunc::InstanceResetUserInterface
        | WasmFunc::SetParamValue => {}
        WasmFunc::Compute => {
            if let Some(body) = compute_body
                && let Ok(lowered) = lower_compute_subset(store, body, memory_layout, options)
            {
                return lowered;
            }
        }
        WasmFunc::GetNumInputs => {
            function.instruction(&Instruction::I32Const(num_inputs));
        }
        WasmFunc::GetNumOutputs => {
            function.instruction(&Instruction::I32Const(num_outputs));
        }
        WasmFunc::GetParamValue => match real_ty {
            ValType::F32 => {
                function.instruction(&Instruction::F32Const(0.0));
            }
            ValType::F64 => {
                function.instruction(&Instruction::F64Const(0.0));
            }
            _ => unreachable!("real type must be f32/f64"),
        },
        WasmFunc::GetSampleRate => {
            if let Some(field) = sample_rate_field {
                function.instruction(&Instruction::LocalGet(0));
                function.instruction(&Instruction::I32Load(memarg(field.offset)));
            } else {
                function.instruction(&Instruction::I32Const(0));
            }
        }
        WasmFunc::Init => {
            function.instruction(&Instruction::LocalGet(0));
            function.instruction(&Instruction::LocalGet(1));
            function.instruction(&Instruction::Call(function_index_for_body(
                WasmFunc::ClassInit,
            )));
            function.instruction(&Instruction::LocalGet(0));
            function.instruction(&Instruction::LocalGet(1));
            function.instruction(&Instruction::Call(function_index_for_body(
                WasmFunc::InstanceInit,
            )));
        }
        WasmFunc::InstanceConstants => {
            if let Some(field) = sample_rate_field {
                function.instruction(&Instruction::LocalGet(0));
                function.instruction(&Instruction::LocalGet(1));
                function.instruction(&Instruction::I32Store(memarg(field.offset)));
            }
        }
        WasmFunc::InstanceInit => {
            function.instruction(&Instruction::LocalGet(0));
            function.instruction(&Instruction::LocalGet(1));
            function.instruction(&Instruction::Call(function_index_for_body(
                WasmFunc::InstanceConstants,
            )));
            function.instruction(&Instruction::LocalGet(0));
            function.instruction(&Instruction::Call(function_index_for_body(
                WasmFunc::InstanceResetUserInterface,
            )));
            function.instruction(&Instruction::LocalGet(0));
            function.instruction(&Instruction::Call(function_index_for_body(
                WasmFunc::InstanceClear,
            )));
        }
        WasmFunc::MaxI => {
            function.instruction(&Instruction::LocalGet(1));
            function.instruction(&Instruction::LocalGet(0));
            function.instruction(&Instruction::LocalGet(0));
            function.instruction(&Instruction::LocalGet(1));
            function.instruction(&Instruction::I32LtS);
            function.instruction(&Instruction::Select);
        }
        WasmFunc::MinI => {
            function.instruction(&Instruction::LocalGet(0));
            function.instruction(&Instruction::LocalGet(1));
            function.instruction(&Instruction::LocalGet(0));
            function.instruction(&Instruction::LocalGet(1));
            function.instruction(&Instruction::I32LtS);
            function.instruction(&Instruction::Select);
        }
    }
    function.instruction(&Instruction::End);
    function
}

fn memarg(offset: u32) -> MemArg {
    MemArg {
        offset: u64::from(offset),
        align: 2,
        memory_index: 0,
    }
}

fn function_index_for_body(func: WasmFunc) -> u32 {
    WasmFunc::ALL
        .iter()
        .position(|item| *item == func)
        .expect("function present in static WASM function list") as u32
}

#[derive(Clone, Debug)]
struct WasmLocal {
    index: u32,
    typ: FirType,
}

/// Partial `compute` subset lowerer for the current WASM bring-up phase.
///
/// # Source provenance (C++)
/// - `compiler/generator/wasm/wasm_code_container.cpp`
/// - `compiler/generator/wasm/wasm_instructions.hh`
///
/// # Supported subset
/// - `Block`
/// - local `DeclareVar(kStack)`
/// - `SimpleForLoop` (forward only)
/// - `LoadVar(kFunArgs=count | kLoop | kStack)`
/// - `LoadTable(kFunArgs=inputs/outputs | kStack aliases)`
/// - `StoreTable(kStack aliases)`
/// - `LoadTable/StoreTable(kStruct)`
/// - `Select2`
///
/// This is intentionally narrow so the backend can start executing the
/// canonical mono passthrough fixture while unsupported FIR still falls back to
/// the valid no-op body.
fn lower_compute_subset(
    store: &FirStore,
    body: FirId,
    memory_layout: &WasmMemoryLayout,
    options: &WasmOptions,
) -> Result<Function, WasmBackendError> {
    let mut local_specs = Vec::new();
    collect_compute_locals(store, body, &mut local_specs)?;

    let mut local_map = HashMap::with_capacity(local_specs.len());
    let mut wasm_locals = Vec::with_capacity(local_specs.len());
    let mut next_local = 4u32;
    for (name, typ) in local_specs {
        local_map.insert(
            name,
            WasmLocal {
                index: next_local,
                typ: typ.clone(),
            },
        );
        wasm_locals.push((1, wasm_val_type_for_fir(&typ, options)?));
        next_local += 1;
    }

    let mut function = Function::new(wasm_locals);
    let mut lowerer = ComputeSubsetLowerer {
        store,
        memory_layout,
        options,
        locals: local_map,
    };
    lowerer.lower_block_into(body, &mut function)?;
    function.instruction(&Instruction::End);
    Ok(function)
}

fn collect_compute_locals(
    store: &FirStore,
    id: FirId,
    out: &mut Vec<(String, FirType)>,
) -> Result<(), WasmBackendError> {
    match match_fir(store, id) {
        FirMatch::Block(items) => {
            for item in items {
                collect_compute_locals(store, item, out)?;
            }
            Ok(())
        }
        FirMatch::DeclareVar {
            name,
            typ,
            access: AccessType::Stack,
            ..
        } => {
            if !out.iter().any(|(known, _)| known == &name) {
                out.push((name, typ));
            }
            Ok(())
        }
        FirMatch::SimpleForLoop {
            var,
            body,
            is_reverse: false,
            ..
        } => {
            if !out.iter().any(|(known, _)| known == &var) {
                out.push((var, FirType::Int32));
            }
            collect_compute_locals(store, body, out)
        }
        FirMatch::DeclareFun { .. }
        | FirMatch::StoreTable { .. }
        | FirMatch::StoreVar { .. }
        | FirMatch::NullStatement
        | FirMatch::Return(None) => Ok(()),
        other => Err(WasmBackendError::new(
            WasmBackendErrorCode::UnsupportedFirNode,
            format!("unsupported compute local collector node in WASM subset: {other:?}"),
        )),
    }
}

struct ComputeSubsetLowerer<'a> {
    store: &'a FirStore,
    memory_layout: &'a WasmMemoryLayout,
    options: &'a WasmOptions,
    locals: HashMap<String, WasmLocal>,
}

impl ComputeSubsetLowerer<'_> {
    fn lower_block_into(
        &mut self,
        id: FirId,
        function: &mut Function,
    ) -> Result<(), WasmBackendError> {
        let FirMatch::Block(items) = match_fir(self.store, id) else {
            return Err(WasmBackendError::new(
                WasmBackendErrorCode::UnsupportedFirNode,
                "compute subset expected FIR Block body",
            ));
        };
        for item in items {
            self.lower_stmt(item, function)?;
        }
        Ok(())
    }

    fn lower_stmt(&mut self, id: FirId, function: &mut Function) -> Result<(), WasmBackendError> {
        match match_fir(self.store, id) {
            FirMatch::Block(_) => self.lower_block_into(id, function),
            FirMatch::DeclareVar {
                name,
                access: AccessType::Stack,
                init,
                ..
            } => {
                let local = self.local(&name)?.clone();
                if let Some(init) = init {
                    self.lower_expr(init, function)?;
                } else {
                    self.emit_default_value(&local.typ, function)?;
                }
                function.instruction(&Instruction::LocalSet(local.index));
                Ok(())
            }
            FirMatch::SimpleForLoop {
                var,
                upper,
                body,
                is_reverse: false,
            } => self.lower_simple_for(var, upper, body, function),
            FirMatch::StoreTable {
                name,
                access: AccessType::Stack,
                index,
                value,
            } => self.lower_store_table_stack(&name, index, value, function),
            FirMatch::StoreTable {
                name,
                access: AccessType::Struct,
                index,
                value,
            } => self.lower_store_table_struct(&name, index, value, function),
            FirMatch::StoreVar {
                name,
                access: AccessType::Struct,
                value,
            } => self.lower_store_var_struct(&name, value, function),
            FirMatch::NullStatement | FirMatch::Return(None) => Ok(()),
            other => Err(WasmBackendError::new(
                WasmBackendErrorCode::UnsupportedFirNode,
                format!("unsupported compute statement in WASM subset: {other:?}"),
            )),
        }
    }

    fn lower_simple_for(
        &mut self,
        var: String,
        upper: FirId,
        body: FirId,
        function: &mut Function,
    ) -> Result<(), WasmBackendError> {
        let local = self.local(&var)?.clone();
        function.instruction(&Instruction::I32Const(0));
        function.instruction(&Instruction::LocalSet(local.index));
        function.instruction(&Instruction::Block(BlockType::Empty));
        function.instruction(&Instruction::Loop(BlockType::Empty));
        function.instruction(&Instruction::LocalGet(local.index));
        self.lower_expr(upper, function)?;
        function.instruction(&Instruction::I32GeS);
        function.instruction(&Instruction::BrIf(1));
        self.lower_block_into(body, function)?;
        function.instruction(&Instruction::LocalGet(local.index));
        function.instruction(&Instruction::I32Const(1));
        function.instruction(&Instruction::I32Add);
        function.instruction(&Instruction::LocalSet(local.index));
        function.instruction(&Instruction::Br(0));
        function.instruction(&Instruction::End);
        function.instruction(&Instruction::End);
        Ok(())
    }

    fn lower_store_table_stack(
        &mut self,
        name: &str,
        index: FirId,
        value: FirId,
        function: &mut Function,
    ) -> Result<(), WasmBackendError> {
        let local = self.local(name)?;
        let elem_type = stack_alias_pointee(&local.typ)?;
        function.instruction(&Instruction::LocalGet(local.index));
        self.lower_index_offset(index, &elem_type, function)?;
        function.instruction(&Instruction::I32Add);
        self.lower_expr(value, function)?;
        function.instruction(&store_instruction_for_type(&elem_type, self.options)?);
        Ok(())
    }

    fn lower_store_var_struct(
        &mut self,
        name: &str,
        value: FirId,
        function: &mut Function,
    ) -> Result<(), WasmBackendError> {
        let field = self.struct_field(name)?.clone();
        let field_val_type = wasm_val_type_for_field(&field);
        function.instruction(&Instruction::LocalGet(0));
        function.instruction(&Instruction::I32Const(field.offset as i32));
        function.instruction(&Instruction::I32Add);
        self.lower_expr(value, function)?;
        let value_type = self.store.value_type(value).ok_or_else(|| {
            WasmBackendError::new(
                WasmBackendErrorCode::UnsupportedFirNode,
                format!("missing value type for struct store `{name}`"),
            )
        })?;
        self.emit_cast_if_needed(&value_type, field_val_type, function)?;
        function.instruction(&store_instruction_for_valtype(field_val_type)?);
        Ok(())
    }

    fn lower_store_table_struct(
        &mut self,
        name: &str,
        index: FirId,
        value: FirId,
        function: &mut Function,
    ) -> Result<(), WasmBackendError> {
        let field = self.struct_field(name)?.clone();
        let field_val_type = wasm_val_type_for_field(&field);
        function.instruction(&Instruction::LocalGet(0));
        function.instruction(&Instruction::I32Const(field.offset as i32));
        function.instruction(&Instruction::I32Add);
        self.lower_index_offset(index, &field_fir_type(&field, self.options), function)?;
        function.instruction(&Instruction::I32Add);
        self.lower_expr(value, function)?;
        let value_type = self.store.value_type(value).ok_or_else(|| {
            WasmBackendError::new(
                WasmBackendErrorCode::UnsupportedFirNode,
                format!("missing value type for struct table store `{name}`"),
            )
        })?;
        self.emit_cast_if_needed(&value_type, field_val_type, function)?;
        function.instruction(&store_instruction_for_valtype(field_val_type)?);
        Ok(())
    }

    fn lower_expr(&mut self, id: FirId, function: &mut Function) -> Result<(), WasmBackendError> {
        match match_fir(self.store, id) {
            FirMatch::Int32 { value, .. } => {
                function.instruction(&Instruction::I32Const(value));
                Ok(())
            }
            FirMatch::Float32 { value, .. } => {
                function.instruction(&Instruction::F32Const(value));
                Ok(())
            }
            FirMatch::Float64 { value, .. } => {
                function.instruction(&Instruction::F64Const(value));
                Ok(())
            }
            FirMatch::LoadVar {
                name,
                access: AccessType::FunArgs,
                ..
            } if name == "count" => {
                function.instruction(&Instruction::LocalGet(1));
                Ok(())
            }
            FirMatch::LoadVar {
                name,
                access: AccessType::Struct,
                typ,
            } => {
                let field = self.struct_field(&name)?;
                function.instruction(&Instruction::LocalGet(0));
                function.instruction(&Instruction::I32Const(field.offset as i32));
                function.instruction(&Instruction::I32Add);
                let storage_ty = wasm_val_type_for_field(field);
                function.instruction(&load_instruction_for_valtype(storage_ty)?);
                self.emit_cast_if_needed(
                    &field_fir_type(field, self.options),
                    wasm_val_type_for_fir(&typ, self.options)?,
                    function,
                )?;
                Ok(())
            }
            FirMatch::LoadVar {
                name,
                access: AccessType::Loop | AccessType::Stack,
                ..
            } => {
                let local = self.local(&name)?;
                function.instruction(&Instruction::LocalGet(local.index));
                Ok(())
            }
            FirMatch::LoadTable {
                name,
                access: AccessType::FunArgs,
                index,
                typ,
            } if name == "inputs" || name == "outputs" => {
                function.instruction(&Instruction::LocalGet(fun_arg_local_index(&name)));
                self.lower_index_offset(index, &FirType::Ptr(Box::new(FirType::Void)), function)?;
                function.instruction(&Instruction::I32Add);
                if matches!(typ, FirType::Ptr(_)) {
                    function.instruction(&Instruction::I32Load(memarg(0)));
                    Ok(())
                } else {
                    Err(WasmBackendError::new(
                        WasmBackendErrorCode::UnsupportedFirNode,
                        format!(
                            "expected pointer type for function-arg table `{name}`, got {typ:?}"
                        ),
                    ))
                }
            }
            FirMatch::LoadTable {
                name,
                access: AccessType::Stack,
                index,
                typ,
            } => {
                let local = self.local(&name)?;
                let elem_type = stack_alias_pointee(&local.typ)?;
                function.instruction(&Instruction::LocalGet(local.index));
                self.lower_index_offset(index, &elem_type, function)?;
                function.instruction(&Instruction::I32Add);
                function.instruction(&load_instruction_for_type(&typ, self.options)?);
                Ok(())
            }
            FirMatch::LoadTable {
                name,
                access: AccessType::Struct,
                index,
                typ,
            } => {
                let field = self.struct_field(&name)?.clone();
                let field_fir = field_fir_type(&field, self.options);
                let storage_ty = wasm_val_type_for_field(&field);
                function.instruction(&Instruction::LocalGet(0));
                function.instruction(&Instruction::I32Const(field.offset as i32));
                function.instruction(&Instruction::I32Add);
                self.lower_index_offset(index, &field_fir, function)?;
                function.instruction(&Instruction::I32Add);
                function.instruction(&load_instruction_for_valtype(storage_ty)?);
                self.emit_cast_if_needed(
                    &field_fir,
                    wasm_val_type_for_fir(&typ, self.options)?,
                    function,
                )?;
                Ok(())
            }
            FirMatch::Cast { typ, value } => {
                let src_ty = self.store.value_type(value).ok_or_else(|| {
                    WasmBackendError::new(
                        WasmBackendErrorCode::UnsupportedFirNode,
                        "missing value type for WASM cast",
                    )
                })?;
                self.lower_expr(value, function)?;
                self.emit_cast_if_needed(
                    &src_ty,
                    wasm_val_type_for_fir(&typ, self.options)?,
                    function,
                )?;
                Ok(())
            }
            FirMatch::BinOp { op, lhs, rhs, typ } => {
                self.lower_expr(lhs, function)?;
                self.lower_expr(rhs, function)?;
                function.instruction(&binop_instruction(op, &typ, self.options)?);
                Ok(())
            }
            FirMatch::Select2 {
                cond,
                then_value,
                else_value,
                typ,
            } => {
                self.lower_expr(then_value, function)?;
                self.lower_expr(else_value, function)?;
                self.lower_expr(cond, function)?;
                self.emit_cast_if_needed(&FirType::Bool, ValType::I32, function)?;
                let _ = wasm_val_type_for_fir(&typ, self.options)?;
                function.instruction(&Instruction::Select);
                Ok(())
            }
            other => Err(WasmBackendError::new(
                WasmBackendErrorCode::UnsupportedFirNode,
                format!("unsupported compute expression in WASM subset: {other:?}"),
            )),
        }
    }

    fn lower_index_offset(
        &mut self,
        index: FirId,
        elem_type: &FirType,
        function: &mut Function,
    ) -> Result<(), WasmBackendError> {
        self.lower_expr(index, function)?;
        function.instruction(&Instruction::I32Const(elem_size_bytes(
            elem_type,
            self.options,
        )?));
        function.instruction(&Instruction::I32Mul);
        Ok(())
    }

    fn emit_default_value(
        &self,
        typ: &FirType,
        function: &mut Function,
    ) -> Result<(), WasmBackendError> {
        match wasm_val_type_for_fir(typ, self.options)? {
            ValType::I32 => function.instruction(&Instruction::I32Const(0)),
            ValType::I64 => function.instruction(&Instruction::I64Const(0)),
            ValType::F32 => function.instruction(&Instruction::F32Const(0.0)),
            ValType::F64 => function.instruction(&Instruction::F64Const(0.0)),
            other => {
                return Err(WasmBackendError::new(
                    WasmBackendErrorCode::UnsupportedFirNode,
                    format!("unsupported WASM local default type: {other:?}"),
                ));
            }
        };
        Ok(())
    }

    fn local(&self, name: &str) -> Result<&WasmLocal, WasmBackendError> {
        self.locals.get(name).ok_or_else(|| {
            WasmBackendError::new(
                WasmBackendErrorCode::UnsupportedFirNode,
                format!("compute subset local `{name}` not found"),
            )
        })
    }

    fn struct_field(&self, name: &str) -> Result<&FieldLayout, WasmBackendError> {
        self.memory_layout.field_offsets.get(name).ok_or_else(|| {
            WasmBackendError::new(
                WasmBackendErrorCode::UnsupportedFirNode,
                format!("compute subset struct field `{name}` not found in WASM layout"),
            )
        })
    }

    fn emit_cast_if_needed(
        &self,
        src: &FirType,
        dst: ValType,
        function: &mut Function,
    ) -> Result<(), WasmBackendError> {
        let src = wasm_val_type_for_fir(src, self.options)?;
        if src == dst {
            return Ok(());
        }
        let instr = match (src, dst) {
            (ValType::F32, ValType::F64) => Instruction::F64PromoteF32,
            (ValType::F64, ValType::F32) => Instruction::F32DemoteF64,
            (ValType::I32, ValType::F32) => Instruction::F32ConvertI32S,
            (ValType::I32, ValType::F64) => Instruction::F64ConvertI32S,
            (ValType::F32, ValType::I32) => Instruction::I32TruncSatF32S,
            (ValType::F64, ValType::I32) => Instruction::I32TruncSatF64S,
            (ValType::I32, ValType::I64) => Instruction::I64ExtendI32S,
            (ValType::I64, ValType::I32) => Instruction::I32WrapI64,
            _ => {
                return Err(WasmBackendError::new(
                    WasmBackendErrorCode::UnsupportedFirNode,
                    format!("unsupported WASM cast in compute subset: {src:?} -> {dst:?}"),
                ));
            }
        };
        function.instruction(&instr);
        Ok(())
    }
}

fn wasm_val_type_for_fir(
    typ: &FirType,
    options: &WasmOptions,
) -> Result<ValType, WasmBackendError> {
    match typ {
        FirType::Int32 | FirType::Bool | FirType::Ptr(_) | FirType::Obj | FirType::Sound => {
            Ok(ValType::I32)
        }
        FirType::Int64 => Ok(ValType::I64),
        FirType::Float32 => Ok(ValType::F32),
        FirType::Float64 => Ok(ValType::F64),
        FirType::FaustFloat => Ok(if options.double_precision {
            ValType::F64
        } else {
            ValType::F32
        }),
        other => Err(WasmBackendError::new(
            WasmBackendErrorCode::UnsupportedFirNode,
            format!("unsupported FIR type in WASM subset: {other:?}"),
        )),
    }
}

fn elem_size_bytes(typ: &FirType, options: &WasmOptions) -> Result<i32, WasmBackendError> {
    match wasm_val_type_for_fir(typ, options)? {
        ValType::I32 | ValType::F32 => Ok(4),
        ValType::I64 | ValType::F64 => Ok(8),
        other => Err(WasmBackendError::new(
            WasmBackendErrorCode::UnsupportedFirNode,
            format!("unsupported element type width in WASM subset: {other:?}"),
        )),
    }
}

fn stack_alias_pointee(typ: &FirType) -> Result<FirType, WasmBackendError> {
    match typ {
        FirType::Ptr(inner) => Ok((**inner).clone()),
        other => Err(WasmBackendError::new(
            WasmBackendErrorCode::UnsupportedFirNode,
            format!("expected stack alias pointer type, got {other:?}"),
        )),
    }
}

fn fun_arg_local_index(name: &str) -> u32 {
    match name {
        "inputs" => 2,
        "outputs" => 3,
        other => panic!("unexpected function-arg table local `{other}`"),
    }
}

fn load_instruction_for_type(
    typ: &FirType,
    options: &WasmOptions,
) -> Result<Instruction<'static>, WasmBackendError> {
    load_instruction_for_valtype(wasm_val_type_for_fir(typ, options)?)
}

fn store_instruction_for_type(
    typ: &FirType,
    options: &WasmOptions,
) -> Result<Instruction<'static>, WasmBackendError> {
    store_instruction_for_valtype(wasm_val_type_for_fir(typ, options)?)
}

fn load_instruction_for_valtype(typ: ValType) -> Result<Instruction<'static>, WasmBackendError> {
    match typ {
        ValType::I32 => Ok(Instruction::I32Load(memarg(0))),
        ValType::I64 => Ok(Instruction::I64Load(memarg(0))),
        ValType::F32 => Ok(Instruction::F32Load(memarg(0))),
        ValType::F64 => Ok(Instruction::F64Load(memarg(0))),
        other => Err(WasmBackendError::new(
            WasmBackendErrorCode::UnsupportedFirNode,
            format!("unsupported load type in WASM subset: {other:?}"),
        )),
    }
}

fn store_instruction_for_valtype(typ: ValType) -> Result<Instruction<'static>, WasmBackendError> {
    match typ {
        ValType::I32 => Ok(Instruction::I32Store(memarg(0))),
        ValType::I64 => Ok(Instruction::I64Store(memarg(0))),
        ValType::F32 => Ok(Instruction::F32Store(memarg(0))),
        ValType::F64 => Ok(Instruction::F64Store(memarg(0))),
        other => Err(WasmBackendError::new(
            WasmBackendErrorCode::UnsupportedFirNode,
            format!("unsupported store type in WASM subset: {other:?}"),
        )),
    }
}

fn wasm_val_type_for_field(field: &FieldLayout) -> ValType {
    match field.typ {
        layout::WasmValType::I32 => ValType::I32,
        layout::WasmValType::F32 => ValType::F32,
        layout::WasmValType::F64 => ValType::F64,
    }
}

fn field_fir_type(field: &FieldLayout, options: &WasmOptions) -> FirType {
    match field.typ {
        layout::WasmValType::I32 => FirType::Int32,
        layout::WasmValType::F32 => {
            if options.double_precision {
                FirType::Float32
            } else {
                FirType::FaustFloat
            }
        }
        layout::WasmValType::F64 => FirType::Float64,
    }
}

fn binop_instruction(
    op: fir::FirBinOp,
    typ: &FirType,
    options: &WasmOptions,
) -> Result<Instruction<'static>, WasmBackendError> {
    let val_ty = wasm_val_type_for_fir(typ, options)?;
    match (op, val_ty) {
        (fir::FirBinOp::Add, ValType::I32) => Ok(Instruction::I32Add),
        (fir::FirBinOp::Sub, ValType::I32) => Ok(Instruction::I32Sub),
        (fir::FirBinOp::Mul, ValType::I32) => Ok(Instruction::I32Mul),
        (fir::FirBinOp::Div, ValType::I32) => Ok(Instruction::I32DivS),
        (fir::FirBinOp::Rem, ValType::I32) => Ok(Instruction::I32RemS),
        (fir::FirBinOp::And, ValType::I32) => Ok(Instruction::I32And),
        (fir::FirBinOp::Or, ValType::I32) => Ok(Instruction::I32Or),
        (fir::FirBinOp::Xor, ValType::I32) => Ok(Instruction::I32Xor),
        (fir::FirBinOp::Lsh, ValType::I32) => Ok(Instruction::I32Shl),
        (fir::FirBinOp::ARsh, ValType::I32) => Ok(Instruction::I32ShrS),
        (fir::FirBinOp::LRsh, ValType::I32) => Ok(Instruction::I32ShrU),
        (fir::FirBinOp::Eq, ValType::I32) => Ok(Instruction::I32Eq),
        (fir::FirBinOp::Ne, ValType::I32) => Ok(Instruction::I32Ne),
        (fir::FirBinOp::Lt, ValType::I32) => Ok(Instruction::I32LtS),
        (fir::FirBinOp::Le, ValType::I32) => Ok(Instruction::I32LeS),
        (fir::FirBinOp::Gt, ValType::I32) => Ok(Instruction::I32GtS),
        (fir::FirBinOp::Ge, ValType::I32) => Ok(Instruction::I32GeS),
        (fir::FirBinOp::Add, ValType::F32) => Ok(Instruction::F32Add),
        (fir::FirBinOp::Sub, ValType::F32) => Ok(Instruction::F32Sub),
        (fir::FirBinOp::Mul, ValType::F32) => Ok(Instruction::F32Mul),
        (fir::FirBinOp::Div, ValType::F32) => Ok(Instruction::F32Div),
        (fir::FirBinOp::Eq, ValType::F32) => Ok(Instruction::F32Eq),
        (fir::FirBinOp::Ne, ValType::F32) => Ok(Instruction::F32Ne),
        (fir::FirBinOp::Lt, ValType::F32) => Ok(Instruction::F32Lt),
        (fir::FirBinOp::Le, ValType::F32) => Ok(Instruction::F32Le),
        (fir::FirBinOp::Gt, ValType::F32) => Ok(Instruction::F32Gt),
        (fir::FirBinOp::Ge, ValType::F32) => Ok(Instruction::F32Ge),
        (fir::FirBinOp::Add, ValType::F64) => Ok(Instruction::F64Add),
        (fir::FirBinOp::Sub, ValType::F64) => Ok(Instruction::F64Sub),
        (fir::FirBinOp::Mul, ValType::F64) => Ok(Instruction::F64Mul),
        (fir::FirBinOp::Div, ValType::F64) => Ok(Instruction::F64Div),
        (fir::FirBinOp::Eq, ValType::F64) => Ok(Instruction::F64Eq),
        (fir::FirBinOp::Ne, ValType::F64) => Ok(Instruction::F64Ne),
        (fir::FirBinOp::Lt, ValType::F64) => Ok(Instruction::F64Lt),
        (fir::FirBinOp::Le, ValType::F64) => Ok(Instruction::F64Le),
        (fir::FirBinOp::Gt, ValType::F64) => Ok(Instruction::F64Gt),
        (fir::FirBinOp::Ge, ValType::F64) => Ok(Instruction::F64Ge),
        _ => Err(WasmBackendError::new(
            WasmBackendErrorCode::UnsupportedFirNode,
            format!("unsupported WASM binop in compute subset: {op:?} / {val_ty:?}"),
        )),
    }
}
