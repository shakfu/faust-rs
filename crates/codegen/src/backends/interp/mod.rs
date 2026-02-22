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
