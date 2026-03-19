//! `cranelift` backend (early bring-up).
//!
//! # Role
//! - Planned native-code backend lowering Faust FIR to machine code via
//!   Cranelift, with a companion `cranelift_dsp` C/C++ export layer.
//!
//! # C++ provenance note
//! - There is no direct C++ `cranelift` backend in upstream Faust.
//! - This backend is a Rust-native extension and follows parity requirements
//!   documented in `porting/cranelift-backend-plan-en.md` for exported runtime
//!   behavior (`llvm_dsp` / `interpreter_dsp`-style API strategy).
//!
//! # Current status
//! - Early backend bring-up with a real Cranelift JIT integration:
//!   - a finalized `compute` symbol is emitted,
//!   - finalized code is kept alive by an owned `JITModule`,
//!   - a backend `dsp*` layout contract is derived from FIR `globals`.
//! - FIR `compute` body lowering is implemented incrementally through a
//!   supported subset (loops, arithmetic, selected control flow, part of math
//!   intrinsics, struct globals/tables, etc.).
//! - When the FIR body exceeds the current subset, the backend deliberately
//!   falls back to a valid no-op `compute` stub instead of failing the whole
//!   compilation.
//!
//! # Design notes (current phase)
//! - The backend prioritizes compile-path integration and diagnosability over
//!   runtime parity completeness.
//! - `FAUSTFLOAT` is currently mapped to `f32` in the Cranelift lowering path.
//! - The exported FFI/runtime layer (`cranelift_dsp`) can consume diagnostic
//!   metadata such as whether `compute` was really lowered or stubbed.

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{AbiParam, FuncRef, InstBuilder, MemFlags, Type, Value, types};
use cranelift_codegen::settings;
use cranelift_codegen::settings::Configurable;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{DataDescription, DataId, Init, Linkage, Module, default_libcall_names};
use fir::{AccessType, FirBinOp, FirId, FirMatch, FirStore, FirType, match_fir};
use std::collections::HashMap;

/// Stable backend identifier used by tooling and future CLI wiring.
pub const BACKEND_NAME: &str = "cranelift";

#[must_use]
/// Returns the stable backend identifier (`"cranelift"`).
pub fn backend_id() -> &'static str {
    BACKEND_NAME
}

/// Cranelift optimization level (backend-local configuration surface).
///
/// API mapping status: `adapted` (no direct C++ Cranelift backend exists).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CraneliftOptLevel {
    /// Fastest compile time.
    None,
    /// Balanced mode (default planned mode for validation bring-up).
    #[default]
    Speed,
    /// Highest optimization effort.
    SpeedAndSize,
}

/// Options controlling Cranelift backend compilation (scaffold).
///
/// This mirrors the shape planned in `porting/cranelift-backend-plan-en.md`,
/// but no codegen semantics are implemented yet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CraneliftOptions {
    /// Optimization level requested for Cranelift.
    pub opt_level: CraneliftOptLevel,
    /// Optional explicit target triple (string form for portability at the
    /// facade boundary; parsed later by the backend implementation).
    pub target_triple: Option<String>,
    /// Enable deterministic NaN canonicalization when supported.
    pub enable_nan_canonicalization: bool,
    /// Emit backend debug IR dumps once implemented.
    pub debug_ir_dump: bool,
    /// Fails compilation when the `compute` body falls outside the current
    /// lowering subset instead of emitting the no-op fallback stub.
    ///
    /// This is intended for strict validation/CI workflows to prevent silent
    /// runtime acceptance with reduced behavior.
    pub fail_on_subset_gap: bool,
}

/// Stable error codes for the Cranelift backend scaffold and future lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CraneliftBackendErrorCode {
    /// Backend is scaffolded but not implemented yet.
    NotImplemented,
    /// FIR root is not a module or has an unexpected shape.
    UnsupportedModuleShape,
    /// FIR module does not contain a supported `compute` declaration.
    MissingCompute,
    /// Failed to initialize or use the Cranelift JIT toolchain.
    JitFailure,
}

impl CraneliftBackendErrorCode {
    /// Returns the stable machine-readable error code string.
    fn as_str(self) -> &'static str {
        match self {
            Self::NotImplemented => "FRS-CGEN-CLIF-0001",
            Self::UnsupportedModuleShape => "FRS-CGEN-CLIF-0002",
            Self::MissingCompute => "FRS-CGEN-CLIF-0003",
            Self::JitFailure => "FRS-CGEN-CLIF-0004",
        }
    }
}

/// Typed Cranelift backend error used by the Cranelift codegen entry points.
///
/// This is the stable Rust-facing error container returned by
/// [`generate_cranelift_module`] and related helpers. It carries:
/// - a stable machine-readable code ([`CraneliftBackendErrorCode`]),
/// - a human-readable message suitable for diagnostics/logging.
///
/// # Stability notes
/// - `code` is intended to remain stable for tooling/tests.
/// - `message` may evolve as lowering coverage and diagnostics improve.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CraneliftBackendError {
    /// Machine-readable stable backend error code.
    pub code: CraneliftBackendErrorCode,
    /// Human-readable message.
    pub message: String,
}

impl CraneliftBackendError {
    /// Builds an `UnsupportedModuleShape` backend error.
    fn unsupported_module_shape(message: impl Into<String>) -> Self {
        Self {
            code: CraneliftBackendErrorCode::UnsupportedModuleShape,
            message: message.into(),
        }
    }

    /// Builds a `MissingCompute` backend error.
    fn missing_compute(message: impl Into<String>) -> Self {
        Self {
            code: CraneliftBackendErrorCode::MissingCompute,
            message: message.into(),
        }
    }

