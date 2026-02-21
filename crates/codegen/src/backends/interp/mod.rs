//! Interpreter (`interp`) backend — FBC bytecode types and execution engine.
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
//! **Step 2 complete**: execution engine (`FbcExecutor`) with full dispatch loop
//! for all 294 opcodes, matching C++ `FBCInterpreter<REAL, TRACE>::executeBlock`.
//! Built on top of the Step 1 foundation (opcode enum, instruction/block types,
//! block arena, `FbcReal` trait).
//!
//! # Module layout
//! - [`opcode`]: `FbcOpcode` enum, `FBC_INSTRUCTION_NAMES`, `INTERP_FILE_VERSION`.
//! - [`bytecode`]: `FbcInstruction`, `FbcBlock`, `FbcBlockArena`, `BlockId`,
//!   `FbcUiInstruction`, `FbcMetaInstruction`.
//! - [`real`]: `FbcReal` trait with f32/f64 implementations.
//! - [`executor`]: `FbcExecutor` — bytecode execution engine with audio I/O.

pub mod bytecode;
pub mod executor;
pub mod opcode;
pub mod real;

// Re-exports for convenient access.
pub use bytecode::{
    BlockId, BlockStoreData, FbcBlock, FbcBlockArena, FbcInstruction, FbcMetaInstruction,
    FbcUiInstruction,
};
pub use executor::FbcExecutor;
pub use opcode::{FBC_INSTRUCTION_NAMES, FBC_OPCODE_COUNT, FbcOpcode, INTERP_FILE_VERSION};
pub use real::FbcReal;

pub const BACKEND_NAME: &str = "interp";

/// Returns the stable backend identifier (`"interp"`).
#[must_use]
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}
