//! Interpreter (`interp`) backend — FBC bytecode types, compiler, and execution engine.
//!
//! # Source provenance (C++)
//! - `compiler/generator/interpreter/` — full C++ interpreter subsystem.
//!
//! # Intended role
//! - Define the Faust Byte Code (FBC) instruction set, instruction/block types,
//!   and the `FbcReal` trait for generic f32/f64 dispatch.
//! - Host the FIR → FBC compiler, bytecode optimizer, execution engine, and
//!   `.fbc` serialization once those phases are implemented.
//!
//! # Current status
//! **Step 4 complete**: FBC bytecode optimizer with 6 peephole optimization levels
//! (index folding, move fusion, block/pair moves, cast fusion, math fusion with
//! constant folding and identity/annihilator elimination).
//! Built on Step 1 (types), Step 2 (executor), and Step 3 (compiler).
//!
//! # Module layout
//! - [`opcode`]: `FbcOpcode` enum, `FBC_INSTRUCTION_NAMES`, `INTERP_FILE_VERSION`.
//! - [`bytecode`]: `FbcInstruction`, `FbcBlock`, `FbcBlockArena`, `BlockId`,
//!   `FbcUiInstruction`, `FbcMetaInstruction`.
//! - [`real`]: `FbcReal` trait with f32/f64 implementations.
//! - [`compiler`]: `FirToFbcCompiler` — FIR → FBC bytecode compiler.
//! - [`optimizer`]: `optimize_block` — peephole bytecode optimizer (6 levels).
//! - [`executor`]: `FbcExecutor` — bytecode execution engine with audio I/O.

pub mod bytecode;
pub mod compiler;
pub mod executor;
pub mod opcode;
pub mod optimizer;
pub mod real;

// Re-exports for convenient access.
pub use bytecode::{
    BlockId, BlockStoreData, FbcBlock, FbcBlockArena, FbcInstruction, FbcMetaInstruction,
    FbcUiInstruction,
};
pub use compiler::{CompileError, FbcCompileResult, FirToFbcCompiler, HeapType, MemoryDesc};
pub use executor::FbcExecutor;
pub use opcode::{FBC_INSTRUCTION_NAMES, FBC_OPCODE_COUNT, FbcOpcode, INTERP_FILE_VERSION};
pub use optimizer::{MAX_OPT_LEVEL, optimize_block};
pub use real::FbcReal;

pub const BACKEND_NAME: &str = "interp";

/// Returns the stable backend identifier (`"interp"`).
#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