    /// Builds a `JitFailure` backend error.
    fn jit_failure(message: impl Into<String>) -> Self {
        Self {
            code: CraneliftBackendErrorCode::JitFailure,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CraneliftBackendError {
    /// Formats the typed error as `[CODE] message`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for CraneliftBackendError {}

/// Compiled JIT module handle for the Cranelift backend bring-up path.
///
/// Current contents:
/// - owned Cranelift JIT module (keeps finalized code alive),
/// - finalized `compute` symbol address (opaque integer),
/// - module/function names for debug/test assertions.
/// - `compute` subset-lowering status,
/// - backend-derived `dsp*` memory layout contract.
///
/// # Ownership / lifetime
/// The finalized machine code remains valid only while this value is alive,
/// because the underlying [`JITModule`] owns the emitted code memory and import
/// bookkeeping. Dropping [`JitDspModule`] invalidates `compute_entry_addr()`.
///
/// # Safety note
/// This type intentionally exposes the finalized entry address as `usize`
/// instead of a typed function pointer because invoking the code would require
/// `unsafe`, which is deferred to the future runtime/FFI layers.
///
/// API mapping status: `adapted`.
pub struct JitDspModule {
    module_name: String,
    compute_symbol_name: String,
    compute_entry_addr: usize,
    compute_body_lowered: bool,
    generated_functions_clif: Vec<(String, String)>,
    struct_layout: StructLayoutPlan,
    jit_module: JITModule,
}

impl std::fmt::Debug for JitDspModule {
    /// Renders a compact debug view that avoids dumping the owned `JITModule`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JitDspModule")
            .field("module_name", &self.module_name)
            .field("compute_symbol_name", &self.compute_symbol_name)
            .field("compute_entry_addr", &self.compute_entry_addr)
            .field(
                "generated_functions_clif_count",
                &self.generated_functions_clif.len(),
            )
            .finish()
    }
}

impl JitDspModule {
    /// Returns the FIR module name captured from the FIR `Module` node.
    ///
    /// This is useful for logging/debugging and for predictable symbol naming
    /// assertions in tests.
    #[must_use]
    pub fn module_name(&self) -> &str {
        &self.module_name
    }

    /// Returns the finalized Cranelift symbol name used for `compute`.
    ///
    /// The name is exported inside the Cranelift JIT module and is currently
    /// derived as `"{module_name}::compute"`.
    #[must_use]
    pub fn compute_symbol_name(&self) -> &str {
        &self.compute_symbol_name
    }

    /// Returns the finalized `compute` entry address as an opaque integer.
    ///
    /// The early bring-up path intentionally avoids exposing/calling a typed
    /// function pointer here because workspace lints forbid `unsafe`.
    #[must_use]
    pub fn compute_entry_addr(&self) -> usize {
        self.compute_entry_addr
    }

    /// Returns `true` when a finalized non-null `compute` symbol address exists.
    ///
    /// This is a cheap postcondition check for callers that only need to know
    /// whether JIT finalization produced an address, without inspecting the
    /// lowering mode (`real subset lowering` vs `stub fallback`).
    #[must_use]
    pub fn has_compute_entry(&self) -> bool {
        self.compute_entry_addr != 0
    }

    /// Returns true when the current subset lowering handled the FIR `compute`
    /// body. False means the backend emitted the no-op stub fallback.
    #[must_use]
    pub fn compute_body_lowered(&self) -> bool {
        self.compute_body_lowered
    }

    /// Returns textual CLIF IR generated for all backend-defined DSP functions.
    ///
    /// Each entry is `(function_symbol_name, clif_text)`.
    /// In the current backend state this contains at least `compute`, and it is
    /// designed to grow as more DSP lifecycle functions are lowered.
    #[must_use]
    pub fn generated_functions_clif(&self) -> &[(String, String)] {
        &self.generated_functions_clif
    }

    /// Returns the backend `dsp*` struct layout contract derived from FIR
    /// `globals` declarations.
    ///
    /// The returned layout is deterministic for a given FIR module and current
    /// backend contract (including the current `FAUSTFLOAT -> f32` decision).
    /// It is used by lowering of `AccessType::Struct` loads/stores and is also
    /// intended to be reused by future runtime allocation paths.
    #[must_use]
    pub fn struct_layout(&self) -> &StructLayoutPlan {
        &self.struct_layout
    }

    /// Internal guard used by tests to ensure the JIT module stays owned/alive.
    ///
    /// This method intentionally touches the private `jit_module` field so test
    /// code can assert the ownership path without exposing the Cranelift type
    /// itself in the public API.
    #[must_use]
    pub fn jit_module_is_alive(&self) -> bool {
        let _ = &self.jit_module;
        true
    }
}

/// Deterministic layout of the backend `dsp*` instance state for Cranelift.
///
/// Contract (current V1-oriented backend contract):
/// - Derived from FIR module `globals` block in declaration order.
/// - Includes `DeclareVar` with `AccessType::Struct`.
/// - `FAUSTFLOAT` is laid out as `f32` during the current bring-up.
/// - Natural alignment is applied per field (backend-target pointer size aware).
/// - Offsets are byte offsets relative to the `dsp*` base pointer.
///
/// This contract is backend-internal for now, but is designed to be stable and
/// reusable by the future `cranelift_dsp` instance allocation/runtime path.
#[derive(Clone, Debug, PartialEq)]
pub struct StructLayoutPlan {
    fields: Vec<StructFieldLayout>,
    size_bytes: u32,
    align_bytes: u32,
}

impl StructLayoutPlan {
    /// Returns all fields in declaration/layout order.
    ///
    /// Order is significant: offsets are assigned by iterating FIR `globals`
    /// in order and applying alignment. Callers should not assume name sorting.
    #[must_use]
    pub fn fields(&self) -> &[StructFieldLayout] {
        &self.fields
    }

    /// Returns the total struct size in bytes, including final padding.
    ///
    /// The size is rounded up to `align_bytes()`.
    #[must_use]
    pub fn size_bytes(&self) -> u32 {
        self.size_bytes
    }

    /// Returns the required alignment of the full `dsp*` state struct in bytes.
    ///
    /// This is currently the maximum alignment of all included fields.
    #[must_use]
    pub fn align_bytes(&self) -> u32 {
        self.align_bytes
    }

    /// Looks up a field by FIR/global name.
    ///
    /// Returns `None` when the name is not part of the backend layout (for
    /// example helper prototypes in `globals`, which are intentionally ignored).
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&StructFieldLayout> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// One field in the Cranelift backend `dsp*` struct layout.
#[derive(Clone, Debug, PartialEq)]
pub struct StructFieldLayout {
    /// FIR/global variable name used as the layout key.
    pub name: String,
    /// Storage shape in the backend contract (scalar or inline table).
    pub kind: StructFieldKind,
    /// Byte offset relative to the `dsp*` base pointer.
    pub offset_bytes: u32,
    /// Field storage size in bytes (table payload size for tables).
    pub size_bytes: u32,
    /// Field alignment in bytes.
    pub align_bytes: u32,
}

/// Backend `dsp*` field storage kind in the Cranelift layout contract.
#[derive(Clone, Debug, PartialEq)]
pub enum StructFieldKind {
    /// Single scalar value stored inline.
    Scalar(FirType),
    /// Inline array/table payload stored inside the `dsp*` allocation.
    ///
    /// `len` is the number of elements (not bytes).
    Table { elem_type: FirType, len: u32 },
}

impl StructFieldLayout {
    /// Returns the scalar FIR type when this field is [`StructFieldKind::Scalar`].
    ///
    /// This is a convenience helper for callers that only care about scalar
    /// state and want to skip manual enum matching.
    #[must_use]
    pub fn scalar_type(&self) -> Option<&FirType> {
        match &self.kind {
            StructFieldKind::Scalar(t) => Some(t),
            StructFieldKind::Table { .. } => None,
        }
    }
}

/// Concrete size/alignment pair for one scalar storage type in the layout plan.
/// Concrete size/alignment pair for one scalar storage type in the layout plan.
///
/// This is internal because only the finalized [`StructLayoutPlan`] is part of
/// the backend contract exposed to callers.
#[derive(Clone, Copy, Debug)]
struct LayoutScalar {
    size: u32,
    align: u32,
}

/// Rounds `value` up to the next multiple of `align`.
///
/// `align <= 1` is treated as already aligned.
fn align_up(value: u32, align: u32) -> u32 {
    if align <= 1 {
        return value;
    }
    let rem = value % align;
    if rem == 0 {
        value
    } else {
        value + (align - rem)
    }
}

/// Finds the FIR module name and the concrete `compute` declaration to lower.
///
/// # Expected FIR shape
/// - root node: `Module`
/// - `functions`: `Block([...])`
/// - a `DeclareFun { name: "compute", body: Some(..) }` entry exists
///
/// Prototype-only `compute` declarations (`body: None`) are ignored because the
/// Cranelift backend currently requires an executable body.
fn find_module_and_compute(
    store: &FirStore,
    module: FirId,
) -> Result<(String, FirId), CraneliftBackendError> {
    let (module_name, _globals, functions) = match match_fir(store, module) {
        FirMatch::Module {
            name,
            globals,
            functions,
            ..
        } => (name, globals, functions),
        other => {
            return Err(CraneliftBackendError::unsupported_module_shape(format!(
                "expected FIR Module root, got {other:?} at {}",
                module.as_u32()
            )));
        }
    };

    let function_nodes = match match_fir(store, functions) {
        FirMatch::Block(items) => items,
        other => {
            return Err(CraneliftBackendError::unsupported_module_shape(format!(
                "module functions must be FIR Block, got {other:?} at {}",
                functions.as_u32()
            )));
        }
    };

    let compute_id = function_nodes
        .into_iter()
        .find(|id| {
            matches!(
                match_fir(store, *id),
                FirMatch::DeclareFun {
                    ref name,
                    body: Some(_),
                    ..
                } if name == "compute"
            )
        })
        .ok_or_else(|| {
            CraneliftBackendError::missing_compute(format!(
                "FIR module `{module_name}` has no supported `compute` definition"
            ))
        })?;

    Ok((module_name, compute_id))
}

/// Maps a FIR scalar/storage type to the backend `dsp*` layout scalar size/alignment.
///
/// This helper is used only while deriving the backend `StructLayoutPlan` from
/// FIR `globals`. It intentionally reflects the current bring-up contract
/// (notably `FAUSTFLOAT -> f32`).
///
/// Unsupported FIR types here are rejected as module-shape issues because they
/// make the current backend state layout contract undefined.
fn fir_type_layout_scalar(
    ptr_size: u32,
    typ: &FirType,
) -> Result<LayoutScalar, CraneliftBackendError> {
    let s = match typ {
        FirType::Bool => LayoutScalar { size: 1, align: 1 },
        FirType::Int32 => LayoutScalar { size: 4, align: 4 },
        FirType::Float32 | FirType::FaustFloat => LayoutScalar { size: 4, align: 4 },
        FirType::Int64 | FirType::Float64 => LayoutScalar { size: 8, align: 8 },
        FirType::Ptr(_) | FirType::Obj | FirType::UI | FirType::Meta | FirType::Sound => {
            LayoutScalar {
                size: ptr_size,
                align: ptr_size,
            }
        }
        other => {
            return Err(CraneliftBackendError::unsupported_module_shape(format!(
                "unsupported Cranelift dsp* layout field type in V1 bring-up contract: {other:?}"
            )));
        }
    };
    Ok(s)
}

/// Builds the deterministic backend `dsp*` state layout from FIR module globals.
///
/// # Inclusion rules (current contract)
/// - includes `DeclareVar { access: Struct, .. }` as scalar fields
/// - includes `DeclareVar { access: Struct, typ: Array(..), .. }` as inline
///   array payload fields
/// - includes `DeclareTable { access: Struct, .. }` as inline table fields
/// - ignores helper function prototypes (`DeclareFun { body: None, .. }`)
/// - ignores helper declarations/prototypes that are not `dsp*` state
///
/// # Rejection policy
/// Any other global entry shape (for example unsupported access classes or
/// unsupported FIR types) is rejected with `UnsupportedModuleShape`, because
/// lowering `AccessType::Struct` depends on a total, unambiguous layout.
fn build_struct_layout_for_module(
    store: &FirStore,
    module: FirId,
    ptr_size: u32,
) -> Result<StructLayoutPlan, CraneliftBackendError> {
    let (dsp_struct, globals) = match match_fir(store, module) {
        FirMatch::Module {
            dsp_struct,
            globals,
            ..
        } => (dsp_struct, globals),
        other => {
            return Err(CraneliftBackendError::unsupported_module_shape(format!(
                "expected FIR Module root for struct layout, got {other:?} at {}",
                module.as_u32()
            )));
        }
    };
    let dsp_struct_items = match match_fir(store, dsp_struct) {
        FirMatch::Block(items) => items,
        other => {
            return Err(CraneliftBackendError::unsupported_module_shape(format!(
                "module dsp_struct must be FIR Block, got {other:?} at {}",
                dsp_struct.as_u32()
            )));
        }
    };
    let global_items = match match_fir(store, globals) {
        FirMatch::Block(items) => items,
        other => {
            return Err(CraneliftBackendError::unsupported_module_shape(format!(
                "module globals must be FIR Block, got {other:?} at {}",
                globals.as_u32()
            )));
        }
    };

    let mut fields = Vec::new();
    let mut offset = 0u32;
    let mut struct_align = 1u32;
    for item in dsp_struct_items.into_iter().chain(global_items.into_iter()) {
        match match_fir(store, item) {
            FirMatch::DeclareVar {
                name,
                typ,
                access: AccessType::Struct,
                ..
            } => match typ {
                FirType::Array(inner, len) => {
                    let elem_type = *inner;
                    let scalar = fir_type_layout_scalar(ptr_size, &elem_type)?;
                    let len = u32::try_from(len).map_err(|_| {
                        CraneliftBackendError::unsupported_module_shape(
                            "Cranelift dsp* array field length does not fit in u32",
                        )
                    })?;
                    let size = scalar.size.checked_mul(len).ok_or_else(|| {
                        CraneliftBackendError::unsupported_module_shape(
                            "Cranelift dsp* array field size overflow",
                        )
                    })?;
                    offset = align_up(offset, scalar.align);
                    fields.push(StructFieldLayout {
                        name,
                        kind: StructFieldKind::Table { elem_type, len },
                        offset_bytes: offset,
                        size_bytes: size,
                        align_bytes: scalar.align,
                    });
                    offset = offset.checked_add(size).ok_or_else(|| {
                        CraneliftBackendError::unsupported_module_shape(
                            "Cranelift dsp* layout size overflow",
                        )
                    })?;
                    struct_align = struct_align.max(scalar.align);
                }
                scalar_ty => {
                    let scalar = fir_type_layout_scalar(ptr_size, &scalar_ty)?;
                    offset = align_up(offset, scalar.align);
                    fields.push(StructFieldLayout {
                        name,
                        kind: StructFieldKind::Scalar(scalar_ty),
                        offset_bytes: offset,
                        size_bytes: scalar.size,
                        align_bytes: scalar.align,
                    });
                    offset = offset.checked_add(scalar.size).ok_or_else(|| {
                        CraneliftBackendError::unsupported_module_shape(
                            "Cranelift dsp* layout size overflow",
                        )
                    })?;
                    struct_align = struct_align.max(scalar.align);
                }
            },
            FirMatch::DeclareVar { access, name, .. } => {
                return Err(CraneliftBackendError::unsupported_module_shape(format!(
                    "unsupported global variable access class for Cranelift dsp* layout: {name} ({access:?})"
                )));
            }
            FirMatch::DeclareTable {
                name,
                access: AccessType::Struct,
                elem_type,
                values,
            } => {
                let scalar = fir_type_layout_scalar(ptr_size, &elem_type)?;
                let len = u32::try_from(values.len()).map_err(|_| {
                    CraneliftBackendError::unsupported_module_shape(
                        "Cranelift dsp* table length does not fit in u32",
                    )
                })?;
                let size = scalar.size.checked_mul(len).ok_or_else(|| {
                    CraneliftBackendError::unsupported_module_shape(
                        "Cranelift dsp* table size overflow",
                    )
                })?;
                offset = align_up(offset, scalar.align);
                fields.push(StructFieldLayout {
                    name,
                    kind: StructFieldKind::Table { elem_type, len },
                    offset_bytes: offset,
                    size_bytes: size,
                    align_bytes: scalar.align,
                });
                offset = offset.checked_add(size).ok_or_else(|| {
                    CraneliftBackendError::unsupported_module_shape(
                        "Cranelift dsp* layout size overflow",
                    )
                })?;
                struct_align = struct_align.max(scalar.align);
            }
            FirMatch::DeclareTable {
                access: AccessType::Static | AccessType::Global,
                ..
            } => {
                // File-scope constant tables — not part of the per-instance dsp* struct.
                // They are handled separately by `define_static_tables_in_jit`.
            }
            FirMatch::DeclareTable { access, name, .. } => {
                return Err(CraneliftBackendError::unsupported_module_shape(format!(
                    "unsupported global table access class for Cranelift dsp* layout: {name} ({access:?})"
                )));
            }
            // Fast-lane FIR may place helper math prototypes (`fmin`, `pow`, ...)
            // in module globals. They are declarations, not `dsp*` state fields.
            FirMatch::DeclareFun { body: None, .. } => {}
            other => {
                return Err(CraneliftBackendError::unsupported_module_shape(format!(
                    "unsupported globals entry for Cranelift dsp* layout: {other:?}"
                )));
            }
        }
    }
    let size_bytes = align_up(offset, struct_align);
    Ok(StructLayoutPlan {
        fields,
        size_bytes,
        align_bytes: struct_align,
    })
}

/// Creates and configures a Cranelift JIT builder for the host machine.
///
/// # Current policy
/// - Uses the host ISA via `cranelift_native`.
/// - Applies backend options such as optimization level.
/// - Disables a few relocation/libcall assumptions (`is_pic`,
///   `use_colocated_libcalls`) to simplify early cross-platform bring-up.
/// - Registers default libcall names (Cranelift helper convention).
///
/// Host math symbols used by FIR math lowering are registered later by
/// [`register_host_symbols`].
fn make_jit_builder(options: &CraneliftOptions) -> Result<JITBuilder, CraneliftBackendError> {
    let mut builder = settings::builder();
    let opt_level = match options.opt_level {
        CraneliftOptLevel::None => "none",
        CraneliftOptLevel::Speed => "speed",
        CraneliftOptLevel::SpeedAndSize => "speed_and_size",
    };
    builder.set("opt_level", opt_level).map_err(|e| {
        CraneliftBackendError::jit_failure(format!("invalid Cranelift opt_level: {e}"))
    })?;
    builder.set("is_pic", "false").map_err(|e| {
        CraneliftBackendError::jit_failure(format!("invalid Cranelift is_pic flag: {e}"))
    })?;
    builder
        .set("use_colocated_libcalls", "false")
        .map_err(|e| {
            CraneliftBackendError::jit_failure(format!(
                "invalid Cranelift use_colocated_libcalls flag: {e}"
            ))
        })?;
    if options.enable_nan_canonicalization {
        let _ = builder.set("enable_nan_canonicalization", "true");
    }
    let isa_builder = cranelift_native::builder().map_err(|msg| {
        CraneliftBackendError::jit_failure(format!("host machine is not supported: {msg}"))
    })?;
    let isa = isa_builder
        .finish(settings::Flags::new(builder))
        .map_err(|e| CraneliftBackendError::jit_failure(format!("native ISA init failed: {e}")))?;
    Ok(JITBuilder::with_isa(isa, default_libcall_names()))
}

extern "C" fn host_sinf(x: f32) -> f32 {
    x.sin()
}

extern "C" fn host_sin(x: f64) -> f64 {
    x.sin()
}

extern "C" fn host_cosf(x: f32) -> f32 {
    x.cos()
}

extern "C" fn host_cos(x: f64) -> f64 {
    x.cos()
}

extern "C" fn host_expf(x: f32) -> f32 {
    x.exp()
}

extern "C" fn host_exp(x: f64) -> f64 {
    x.exp()
}

extern "C" fn host_logf(x: f32) -> f32 {
    x.ln()
}

extern "C" fn host_log(x: f64) -> f64 {
    x.ln()
}

extern "C" fn host_log10f(x: f32) -> f32 {
    x.log10()
}

extern "C" fn host_log10(x: f64) -> f64 {
    x.log10()
}

extern "C" fn host_sqrtf(x: f32) -> f32 {
    x.sqrt()
}

extern "C" fn host_sqrt(x: f64) -> f64 {
    x.sqrt()
}

extern "C" fn host_fabsf(x: f32) -> f32 {
    x.abs()
}

extern "C" fn host_fabs(x: f64) -> f64 {
    x.abs()
}

extern "C" fn host_abs(a: i32) -> i32 {
    a.checked_abs().unwrap_or(a)
}

extern "C" fn host_min_i(a: i32, b: i32) -> i32 {
    a.min(b)
}

extern "C" fn host_max_i(a: i32, b: i32) -> i32 {
    a.max(b)
}

extern "C" fn host_floorf(x: f32) -> f32 {
    x.floor()
}

extern "C" fn host_floor(x: f64) -> f64 {
    x.floor()
}

extern "C" fn host_ceilf(x: f32) -> f32 {
    x.ceil()
}

extern "C" fn host_ceil(x: f64) -> f64 {
    x.ceil()
}

extern "C" fn host_tanf(x: f32) -> f32 {
    x.tan()
}

extern "C" fn host_tan(x: f64) -> f64 {
    x.tan()
}

extern "C" fn host_atanf(x: f32) -> f32 {
    x.atan()
}

extern "C" fn host_atan(x: f64) -> f64 {
    x.atan()
}

extern "C" fn host_asinf(x: f32) -> f32 {
    x.asin()
}

extern "C" fn host_asin(x: f64) -> f64 {
    x.asin()
}

extern "C" fn host_acosf(x: f32) -> f32 {
    x.acos()
}

extern "C" fn host_acos(x: f64) -> f64 {
    x.acos()
}

extern "C" fn host_roundf(x: f32) -> f32 {
    x.round()
}

extern "C" fn host_round(x: f64) -> f64 {
    x.round()
}

extern "C" fn host_fminf(a: f32, b: f32) -> f32 {
    a.min(b)
}

extern "C" fn host_fmin(a: f64, b: f64) -> f64 {
    a.min(b)
}

extern "C" fn host_fmaxf(a: f32, b: f32) -> f32 {
    a.max(b)
}

extern "C" fn host_fmax(a: f64, b: f64) -> f64 {
    a.max(b)
}

extern "C" fn host_powf(a: f32, b: f32) -> f32 {
    a.powf(b)
}

extern "C" fn host_pow(a: f64, b: f64) -> f64 {
    a.powf(b)
}

extern "C" fn host_atan2f(a: f32, b: f32) -> f32 {
    a.atan2(b)
}

extern "C" fn host_atan2(a: f64, b: f64) -> f64 {
    a.atan2(b)
}

extern "C" fn host_fmodf(a: f32, b: f32) -> f32 {
    a % b
}

extern "C" fn host_fmod(a: f64, b: f64) -> f64 {
    a % b
}

extern "C" fn host_rintf(a: f32) -> f32 {
    a.round_ties_even()
}

extern "C" fn host_rint(a: f64) -> f64 {
    a.round_ties_even()
}

extern "C" fn host_remainderf(a: f32, b: f32) -> f32 {
    a - (a / b).round_ties_even() * b
}

extern "C" fn host_remainder(a: f64, b: f64) -> f64 {
    a - (a / b).round_ties_even() * b
}

extern "C" fn host_isnanf(x: f32) -> i32 {
    i32::from(x.is_nan())
}

extern "C" fn host_isnan(x: f64) -> i32 {
    i32::from(x.is_nan())
}

extern "C" fn host_isinff(x: f32) -> i32 {
    i32::from(x.is_infinite())
}

extern "C" fn host_isinf(x: f64) -> i32 {
    i32::from(x.is_infinite())
}

extern "C" fn host_copysignf(a: f32, b: f32) -> f32 {
    a.copysign(b)
}

extern "C" fn host_copysign(a: f64, b: f64) -> f64 {
    a.copysign(b)
}

extern "C" fn host_acoshf(x: f32) -> f32 {
    x.acosh()
}

extern "C" fn host_acosh(x: f64) -> f64 {
    x.acosh()
}

extern "C" fn host_asinhf(x: f32) -> f32 {
    x.asinh()
}

extern "C" fn host_asinh(x: f64) -> f64 {
    x.asinh()
}

extern "C" fn host_atanhf(x: f32) -> f32 {
    x.atanh()
}

extern "C" fn host_atanh(x: f64) -> f64 {
    x.atanh()
}

extern "C" fn host_coshf(x: f32) -> f32 {
    x.cosh()
}

extern "C" fn host_cosh(x: f64) -> f64 {
    x.cosh()
}

extern "C" fn host_sinhf(x: f32) -> f32 {
    x.sinh()
}

extern "C" fn host_sinh(x: f64) -> f64 {
    x.sinh()
}

extern "C" fn host_tanhf(x: f32) -> f32 {
    x.tanh()
}

extern "C" fn host_tanh(x: f64) -> f64 {
    x.tanh()
}

/// Registers Rust host math functions as JIT-importable symbols.
///
/// The Cranelift lowering emits imported calls for many FIR math operations
/// (`sin`, `pow`, `fmin`, etc.). This function binds those symbol names to Rust
/// implementations so the JIT can resolve them during finalization.
///
/// Both `f32` (`*f`) and `f64` symbol variants are registered where the subset
/// lowering supports both result types. The `host_*` helpers immediately above
/// are intentionally tiny wrappers whose semantics match the interpreter/C++
/// path (notably `rint` and `remainder`).
fn register_host_symbols(jit_builder: &mut JITBuilder) {
    jit_builder.symbol("sinf", host_sinf as *const u8);
    jit_builder.symbol("sin", host_sin as *const u8);
    jit_builder.symbol("cosf", host_cosf as *const u8);
    jit_builder.symbol("cos", host_cos as *const u8);
    jit_builder.symbol("expf", host_expf as *const u8);
    jit_builder.symbol("exp", host_exp as *const u8);
    jit_builder.symbol("logf", host_logf as *const u8);
    jit_builder.symbol("log", host_log as *const u8);
    jit_builder.symbol("log10f", host_log10f as *const u8);
    jit_builder.symbol("log10", host_log10 as *const u8);
    jit_builder.symbol("sqrtf", host_sqrtf as *const u8);
    jit_builder.symbol("sqrt", host_sqrt as *const u8);
    jit_builder.symbol("fabsf", host_fabsf as *const u8);
    jit_builder.symbol("fabs", host_fabs as *const u8);
    jit_builder.symbol("abs", host_abs as *const u8);
    jit_builder.symbol("floorf", host_floorf as *const u8);
    jit_builder.symbol("floor", host_floor as *const u8);
    jit_builder.symbol("ceilf", host_ceilf as *const u8);
    jit_builder.symbol("ceil", host_ceil as *const u8);
    jit_builder.symbol("tanf", host_tanf as *const u8);
    jit_builder.symbol("tan", host_tan as *const u8);
    jit_builder.symbol("atanf", host_atanf as *const u8);
    jit_builder.symbol("atan", host_atan as *const u8);
    jit_builder.symbol("asinf", host_asinf as *const u8);
    jit_builder.symbol("asin", host_asin as *const u8);
    jit_builder.symbol("acosf", host_acosf as *const u8);
    jit_builder.symbol("acos", host_acos as *const u8);
    jit_builder.symbol("rintf", host_rintf as *const u8);
    jit_builder.symbol("rint", host_rint as *const u8);
    jit_builder.symbol("roundf", host_roundf as *const u8);
    jit_builder.symbol("round", host_round as *const u8);
    jit_builder.symbol("min_i", host_min_i as *const u8);
    jit_builder.symbol("max_i", host_max_i as *const u8);
    jit_builder.symbol("fminf", host_fminf as *const u8);
    jit_builder.symbol("fmin", host_fmin as *const u8);
    jit_builder.symbol("fmaxf", host_fmaxf as *const u8);
    jit_builder.symbol("fmax", host_fmax as *const u8);
    jit_builder.symbol("powf", host_powf as *const u8);
    jit_builder.symbol("pow", host_pow as *const u8);
    jit_builder.symbol("atan2f", host_atan2f as *const u8);
    jit_builder.symbol("atan2", host_atan2 as *const u8);
    jit_builder.symbol("fmodf", host_fmodf as *const u8);
    jit_builder.symbol("fmod", host_fmod as *const u8);
    jit_builder.symbol("remainderf", host_remainderf as *const u8);
    jit_builder.symbol("remainder", host_remainder as *const u8);
    jit_builder.symbol("isnanf", host_isnanf as *const u8);
    jit_builder.symbol("isnan", host_isnan as *const u8);
    jit_builder.symbol("isinff", host_isinff as *const u8);
    jit_builder.symbol("isinf", host_isinf as *const u8);
    jit_builder.symbol("copysignf", host_copysignf as *const u8);
    jit_builder.symbol("copysign", host_copysign as *const u8);
    jit_builder.symbol("acoshf", host_acoshf as *const u8);
    jit_builder.symbol("acosh", host_acosh as *const u8);
    jit_builder.symbol("asinhf", host_asinhf as *const u8);
    jit_builder.symbol("asinh", host_asinh as *const u8);
    jit_builder.symbol("atanhf", host_atanhf as *const u8);
    jit_builder.symbol("atanh", host_atanh as *const u8);
    jit_builder.symbol("coshf", host_coshf as *const u8);
    jit_builder.symbol("cosh", host_cosh as *const u8);
    jit_builder.symbol("sinhf", host_sinhf as *const u8);
    jit_builder.symbol("sinh", host_sinh as *const u8);
    jit_builder.symbol("tanhf", host_tanhf as *const u8);
    jit_builder.symbol("tanh", host_tanh as *const u8);
}

/// Lowered expression value tracked in the local Cranelift lowering environment.
///
/// FIR names in the current subset can denote either:
/// - scalar SSA values (ints/floats/bools), or
/// - pointer values (for stack aliases like `input0` / `output0`, fun args, etc.)
///
/// This enum preserves that distinction so statement lowering can reject invalid
/// uses early (for example writing a scalar as if it were a pointer table base).
#[derive(Clone, Copy, Debug)]
/// Lowered expression value tracked in the local Cranelift lowering environment.
///
/// FIR names in the current subset can denote either scalar SSA values or
/// pointer aliases. This enum preserves that distinction for later loads/stores.
enum LoweredExpr {
    Scalar(Value),
    Ptr {
        value: Value,
        pointee: Option<FirTypeRef>,
    },
}

impl LoweredExpr {
    /// Returns the underlying CLIF value regardless of scalar/pointer tagging.
    ///
    /// Use this only when the consumer already knows the semantic category.
    fn value(self) -> Value {
        match self {
            Self::Scalar(v) => v,
            Self::Ptr { value, .. } => value,
        }
    }

    /// Returns the pointer CLIF value when this expression represents a pointer.
    ///
    /// This is mainly used by table alias lowering on stack/function-argument
    /// pointers.
    fn ptr(self) -> Option<Value> {
        match self {
            Self::Ptr { value, .. } => Some(value),
            Self::Scalar(_) => None,
        }
    }

    /// Returns the tracked pointee category for pointer expressions.
    ///
    /// This metadata is backend-local and intentionally coarse (see
    /// [`FirTypeRef`]); it is sufficient for current alias/table lowering.
    fn pointee(self) -> Option<FirTypeRef> {
        match self {
            Self::Ptr { pointee, .. } => pointee,
            Self::Scalar(_) => None,
        }
    }
}

/// Lightweight FIR type classifier used in local lowering metadata.
///
/// This is intentionally smaller than [`FirType`] and mainly exists to annotate
/// pointer aliases (`LoweredExpr::Ptr`) with enough information to safely lower
/// `LoadTable`/`StoreTable` on stack aliases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Lightweight FIR type classifier used in local lowering metadata.
///
/// This reduced classifier is intentionally coarser than [`FirType`]; it only
/// carries the information needed by pointer alias lowering.
enum FirTypeRef {
    Bool,
    Int32,
    Int64,
    Float32,
    Float64,
    FaustFloat,
    Obj,
    UI,
    Meta,
    Sound,
    Ptr,
}

impl FirTypeRef {
    /// Converts a full FIR type to the reduced local classification.
    ///
    /// Unsupported/compound FIR types collapse to `Ptr` because the current
    /// pointer alias metadata only needs rough categories, not full fidelity.
    fn from_fir_type(typ: &FirType) -> Self {
        match typ {
            FirType::Bool => Self::Bool,
            FirType::Int32 => Self::Int32,
            FirType::Int64 => Self::Int64,
            FirType::Float32 => Self::Float32,
            FirType::Float64 => Self::Float64,
            FirType::FaustFloat => Self::FaustFloat,
            FirType::Obj => Self::Obj,
            FirType::UI => Self::UI,
            FirType::Meta => Self::Meta,
            FirType::Sound => Self::Sound,
            FirType::Ptr(_) => Self::Ptr,
            _ => Self::Ptr,
        }
    }
}

/// Internal lowering failure type used while emitting the Cranelift `compute` body.
///
/// `Unsupported` means "valid FIR, but outside current subset"; callers may
/// convert this into a stub fallback. `Jit` represents structural/codegen
/// failures that should surface as backend errors.
/// Internal lowering failure used while emitting one Cranelift function body.
#[derive(Debug)]
enum LoweringError {
    Unsupported(String),
    Jit(String),
}

/// Emits a valid no-op `return` for the current CLIF function.
///
/// This is the canonical stub body used by the early fallback policy.
fn emit_return_stub(fb: &mut FunctionBuilder<'_>) {
    fb.ins().return_(&[]);
}

/// Returns `true` when the current block already ends with a `return`.
///
/// Used to avoid emitting duplicate terminators after lowering control-flow
/// constructs that may already have returned.
fn is_return_terminated(fb: &FunctionBuilder<'_>) -> bool {
    let Some(block) = fb.current_block() else {
        return false;
    };
    let Some(inst) = fb.func.layout.last_inst(block) else {
        return false;
    };
    matches!(
        fb.func.dfg.insts[inst].opcode(),
        cranelift_codegen::ir::Opcode::Return
    )
}

/// Stateful FIR -> Cranelift lowering context for a single `compute` function.
///
/// This context owns:
/// - references to FIR storage and the active Cranelift JIT module,
/// - the current CLIF function builder,
/// - the backend `dsp*` layout contract for `AccessType::Struct`,
/// - a local name environment (`vars`) for FIR variables/aliases,
/// - cached imported function refs for math calls.
///
/// It is intentionally function-scoped: one instance per `compute` lowering.
/// Stateful FIR -> Cranelift lowering context for one `compute` function.
///
/// The context owns the CLIF builder-side environment, imported function cache,
/// and local FIR variable bindings required during subset lowering.
struct ComputeLowering<'a, 'b, 'c> {
    store: &'a FirStore,
    jit: &'a mut JITModule,
    fb: &'a mut FunctionBuilder<'b>,
    struct_layout: &'a StructLayoutPlan,
    ptr_ty: Type,
    vars: HashMap<String, LoweredExpr>,
    import_refs: HashMap<String, FuncRef>,
    /// Pre-declared JIT data IDs for `AccessType::Static` tables.
    static_data_ids: &'a HashMap<String, DataId>,
    _marker: std::marker::PhantomData<&'c ()>,
}

impl<'a, 'b, 'c> ComputeLowering<'a, 'b, 'c> {
    /// Converts a CLIF boolean (`b1`) to the backend FIR-bool representation (`i8` 0/1).
    fn bool_b1_to_i8(&mut self, b1: Value) -> Value {
        let one = self.fb.ins().iconst(types::I8, 1);
        let zero = self.fb.ins().iconst(types::I8, 0);
        self.fb.ins().select(b1, one, zero)
    }

    /// Emits an integer comparison and returns FIR-style boolean `i8` (0/1).
    fn int_cmp_to_i8(&mut self, cc: IntCC, lhs: Value, rhs: Value) -> Value {
        let b1 = self.fb.ins().icmp(cc, lhs, rhs);
        self.bool_b1_to_i8(b1)
    }

    /// Emits a floating-point comparison and returns FIR-style boolean `i8` (0/1).
    fn float_cmp_to_i8(
        &mut self,
        cc: cranelift_codegen::ir::condcodes::FloatCC,
        lhs: Value,
        rhs: Value,
    ) -> Value {
        let b1 = self.fb.ins().fcmp(cc, lhs, rhs);
        self.bool_b1_to_i8(b1)
    }

    /// Maps a FIR type to the corresponding Cranelift value type used by this backend.
    ///
    /// This reflects current bring-up decisions, especially `FAUSTFLOAT -> F32`.
    fn fir_type_to_clif(&self, typ: &FirType) -> Result<Type, LoweringError> {
        match typ {
            FirType::Int32 => Ok(types::I32),
            FirType::Int64 => Ok(types::I64),
            FirType::Float32 => Ok(types::F32),
            FirType::Float64 => Ok(types::F64),
            FirType::FaustFloat => Ok(types::F32),
            FirType::Bool => Ok(types::I8),
            FirType::Ptr(_) | FirType::Obj | FirType::UI | FirType::Meta | FirType::Sound => {
                Ok(self.ptr_ty)
            }
            other => Err(LoweringError::Unsupported(format!(
                "unsupported FIR type in Cranelift subset lowering: {other:?}"
            ))),
        }
    }

    /// Returns the CLIF `dsp*` base pointer argument from the local environment.
    fn dsp_base_ptr(&self) -> Result<Value, LoweringError> {
        self.vars.get("dsp").and_then(|v| v.ptr()).ok_or_else(|| {
            LoweringError::Unsupported("missing `dsp` base pointer argument".to_string())
        })
    }

    /// Looks up a named `dsp*` field in the backend struct layout contract.
    fn struct_field(&self, name: &str) -> Result<&StructFieldLayout, LoweringError> {
        self.struct_layout.field(name).ok_or_else(|| {
            LoweringError::Unsupported(format!(
                "struct field `{name}` not present in Cranelift dsp* layout contract"
            ))
        })
    }

    /// Coerces a CLIF value to the CLIF type expected by a target FIR type.
    ///
    /// This is a small, explicit conversion set used by current subset lowering
    /// (not a general FIR coercion engine). Unsupported conversions are surfaced
    /// as subset-lowering failures.
    ///
    /// Numeric FIR `Cast` nodes are inserted upstream by signal/FIR preparation,
    /// for example when integer expressions feed float arithmetic or when float
    /// indices are truncated before table access. The subset matcher already
    /// accepts those `Cast` nodes, so the backend must lower the same signed
    /// int/real coercions explicitly to avoid matcher/lowerer drift.
    fn coerce_value_to_fir_type(
        &mut self,
        value: Value,
        target: &FirType,
    ) -> Result<Value, LoweringError> {
        let src_ty = self.fb.func.dfg.value_type(value);
        let dst_ty = self.fir_type_to_clif(target)?;
        if src_ty == dst_ty {
            return Ok(value);
        }
        match (src_ty, dst_ty) {
            (types::F32, types::F64) => Ok(self.fb.ins().fpromote(types::F64, value)),
            (types::F64, types::F32) => Ok(self.fb.ins().fdemote(types::F32, value)),
            (types::I8, types::I32) => Ok(self.fb.ins().uextend(types::I32, value)),
            (types::I8, types::I64) => Ok(self.fb.ins().uextend(types::I64, value)),
            // Bool (i8) → float: widen to i32 first, then convert to float.
            (types::I8, types::F32) => {
                let wide = self.fb.ins().uextend(types::I32, value);
                Ok(self.fb.ins().fcvt_from_sint(types::F32, wide))
            }
            (types::I8, types::F64) => {
                let wide = self.fb.ins().uextend(types::I32, value);
                Ok(self.fb.ins().fcvt_from_sint(types::F64, wide))
            }
            (types::I32, types::F32) | (types::I64, types::F32) => {
                Ok(self.fb.ins().fcvt_from_sint(types::F32, value))
            }
            (types::I32, types::F64) | (types::I64, types::F64) => {
                Ok(self.fb.ins().fcvt_from_sint(types::F64, value))
            }
            (types::F32, types::I32) | (types::F64, types::I32) => {
                Ok(self.fb.ins().fcvt_to_sint(types::I32, value))
            }
            (types::F32, types::I64) | (types::F64, types::I64) => {
                Ok(self.fb.ins().fcvt_to_sint(types::I64, value))
            }
            _ => Err(LoweringError::Unsupported(format!(
                "unsupported Cranelift coercion {src_ty} -> {dst_ty} for FIR target {target:?}"
            ))),
        }
    }

    /// Produces a conservative default value for uninitialized stack locals.
    ///
    /// Current policy:
    /// - scalars => zero
    /// - pointers/object-like refs => null
    ///
    /// This supports FIR patterns where `DeclareVar { access: Stack, init: None }`
    /// appears in `compute` and is assigned before use.
    fn default_lowered_value_for_type(
        &mut self,
        typ: &FirType,
    ) -> Result<LoweredExpr, LoweringError> {
        match typ {
            FirType::Int32 => Ok(LoweredExpr::Scalar(self.fb.ins().iconst(types::I32, 0))),
            FirType::Int64 => Ok(LoweredExpr::Scalar(self.fb.ins().iconst(types::I64, 0))),
            FirType::Bool => Ok(LoweredExpr::Scalar(self.fb.ins().iconst(types::I8, 0))),
            FirType::Float32 | FirType::FaustFloat => {
                Ok(LoweredExpr::Scalar(self.fb.ins().f32const(0.0)))
            }
            FirType::Float64 => Ok(LoweredExpr::Scalar(self.fb.ins().f64const(0.0))),
            FirType::Ptr(inner) => Ok(LoweredExpr::Ptr {
                value: self.fb.ins().iconst(self.ptr_ty, 0),
                pointee: Some(FirTypeRef::from_fir_type(inner)),
            }),
            FirType::Obj | FirType::UI | FirType::Meta | FirType::Sound => Ok(LoweredExpr::Ptr {
                value: self.fb.ins().iconst(self.ptr_ty, 0),
                pointee: None,
            }),
            other => Err(LoweringError::Unsupported(format!(
                "unsupported default stack init type in Cranelift subset lowering: {other:?}"
            ))),
        }
    }

    /// Lowers a FIR statement node in the current subset.
    ///
    /// This is the central dispatcher for statement lowering. Unsupported
    /// variants return `LoweringError::Unsupported`, which callers may route to
    /// the stub fallback policy.
    fn lower_stmt(&mut self, id: FirId) -> Result<(), LoweringError> {
        match match_fir(self.store, id) {
            FirMatch::Block(items) => {
                for item in items {
                    self.lower_stmt(item)?;
                }
                Ok(())
            }
            FirMatch::DeclareVar {
                name,
                typ,
                access: AccessType::Stack,
                init: Some(init),
            } => {
                let init_v = self.lower_expr(init, Some(&typ))?;
                let stored = match (&typ, init_v) {
                    (FirType::Ptr(inner), LoweredExpr::Ptr { value, .. }) => LoweredExpr::Ptr {
                        value,
                        pointee: Some(FirTypeRef::from_fir_type(inner)),
                    },
                    (_, other) => other,
                };
                self.vars.insert(name, stored);
                Ok(())
            }
            FirMatch::DeclareVar {
                name,
                typ,
                access: AccessType::Stack,
                init: None,
            } => {
                let init_v = self.default_lowered_value_for_type(&typ)?;
                self.vars.insert(name, init_v);
                Ok(())
            }
            FirMatch::Label(_) => Ok(()),
            FirMatch::SimpleForLoop {
                var,
                upper,
                body,
                is_reverse: false,
            } => self.lower_simple_for(var, upper, body),
            FirMatch::StoreTable {
                name,
                access: AccessType::Stack,
                index,
                value,
            } => self.lower_store_table_stack(&name, index, value),
            FirMatch::StoreTable {
                name,
                access: AccessType::Struct,
                index,
                value,
            } => self.lower_store_table_struct(&name, index, value),
            FirMatch::StoreVar {
                name,
                access: AccessType::Struct,
                value,
            } => self.lower_store_var_struct(&name, value),
            FirMatch::StoreVar {
                name,
                access: AccessType::Stack | AccessType::Loop,
                value,
            } => self.lower_store_var_local(&name, value),
            FirMatch::ShiftArrayVar {
                name,
                access: AccessType::Struct,
                delay,
            } => self.lower_shift_array_var_struct(&name, delay),
            FirMatch::If {
                cond,
                then_block,
                else_block,
            } => self.lower_if_stmt(cond, then_block, else_block),
            FirMatch::Control { cond, stmt } => self.lower_control_stmt(cond, stmt),
            FirMatch::Switch {
                cond,
                cases,
                default,
            } => self.lower_switch_stmt(cond, &cases, default),
            FirMatch::ForLoop {
                var,
                init,
                end,
                step,
                body,
                is_reverse,
            } => self.lower_for_loop(var, init, end, step, body, is_reverse),
            FirMatch::WhileLoop { cond, body } => self.lower_while_loop(cond, body),
            FirMatch::ShiftArrayVar { .. } => Err(LoweringError::Unsupported(
                "ShiftArrayVar is currently only supported for AccessType::Struct tables"
                    .to_string(),
            )),
            FirMatch::Drop(value) => {
                let _ = self.lower_expr(value, None)?;
                Ok(())
            }
            FirMatch::NullStatement => Ok(()),
            FirMatch::Return(None) => {
                emit_return_stub(self.fb);
                Ok(())
            }
            other => Err(LoweringError::Unsupported(format!(
                "unsupported FIR statement in Cranelift subset lowering: {other:?}"
            ))),
        }
    }

    /// Lowers `SimpleForLoop` (`for i in 0..upper`) in forward direction.
    fn lower_simple_for(
        &mut self,
        var: String,
        upper: FirId,
        body: FirId,
    ) -> Result<(), LoweringError> {
        let upper_v = self.lower_expr(upper, Some(&FirType::Int32))?.value();
        let zero = self.fb.ins().iconst(types::I32, 0);
        let one = self.fb.ins().iconst(types::I32, 1);

        let header = self.fb.create_block();
        let body_block = self.fb.create_block();
        let exit = self.fb.create_block();
        self.fb.append_block_param(header, types::I32);
        self.fb.ins().jump(header, &[zero]);

        self.fb.switch_to_block(header);
        let i_val = self.fb.block_params(header)[0];
        let cond = self.fb.ins().icmp(IntCC::SignedLessThan, i_val, upper_v);
        self.fb.ins().brif(cond, body_block, &[], exit, &[]);

        self.fb.switch_to_block(body_block);
        let prev = self.vars.insert(var.clone(), LoweredExpr::Scalar(i_val));
        self.lower_stmt(body)?;
        if !is_return_terminated(self.fb) {
            let next = self.fb.ins().iadd(i_val, one);
            self.fb.ins().jump(header, &[next]);
        }
        if let Some(old) = prev {
            self.vars.insert(var, old);
        } else {
            self.vars.remove(&var);
        }
        self.fb.seal_block(body_block);
        self.fb.seal_block(header);

        self.fb.switch_to_block(exit);
        self.fb.seal_block(exit);
        Ok(())
    }

    /// Lowers `StoreTable` for stack pointer aliases (for example `output0[i] = ...`).
    ///
    /// The alias must resolve to a pointer-valued local in `vars`.
    fn lower_store_table_stack(
        &mut self,
        name: &str,
        index: FirId,
        value: FirId,
    ) -> Result<(), LoweringError> {
        let alias = self.vars.get(name).copied().ok_or_else(|| {
            LoweringError::Unsupported(format!("stack pointer alias `{name}` not found"))
        })?;
        let base_ptr = alias.ptr().ok_or_else(|| {
            LoweringError::Unsupported(format!("stack alias `{name}` is not a pointer"))
        })?;
        let index_v = self.lower_expr(index, Some(&FirType::Int32))?.value();
        let mut value_v = self.lower_expr(value, None)?.value();
        let elem_ty = if let Some(pointee) = alias.pointee() {
            let fir_target = match pointee {
                FirTypeRef::FaustFloat => FirType::FaustFloat,
                FirTypeRef::Float32 => FirType::Float32,
                FirTypeRef::Float64 => FirType::Float64,
                FirTypeRef::Int32 => FirType::Int32,
                FirTypeRef::Int64 => FirType::Int64,
                FirTypeRef::Bool => FirType::Bool,
                _ => {
                    return Err(LoweringError::Unsupported(format!(
                        "unsupported stack pointer alias pointee for store_table `{name}`: {pointee:?}"
                    )));
                }
            };
            value_v = self.coerce_value_to_fir_type(value_v, &fir_target)?;
            self.fb.func.dfg.value_type(value_v)
        } else {
            self.fb.func.dfg.value_type(value_v)
        };
        let elem_size = i64::from(elem_ty.bytes());
        let addr = self.indexed_addr(base_ptr, index_v, elem_size);
        self.fb.ins().store(MemFlags::new(), value_v, addr, 0);
        Ok(())
    }

    /// Lowers `StoreVar` into a scalar field of the backend `dsp*` struct.
    fn lower_store_var_struct(&mut self, name: &str, value: FirId) -> Result<(), LoweringError> {
        let field = self.struct_field(name)?.clone();
        let scalar_ty = match &field.kind {
            StructFieldKind::Scalar(typ) => typ.clone(),
            StructFieldKind::Table { .. } => {
                return Err(LoweringError::Unsupported(format!(
                    "`StoreVar` cannot target table field `{name}`"
                )));
            }
        };
        let dsp = self.dsp_base_ptr()?;
        let addr = self.fb.ins().iadd_imm(dsp, i64::from(field.offset_bytes));
        let mut value_v = self.lower_expr(value, Some(&scalar_ty))?.value();
        value_v = self.coerce_value_to_fir_type(value_v, &scalar_ty)?;
        self.fb.ins().store(MemFlags::new(), value_v, addr, 0);
        Ok(())
    }

    /// Lowers `StoreTable` into an inline table field of the backend `dsp*` struct.
    fn lower_store_table_struct(
        &mut self,
        name: &str,
        index: FirId,
        value: FirId,
    ) -> Result<(), LoweringError> {
        let field = self.struct_field(name)?.clone();
        let elem_type = match &field.kind {
            StructFieldKind::Table { elem_type, .. } => elem_type.clone(),
            StructFieldKind::Scalar(_) => {
                return Err(LoweringError::Unsupported(format!(
                    "`StoreTable` cannot target scalar field `{name}`"
                )));
            }
        };
        let dsp = self.dsp_base_ptr()?;
        let base = self.fb.ins().iadd_imm(dsp, i64::from(field.offset_bytes));
        let index_v = self.lower_expr(index, Some(&FirType::Int32))?.value();
        let mut value_v = self.lower_expr(value, Some(&elem_type))?.value();
        value_v = self.coerce_value_to_fir_type(value_v, &elem_type)?;
        let elem_clif = self.fir_type_to_clif(&elem_type)?;
        let addr = self.indexed_addr(base, index_v, i64::from(elem_clif.bytes()));
        self.fb.ins().store(MemFlags::new(), value_v, addr, 0);
        Ok(())
    }

    /// Lowers `ShiftArrayVar` on an inline `Struct` table using a descending loop.
    ///
    /// Semantics (current implementation):
    /// - shifts right by one (`tbl[i] = tbl[i-1]`) for `i = shift_len .. 1`
    /// - clamps the effective shift range to the declared table length
    fn lower_shift_array_var_struct(
        &mut self,
        name: &str,
        delay: i32,
    ) -> Result<(), LoweringError> {
        let field = self.struct_field(name)?.clone();
        let (elem_type, len) = match &field.kind {
            StructFieldKind::Table { elem_type, len } => (elem_type.clone(), *len),
            StructFieldKind::Scalar(_) => {
                return Err(LoweringError::Unsupported(format!(
                    "`ShiftArrayVar` cannot target scalar field `{name}`"
                )));
            }
        };
        let max_shift = delay.max(0) as u32;
        if len <= 1 || max_shift == 0 {
            return Ok(());
        }
        let shift_len = max_shift.min(len.saturating_sub(1));
        if shift_len == 0 {
            return Ok(());
        }

        let dsp = self.dsp_base_ptr()?;
        let base = self.fb.ins().iadd_imm(dsp, i64::from(field.offset_bytes));
        let elem_clif = self.fir_type_to_clif(&elem_type)?;
        let elem_bytes = i64::from(elem_clif.bytes());

        let header = self.fb.create_block();
        let body = self.fb.create_block();
        let exit = self.fb.create_block();
        let init_i = self.fb.ins().iconst(types::I32, i64::from(shift_len));
        self.fb.append_block_param(header, types::I32);
        self.fb.ins().jump(header, &[init_i]);

        self.fb.switch_to_block(header);
        let i_val = self.fb.block_params(header)[0];
        let cond = self.fb.ins().icmp_imm(IntCC::SignedGreaterThan, i_val, 0);
        self.fb.ins().brif(cond, body, &[], exit, &[]);

        self.fb.switch_to_block(body);
        let src_idx = self.fb.ins().iadd_imm(i_val, -1);
        let src_addr = self.indexed_addr(base, src_idx, elem_bytes);
        let dst_addr = self.indexed_addr(base, i_val, elem_bytes);
        let v = self.fb.ins().load(elem_clif, MemFlags::new(), src_addr, 0);
        self.fb.ins().store(MemFlags::new(), v, dst_addr, 0);
        let next = self.fb.ins().iadd_imm(i_val, -1);
        self.fb.ins().jump(header, &[next]);
        self.fb.seal_block(body);
        self.fb.seal_block(header);

        self.fb.switch_to_block(exit);
        self.fb.seal_block(exit);
        Ok(())
    }

    /// Updates a local stack/loop variable in the lowering environment.
    ///
    /// This mutates the name->value mapping only; it does not emit memory
    /// traffic because stack locals in the current subset are modelled as SSA
    /// values/pointers in the lowering environment.
    fn lower_store_var_local(&mut self, name: &str, value: FirId) -> Result<(), LoweringError> {
        let prev = self.vars.get(name).copied().ok_or_else(|| {
            LoweringError::Unsupported(format!("local variable `{name}` not found"))
        })?;
        let new_value = match prev {
            LoweredExpr::Scalar(_) => LoweredExpr::Scalar(self.lower_expr(value, None)?.value()),
            LoweredExpr::Ptr { pointee, .. } => {
                let v = self.lower_expr(value, None)?.value();
                LoweredExpr::Ptr { value: v, pointee }
            }
        };
        self.vars.insert(name.to_string(), new_value);
        Ok(())
    }

    /// Lowers FIR `If` with explicit then/else/continuation CLIF blocks.
    fn lower_if_stmt(
        &mut self,
        cond: FirId,
        then_block: FirId,
        else_block: Option<FirId>,
    ) -> Result<(), LoweringError> {
        let cond_v = self.lower_expr(cond, Some(&FirType::Bool))?.value();
        let cond_b1 = self.fb.ins().icmp_imm(IntCC::NotEqual, cond_v, 0);
        let then_b = self.fb.create_block();
        let else_b = self.fb.create_block();
        let cont_b = self.fb.create_block();
        self.fb.ins().brif(cond_b1, then_b, &[], else_b, &[]);

        self.fb.switch_to_block(then_b);
        self.lower_stmt(then_block)?;
        if !is_return_terminated(self.fb) {
            self.fb.ins().jump(cont_b, &[]);
        }
        self.fb.seal_block(then_b);

        self.fb.switch_to_block(else_b);
        if let Some(else_block) = else_block {
            self.lower_stmt(else_block)?;
        }
        if !is_return_terminated(self.fb) {
            self.fb.ins().jump(cont_b, &[]);
        }
        self.fb.seal_block(else_b);

        self.fb.switch_to_block(cont_b);
        self.fb.seal_block(cont_b);
        Ok(())
    }

    /// Lowers FIR `Control` as an `if (cond) stmt`.
    fn lower_control_stmt(&mut self, cond: FirId, stmt: FirId) -> Result<(), LoweringError> {
        self.lower_if_stmt(cond, stmt, None)
    }

    /// Lowers FIR `Switch` as a chain of integer comparisons and branches.
    ///
    /// This is a simple, explicit lowering intended for bring-up and debugging.
    /// It favors clarity and deterministic diagnostics over optimal jump-table
    /// generation (which can be considered later).
    fn lower_switch_stmt(
        &mut self,
        cond: FirId,
        cases: &[(i64, FirId)],
        default: Option<FirId>,
    ) -> Result<(), LoweringError> {
        let cond_v = self.lower_expr(cond, None)?.value();
        let cond_ty = self.fb.func.dfg.value_type(cond_v);
        match cond_ty {
            types::I8 | types::I32 | types::I64 => {}
            other => {
                return Err(LoweringError::Unsupported(format!(
                    "unsupported `Switch` condition CLIF type in subset lowering: {other}"
                )));
            }
        }

        let cont_b = self.fb.create_block();
        let default_b = default.map(|_| self.fb.create_block());
        if cases.is_empty() {
            if let Some(default_stmt) = default {
                self.lower_stmt(default_stmt)?;
            }
            if !is_return_terminated(self.fb) {
                self.fb.ins().jump(cont_b, &[]);
            }
            self.fb.switch_to_block(cont_b);
            self.fb.seal_block(cont_b);
            return Ok(());
        }

        for (idx, (case_value, case_stmt)) in cases.iter().enumerate() {
            let then_b = self.fb.create_block();
            let is_last = idx + 1 == cases.len();
            let next_b = if is_last {
                None
            } else {
                Some(self.fb.create_block())
            };
            let fallthrough_b = next_b.or(default_b).unwrap_or(cont_b);

            let cond_match = self.fb.ins().icmp_imm(IntCC::Equal, cond_v, *case_value);
            self.fb
                .ins()
                .brif(cond_match, then_b, &[], fallthrough_b, &[]);

            self.fb.switch_to_block(then_b);
            self.lower_stmt(*case_stmt)?;
            if !is_return_terminated(self.fb) {
                self.fb.ins().jump(cont_b, &[]);
            }
            self.fb.seal_block(then_b);

            if let Some(next_b) = next_b {
                self.fb.switch_to_block(next_b);
                self.fb.seal_block(next_b);
            }
        }

        if let Some(default_stmt) = default {
            self.fb
                .switch_to_block(default_b.expect("present when default stmt exists"));
            self.lower_stmt(default_stmt)?;
            self.fb
                .seal_block(default_b.expect("present when default stmt exists"));
        }
        if !is_return_terminated(self.fb) {
            self.fb.ins().jump(cont_b, &[]);
        }
        self.fb.switch_to_block(cont_b);
        self.fb.seal_block(cont_b);
        Ok(())
    }

    /// Lowers FIR `ForLoop` with explicit header/body/exit blocks.
    ///
    /// The loop variable is installed in the local environment as a loop-local
    /// scalar and restored/removed after lowering the loop.
    fn lower_for_loop(
        &mut self,
        var: String,
        init: FirId,
        end: FirId,
        step: FirId,
        body: FirId,
        is_reverse: bool,
    ) -> Result<(), LoweringError> {
        let init_v = self.lower_expr(init, Some(&FirType::Int32))?.value();
        let header = self.fb.create_block();
        let body_block = self.fb.create_block();
        let exit = self.fb.create_block();
        self.fb.append_block_param(header, types::I32);
        self.fb.ins().jump(header, &[init_v]);

        self.fb.switch_to_block(header);
        let i_val = self.fb.block_params(header)[0];
        let prev = self.vars.insert(var.clone(), LoweredExpr::Scalar(i_val));
        let end_v = self.lower_expr(end, Some(&FirType::Int32))?.value();
        let cc = if is_reverse {
            IntCC::SignedGreaterThan
        } else {
            IntCC::SignedLessThan
        };
        let cond = self.fb.ins().icmp(cc, i_val, end_v);
        self.fb.ins().brif(cond, body_block, &[], exit, &[]);

        self.fb.switch_to_block(body_block);
        self.lower_stmt(body)?;
        let step_v = self.lower_expr(step, Some(&FirType::Int32))?.value();
        let next = self.fb.ins().iadd(i_val, step_v);
        self.fb.ins().jump(header, &[next]);
        self.fb.seal_block(body_block);
        self.fb.seal_block(header);

        if let Some(old) = prev {
            self.vars.insert(var, old);
        } else {
            self.vars.remove(&var);
        }

        self.fb.switch_to_block(exit);
        self.fb.seal_block(exit);
        Ok(())
    }

    /// Lowers FIR `WhileLoop` with explicit header/body/exit blocks.
    fn lower_while_loop(&mut self, cond: FirId, body: FirId) -> Result<(), LoweringError> {
        let header = self.fb.create_block();
        let body_block = self.fb.create_block();
        let exit = self.fb.create_block();
        self.fb.ins().jump(header, &[]);

        self.fb.switch_to_block(header);
        let cond_v = self.lower_expr(cond, Some(&FirType::Bool))?.value();
        let cond_b1 = self.fb.ins().icmp_imm(IntCC::NotEqual, cond_v, 0);
        self.fb.ins().brif(cond_b1, body_block, &[], exit, &[]);

        self.fb.switch_to_block(body_block);
        self.lower_stmt(body)?;
        if !is_return_terminated(self.fb) {
            self.fb.ins().jump(header, &[]);
        }
        self.fb.seal_block(body_block);
        self.fb.seal_block(header);

        self.fb.switch_to_block(exit);
        self.fb.seal_block(exit);
        Ok(())
    }

    /// Computes an element address `base + index * elem_size`.
    ///
    /// `index_i32` is widened to pointer width as needed (`I32`/`I64` target).
    fn indexed_addr(&mut self, base_ptr: Value, index_i32: Value, elem_size: i64) -> Value {
        let idx_ptr = if self.ptr_ty == types::I64 {
            self.fb.ins().uextend(types::I64, index_i32)
        } else {
            self.fb.ins().uextend(types::I32, index_i32)
        };
        let scale = self.fb.ins().iconst(self.ptr_ty, elem_size);
        let offset = self.fb.ins().imul(idx_ptr, scale);
        self.fb.ins().iadd(base_ptr, offset)
    }

    /// Lowers a FIR expression node in the current subset.
    ///
    /// `expected` is a backend-local hint used in a few places to guide type
    /// coercions and pointer/scalar handling; it is not a full FIR typechecker.
    fn lower_expr(
        &mut self,
        id: FirId,
        expected: Option<&FirType>,
    ) -> Result<LoweredExpr, LoweringError> {
        match match_fir(self.store, id) {
            FirMatch::Int32 { value, .. } => Ok(LoweredExpr::Scalar(
                self.fb.ins().iconst(types::I32, i64::from(value)),
            )),
            FirMatch::Bool { value, .. } => Ok(LoweredExpr::Scalar(
                self.fb.ins().iconst(types::I8, if value { 1 } else { 0 }),
            )),
            FirMatch::Float32 { value, .. } => {
                Ok(LoweredExpr::Scalar(self.fb.ins().f32const(value)))
            }
            FirMatch::Float64 { value, .. } => {
                Ok(LoweredExpr::Scalar(self.fb.ins().f64const(value)))
            }
            FirMatch::LoadVar {
                name,
                access: AccessType::Struct,
                typ,
            } => {
                let field = self.struct_field(&name)?.clone();
                let scalar_ty = match &field.kind {
                    StructFieldKind::Scalar(t) => t.clone(),
                    StructFieldKind::Table { .. } => {
                        return Err(LoweringError::Unsupported(format!(
                            "`LoadVar` cannot target table field `{name}`"
                        )));
                    }
                };
                let dsp = self.dsp_base_ptr()?;
                let addr = self.fb.ins().iadd_imm(dsp, i64::from(field.offset_bytes));
                let field_clif_ty = self.fir_type_to_clif(&scalar_ty)?;
                let raw = self.fb.ins().load(field_clif_ty, MemFlags::new(), addr, 0);
                let coerced = self.coerce_value_to_fir_type(raw, &typ)?;
                Ok(LoweredExpr::Scalar(coerced))
            }
            FirMatch::LoadVar { name, .. } => self.vars.get(&name).copied().ok_or_else(|| {
                LoweringError::Unsupported(format!("load of unknown variable `{name}`"))
            }),
            FirMatch::LoadTable {
                name,
                access: AccessType::Stack,
                index,
                typ,
            } => {
                let alias = self.vars.get(&name).copied().ok_or_else(|| {
                    LoweringError::Unsupported(format!("stack table base `{name}` not found"))
                })?;
                let base_ptr = alias.ptr().ok_or_else(|| {
                    LoweringError::Unsupported(format!(
                        "stack table base `{name}` is not a pointer alias"
                    ))
                })?;
                let index_v = self.lower_expr(index, Some(&FirType::Int32))?.value();
                let elem_fir_ty = match alias.pointee() {
                    Some(FirTypeRef::FaustFloat) => FirType::FaustFloat,
                    Some(FirTypeRef::Float32) => FirType::Float32,
                    Some(FirTypeRef::Float64) => FirType::Float64,
                    Some(FirTypeRef::Int32) => FirType::Int32,
                    Some(FirTypeRef::Int64) => FirType::Int64,
                    Some(FirTypeRef::Bool) => FirType::Bool,
                    Some(other) => {
                        return Err(LoweringError::Unsupported(format!(
                            "unsupported stack pointer alias pointee for load_table `{name}`: {other:?}"
                        )));
                    }
                    None => typ.clone(),
                };
                let elem_clif = self.fir_type_to_clif(&elem_fir_ty)?;
                let addr = self.indexed_addr(base_ptr, index_v, i64::from(elem_clif.bytes()));
                let raw = self.fb.ins().load(elem_clif, MemFlags::new(), addr, 0);
                let coerced = self.coerce_value_to_fir_type(raw, &typ)?;
                Ok(LoweredExpr::Scalar(coerced))
            }
            FirMatch::LoadTable {
                name,
                access: AccessType::FunArgs,
                index,
                typ,
            } => {
                let base_ptr = self.vars.get(&name).and_then(|v| v.ptr()).ok_or_else(|| {
                    LoweringError::Unsupported(format!(
                        "function-arg table base `{name}` not found"
                    ))
                })?;
                let index_v = self.lower_expr(index, Some(&FirType::Int32))?.value();
                let elem_ty = self.fir_type_to_clif(&typ)?;
                let addr = self.indexed_addr(base_ptr, index_v, i64::from(self.ptr_ty.bytes()));
                let loaded = self.fb.ins().load(elem_ty, MemFlags::new(), addr, 0);
                let pointee = match &typ {
                    FirType::Ptr(inner) => Some(FirTypeRef::from_fir_type(inner)),
                    _ => None,
                };
                Ok(LoweredExpr::Ptr {
                    value: loaded,
                    pointee,
                })
            }
            FirMatch::LoadTable {
                name,
                access: AccessType::Struct,
                index,
                typ,
            } => {
                let field = self.struct_field(&name)?.clone();
                let elem_type = match &field.kind {
                    StructFieldKind::Table { elem_type, .. } => elem_type.clone(),
                    StructFieldKind::Scalar(_) => {
                        return Err(LoweringError::Unsupported(format!(
                            "`LoadTable` cannot target scalar field `{name}`"
                        )));
                    }
                };
                let dsp = self.dsp_base_ptr()?;
                let base = self.fb.ins().iadd_imm(dsp, i64::from(field.offset_bytes));
                let index_v = self.lower_expr(index, Some(&FirType::Int32))?.value();
                let elem_clif = self.fir_type_to_clif(&elem_type)?;
                let addr = self.indexed_addr(base, index_v, i64::from(elem_clif.bytes()));
                let raw = self.fb.ins().load(elem_clif, MemFlags::new(), addr, 0);
                let coerced = self.coerce_value_to_fir_type(raw, &typ)?;
                Ok(LoweredExpr::Scalar(coerced))
            }
            FirMatch::LoadTable {
                name,
                access: AccessType::Static,
                index,
                typ,
            } => {
                let data_id = self.static_data_ids.get(&name).copied().ok_or_else(|| {
                    LoweringError::Unsupported(format!(
                        "static table `{name}` not found in pre-declared JIT data"
                    ))
                })?;
                let gv = self.jit.declare_data_in_func(data_id, self.fb.func);
                let base = self.fb.ins().global_value(self.ptr_ty, gv);
                let index_v = self.lower_expr(index, Some(&FirType::Int32))?.value();
                let elem_clif = self.fir_type_to_clif(&typ)?;
                let addr = self.indexed_addr(base, index_v, i64::from(elem_clif.bytes()));
                let raw = self.fb.ins().load(elem_clif, MemFlags::new(), addr, 0);
                let coerced = self.coerce_value_to_fir_type(raw, &typ)?;
                Ok(LoweredExpr::Scalar(coerced))
            }
            FirMatch::Select2 {
                cond,
                then_value,
                else_value,
                typ,
            } => {
                let cond_v = self.lower_expr(cond, Some(&FirType::Bool))?.value();
                let then_v = self.lower_expr(then_value, Some(&typ))?.value();
                let else_v = self.lower_expr(else_value, Some(&typ))?.value();
                let bool_cond = self.fb.ins().icmp_imm(IntCC::NotEqual, cond_v, 0);
                let out = self.fb.ins().select(bool_cond, then_v, else_v);
                Ok(LoweredExpr::Scalar(out))
            }
            FirMatch::Neg { value, typ } => {
                let v = self.lower_expr(value, Some(&typ))?.value();
                let v = self.coerce_value_to_fir_type(v, &typ)?;
                let out = match typ {
                    FirType::Int32 | FirType::Int64 => self.fb.ins().ineg(v),
                    FirType::Float32 | FirType::Float64 | FirType::FaustFloat => {
                        self.fb.ins().fneg(v)
                    }
                    _ => {
                        return Err(LoweringError::Unsupported(format!(
                            "unsupported negation type in subset lowering: {typ:?}"
                        )));
                    }
                };
                Ok(LoweredExpr::Scalar(out))
            }
            FirMatch::Cast { typ, value } => {
                let src = self.lower_expr(value, None)?.value();
                let out = self.coerce_value_to_fir_type(src, &typ)?;
                Ok(LoweredExpr::Scalar(out))
            }
            FirMatch::BinOp { op, lhs, rhs, typ } => self.lower_binop(op, lhs, rhs, &typ),
            FirMatch::FunCall { name, args, typ } => self.lower_fun_call(&name, &args, &typ),
            other => Err(LoweringError::Unsupported(format!(
                "unsupported FIR expression in Cranelift subset lowering: {other:?}; expected={expected:?}"
            ))),
        }
    }

    /// Lowers FIR `BinOp` arithmetic/comparisons to CLIF instructions.
    ///
    /// Comparisons return FIR-style booleans (`i8` 0/1), not CLIF `b1`.
    fn lower_binop(
        &mut self,
        op: FirBinOp,
        lhs: FirId,
        rhs: FirId,
        typ: &FirType,
    ) -> Result<LoweredExpr, LoweringError> {
        let l = self.lower_expr(lhs, Some(typ))?.value();
        let r = self.lower_expr(rhs, Some(typ))?.value();
        if matches!(typ, FirType::Bool) {
            let lty = self.fb.func.dfg.value_type(l);
            let out = if lty.is_int() {
                match op {
                    FirBinOp::Eq => self.int_cmp_to_i8(IntCC::Equal, l, r),
                    FirBinOp::Ne => self.int_cmp_to_i8(IntCC::NotEqual, l, r),
                    FirBinOp::Lt => self.int_cmp_to_i8(IntCC::SignedLessThan, l, r),
                    FirBinOp::Le => self.int_cmp_to_i8(IntCC::SignedLessThanOrEqual, l, r),
                    FirBinOp::Gt => self.int_cmp_to_i8(IntCC::SignedGreaterThan, l, r),
                    FirBinOp::Ge => self.int_cmp_to_i8(IntCC::SignedGreaterThanOrEqual, l, r),
                    _ => {
                        return Err(LoweringError::Unsupported(format!(
                            "unsupported bool-result int comparison in subset lowering: {op:?}"
                        )));
                    }
                }
            } else if lty.is_float() {
                match op {
                    FirBinOp::Eq => {
                        self.float_cmp_to_i8(cranelift_codegen::ir::condcodes::FloatCC::Equal, l, r)
                    }
                    FirBinOp::Ne => self.float_cmp_to_i8(
                        cranelift_codegen::ir::condcodes::FloatCC::NotEqual,
                        l,
                        r,
                    ),
                    FirBinOp::Lt => self.float_cmp_to_i8(
                        cranelift_codegen::ir::condcodes::FloatCC::LessThan,
                        l,
                        r,
                    ),
                    FirBinOp::Le => self.float_cmp_to_i8(
                        cranelift_codegen::ir::condcodes::FloatCC::LessThanOrEqual,
                        l,
                        r,
                    ),
                    FirBinOp::Gt => self.float_cmp_to_i8(
                        cranelift_codegen::ir::condcodes::FloatCC::GreaterThan,
                        l,
                        r,
                    ),
                    FirBinOp::Ge => self.float_cmp_to_i8(
                        cranelift_codegen::ir::condcodes::FloatCC::GreaterThanOrEqual,
                        l,
                        r,
                    ),
                    _ => {
                        return Err(LoweringError::Unsupported(format!(
                            "unsupported bool-result float comparison in subset lowering: {op:?}"
                        )));
                    }
                }
            } else {
                return Err(LoweringError::Unsupported(format!(
                    "unsupported comparison operand CLIF type in subset lowering: {lty}"
                )));
            };
            return Ok(LoweredExpr::Scalar(out));
        }
        let l = self.coerce_value_to_fir_type(l, typ)?;
        let r = self.coerce_value_to_fir_type(r, typ)?;
        let out = match typ {
            FirType::Int32 => match op {
                FirBinOp::Add => self.fb.ins().iadd(l, r),
                FirBinOp::Sub => self.fb.ins().isub(l, r),
                FirBinOp::Mul => self.fb.ins().imul(l, r),
                FirBinOp::Div => self.fb.ins().sdiv(l, r),
                FirBinOp::Rem => self.fb.ins().srem(l, r),
                FirBinOp::And => self.fb.ins().band(l, r),
                FirBinOp::Or => self.fb.ins().bor(l, r),
                FirBinOp::Xor => self.fb.ins().bxor(l, r),
                FirBinOp::Lsh => self.fb.ins().ishl(l, r),
                FirBinOp::ARsh => self.fb.ins().sshr(l, r),
                FirBinOp::LRsh => self.fb.ins().ushr(l, r),
                FirBinOp::Eq => self.int_cmp_to_i8(IntCC::Equal, l, r),
                FirBinOp::Ne => self.int_cmp_to_i8(IntCC::NotEqual, l, r),
                FirBinOp::Lt => self.int_cmp_to_i8(IntCC::SignedLessThan, l, r),
                FirBinOp::Le => self.int_cmp_to_i8(IntCC::SignedLessThanOrEqual, l, r),
                FirBinOp::Gt => self.int_cmp_to_i8(IntCC::SignedGreaterThan, l, r),
                FirBinOp::Ge => self.int_cmp_to_i8(IntCC::SignedGreaterThanOrEqual, l, r),
            },
            FirType::Float32 | FirType::FaustFloat => match op {
                FirBinOp::Add => self.fb.ins().fadd(l, r),
                FirBinOp::Sub => self.fb.ins().fsub(l, r),
                FirBinOp::Mul => self.fb.ins().fmul(l, r),
                FirBinOp::Div => self.fb.ins().fdiv(l, r),
                FirBinOp::Eq => {
                    self.float_cmp_to_i8(cranelift_codegen::ir::condcodes::FloatCC::Equal, l, r)
                }
                FirBinOp::Ne => {
                    self.float_cmp_to_i8(cranelift_codegen::ir::condcodes::FloatCC::NotEqual, l, r)
                }
                FirBinOp::Lt => {
                    self.float_cmp_to_i8(cranelift_codegen::ir::condcodes::FloatCC::LessThan, l, r)
                }
                FirBinOp::Le => self.float_cmp_to_i8(
                    cranelift_codegen::ir::condcodes::FloatCC::LessThanOrEqual,
                    l,
                    r,
                ),
                FirBinOp::Gt => self.float_cmp_to_i8(
                    cranelift_codegen::ir::condcodes::FloatCC::GreaterThan,
                    l,
                    r,
                ),
                FirBinOp::Ge => self.float_cmp_to_i8(
                    cranelift_codegen::ir::condcodes::FloatCC::GreaterThanOrEqual,
                    l,
                    r,
                ),
                _ => {
                    return Err(LoweringError::Unsupported(format!(
                        "unsupported float32/faustfloat binop in subset lowering: {op:?}"
                    )));
                }
            },
            FirType::Float64 => match op {
                FirBinOp::Add => self.fb.ins().fadd(l, r),
                FirBinOp::Sub => self.fb.ins().fsub(l, r),
                FirBinOp::Mul => self.fb.ins().fmul(l, r),
                FirBinOp::Div => self.fb.ins().fdiv(l, r),
                FirBinOp::Eq => {
                    self.float_cmp_to_i8(cranelift_codegen::ir::condcodes::FloatCC::Equal, l, r)
                }
                FirBinOp::Ne => {
                    self.float_cmp_to_i8(cranelift_codegen::ir::condcodes::FloatCC::NotEqual, l, r)
                }
                FirBinOp::Lt => {
                    self.float_cmp_to_i8(cranelift_codegen::ir::condcodes::FloatCC::LessThan, l, r)
                }
                FirBinOp::Le => self.float_cmp_to_i8(
                    cranelift_codegen::ir::condcodes::FloatCC::LessThanOrEqual,
                    l,
                    r,
                ),
                FirBinOp::Gt => self.float_cmp_to_i8(
                    cranelift_codegen::ir::condcodes::FloatCC::GreaterThan,
                    l,
                    r,
                ),
                FirBinOp::Ge => self.float_cmp_to_i8(
                    cranelift_codegen::ir::condcodes::FloatCC::GreaterThanOrEqual,
                    l,
                    r,
                ),
                _ => {
                    return Err(LoweringError::Unsupported(format!(
                        "unsupported Float64 binop in subset lowering: {op:?}"
                    )));
                }
            },
            _ => {
                return Err(LoweringError::Unsupported(format!(
                    "unsupported binop result type in subset lowering: {typ:?}"
                )));
            }
        };
        Ok(LoweredExpr::Scalar(out))
    }

    /// Lowers FIR math calls to imported host functions (`sinf`, `pow`, ...).
    ///
    /// Supported operations are determined by the `*_math_symbol_*` helpers and
    /// are intentionally explicit to keep subset coverage auditable.
    fn lower_fun_call(
        &mut self,
        name: &str,
        args: &[FirId],
        typ: &FirType,
    ) -> Result<LoweredExpr, LoweringError> {
        match (name, typ, args) {
            ("abs", FirType::Int32, [x]) => {
                let xv = self.lower_expr(*x, Some(typ))?.value();
                let xv = self.coerce_value_to_fir_type(xv, typ)?;
                let fref = self.ensure_import("abs", &[types::I32], types::I32)?;
                let call = self.fb.ins().call(fref, &[xv]);
                return Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]));
            }
            ("min_i", FirType::Int32, [x, y]) | ("max_i", FirType::Int32, [x, y]) => {
                let xv = self.lower_expr(*x, Some(typ))?.value();
                let xv = self.coerce_value_to_fir_type(xv, typ)?;
                let yv = self.lower_expr(*y, Some(typ))?.value();
                let yv = self.coerce_value_to_fir_type(yv, typ)?;
                let fref = self.ensure_import(name, &[types::I32, types::I32], types::I32)?;
                let call = self.fb.ins().call(fref, &[xv, yv]);
                return Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]));
            }
            ("isnanf", FirType::Int32, [x]) => {
                let xv = self.lower_expr(*x, Some(&FirType::Float32))?.value();
                let xv = self.coerce_value_to_fir_type(xv, &FirType::Float32)?;
                let fref = self.ensure_import("isnanf", &[types::F32], types::I32)?;
                let call = self.fb.ins().call(fref, &[xv]);
                return Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]));
            }
            ("isnan", FirType::Int32, [x]) => {
                let xv = self.lower_expr(*x, Some(&FirType::Float64))?.value();
                let xv = self.coerce_value_to_fir_type(xv, &FirType::Float64)?;
                let fref = self.ensure_import("isnan", &[types::F64], types::I32)?;
                let call = self.fb.ins().call(fref, &[xv]);
                return Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]));
            }
            ("isinff", FirType::Int32, [x]) => {
                let xv = self.lower_expr(*x, Some(&FirType::Float32))?.value();
                let xv = self.coerce_value_to_fir_type(xv, &FirType::Float32)?;
                let fref = self.ensure_import("isinff", &[types::F32], types::I32)?;
                let call = self.fb.ins().call(fref, &[xv]);
                return Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]));
            }
            ("isinf", FirType::Int32, [x]) => {
                let xv = self.lower_expr(*x, Some(&FirType::Float64))?.value();
                let xv = self.coerce_value_to_fir_type(xv, &FirType::Float64)?;
                let fref = self.ensure_import("isinf", &[types::F64], types::I32)?;
                let call = self.fb.ins().call(fref, &[xv]);
                return Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]));
            }
            ("copysignf", FirType::FaustFloat | FirType::Float32, [x, y]) => {
                let mut xv = self.lower_expr(*x, Some(&FirType::Float32))?.value();
                let mut yv = self.lower_expr(*y, Some(&FirType::Float32))?.value();
                xv = self.coerce_value_to_fir_type(xv, &FirType::Float32)?;
                yv = self.coerce_value_to_fir_type(yv, &FirType::Float32)?;
                let fref = self.ensure_import("copysignf", &[types::F32, types::F32], types::F32)?;
                let call = self.fb.ins().call(fref, &[xv, yv]);
                return Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]));
            }
            ("copysign", FirType::Float64, [x, y]) => {
                let mut xv = self.lower_expr(*x, Some(&FirType::Float64))?.value();
                let mut yv = self.lower_expr(*y, Some(&FirType::Float64))?.value();
                xv = self.coerce_value_to_fir_type(xv, &FirType::Float64)?;
                yv = self.coerce_value_to_fir_type(yv, &FirType::Float64)?;
                let fref = self.ensure_import("copysign", &[types::F64, types::F64], types::F64)?;
                let call = self.fb.ins().call(fref, &[xv, yv]);
                return Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]));
            }
            ("acoshf" | "asinhf" | "atanhf" | "coshf" | "sinhf" | "tanhf", FirType::FaustFloat | FirType::Float32, [x]) => {
                let xv = self.lower_expr(*x, Some(&FirType::Float32))?.value();
                let xv = self.coerce_value_to_fir_type(xv, &FirType::Float32)?;
                let fref = self.ensure_import(name, &[types::F32], types::F32)?;
                let call = self.fb.ins().call(fref, &[xv]);
                return Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]));
            }
            ("acosh" | "asinh" | "atanh" | "cosh" | "sinh" | "tanh", FirType::Float64, [x]) => {
                let xv = self.lower_expr(*x, Some(&FirType::Float64))?.value();
                let xv = self.coerce_value_to_fir_type(xv, &FirType::Float64)?;
                let fref = self.ensure_import(name, &[types::F64], types::F64)?;
                let call = self.fb.ins().call(fref, &[xv]);
                return Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]));
            }
            _ => {}
        }
        let math = fir::FirMathOp::from_symbol(name).ok_or_else(|| {
            LoweringError::Unsupported(format!("unsupported function call `{name}`"))
        })?;
        match (math, typ, args) {
            (math, FirType::FaustFloat | FirType::Float32, [x])
                if unary_math_symbol_f32(math).is_some() =>
            {
                let mut xv = self.lower_expr(*x, Some(typ))?.value();
                xv = self.coerce_value_to_fir_type(xv, typ)?;
                let fref = self.ensure_unary_import(
                    unary_math_symbol_f32(math).expect("guarded by is_some"),
                    types::F32,
                )?;
                let call = self.fb.ins().call(fref, &[xv]);
                Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]))
            }
            (math, FirType::Float64, [x]) if unary_math_symbol_f64(math).is_some() => {
                let mut xv = self.lower_expr(*x, Some(typ))?.value();
                xv = self.coerce_value_to_fir_type(xv, typ)?;
                let fref = self.ensure_unary_import(
                    unary_math_symbol_f64(math).expect("guarded by is_some"),
                    types::F64,
                )?;
                let call = self.fb.ins().call(fref, &[xv]);
                Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]))
            }
            (math, FirType::FaustFloat | FirType::Float32, [x, y])
                if binary_math_symbol_f32(math).is_some() =>
            {
                let mut xv = self.lower_expr(*x, Some(typ))?.value();
                let mut yv = self.lower_expr(*y, Some(typ))?.value();
                xv = self.coerce_value_to_fir_type(xv, typ)?;
                yv = self.coerce_value_to_fir_type(yv, typ)?;
                let fref = self.ensure_binary_import(
                    binary_math_symbol_f32(math).expect("guarded by is_some"),
                    types::F32,
                )?;
                let call = self.fb.ins().call(fref, &[xv, yv]);
                Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]))
            }
            (math, FirType::Float64, [x, y]) if binary_math_symbol_f64(math).is_some() => {
                let mut xv = self.lower_expr(*x, Some(typ))?.value();
                let mut yv = self.lower_expr(*y, Some(typ))?.value();
                xv = self.coerce_value_to_fir_type(xv, typ)?;
                yv = self.coerce_value_to_fir_type(yv, typ)?;
                let fref = self.ensure_binary_import(
                    binary_math_symbol_f64(math).expect("guarded by is_some"),
                    types::F64,
                )?;
                let call = self.fb.ins().call(fref, &[xv, yv]);
                Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]))
            }
            _ => Err(LoweringError::Unsupported(format!(
                "unsupported function call lowering `{name}` with typ={typ:?} args={}",
                args.len()
            ))),
        }
    }

    /// Ensures a cached unary imported function reference exists.
    fn ensure_unary_import(&mut self, symbol: &str, ty: Type) -> Result<FuncRef, LoweringError> {
        self.ensure_import(symbol, &[ty], ty)
    }

    /// Ensures a cached binary imported function reference exists.
    fn ensure_binary_import(&mut self, symbol: &str, ty: Type) -> Result<FuncRef, LoweringError> {
        self.ensure_import(symbol, &[ty, ty], ty)
    }

    /// Declares/imports a CLIF function and caches the `FuncRef` by signature key.
    ///
    /// Cranelift imports are identified by both symbol and signature, so the
    /// cache key includes parameter and return types.
    fn ensure_import(
        &mut self,
        symbol: &str,
        params: &[Type],
        ret: Type,
    ) -> Result<FuncRef, LoweringError> {
        let cache_key = format!(
            "{symbol}({})->{}",
            params
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<_>>()
                .join(","),
            ret
        );
        if let Some(fref) = self.import_refs.get(&cache_key).copied() {
            return Ok(fref);
        }
        let mut sig = self.jit.make_signature();
        for param in params {
            sig.params.push(AbiParam::new(*param));
        }
        sig.returns.push(AbiParam::new(ret));
        let func_id = self
            .jit
            .declare_function(symbol, Linkage::Import, &sig)
            .map_err(|e| LoweringError::Jit(format!("declare import `{symbol}` failed: {e}")))?;
        let fref = self.jit.declare_func_in_func(func_id, self.fb.func);
        self.import_refs.insert(cache_key, fref);
        Ok(fref)
    }
}

