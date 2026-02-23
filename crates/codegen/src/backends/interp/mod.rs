//! Interpreter (`interp`) backend — FBC bytecode types, compiler, execution
//! engine, optimizer, factory, instance, and `.fbc` serialization.
//!
//! # Source provenance (C++)
//! - `compiler/generator/interpreter/` — full C++ interpreter subsystem.
//!
//! # Intended role
//! - Define the Faust Byte Code (FBC) instruction set, instruction/block types,
//!   and the `FbcReal` trait for generic f32/f64 dispatch.
//! - Host the FIR → FBC compiler, bytecode optimizer, execution engine,
//!   DSP factory/instance, and `.fbc` serialization.
//!
//! # Current status
//! **Step 5 complete**: Factory, serialization, DSP interface — the final
//! integration step. Complete pipeline: FIR → compile → optimize → serialize
//! → deserialize → instantiate → compute.
//! Built on Steps 1–4 (types, executor, compiler, optimizer).
//!
//! # Module layout
//! - [`opcode`]: `FbcOpcode` enum, `FBC_INSTRUCTION_NAMES`, `INTERP_FILE_VERSION`.
//! - [`bytecode`]: `FbcInstruction`, `FbcBlock`, `FbcBlockArena`, `BlockId`,
//!   `FbcUiInstruction`, `FbcMetaInstruction`.
//! - [`real`]: `FbcReal` trait with f32/f64 implementations.
//! - [`compiler`]: `FirToFbcCompiler` — FIR → FBC bytecode compiler.
//! - [`optimizer`]: `optimize_block` — peephole bytecode optimizer (6 levels).
//! - [`executor`]: `FbcExecutor` — bytecode execution engine with audio I/O.
//! - [`factory`]: `FbcDspFactory` — compiled bytecode program with optimization.
//! - [`instance`]: `FbcDspInstance` — runtime DSP state with `compute()` loop.
//! - [`serial`]: `write_fbc` / `read_fbc` — `.fbc` text format serialization.

pub mod bytecode;
pub mod compiler;
pub mod executor;
pub mod factory;
pub mod instance;
pub mod opcode;
pub mod optimizer;
pub mod real;
pub mod serial;

// Re-exports for convenient access.
pub use bytecode::{
    BlockId, BlockStoreData, FbcBlock, FbcBlockArena, FbcInstruction, FbcMetaInstruction,
    FbcUiInstruction,
};
pub use compiler::{CompileError, FbcCompileResult, FirToFbcCompiler, HeapType, MemoryDesc};
pub use executor::FbcExecutor;
pub use factory::FbcDspFactory;
pub use instance::FbcDspInstance;
pub use opcode::{FBC_INSTRUCTION_NAMES, FBC_OPCODE_COUNT, FbcOpcode, INTERP_FILE_VERSION};
pub use optimizer::{MAX_OPT_LEVEL, optimize_block};
pub use real::FbcReal;
pub use serial::{FAUST_VERSION, FbcSerialError, read_fbc, write_fbc};

pub const BACKEND_NAME: &str = "interp";

/// Returns the stable backend identifier (`"interp"`).
#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}

// ─── Options ────────────────────────────────────────────────────────────────

/// Options controlling interpreter bytecode generation.
///
/// Parallel to `COptions` and `CppOptions` in the C/C++ backends.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InterpOptions {
    /// Bytecode optimizer level (0 = none, up to [`MAX_OPT_LEVEL`]).
    pub opt_level: i32,
    /// Override module/class name.  When `None`, the name embedded in the FIR
    /// module is used.
    pub module_name: Option<String>,
    /// Number of audio inputs.  When `0`, the compiler facade fills this from
    /// the propagation arity.
    pub num_inputs: usize,
    /// Number of audio outputs.  When `0`, the compiler facade fills this from
    /// the propagation arity.
    pub num_outputs: usize,
}

// ─── Error types ────────────────────────────────────────────────────────────

/// Error codes for interpreter code-generation failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodegenErrorCode {
    /// FIR root node is not a `Module`.
    RootNotModule,
    /// A module section could not be decoded as a `Block`.
    InvalidModuleSection,
    /// A FIR node could not be lowered to FBC bytecode.
    CompilationFailed,
}

impl CodegenErrorCode {
    fn as_str(self) -> &'static str {
        match self {
            Self::RootNotModule => "FRS-CGEN-INTERP-0001",
            Self::InvalidModuleSection => "FRS-CGEN-INTERP-0002",
            Self::CompilationFailed => "FRS-CGEN-INTERP-0003",
        }
    }
}

/// An interpreter code-generation error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodegenError {
    /// Machine-readable error code.
    pub code: CodegenErrorCode,
    /// Human-readable message.
    pub message: String,
}

