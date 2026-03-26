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
//! - Function bodies are intentionally trivial placeholders; FIR instruction
//!   lowering is deferred to the next implementation steps.

use fir::{FirId, FirMatch, FirStore, match_fir};
use wasm_encoder::{
    CodeSection, ConstExpr, DataSection, EntityType, ExportKind, ExportSection, Function,
    FunctionSection, ImportSection, Instruction, MemArg, MemorySection, MemoryType, Module,
    TypeSection, ValType,
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

    let mut code = CodeSection::new();
    for func in WasmFunc::ALL {
        code.function(&scaffold_function_body(
            func,
            num_inputs as i32,
            num_outputs as i32,
            real_ty,
            memory_layout.field_offsets.get("fSampleRate"),
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
) -> Function {
    let mut function = Function::new(Vec::new());
    match func {
        WasmFunc::ClassInit
        | WasmFunc::Compute
        | WasmFunc::InstanceClear
        | WasmFunc::InstanceResetUserInterface
        | WasmFunc::SetParamValue => {}
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