/// Maps a FIR unary math op to the imported `f32` host symbol used by lowering.
///
/// Returns `None` when the operation is not yet supported in the current
/// subset-lowering implementation.
fn unary_math_symbol_f32(math: fir::FirMathOp) -> Option<&'static str> {
    Some(match math {
        fir::FirMathOp::Sin => "sinf",
        fir::FirMathOp::Cos => "cosf",
        fir::FirMathOp::Acos => "acosf",
        fir::FirMathOp::Asin => "asinf",
        fir::FirMathOp::Atan => "atanf",
        fir::FirMathOp::Tan => "tanf",
        fir::FirMathOp::Exp => "expf",
        fir::FirMathOp::Log => "logf",
        fir::FirMathOp::Log10 => "log10f",
        fir::FirMathOp::Sqrt => "sqrtf",
        fir::FirMathOp::Abs => "fabsf",
        fir::FirMathOp::Floor => "floorf",
        fir::FirMathOp::Ceil => "ceilf",
        fir::FirMathOp::Rint => "rintf",
        fir::FirMathOp::Round => "roundf",
        _ => return None,
    })
}

/// Maps a FIR unary math op to the imported `f64` host symbol used by lowering.
///
/// Returns `None` when the operation is not yet supported in the current
/// subset-lowering implementation.
fn unary_math_symbol_f64(math: fir::FirMathOp) -> Option<&'static str> {
    Some(match math {
        fir::FirMathOp::Sin => "sin",
        fir::FirMathOp::Cos => "cos",
        fir::FirMathOp::Acos => "acos",
        fir::FirMathOp::Asin => "asin",
        fir::FirMathOp::Atan => "atan",
        fir::FirMathOp::Tan => "tan",
        fir::FirMathOp::Exp => "exp",
        fir::FirMathOp::Log => "log",
        fir::FirMathOp::Log10 => "log10",
        fir::FirMathOp::Sqrt => "sqrt",
        fir::FirMathOp::Abs => "fabs",
        fir::FirMathOp::Floor => "floor",
        fir::FirMathOp::Ceil => "ceil",
        fir::FirMathOp::Rint => "rint",
        fir::FirMathOp::Round => "round",
        _ => return None,
    })
}

