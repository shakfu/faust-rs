//! WASM linear-memory layout descriptors.
//!
//! Current scope is the Step-1 scaffold: the layout records the reserved
//! memory envelope for the emitted module skeleton, while real FIR field/table
//! placement is deferred to the dedicated layout-engine step from the porting
//! plan.

use std::collections::BTreeMap;

/// Scalar value kinds used by the public WASM layout descriptor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WasmValType {
    I32,
    F32,
    F64,
}

/// One field offset/size entry inside the linear-memory layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldLayout {
    pub offset: u32,
    pub typ: WasmValType,
    pub size: u32,
}

/// Maps FIR struct globals to WASM linear-memory offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WasmMemoryLayout {
    /// Byte offset for each struct field, keyed by FIR variable name.
    pub field_offsets: BTreeMap<String, FieldLayout>,
    /// Total DSP struct size in bytes.
    pub struct_size: u32,
    /// Offset where static tables begin.
    pub tables_offset: u32,
    /// Offset where I/O zone begins.
    pub io_zone_offset: u32,
    /// Total memory required in bytes.
    pub total_bytes: u32,
    /// WASM pages required (ceil(total_bytes / 65536)).
    pub pages: u32,
}

impl WasmMemoryLayout {
    /// Creates the placeholder layout used by the current WASM scaffold.
    #[must_use]
    pub fn scaffold(pages: u32, total_bytes: u32) -> Self {
        Self {
            field_offsets: BTreeMap::new(),
            struct_size: 0,
            tables_offset: 0,
            io_zone_offset: 0,
            total_bytes,
            pages,
        }
    }
}