impl CodegenError {
    fn new(code: CodegenErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CodegenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for CodegenError {}

// ─── Module generator ────────────────────────────────────────────────────────

/// Generates an [`FbcDspFactory`] from a FIR module.
///
/// This is the interpreter backend's entry point, parallel to
/// `generate_cpp_module` and `generate_c_module` in the C/C++ backends.
///
/// # Function-to-block mapping
///
/// The FIR module's `declarations` block is scanned for `DeclareFun` nodes.
/// Known function names are mapped to the six [`FbcDspFactory`] code blocks:
///
/// | FIR function name              | Factory block          |
/// |--------------------------------|------------------------|
/// | `"staticInit"`                 | `static_init_block`    |
/// | `"instanceConstants"`          | `init_block`           |
/// | `"instanceResetUserInterface"` | `reset_ui_block`       |
/// | `"instanceClear"`              | `clear_block`          |
/// | `"compute"`                    | `compute_block`        |
/// | `"computeThread"`              | `compute_dsp_block`    |
///
/// DSP sections absent from the FIR module produce an empty block (only
/// `kReturn`), which is correct for the legacy-bridge lane whose FIR module
/// only carries a minimal `compute` stub.
///
/// # Source provenance (C++)
/// - `InterpreterInstVisitor<REAL>` + `interpreter_dsp_factory_aux` in
///   `interpreter_instructions.hh` / `interpreter_dsp_aux.hh`.
pub fn generate_interp_module(
    store: &fir::FirStore,
    module: fir::FirId,
    options: &InterpOptions,
) -> Result<FbcDspFactory<f32>, CodegenError> {
    use fir::match_fir;
    use std::collections::HashMap;

    // 1. Decode module root.
    let (module_name_fir, declarations) = match match_fir(store, module) {
        fir::FirMatch::Module {
            name, declarations, ..
        } => (name, declarations),
        _ => {
            return Err(CodegenError::new(
                CodegenErrorCode::RootNotModule,
                "FIR root is not a Module",
            ));
        }
    };

    let module_name = options.module_name.clone().unwrap_or(module_name_fir);

    // 2. Extract declared functions from the declarations block.
    let decl_ids = match match_fir(store, declarations) {
        fir::FirMatch::Block(ids) => ids,
        _ => {
            return Err(CodegenError::new(
                CodegenErrorCode::InvalidModuleSection,
                "declarations is not a FIR Block",
            ));
        }
    };

    // 3. Compile each function body using a shared FirToFbcCompiler.
    let mut compiler: compiler::FirToFbcCompiler<f32> = compiler::FirToFbcCompiler::new();
    let mut fn_blocks: HashMap<String, BlockId> = HashMap::new();

    for decl_id in &decl_ids {
        if let fir::FirMatch::DeclareFun {
            name: fn_name,
            body: Some(body),
            ..
        } = match_fir(store, *decl_id)
        {
            // Prototype-only DeclareFun nodes (body: None) have no bytecode to compile.
            let block_id = compiler.compile_fir_block(store, body).map_err(|e| {
                CodegenError::new(
                    CodegenErrorCode::CompilationFailed,
                    format!("in '{fn_name}': {e}"),
                )
            })?;
            fn_blocks.insert(fn_name, block_id);
        }
    }

    // 4. Map function names to the six factory block slots.
    //    Slots without a matching function get a dedicated empty block.
    let static_init_block = fn_blocks
        .get("staticInit")
        .copied()
        .unwrap_or_else(|| compiler.alloc_empty_block());
    let init_block = fn_blocks
        .get("instanceConstants")
        .copied()
        .unwrap_or_else(|| compiler.alloc_empty_block());
    let reset_ui_block = fn_blocks
        .get("instanceResetUserInterface")
        .copied()
        .unwrap_or_else(|| compiler.alloc_empty_block());
    let clear_block = fn_blocks
        .get("instanceClear")
        .copied()
        .unwrap_or_else(|| compiler.alloc_empty_block());
    let compute_block = fn_blocks
        .get("compute")
        .copied()
        .unwrap_or_else(|| compiler.alloc_empty_block());
    let compute_dsp_block = fn_blocks
        .get("computeThread")
        .copied()
        .unwrap_or_else(|| compiler.alloc_empty_block());

    // 5. Extract arena, heap layout, UI instructions, and field table.
    let (arena, int_heap_size, real_heap_size, ui_block, field_table) = compiler.into_parts();

    // 6. Resolve well-known heap offsets from the field table.
    let sr_offset = field_table
        .get("fSamplingFreq")
        .or_else(|| field_table.get("fSampleRate"))
        .map(|d| d.offset)
        .unwrap_or(0);
    let count_offset = field_table.get("count").map(|d| d.offset).unwrap_or(0);
    let iota_offset = field_table
        .get("IOTA")
        .or_else(|| field_table.get("fIOTA"))
        .map(|d| d.offset)
        .unwrap_or(0);

    // 7. Resolve audio I/O counts.
    let num_inputs = if options.num_inputs > 0 {
        options.num_inputs as i32
    } else {
        0
    };
    let num_outputs = if options.num_outputs > 0 {
        options.num_outputs as i32
    } else {
        0
    };

    // 8. Build and optionally optimize the factory.
    let mut factory = FbcDspFactory::new(
        module_name,
        "", // sha_key: not computed at this layer
        "", // compile_options: not set at this layer
        INTERP_FILE_VERSION,
        num_inputs,
        num_outputs,
        int_heap_size,
        real_heap_size,
        sr_offset,
        count_offset,
        iota_offset,
        options.opt_level,
        arena,
        Vec::new(), // meta_block: populated by higher-level APIs
        ui_block,
        static_init_block,
        init_block,
        reset_ui_block,
        clear_block,
        compute_block,
        compute_dsp_block,
    );

    if options.opt_level > 0 {
        factory.optimize();
    }

    Ok(factory)
}