/// Maps a FIR binary math op to the imported `f32` host symbol used by lowering.
fn binary_math_symbol_f32(math: fir::FirMathOp) -> Option<&'static str> {
    Some(match math {
        fir::FirMathOp::Pow => "powf",
        fir::FirMathOp::Min => "fminf",
        fir::FirMathOp::Max => "fmaxf",
        fir::FirMathOp::Atan2 => "atan2f",
        fir::FirMathOp::Fmod => "fmodf",
        fir::FirMathOp::Remainder => "remainderf",
        _ => return None,
    })
}

/// Maps a FIR binary math op to the imported `f64` host symbol used by lowering.
fn binary_math_symbol_f64(math: fir::FirMathOp) -> Option<&'static str> {
    Some(match math {
        fir::FirMathOp::Pow => "pow",
        fir::FirMathOp::Min => "fmin",
        fir::FirMathOp::Max => "fmax",
        fir::FirMathOp::Atan2 => "atan2",
        fir::FirMathOp::Fmod => "fmod",
        fir::FirMathOp::Remainder => "remainder",
        _ => return None,
    })
}

/// Attempts to lower the FIR `compute` body into the current Cranelift subset.
///
/// Returns `Ok(true)` when lowering succeeds and a real body is emitted.
///
/// This function assumes the caller already created and switched to a valid
/// Cranelift entry block with function params matching the `compute` ABI.
/// It binds FIR arguments (`dsp`, `count`, `inputs`, `outputs`) into the local
/// lowering environment before recursively lowering the statement body.
///
/// Any unsupported FIR node shape returns `LoweringError::Unsupported`, which
/// the caller may convert into a controlled stub fallback.
fn try_lower_compute_body(
    store: &FirStore,
    jit: &mut JITModule,
    fb: &mut FunctionBuilder<'_>,
    struct_layout: &StructLayoutPlan,
    ptr_ty: Type,
    compute_decl: FirId,
    static_data_ids: &HashMap<String, DataId>,
) -> Result<bool, LoweringError> {
    let (args, body) = match match_fir(store, compute_decl) {
        FirMatch::DeclareFun {
            args,
            body: Some(body),
            ..
        } => (args, body),
        other => {
            return Err(LoweringError::Unsupported(format!(
                "`compute` declaration shape unsupported: {other:?}"
            )));
        }
    };

    let entry = fb
        .current_block()
        .ok_or_else(|| LoweringError::Jit("missing active entry block".to_string()))?;
    let params = fb.block_params(entry).to_vec();
    if params.len() != args.len() {
        return Err(LoweringError::Unsupported(format!(
            "compute arg count mismatch: clif={} fir={}",
            params.len(),
            args.len()
        )));
    }
    let mut vars = HashMap::new();
    for (arg, value) in args.iter().zip(params) {
        let lowered = match arg.typ {
            FirType::Ptr(ref inner) => LoweredExpr::Ptr {
                value,
                pointee: Some(FirTypeRef::from_fir_type(inner)),
            },
            FirType::Obj | FirType::UI | FirType::Meta | FirType::Sound => LoweredExpr::Ptr {
                value,
                pointee: None,
            },
            _ => LoweredExpr::Scalar(value),
        };
        vars.insert(arg.name.clone(), lowered);
    }

    let mut lowering = ComputeLowering {
        store,
        jit,
        fb,
        struct_layout,
        ptr_ty,
        vars,
        import_refs: HashMap::new(),
        static_data_ids,
        _marker: std::marker::PhantomData,
    };
    lowering.lower_stmt(body)?;
    if !is_return_terminated(lowering.fb) {
        emit_return_stub(lowering.fb);
    }
    Ok(true)
}

