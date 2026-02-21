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
//! **Step 3 complete**: FIR → FBC compiler (`FirToFbcCompiler`) translating FIR
//! nodes into FBC bytecode blocks, with end-to-end compilation + execution tests.
//! Built on the Step 1 foundation (opcode enum, instruction/block types, block
//! arena, `FbcReal` trait) and Step 2 execution engine (`FbcExecutor`).
//!
//! # Module layout
//! - [`opcode`]: `FbcOpcode` enum, `FBC_INSTRUCTION_NAMES`, `INTERP_FILE_VERSION`.
//! - [`bytecode`]: `FbcInstruction`, `FbcBlock`, `FbcBlockArena`, `BlockId`,
//!   `FbcUiInstruction`, `FbcMetaInstruction`.
//! - [`real`]: `FbcReal` trait with f32/f64 implementations.
//! - [`compiler`]: `FirToFbcCompiler` — FIR → FBC bytecode compiler.
//! - [`executor`]: `FbcExecutor` — bytecode execution engine with audio I/O.

pub mod bytecode;
pub mod compiler;
pub mod executor;
pub mod opcode;
pub mod real;

// Re-exports for convenient access.
pub use bytecode::{
    BlockId, BlockStoreData, FbcBlock, FbcBlockArena, FbcInstruction, FbcMetaInstruction,
    FbcUiInstruction,
};
pub use compiler::{CompileError, FbcCompileResult, FirToFbcCompiler, HeapType, MemoryDesc};
pub use executor::FbcExecutor;
pub use opcode::{FBC_INSTRUCTION_NAMES, FBC_OPCODE_COUNT, FbcOpcode, INTERP_FILE_VERSION};
pub use real::FbcReal;

pub const BACKEND_NAME: &str = "interp";

/// Returns the stable backend identifier (`"interp"`).
#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