/// Fast pre-check: returns `true` when the current subset matcher accepts the
/// FIR `compute` body, `false` when the backend should fall back to a stub.
///
/// This is implemented as a thin wrapper over
/// [`compute_body_subset_gap_reason_from_compute_decl`] so the backend can keep
/// a cheap boolean decision while diagnostics tooling can request the reason.
fn compute_body_matches_current_subset(store: &FirStore, compute_decl: FirId) -> bool {
    compute_body_subset_gap_reason_from_compute_decl(store, compute_decl).is_none()
}

/// Returns the first subset-gap reason for a FIR `compute` declaration.
///
/// `None` means the `compute` body matches the currently supported lowering
/// subset. `Some(reason)` captures the first unsupported shape encountered while
/// recursively walking statements/expressions.
///
/// The reason string is intentionally human-readable and may contain FIR debug
/// formatting; it is meant for diagnostics and prioritization, not a stable ABI.
fn compute_body_subset_gap_reason_from_compute_decl(
    store: &FirStore,
    compute_decl: FirId,
) -> Option<String> {
    let body = match match_fir(store, compute_decl) {
        FirMatch::DeclareFun {
            body: Some(body), ..
        } => body,
        other => return Some(format!("unsupported compute declaration shape: {other:?}")),
    };
    subset_stmt_gap_reason(store, body)
}

/// Recursive subset matcher for FIR statements used by stub-fallback diagnostics.
///
/// The function returns the first unsupported statement/expression shape found
/// in depth-first order. This "first gap" policy keeps diagnostics concise and
/// deterministic, which is useful for corpus scans and progress tracking.
fn subset_stmt_gap_reason(store: &FirStore, id: FirId) -> Option<String> {
    match match_fir(store, id) {
        FirMatch::Block(items) => items
            .into_iter()
            .find_map(|x| subset_stmt_gap_reason(store, x)),
        FirMatch::DeclareVar {
            access: AccessType::Stack,
            init: Some(init),
            ..
        } => subset_expr_gap_reason(store, init),
        FirMatch::DeclareVar {
            access: AccessType::Stack,
            init: None,
            ..
        } => None,
        FirMatch::Label(_) => None,
        FirMatch::StoreVar {
            access: AccessType::Struct,
            value,
            ..
        } => subset_expr_gap_reason(store, value),
        FirMatch::StoreVar {
            access: AccessType::Stack | AccessType::Loop,
            value,
            ..
        } => subset_expr_gap_reason(store, value),
        FirMatch::ShiftArrayVar {
            access: AccessType::Struct,
            ..
        } => None,
        FirMatch::If {
            cond,
            then_block,
            else_block,
        } => subset_expr_gap_reason(store, cond)
            .or_else(|| subset_stmt_gap_reason(store, then_block))
            .or_else(|| else_block.and_then(|b| subset_stmt_gap_reason(store, b))),
        FirMatch::Control { cond, stmt } => {
            subset_expr_gap_reason(store, cond).or_else(|| subset_stmt_gap_reason(store, stmt))
        }
        FirMatch::Switch {
            cond,
            cases,
            default,
        } => subset_expr_gap_reason(store, cond)
            .or_else(|| {
                cases
                    .into_iter()
                    .find_map(|(_, stmt)| subset_stmt_gap_reason(store, stmt))
            })
            .or_else(|| default.and_then(|stmt| subset_stmt_gap_reason(store, stmt))),
        FirMatch::SimpleForLoop {
            upper,
            body,
            is_reverse: false,
            ..
        } => subset_expr_gap_reason(store, upper).or_else(|| subset_stmt_gap_reason(store, body)),
        FirMatch::ForLoop {
            init,
            end,
            step,
            body,
            ..
        } => subset_expr_gap_reason(store, init)
            .or_else(|| subset_expr_gap_reason(store, end))
            .or_else(|| subset_expr_gap_reason(store, step))
            .or_else(|| subset_stmt_gap_reason(store, body)),
        FirMatch::WhileLoop { cond, body } => {
            subset_expr_gap_reason(store, cond).or_else(|| subset_stmt_gap_reason(store, body))
        }
        FirMatch::StoreTable {
            access: AccessType::Stack,
            index,
            value,
            ..
        } => subset_expr_gap_reason(store, index).or_else(|| subset_expr_gap_reason(store, value)),
        FirMatch::StoreTable {
            access: AccessType::Struct,
            index,
            value,
            ..
        } => subset_expr_gap_reason(store, index).or_else(|| subset_expr_gap_reason(store, value)),
        FirMatch::Drop(v) => subset_expr_gap_reason(store, v),
        FirMatch::NullStatement | FirMatch::Return(None) => None,
        other => Some(format!("unsupported stmt variant in subset: {other:?}")),
    }
}

/// Recursive subset matcher for FIR expressions used by stub-fallback diagnostics.
///
/// This matcher intentionally mirrors the expression coverage expected by the
/// current lowering implementation (`ComputeLowering::lower_expr` and friends).
/// When new lowering support is added, this function should be updated in the
/// same change so subset pre-checks and diagnostics stay aligned.
fn subset_expr_gap_reason(store: &FirStore, id: FirId) -> Option<String> {
    match match_fir(store, id) {
        FirMatch::Int32 { .. }
        | FirMatch::Bool { .. }
        | FirMatch::Float32 { .. }
        | FirMatch::Float64 { .. } => None,
        FirMatch::LoadVar {
            access: AccessType::Stack | AccessType::FunArgs | AccessType::Loop | AccessType::Struct,
            ..
        } => None,
        FirMatch::LoadTable {
            access: AccessType::Stack,
            index,
            ..
        } => subset_expr_gap_reason(store, index),
        FirMatch::LoadTable {
            access: AccessType::FunArgs,
            index,
            ..
        } => subset_expr_gap_reason(store, index),
        FirMatch::LoadTable {
            access: AccessType::Struct,
            index,
            ..
        } => subset_expr_gap_reason(store, index),
        FirMatch::LoadTable {
            access: AccessType::Static,
            index,
            ..
        } => subset_expr_gap_reason(store, index),
        FirMatch::BinOp { lhs, rhs, .. } => {
            subset_expr_gap_reason(store, lhs).or_else(|| subset_expr_gap_reason(store, rhs))
        }
        FirMatch::Select2 {
            cond,
            then_value,
            else_value,
            ..
        } => subset_expr_gap_reason(store, cond)
            .or_else(|| subset_expr_gap_reason(store, then_value))
            .or_else(|| subset_expr_gap_reason(store, else_value)),
        FirMatch::Neg { value, .. } => subset_expr_gap_reason(store, value),
        FirMatch::FunCall { name, args, .. } => {
            if fir::FirMathOp::from_symbol(&name).is_none()
                && !matches!(
                    name.as_str(),
                    "abs"
                        | "min_i"
                        | "max_i"
                        | "isnanf"
                        | "isnan"
                        | "isinff"
                        | "isinf"
                        | "copysignf"
                        | "copysign"
                        | "acoshf"
                        | "acosh"
                        | "asinhf"
                        | "asinh"
                        | "atanhf"
                        | "atanh"
                        | "coshf"
                        | "cosh"
                        | "sinhf"
                        | "sinh"
                        | "tanhf"
                        | "tanh"
                )
            {
                Some(format!("unsupported math call in subset: {name}"))
            } else {
                args.into_iter()
                    .find_map(|x| subset_expr_gap_reason(store, x))
            }
        }
        FirMatch::Cast { value, .. } => subset_expr_gap_reason(store, value),
        other => Some(format!("unsupported expr variant in subset: {other:?}")),
    }
}

/// Declares and defines every `AccessType::Static` table from the FIR
/// `static_decls` block as a JIT read-only data object.
///
/// Static (file-scope constant) tables are emitted as `const static` arrays in
/// the C/C++ backends.  In the Cranelift JIT they must be materialized as named
/// data sections before the `compute` function is compiled, because function
/// bodies reference them via `GlobalValue` handles obtained from the `DataId`.
///
/// Returns a map `name → DataId` that `LoadTable { access: Static }` lowering
/// uses inside `ComputeLowering::lower_expr`.
fn define_static_tables_in_jit(
    store: &FirStore,
    module: FirId,
    jit: &mut JITModule,
) -> Result<HashMap<String, DataId>, CraneliftBackendError> {
    let static_decls_block = match match_fir(store, module) {
        FirMatch::Module { static_decls, .. } => static_decls,
        _ => return Ok(HashMap::new()),
    };
    let items = match match_fir(store, static_decls_block) {
        FirMatch::Block(ids) => ids,
        _ => return Ok(HashMap::new()),
    };

    let mut result = HashMap::new();
    for id in items {
        let FirMatch::DeclareTable {
            name,
            elem_type,
            values,
            ..
        } = match_fir(store, id)
        else {
            continue;
        };
        if values.is_empty() {
            continue;
        }

        // Serialise element values to little-endian bytes.
        let bytes: Box<[u8]> = match &elem_type {
            FirType::Int32 => {
                let mut buf = Vec::with_capacity(values.len() * 4);
                for &v in &values {
                    if let FirMatch::Int32 { value, .. } = match_fir(store, v) {
                        buf.extend_from_slice(&value.to_le_bytes());
                    }
                }
                buf.into_boxed_slice()
            }
            FirType::Float32 | FirType::FaustFloat => {
                let mut buf = Vec::with_capacity(values.len() * 4);
                for &v in &values {
                    match match_fir(store, v) {
                        FirMatch::Float32 { value, .. } => {
                            buf.extend_from_slice(&value.to_le_bytes());
                        }
                        FirMatch::Float64 { value, .. } => {
                            buf.extend_from_slice(&(value as f32).to_le_bytes());
                        }
                        _ => {}
                    }
                }
                buf.into_boxed_slice()
            }
            FirType::Float64 => {
                let mut buf = Vec::with_capacity(values.len() * 8);
                for &v in &values {
                    match match_fir(store, v) {
                        FirMatch::Float64 { value, .. } => {
                            buf.extend_from_slice(&value.to_le_bytes());
                        }
                        FirMatch::Float32 { value, .. } => {
                            buf.extend_from_slice(&(value as f64).to_le_bytes());
                        }
                        _ => {}
                    }
                }
                buf.into_boxed_slice()
            }
            other => {
                return Err(CraneliftBackendError::unsupported_module_shape(format!(
                    "static table `{name}` has unsupported element type for JIT data: {other:?}"
                )));
            }
        };

        let align: u64 = match &elem_type {
            FirType::Float64 | FirType::Int64 => 8,
            _ => 4,
        };

        // Declare as local (not exported), read-only, not thread-local.
        let data_id = jit
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|e| {
                CraneliftBackendError::jit_failure(format!("declare_data `{name}` failed: {e}"))
            })?;

        let mut desc = DataDescription::new();
        desc.init = Init::Bytes { contents: bytes };
        desc.align = Some(align);
        jit.define_data(data_id, &desc).map_err(|e| {
            CraneliftBackendError::jit_failure(format!("define_data `{name}` failed: {e}"))
        })?;

        result.insert(name, data_id);
    }
    Ok(result)
}

/// Declares, defines and finalizes the exported `compute` function in the JIT.
///
/// # Behavior
/// - Creates the Cranelift function signature for the Faust `compute` ABI.
/// - Tries real subset lowering when the subset pre-check accepts the body.
/// - Emits a no-op `return` stub otherwise (or when lowering reports an
///   unsupported shape).
/// - Finalizes definitions and returns:
///   - exported symbol name,
///   - finalized function address,
///   - whether a real body was lowered.
///
/// # Why the name says `stub`
/// Historically this helper started as pure stub emission during bring-up; it
/// now owns both real subset lowering and stub fallback while keeping the same
/// outer responsibility (emit/finalize `compute`).
fn declare_compute_stub(
    module_name: &str,
    compute_decl: FirId,
    store: &FirStore,
    struct_layout: &StructLayoutPlan,
    fail_on_subset_gap: bool,
    jit: &mut JITModule,
    static_data_ids: &HashMap<String, DataId>,
) -> Result<(String, usize, bool, String), CraneliftBackendError> {
    let ptr_ty = jit.target_config().pointer_type();
    let compute_symbol_name = format!("{module_name}::compute");

    let mut ctx = jit.make_context();
    ctx.func.signature.params = vec![
        AbiParam::new(ptr_ty),     // dsp*
        AbiParam::new(types::I32), // count
        AbiParam::new(ptr_ty),     // inputs**
        AbiParam::new(ptr_ty),     // outputs**
    ];
    // void return

    let func_id = jit
        .declare_function(&compute_symbol_name, Linkage::Export, &ctx.func.signature)
        .map_err(|e| {
            CraneliftBackendError::jit_failure(format!(
                "declare_function `{compute_symbol_name}` failed: {e}"
            ))
        })?;

    let mut fb_ctx = FunctionBuilderContext::new();
    let compute_body_lowered;
    {
        let mut fb = FunctionBuilder::new(&mut ctx.func, &mut fb_ctx);
        let entry = fb.create_block();
        fb.append_block_params_for_function_params(entry);
        fb.switch_to_block(entry);
        fb.seal_block(entry);
        if compute_body_matches_current_subset(store, compute_decl) {
            match try_lower_compute_body(
                store,
                jit,
                &mut fb,
                struct_layout,
                ptr_ty,
                compute_decl,
                static_data_ids,
            ) {
                Ok(lowered) => compute_body_lowered = lowered,
                Err(LoweringError::Unsupported(reason)) => {
                    return Err(CraneliftBackendError::unsupported_module_shape(format!(
                        "Cranelift subset matcher drift: pre-check accepted `compute`, but lowering rejected it: {reason}"
                    )));
                }
                Err(LoweringError::Jit(msg)) => {
                    return Err(CraneliftBackendError::jit_failure(msg));
                }
            }
        } else {
            if fail_on_subset_gap {
                let reason = compute_body_subset_gap_reason_from_compute_decl(store, compute_decl)
                    .unwrap_or_else(|| "unknown subset-gap reason".to_owned());
                return Err(CraneliftBackendError::unsupported_module_shape(format!(
                    "Cranelift strict mode rejected fallback to compute stub: {reason}"
                )));
            }
            // Early bring-up policy: emit a valid no-op `compute` stub when the
            // FIR body exceeds the currently supported lowering subset.
            emit_return_stub(&mut fb);
            compute_body_lowered = false;
        }
        fb.seal_all_blocks();
        fb.finalize();
    }

    let compute_clif_text = ctx.func.display().to_string();

    jit.define_function(func_id, &mut ctx).map_err(|e| {
        CraneliftBackendError::jit_failure(format!(
            "define_function `{compute_symbol_name}` failed: {e}\nCLIF:\n{}",
            ctx.func.display()
        ))
    })?;
    jit.clear_context(&mut ctx);
    jit.finalize_definitions().map_err(|e| {
        CraneliftBackendError::jit_failure(format!("finalize_definitions failed: {e}"))
    })?;
    let addr = jit.get_finalized_function(func_id) as usize;
    Ok((
        compute_symbol_name,
        addr,
        compute_body_lowered,
        compute_clif_text,
    ))
}

/// Compiles a FIR module to a Cranelift JIT module.
///
/// This is the main backend entry point used by higher-level crates (`compiler`,
/// `cranelift-ffi`, tests) to turn FIR into an owned Cranelift JIT artifact.
///
/// # What it does
/// - validates FIR module shape and locates `compute`,
/// - builds the current backend `dsp*` layout contract from FIR `globals`,
/// - initializes a Cranelift JIT module and registers required host symbols,
/// - emits and finalizes the `compute` function,
/// - returns an owned [`JitDspModule`] that keeps code memory alive.
///
/// # Lowering policy (current phase)
/// - If `compute` matches the currently supported FIR subset, the backend emits
///   a real lowered body and `JitDspModule::compute_body_lowered()` returns
///   `true`.
/// - Otherwise:
///   - when `options.fail_on_subset_gap == false` (default), the backend emits
///     a valid no-op `compute` stub and returns success with
///     `compute_body_lowered() == false`;
///   - when `options.fail_on_subset_gap == true`, compilation fails with
///     `UnsupportedModuleShape`.
///
/// This "compile-success + stub fallback" policy is intentional during bring-up
/// because it allows end-to-end integration and corpus diagnostics to progress
/// while lowering coverage is expanded.
///
/// # Errors
/// Returns [`CraneliftBackendError`] for:
/// - invalid FIR module/`compute` shapes,
/// - missing `compute`,
/// - Cranelift JIT initialization/verification/finalization failures.
pub fn generate_cranelift_module(
    store: &FirStore,
    module: FirId,
    options: &CraneliftOptions,
) -> Result<JitDspModule, CraneliftBackendError> {
    let (module_name, compute_decl) = find_module_and_compute(store, module)?;
    let mut jit_builder = make_jit_builder(options)?;
    register_host_symbols(&mut jit_builder);
    let mut jit = JITModule::new(jit_builder);
    let ptr_size = jit.target_config().pointer_type().bytes();
    let struct_layout = build_struct_layout_for_module(store, module, ptr_size)?;
    // Define file-scope static tables as JIT read-only data objects before
    // compiling `compute`, so function bodies can reference them by DataId.
    let static_data_ids = define_static_tables_in_jit(store, module, &mut jit)?;
    let (compute_symbol_name, compute_entry_addr, compute_body_lowered, compute_clif_text) =
        declare_compute_stub(
            &module_name,
            compute_decl,
            store,
            &struct_layout,
            options.fail_on_subset_gap,
            &mut jit,
            &static_data_ids,
        )?;
    if compute_entry_addr == 0 {
        return Err(CraneliftBackendError::jit_failure(
            "finalized compute symbol address is null",
        ));
    }

    let generated_functions_clif = vec![(compute_symbol_name.clone(), compute_clif_text)];
    Ok(JitDspModule {
        module_name,
        compute_symbol_name,
        compute_entry_addr,
        compute_body_lowered,
        generated_functions_clif,
        struct_layout,
        jit_module: jit,
    })
}

/// Diagnoses why the current Cranelift `compute` subset matcher would fall back
/// to the no-op stub for a given FIR module.
///
/// Returns `Ok(None)` when the `compute` body matches the current lowering
/// subset, and `Ok(Some(reason))` otherwise.
///
/// # Intended use
/// This helper is for diagnostics/tooling (tests, temporary corpus scanners,
/// future `xtask` checks), not for production runtime decisions.
///
/// The returned reason is intentionally human-readable and may include FIR
/// debug formatting (for example unsupported node variants). It is useful for
/// prioritizing backend work, but should not be treated as a stable machine
/// interface.
pub fn diagnose_cranelift_compute_subset_gap(
    store: &FirStore,
    module: FirId,
) -> Result<Option<String>, CraneliftBackendError> {
    let (_module_name, compute_decl) = find_module_and_compute(store, module)?;
    Ok(compute_body_subset_gap_reason_from_compute_decl(
        store,
        compute_decl,
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        BACKEND_NAME, CraneliftBackendErrorCode, CraneliftOptions, StructFieldKind, backend_id,
        generate_cranelift_module,
    };
    use crate::fixtures::build_sine_phasor_test_module;
    use fir::{AccessType, FirBinOp, FirBuilder, FirId, FirType, NamedType};

    #[test]
    /// Verifies the public backend identifier remains stable.
    fn backend_id_is_stable() {
        assert_eq!(BACKEND_NAME, "cranelift");
        assert_eq!(backend_id(), "cranelift");
    }

    #[test]
    /// Verifies non-module FIR roots are rejected with the stable error code.
    fn compile_rejects_non_module_root() {
        let mut store = fir::FirStore::new();
        let root = {
            let mut b = fir::FirBuilder::new(&mut store);
            b.int32(0)
        };
        let err = generate_cranelift_module(&store, root, &CraneliftOptions::default())
            .expect_err("non-module root should be rejected");
        assert_eq!(err.code, CraneliftBackendErrorCode::UnsupportedModuleShape);
        assert!(err.to_string().contains("FRS-CGEN-CLIF-0002"));
    }

    #[test]
    /// Verifies a representative fixture produces a live finalized compute symbol.
    fn compile_module_emits_real_cranelift_compute_stub() {
        let (store, module) = build_sine_phasor_test_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("sine phasor fixture should compile to a Cranelift compute stub");
        assert_eq!(compiled.module_name(), "mydsp");
        assert_eq!(compiled.compute_symbol_name(), "mydsp::compute");
        assert!(compiled.has_compute_entry());
        assert_ne!(compiled.compute_entry_addr(), 0);
        assert!(compiled.compute_body_lowered());
        let layout = compiled.struct_layout();
        assert_eq!(layout.align_bytes(), 8);
        assert_eq!(layout.size_bytes(), 16);
        assert_eq!(layout.fields().len(), 3);
        assert_eq!(layout.field("fFreq").expect("fFreq field").offset_bytes, 0);
        assert_eq!(layout.field("fGain").expect("fGain field").offset_bytes, 4);
        assert_eq!(
            layout.field("fPhase").expect("fPhase field").offset_bytes,
            8
        );
        assert!(compiled.jit_module_is_alive());
    }

    /// Builds a module whose `compute` body intentionally exceeds the current lowering subset.
    fn build_subset_gap_fun_call_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let globals = b.block(&[]);
        let dsp_struct = b.block(&[]);

        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let x = b.float32(0.25);
        // Deliberately unsupported in current subset matcher.
        let y = b.fun_call("std::erf", &[x], FirType::FaustFloat);
        let store_out = b.store_table("output0", AccessType::Stack, i0, y);
        let loop_body = b.block(&[store_out]);
        let sample_loop = b.simple_for_loop("i0", count, loop_body, false);
        let compute_body = b.block(&[out_alias, sample_loop]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let static_decls = b.block(&[]);
        let module = b.module(
            0,
            0,
            "subset_gap_fun_call",
            dsp_struct,
            globals,
            functions,
            static_decls,
        );
        (store, module)
    }

    /// Builds a minimal `compute` body using one supported foreign math call.
    fn build_subset_supported_foreign_fun_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let globals = b.block(&[]);
        let dsp_struct = b.block(&[]);

        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let x = b.float32(0.25);
        let y = b.fun_call("isnanf", &[x], FirType::Int32);
        let y = b.cast(FirType::FaustFloat, y);
        let store_out = b.store_table("output0", AccessType::Stack, i0, y);
        let loop_body = b.block(&[store_out]);
        let sample_loop = b.simple_for_loop("i0", count, loop_body, false);
        let compute_body = b.block(&[out_alias, sample_loop]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let static_decls = b.block(&[]);
        let module = b.module(
            0,
            0,
            "subset_supported_foreign_fun",
            dsp_struct,
            globals,
            functions,
            static_decls,
        );
        (store, module)
    }

    #[test]
    /// Verifies non-strict mode falls back to the no-op compute stub on subset gaps.
    fn compile_module_falls_back_to_stub_without_strict_subset_mode() {
        let (store, module) = build_subset_gap_fun_call_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("default mode should allow subset-gap fallback");
        assert!(!compiled.compute_body_lowered());
    }

    #[test]
    /// Verifies strict mode rejects subset-gap fallback.
    fn compile_module_fails_on_subset_gap_with_strict_mode() {
        let (store, module) = build_subset_gap_fun_call_module();
        let options = CraneliftOptions {
            fail_on_subset_gap: true,
            ..CraneliftOptions::default()
        };
        let err = generate_cranelift_module(&store, module, &options)
            .expect_err("strict mode must reject subset-gap fallback");
        assert_eq!(err.code, CraneliftBackendErrorCode::UnsupportedModuleShape);
        assert!(err.message.contains("strict mode rejected fallback"));
    }

    #[test]
    fn compile_module_lowers_supported_foreign_fun_subset_call() {
        let (store, module) = build_subset_supported_foreign_fun_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("supported foreign subset call should lower");
        assert!(compiled.compute_body_lowered());
    }

    /// Builds a minimal `compute` body that should lower fully through the subset path.
    fn build_subset_lowerable_compute_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let globals = b.block(&[]);
        let dsp_struct = b.block(&[]);

        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let x = b.float32(0.5);
        let half = b.float32(0.5);
        let cond = b.binop(FirBinOp::Ge, x, half, FirType::Bool);
        let s = b.fun_call("std::sin", &[x], FirType::FaustFloat);
        let g = b.float32(0.25);
        let sg = b.binop(FirBinOp::Mul, s, g, FirType::FaustFloat);
        let y = b.select2(cond, sg, g, FirType::FaustFloat);
        let store_out = b.store_table("output0", AccessType::Stack, i0, y);
        let loop_body = b.block(&[store_out]);
        let sample_loop = b.simple_for_loop("i0", count, loop_body, false);
        let compute_body = b.block(&[out_alias, sample_loop]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let static_decls = b.block(&[]);
        let module = b.module(
            0,
            0,
            "subset_lowerable",
            dsp_struct,
            globals,
            functions,
            static_decls,
        );
        (store, module)
    }

    #[test]
    /// Verifies the current arithmetic/control subset lowers without fallback.
    fn compile_module_lowers_requested_compute_subset_body() {
        let (store, module) = build_subset_lowerable_compute_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("subset fixture should compile with body lowering");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
    }

    /// Builds a subset fixture covering stack aliases over input/output tables.
    fn build_stack_input_load_subset_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let globals = b.block(&[]);
        let dsp_struct = b.block(&[]);

        let chan0 = b.int32(0);
        let ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let in_ptr = b.load_table("inputs", AccessType::FunArgs, chan0, ptr_ty.clone());
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, chan0, ptr_ty.clone());
        let in_alias = b.declare_var("input0", ptr_ty.clone(), AccessType::Stack, Some(in_ptr));
        let out_alias = b.declare_var("output0", ptr_ty, AccessType::Stack, Some(out_ptr));
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let x = b.load_table("input0", AccessType::Stack, i0, FirType::FaustFloat);
        let half = b.float32(0.5);
        let y = b.binop(FirBinOp::Mul, x, half, FirType::FaustFloat);
        let store_out = b.store_table("output0", AccessType::Stack, i0, y);
        let loop_body = b.block(&[store_out]);
        let sample_loop = b.simple_for_loop("i0", count, loop_body, false);
        let compute_body = b.block(&[in_alias, out_alias, sample_loop]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let sd = b.block(&[]);
        let module = b.module(
            0,
            0,
            "stack_input_load_subset",
            dsp_struct,
            globals,
            functions,
            sd,
        );
        (store, module)
    }

    #[test]
    /// Verifies stack-local input/output alias lowering works in the subset path.
    fn compile_module_lowers_stack_input_load_subset_body() {
        let (store, module) = build_stack_input_load_subset_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("stack-input-load subset fixture should compile with body lowering");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
    }

    /// Builds a subset fixture covering the currently supported math intrinsics.
    fn build_math_intrinsics_subset_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let globals = b.block(&[]);
        let dsp_struct = b.block(&[]);

        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let x = b.float32(0.25);
        let y = b.float32(0.75);
        let cosx = b.fun_call("std::cos", &[x], FirType::FaustFloat);
        let ex = b.fun_call("std::exp", &[x], FirType::FaustFloat);
        let sqrt_ex = b.fun_call("std::sqrt", &[ex], FirType::FaustFloat);
        let m = b.fun_call("std::fmax", &[cosx, sqrt_ex], FirType::FaustFloat);
        let p = b.fun_call("std::pow", &[m, y], FirType::FaustFloat);
        let z = b.fun_call("std::fmod", &[p, y], FirType::FaustFloat);
        let r = b.fun_call("std::remainder", &[p, y], FirType::FaustFloat);
        let out = b.binop(FirBinOp::Add, z, r, FirType::FaustFloat);
        let store_out = b.store_table("output0", AccessType::Stack, i0, out);
        let loop_body = b.block(&[store_out]);
        let sample_loop = b.simple_for_loop("i0", count, loop_body, false);
        let compute_body = b.block(&[out_alias, sample_loop]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let sd = b.block(&[]);
        let module = b.module(
            0,
            0,
            "math_intrinsics_subset",
            dsp_struct,
            globals,
            functions,
            sd,
        );
        (store, module)
    }

    #[test]
    /// Verifies the supported math intrinsic family lowers without fallback.
    fn compile_module_lowers_common_math_intrinsics_subset() {
        let (store, module) = build_math_intrinsics_subset_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("math intrinsics subset fixture should compile with body lowering");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
    }

    fn build_label_and_uninitialized_stack_subset_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let globals = b.block(&[]);
        let dsp_struct = b.block(&[]);

        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let tmp_decl = b.declare_var("tmp", FirType::FaustFloat, AccessType::Stack, None);
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let x = b.float32(0.125);
        let store_tmp = b.store_var("tmp", AccessType::Stack, x);
        let tmp = b.load_var("tmp", AccessType::Stack, FirType::FaustFloat);
        let store_out = b.store_table("output0", AccessType::Stack, i0, tmp);
        let loop_body = b.block(&[store_tmp, store_out]);
        let sample_loop = b.simple_for_loop("i0", count, loop_body, false);
        let label_phase = b.label("signal_fir_fastlane_step2a: executable base slice");
        let label_io = b.label("io: inputs=0 outputs=1");
        let compute_body = b.block(&[label_phase, label_io, out_alias, tmp_decl, sample_loop]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let sd = b.block(&[]);
        let module = b.module(
            0,
            0,
            "labels_uninit_stack_subset",
            dsp_struct,
            globals,
            functions,
            sd,
        );
        (store, module)
    }

    #[test]
    fn compile_module_lowers_labels_and_uninitialized_stack_subset() {
        let (store, module) = build_label_and_uninitialized_stack_subset_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("label/uninitialized-stack subset fixture should compile with body lowering");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
    }

    fn build_switch_subset_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let globals = b.block(&[]);
        let dsp_struct = b.block(&[]);

        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let v0 = b.float32(0.0);
        let v1 = b.float32(1.0);
        let v2 = b.float32(2.0);
        let v3 = b.float32(3.0);
        let store_case0 = b.store_table("output0", AccessType::Stack, i0, v0);
        let store_case1 = b.store_table("output0", AccessType::Stack, i0, v1);
        let store_case2 = b.store_table("output0", AccessType::Stack, i0, v2);
        let store_default = b.store_table("output0", AccessType::Stack, i0, v3);
        let case0 = b.block(&[store_case0]);
        let case1 = b.block(&[store_case1]);
        let case2 = b.block(&[store_case2]);
        let default_case = b.block(&[store_default]);
        let switch_stmt = b.switch(
            i0,
            &[(0, case0), (1, case1), (2, case2)],
            Some(default_case),
        );
        let loop_body = b.block(&[switch_stmt]);
        let sample_loop = b.simple_for_loop("i0", count, loop_body, false);
        let compute_body = b.block(&[out_alias, sample_loop]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let static_decls = b.block(&[]);
        let module = b.module(
            0,
            0,
            "switch_subset",
            dsp_struct,
            globals,
            functions,
            static_decls,
        );
        (store, module)
    }

    #[test]
    fn compile_module_lowers_switch_subset_body() {
        let (store, module) = build_switch_subset_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("switch subset fixture should compile with body lowering");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
    }

    fn build_if_control_neg_subset_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let globals = b.block(&[]);
        let dsp_struct = b.block(&[]);

        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let one_i = b.int32(1);
        let cond = b.binop(FirBinOp::Ge, count, one_i, FirType::Bool);
        let base = b.float32(0.125);
        let neg = b.neg(base, FirType::FaustFloat);
        let store_then = b.store_table("output0", AccessType::Stack, i0, neg);
        let then_block = b.block(&[store_then]);
        let else_store = b.store_table("output0", AccessType::Stack, i0, base);
        let else_block = b.block(&[else_store]);
        let if_stmt = b.if_(cond, then_block, Some(else_block));
        let loop_body = b.block(&[if_stmt]);
        let loop_ = b.simple_for_loop("i0", count, loop_body, false);
        let compute_body = b.block(&[out_alias, loop_]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let sd = b.block(&[]);
        let module = b.module(
            0,
            0,
            "if_control_neg_subset",
            dsp_struct,
            globals,
            functions,
            sd,
        );
        (store, module)
    }

    #[test]
    fn compile_module_lowers_if_control_and_neg_subset_body() {
        let (store, module) = build_if_control_neg_subset_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("if/control/neg subset fixture should compile with body lowering");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
    }

    fn build_for_while_local_store_subset_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let globals = b.block(&[]);
        let dsp_struct = b.block(&[]);
        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let tmp0 = b.float32(0.2);
        let tmp = b.declare_var("tmp", FirType::FaustFloat, AccessType::Stack, Some(tmp0));
        let tmpv = b.load_var("tmp", AccessType::Stack, FirType::FaustFloat);
        let neg = b.neg(tmpv, FirType::FaustFloat);
        let tmp_set = b.store_var("tmp", AccessType::Stack, neg);
        let false_cond = b.bool_(false);
        let empty = b.block(&[]);
        let while_ = b.while_loop(false_cond, empty);

        let init = b.int32(0);
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let step = b.int32(1);
        let i = b.load_var("i", AccessType::Loop, FirType::Int32);
        let tmp_cur = b.load_var("tmp", AccessType::Stack, FirType::FaustFloat);
        let store_out = b.store_table("output0", AccessType::Stack, i, tmp_cur);
        let for_body = b.block(&[store_out]);
        let for_ = b.for_loop("i", init, count, step, for_body, false);

        let compute_body = b.block(&[out_alias, tmp, tmp_set, while_, for_]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let sd = b.block(&[]);
        let module = b.module(
            0,
            0,
            "for_while_local_store_subset",
            dsp_struct,
            globals,
            functions,
            sd,
        );
        (store, module)
    }

    #[test]
    fn compile_module_lowers_for_while_and_local_store_subset_body() {
        let (store, module) = build_for_while_local_store_subset_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("for/while/local-store subset fixture should compile with body lowering");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
    }

    fn build_global_table_subset_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let t0 = b.float64(0.0);
        let t1 = b.float64(1.0);
        let t2 = b.float64(2.0);
        let table = b.declare_table(
            "fTbl0",
            AccessType::Struct,
            FirType::FaustFloat,
            &[t0, t1, t2],
        );
        let globals = b.block(&[table]);
        let dsp_struct = b.block(&[]);

        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let zero = b.int32(0);
        let read0 = b.load_table("fTbl0", AccessType::Struct, zero, FirType::FaustFloat);
        let write0 = b.store_table("fTbl0", AccessType::Struct, zero, read0);
        let i = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let out_val = b.load_table("fTbl0", AccessType::Struct, zero, FirType::FaustFloat);
        let store_out = b.store_table("output0", AccessType::Stack, i, out_val);
        let loop_body = b.block(&[write0, store_out]);
        let loop_ = b.simple_for_loop("i0", count, loop_body, false);
        let compute_body = b.block(&[out_alias, loop_]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let static_decls = b.block(&[]);
        let module = b.module(
            0,
            0,
            "global_table_subset",
            dsp_struct,
            globals,
            functions,
            static_decls,
        );
        (store, module)
    }

    fn build_struct_array_var_subset_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let z0 = b.float32(0.0);
        let z1 = b.float32(0.0);
        let init = b.value_array(&[z0, z1], FirType::Array(Box::new(FirType::Float32), 2));
        let rec = b.declare_var(
            "fRec0",
            FirType::Array(Box::new(FirType::Float32), 2),
            AccessType::Struct,
            Some(init),
        );
        let globals = b.block(&[]);
        let dsp_struct = b.block(&[rec]);

        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let zero = b.int32(0);
        let one = b.int32(1);
        let prev = b.load_table("fRec0", AccessType::Struct, one, FirType::Float32);
        let write_cur = b.store_table("fRec0", AccessType::Struct, zero, prev);
        let outv = b.load_table("fRec0", AccessType::Struct, zero, FirType::FaustFloat);
        let store_out = b.store_table("output0", AccessType::Stack, i0, outv);
        let loop_body = b.block(&[write_cur, store_out]);
        let sample_loop = b.simple_for_loop("i0", count, loop_body, false);
        let compute_body = b.block(&[out_alias, sample_loop]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let sd = b.block(&[]);
        let module = b.module(
            0,
            0,
            "struct_array_var_subset",
            dsp_struct,
            globals,
            functions,
            sd,
        );
        (store, module)
    }

    fn build_int32_and_subset_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let iota_init = b.int32(0);
        let iota = b.declare_var("fIOTA", FirType::Int32, AccessType::Struct, Some(iota_init));
        let globals = b.block(&[]);
        let dsp_struct = b.block(&[iota]);

        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let cur = b.load_var("fIOTA", AccessType::Struct, FirType::Int32);
        let mask = b.int32(16383);
        let masked = b.binop(FirBinOp::And, cur, mask, FirType::Int32);
        let store_iota = b.store_var("fIOTA", AccessType::Struct, masked);
        let zero = b.float32(0.0);
        let store_out = b.store_table("output0", AccessType::Stack, i0, zero);
        let loop_body = b.block(&[store_iota, store_out]);
        let sample_loop = b.simple_for_loop("i0", count, loop_body, false);
        let compute_body = b.block(&[out_alias, sample_loop]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let static_decls = b.block(&[]);
        let module = b.module(
            0,
            0,
            "int32_and_subset",
            dsp_struct,
            globals,
            functions,
            static_decls,
        );
        (store, module)
    }

    #[test]
    fn compile_module_lowers_global_struct_table_subset_body() {
        let (store, module) = build_global_table_subset_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("global-struct-table subset fixture should compile with body lowering");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
        let table = compiled
            .struct_layout()
            .field("fTbl0")
            .expect("table field in layout");
        assert!(matches!(
            &table.kind,
            StructFieldKind::Table {
                elem_type: FirType::FaustFloat,
                len: 3
            }
        ));
    }

    #[test]
    fn compile_module_lowers_struct_array_var_subset_body() {
        let (store, module) = build_struct_array_var_subset_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("struct-array var subset fixture should compile with body lowering");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
        let field = compiled
            .struct_layout()
            .field("fRec0")
            .expect("array-backed struct field in layout");
        assert!(matches!(
            &field.kind,
            StructFieldKind::Table {
                elem_type: FirType::Float32,
                len: 2
            }
        ));
    }

    #[test]
    fn compile_module_lowers_int32_and_subset_body() {
        let (store, module) = build_int32_and_subset_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("int32-and subset fixture should compile with body lowering");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
    }

    fn build_globals_with_helper_prototype_subset_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let init = b.float64(0.5);
        let gain = b.declare_var("fGain", FirType::FaustFloat, AccessType::Struct, Some(init));
        let helper_args = [
            NamedType {
                name: "arg0".to_string(),
                typ: FirType::FaustFloat,
            },
            NamedType {
                name: "arg1".to_string(),
                typ: FirType::FaustFloat,
            },
        ];
        let helper_proto = b.declare_fun(
            "fmin",
            FirType::Fun {
                args: vec![FirType::FaustFloat, FirType::FaustFloat],
                ret: Box::new(FirType::FaustFloat),
            },
            &helper_args,
            None,
            false,
        );
        let globals = b.block(&[gain, helper_proto]);
        let dsp_struct = b.block(&[]);

        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let g = b.load_var("fGain", AccessType::Struct, FirType::FaustFloat);
        let store_out = b.store_table("output0", AccessType::Stack, i0, g);
        let loop_body = b.block(&[store_out]);
        let loop_ = b.simple_for_loop("i0", count, loop_body, false);
        let compute_body = b.block(&[out_alias, loop_]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let sd = b.block(&[]);
        let module = b.module(
            0,
            0,
            "globals_helper_proto_subset",
            dsp_struct,
            globals,
            functions,
            sd,
        );
        (store, module)
    }

    #[test]
    fn compile_module_ignores_helper_prototypes_in_globals_layout() {
        let (store, module) = build_globals_with_helper_prototype_subset_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("helper prototypes in globals should be ignored for dsp* layout");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
        let layout = compiled.struct_layout();
        assert!(layout.field("fGain").is_some());
        assert!(layout.field("fmin").is_none());
    }

    fn build_shift_array_var_struct_subset_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let z = b.float32(0.0);
        let o = b.float32(1.0);
        let t = b.float32(2.0);
        let tbl = b.declare_table("hist", AccessType::Struct, FirType::FaustFloat, &[z, o, t]);
        let globals = b.block(&[tbl]);
        let dsp_struct = b.block(&[]);

        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let idx0 = b.int32(0);
        let sample = b.load_table("hist", AccessType::Struct, idx0, FirType::FaustFloat);
        let push = b.store_table("hist", AccessType::Struct, idx0, sample);
        let shift = b.shift_array_var("hist", AccessType::Struct, 2);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let outv = b.load_table("hist", AccessType::Struct, idx0, FirType::FaustFloat);
        let store_out = b.store_table("output0", AccessType::Stack, i0, outv);
        let loop_body = b.block(&[shift, push, store_out]);
        let loop_ = b.simple_for_loop("i0", count, loop_body, false);
        let compute_body = b.block(&[out_alias, loop_]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let sd = b.block(&[]);
        let module = b.module(
            0,
            0,
            "shift_array_var_struct_subset",
            dsp_struct,
            globals,
            functions,
            sd,
        );
        (store, module)
    }

    fn build_int_to_float_cast_subset_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let dsp_struct = b.block(&[]);
        let globals = b.block(&[]);

        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let one = b.int32(1);
        let sum = b.binop(FirBinOp::Add, i0, one, FirType::Int32);
        let sample = b.cast(FirType::Float32, sum);
        let store_out = b.store_table("output0", AccessType::Stack, i0, sample);
        let loop_body = b.block(&[store_out]);
        let loop_ = b.simple_for_loop("i0", count, loop_body, false);
        let compute_body = b.block(&[out_alias, loop_]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let sd = b.block(&[]);
        let module = b.module(
            0,
            0,
            "int_to_float_cast_subset",
            dsp_struct,
            globals,
            functions,
            sd,
        );
        (store, module)
    }

    fn build_float_to_int_cast_subset_module() -> (fir::FirStore, FirId) {
        let mut store = fir::FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        let zero = b.float32(10.0);
        let one = b.float32(20.0);
        let two = b.float32(30.0);
        let table = b.declare_table(
            "fTbl0",
            AccessType::Struct,
            FirType::Float32,
            &[zero, one, two],
        );
        let globals = b.block(&[table]);
        let dsp_struct = b.block(&[]);

        let out_chan = b.int32(0);
        let out_ptr_ty = FirType::Ptr(Box::new(FirType::FaustFloat));
        let out_ptr = b.load_table("outputs", AccessType::FunArgs, out_chan, out_ptr_ty.clone());
        let out_alias = b.declare_var("output0", out_ptr_ty, AccessType::Stack, Some(out_ptr));
        let count = b.load_var("count", AccessType::FunArgs, FirType::Int32);
        let i0 = b.load_var("i0", AccessType::Loop, FirType::Int32);
        let idx_f = b.float32(1.75);
        let idx_i = b.cast(FirType::Int32, idx_f);
        let sample = b.load_table("fTbl0", AccessType::Struct, idx_i, FirType::Float32);
        let store_out = b.store_table("output0", AccessType::Stack, i0, sample);
        let loop_body = b.block(&[store_out]);
        let loop_ = b.simple_for_loop("i0", count, loop_body, false);
        let compute_body = b.block(&[out_alias, loop_]);
        let compute_args = [
            NamedType {
                name: "dsp".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Obj)),
            },
            NamedType {
                name: "count".to_string(),
                typ: FirType::Int32,
            },
            NamedType {
                name: "inputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
            NamedType {
                name: "outputs".to_string(),
                typ: FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
            },
        ];
        let compute = b.declare_fun(
            "compute",
            FirType::Fun {
                args: vec![
                    FirType::Ptr(Box::new(FirType::Obj)),
                    FirType::Int32,
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                    FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat)))),
                ],
                ret: Box::new(FirType::Void),
            },
            &compute_args,
            Some(compute_body),
            false,
        );
        let functions = b.block(&[compute]);
        let sd = b.block(&[]);
        let module = b.module(
            0,
            0,
            "float_to_int_cast_subset",
            dsp_struct,
            globals,
            functions,
            sd,
        );
        (store, module)
    }

    #[test]
    fn compile_module_lowers_shift_array_var_struct_subset_body() {
        let (store, module) = build_shift_array_var_struct_subset_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("shift-array-var struct subset fixture should compile with body lowering");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
    }

    #[test]
    fn compile_module_lowers_int_to_float_cast_subset_body() {
        let (store, module) = build_int_to_float_cast_subset_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("int-to-float cast subset fixture should compile with body lowering");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
        let compute_clif = &compiled.generated_functions_clif()[0].1;
        assert!(
            compute_clif.contains("fcvt_from_sint"),
            "expected int-to-float cast lowering in CLIF, got:\n{compute_clif}"
        );
    }

    #[test]
    fn compile_module_lowers_float_to_int_cast_subset_body() {
        let (store, module) = build_float_to_int_cast_subset_module();
        let compiled = generate_cranelift_module(&store, module, &CraneliftOptions::default())
            .expect("float-to-int cast subset fixture should compile with body lowering");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
        let compute_clif = &compiled.generated_functions_clif()[0].1;
        assert!(
            compute_clif.contains("fcvt_to_sint"),
            "expected float-to-int cast lowering in CLIF, got:\n{compute_clif}"
        );
    }
}
