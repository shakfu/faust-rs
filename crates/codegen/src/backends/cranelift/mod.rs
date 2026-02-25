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
//! - Phase 1.5 bring-up: a real Cranelift JIT module is emitted for the FIR
//!   module `compute` entry point, but the emitted `compute` body is currently
//!   a no-op stub (`return`) while FIR body lowering is implemented
//!   incrementally.
//! - This validates Cranelift toolchain integration, symbol declaration,
//!   function definition/finalization, and module ownership.

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{AbiParam, FuncRef, InstBuilder, MemFlags, Type, Value, types};
use cranelift_codegen::settings;
use cranelift_codegen::settings::Configurable;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_jit::{JITBuilder, JITModule};
use cranelift_module::{Linkage, Module, default_libcall_names};
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
    fn as_str(self) -> &'static str {
        match self {
            Self::NotImplemented => "FRS-CGEN-CLIF-0001",
            Self::UnsupportedModuleShape => "FRS-CGEN-CLIF-0002",
            Self::MissingCompute => "FRS-CGEN-CLIF-0003",
            Self::JitFailure => "FRS-CGEN-CLIF-0004",
        }
    }
}

/// Typed Cranelift backend error (scaffold).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CraneliftBackendError {
    /// Machine-readable stable backend error code.
    pub code: CraneliftBackendErrorCode,
    /// Human-readable message.
    pub message: String,
}

impl CraneliftBackendError {
    fn unsupported_module_shape(message: impl Into<String>) -> Self {
        Self {
            code: CraneliftBackendErrorCode::UnsupportedModuleShape,
            message: message.into(),
        }
    }

    fn missing_compute(message: impl Into<String>) -> Self {
        Self {
            code: CraneliftBackendErrorCode::MissingCompute,
            message: message.into(),
        }
    }

    fn jit_failure(message: impl Into<String>) -> Self {
        Self {
            code: CraneliftBackendErrorCode::JitFailure,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for CraneliftBackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for CraneliftBackendError {}

/// Compiled JIT module handle for the Cranelift backend bring-up path.
///
/// Current contents:
/// - owned Cranelift JIT module (keeps finalized code alive),
/// - finalized `compute` symbol address (opaque, not yet invoked here),
/// - module/function names for debug/test assertions.
///
/// API mapping status: `adapted`.
pub struct JitDspModule {
    module_name: String,
    compute_symbol_name: String,
    compute_entry_addr: usize,
    compute_body_lowered: bool,
    struct_layout: StructLayoutPlan,
    jit_module: JITModule,
}

impl std::fmt::Debug for JitDspModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JitDspModule")
            .field("module_name", &self.module_name)
            .field("compute_symbol_name", &self.compute_symbol_name)
            .field("compute_entry_addr", &self.compute_entry_addr)
            .finish()
    }
}

impl JitDspModule {
    /// Returns the FIR module name captured during compilation.
    #[must_use]
    pub fn module_name(&self) -> &str {
        &self.module_name
    }

    /// Returns the finalized Cranelift symbol name used for the `compute` stub.
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

    /// Returns true when a finalized `compute` symbol address is present.
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

    /// Returns the backend `dsp*` struct layout contract derived from FIR
    /// `globals` declarations.
    #[must_use]
    pub fn struct_layout(&self) -> &StructLayoutPlan {
        &self.struct_layout
    }

    /// Internal guard used by tests to ensure the JIT module stays owned/alive.
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
    #[must_use]
    pub fn fields(&self) -> &[StructFieldLayout] {
        &self.fields
    }

    #[must_use]
    pub fn size_bytes(&self) -> u32 {
        self.size_bytes
    }

    #[must_use]
    pub fn align_bytes(&self) -> u32 {
        self.align_bytes
    }

    #[must_use]
    pub fn field(&self, name: &str) -> Option<&StructFieldLayout> {
        self.fields.iter().find(|f| f.name == name)
    }
}

/// One field in the Cranelift backend `dsp*` struct layout.
#[derive(Clone, Debug, PartialEq)]
pub struct StructFieldLayout {
    pub name: String,
    pub typ: FirType,
    pub offset_bytes: u32,
    pub size_bytes: u32,
    pub align_bytes: u32,
}

#[derive(Clone, Copy, Debug)]
struct LayoutScalar {
    size: u32,
    align: u32,
}

fn align_up(value: u32, align: u32) -> u32 {
    if align <= 1 {
        return value;
    }
    let rem = value % align;
    if rem == 0 { value } else { value + (align - rem) }
}

fn find_module_and_compute(
    store: &FirStore,
    module: FirId,
) -> Result<(String, FirId), CraneliftBackendError> {
    let (module_name, _globals, declarations) = match match_fir(store, module) {
        FirMatch::Module {
            name,
            globals,
            declarations,
            ..
        } => (name, globals, declarations),
        other => {
            return Err(CraneliftBackendError::unsupported_module_shape(format!(
                "expected FIR Module root, got {other:?} at {}",
                module.as_u32()
            )));
        }
    };

    let decls = match match_fir(store, declarations) {
        FirMatch::Block(items) => items,
        other => {
            return Err(CraneliftBackendError::unsupported_module_shape(format!(
                "module declarations must be FIR Block, got {other:?} at {}",
                declarations.as_u32()
            )));
        }
    };

    let compute_id = decls
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

fn fir_type_layout_scalar(ptr_size: u32, typ: &FirType) -> Result<LayoutScalar, CraneliftBackendError> {
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

fn build_struct_layout_for_module(
    store: &FirStore,
    module: FirId,
    ptr_size: u32,
) -> Result<StructLayoutPlan, CraneliftBackendError> {
    let globals = match match_fir(store, module) {
        FirMatch::Module { globals, .. } => globals,
        other => {
            return Err(CraneliftBackendError::unsupported_module_shape(format!(
                "expected FIR Module root for struct layout, got {other:?} at {}",
                module.as_u32()
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
    for item in global_items {
        match match_fir(store, item) {
            FirMatch::DeclareVar {
                name,
                typ,
                access: AccessType::Struct,
                ..
            } => {
                let scalar = fir_type_layout_scalar(ptr_size, &typ)?;
                offset = align_up(offset, scalar.align);
                fields.push(StructFieldLayout {
                    name,
                    typ,
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
            FirMatch::DeclareVar { access, name, .. } => {
                return Err(CraneliftBackendError::unsupported_module_shape(format!(
                    "unsupported global variable access class for Cranelift dsp* layout: {name} ({access:?})"
                )));
            }
            FirMatch::DeclareTable { name, .. } => {
                return Err(CraneliftBackendError::unsupported_module_shape(format!(
                    "Struct layout contract v1 bring-up does not support global tables yet: `{name}`"
                )));
            }
            FirMatch::NullDeclareVar => {}
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

fn register_host_symbols(jit_builder: &mut JITBuilder) {
    jit_builder.symbol("sinf", host_sinf as *const u8);
    jit_builder.symbol("sin", host_sin as *const u8);
}

#[derive(Clone, Copy, Debug)]
enum LoweredExpr {
    Scalar(Value),
    Ptr(Value),
}

impl LoweredExpr {
    fn value(self) -> Value {
        match self {
            Self::Scalar(v) | Self::Ptr(v) => v,
        }
    }

    fn ptr(self) -> Option<Value> {
        match self {
            Self::Ptr(v) => Some(v),
            Self::Scalar(_) => None,
        }
    }
}

#[derive(Debug)]
enum LoweringError {
    Unsupported(String),
    Jit(String),
}

fn emit_return_stub(fb: &mut FunctionBuilder<'_>) {
    fb.ins().return_(&[]);
}

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

struct ComputeLowering<'a, 'b, 'c> {
    store: &'a FirStore,
    jit: &'a mut JITModule,
    fb: &'a mut FunctionBuilder<'b>,
    ptr_ty: Type,
    vars: HashMap<String, LoweredExpr>,
    sinf_ref: Option<FuncRef>,
    sin_ref: Option<FuncRef>,
    _marker: std::marker::PhantomData<&'c ()>,
}

impl<'a, 'b, 'c> ComputeLowering<'a, 'b, 'c> {
    fn bool_b1_to_i8(&mut self, b1: Value) -> Value {
        let one = self.fb.ins().iconst(types::I8, 1);
        let zero = self.fb.ins().iconst(types::I8, 0);
        self.fb.ins().select(b1, one, zero)
    }

    fn int_cmp_to_i8(&mut self, cc: IntCC, lhs: Value, rhs: Value) -> Value {
        let b1 = self.fb.ins().icmp(cc, lhs, rhs);
        self.bool_b1_to_i8(b1)
    }

    fn float_cmp_to_i8(
        &mut self,
        cc: cranelift_codegen::ir::condcodes::FloatCC,
        lhs: Value,
        rhs: Value,
    ) -> Value {
        let b1 = self.fb.ins().fcmp(cc, lhs, rhs);
        self.bool_b1_to_i8(b1)
    }

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
                self.vars.insert(name, init_v);
                Ok(())
            }
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
        let next = self.fb.ins().iadd(i_val, one);
        self.fb.ins().jump(header, &[next]);
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

    fn lower_store_table_stack(
        &mut self,
        name: &str,
        index: FirId,
        value: FirId,
    ) -> Result<(), LoweringError> {
        let base_ptr = self.vars.get(name).and_then(|v| v.ptr()).ok_or_else(|| {
            LoweringError::Unsupported(format!("stack pointer alias `{name}` not found"))
        })?;
        let index_v = self.lower_expr(index, Some(&FirType::Int32))?.value();
        let value_v = self.lower_expr(value, None)?.value();
        let elem_ty = self.fb.func.dfg.value_type(value_v);
        let elem_size = i64::from(elem_ty.bytes());
        let addr = self.indexed_addr(base_ptr, index_v, elem_size);
        self.fb.ins().store(MemFlags::new(), value_v, addr, 0);
        Ok(())
    }

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
            FirMatch::LoadVar { name, .. } => self.vars.get(&name).copied().ok_or_else(|| {
                LoweringError::Unsupported(format!("load of unknown variable `{name}`"))
            }),
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
                Ok(LoweredExpr::Ptr(loaded))
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
            FirMatch::BinOp { op, lhs, rhs, typ } => self.lower_binop(op, lhs, rhs, &typ),
            FirMatch::FunCall { name, args, typ } => self.lower_fun_call(&name, &args, &typ),
            other => Err(LoweringError::Unsupported(format!(
                "unsupported FIR expression in Cranelift subset lowering: {other:?}; expected={expected:?}"
            ))),
        }
    }

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
        let out = match typ {
            FirType::Int32 => match op {
                FirBinOp::Add => self.fb.ins().iadd(l, r),
                FirBinOp::Sub => self.fb.ins().isub(l, r),
                FirBinOp::Mul => self.fb.ins().imul(l, r),
                FirBinOp::Div => self.fb.ins().sdiv(l, r),
                FirBinOp::Eq => self.int_cmp_to_i8(IntCC::Equal, l, r),
                FirBinOp::Ne => self.int_cmp_to_i8(IntCC::NotEqual, l, r),
                FirBinOp::Lt => self.int_cmp_to_i8(IntCC::SignedLessThan, l, r),
                FirBinOp::Le => self.int_cmp_to_i8(IntCC::SignedLessThanOrEqual, l, r),
                FirBinOp::Gt => self.int_cmp_to_i8(IntCC::SignedGreaterThan, l, r),
                FirBinOp::Ge => self.int_cmp_to_i8(IntCC::SignedGreaterThanOrEqual, l, r),
                _ => {
                    return Err(LoweringError::Unsupported(format!(
                        "unsupported Int32 binop in subset lowering: {op:?}"
                    )));
                }
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

    fn lower_fun_call(
        &mut self,
        name: &str,
        args: &[FirId],
        typ: &FirType,
    ) -> Result<LoweredExpr, LoweringError> {
        let math = fir::FirMathOp::from_symbol(name).ok_or_else(|| {
            LoweringError::Unsupported(format!("unsupported function call `{name}`"))
        })?;
        match (math, typ, args) {
            (fir::FirMathOp::Sin, FirType::FaustFloat, [x])
            | (fir::FirMathOp::Sin, FirType::Float32, [x]) => {
                let xv = self.lower_expr(*x, Some(typ))?.value();
                let fref = self.ensure_unary_import("sinf", types::F32)?;
                let call = self.fb.ins().call(fref, &[xv]);
                Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]))
            }
            (fir::FirMathOp::Sin, FirType::Float64, [x]) => {
                let xv = self.lower_expr(*x, Some(typ))?.value();
                let fref = self.ensure_unary_import("sin", types::F64)?;
                let call = self.fb.ins().call(fref, &[xv]);
                Ok(LoweredExpr::Scalar(self.fb.inst_results(call)[0]))
            }
            _ => Err(LoweringError::Unsupported(format!(
                "unsupported function call lowering `{name}` with typ={typ:?} args={}",
                args.len()
            ))),
        }
    }

    fn ensure_unary_import(&mut self, symbol: &str, ty: Type) -> Result<FuncRef, LoweringError> {
        let cache = if ty == types::F32 {
            &mut self.sinf_ref
        } else {
            &mut self.sin_ref
        };
        if let Some(fref) = *cache {
            return Ok(fref);
        }
        let mut sig = self.jit.make_signature();
        sig.params.push(AbiParam::new(ty));
        sig.returns.push(AbiParam::new(ty));
        let func_id = self
            .jit
            .declare_function(symbol, Linkage::Import, &sig)
            .map_err(|e| LoweringError::Jit(format!("declare import `{symbol}` failed: {e}")))?;
        let fref = self.jit.declare_func_in_func(func_id, self.fb.func);
        *cache = Some(fref);
        Ok(fref)
    }
}

fn try_lower_compute_body(
    store: &FirStore,
    jit: &mut JITModule,
    fb: &mut FunctionBuilder<'_>,
    ptr_ty: Type,
    compute_decl: FirId,
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
            FirType::Ptr(_) | FirType::Obj | FirType::UI | FirType::Meta | FirType::Sound => {
                LoweredExpr::Ptr(value)
            }
            _ => LoweredExpr::Scalar(value),
        };
        vars.insert(arg.name.clone(), lowered);
    }

    let mut lowering = ComputeLowering {
        store,
        jit,
        fb,
        ptr_ty,
        vars,
        sinf_ref: None,
        sin_ref: None,
        _marker: std::marker::PhantomData,
    };
    lowering.lower_stmt(body)?;
    if !is_return_terminated(lowering.fb) {
        emit_return_stub(lowering.fb);
    }
    Ok(true)
}

fn compute_body_matches_current_subset(store: &FirStore, compute_decl: FirId) -> bool {
    let body = match match_fir(store, compute_decl) {
        FirMatch::DeclareFun {
            body: Some(body), ..
        } => body,
        _ => return false,
    };
    subset_stmt_shape(store, body)
}

fn subset_stmt_shape(store: &FirStore, id: FirId) -> bool {
    match match_fir(store, id) {
        FirMatch::Block(items) => items.into_iter().all(|x| subset_stmt_shape(store, x)),
        FirMatch::DeclareVar {
            access: AccessType::Stack,
            init: Some(init),
            ..
        } => subset_expr_shape(store, init),
        FirMatch::SimpleForLoop {
            upper,
            body,
            is_reverse: false,
            ..
        } => subset_expr_shape(store, upper) && subset_stmt_shape(store, body),
        FirMatch::StoreTable {
            access: AccessType::Stack,
            index,
            value,
            ..
        } => subset_expr_shape(store, index) && subset_expr_shape(store, value),
        FirMatch::Drop(v) => subset_expr_shape(store, v),
        FirMatch::NullStatement | FirMatch::Return(None) => true,
        _ => false,
    }
}

fn subset_expr_shape(store: &FirStore, id: FirId) -> bool {
    match match_fir(store, id) {
        FirMatch::Int32 { .. }
        | FirMatch::Bool { .. }
        | FirMatch::Float32 { .. }
        | FirMatch::Float64 { .. } => true,
        FirMatch::LoadVar {
            access: AccessType::Stack | AccessType::FunArgs | AccessType::Loop,
            ..
        } => true,
        FirMatch::LoadTable {
            access: AccessType::FunArgs,
            index,
            ..
        } => subset_expr_shape(store, index),
        FirMatch::BinOp { lhs, rhs, .. } => {
            subset_expr_shape(store, lhs) && subset_expr_shape(store, rhs)
        }
        FirMatch::Select2 {
            cond,
            then_value,
            else_value,
            ..
        } => {
            subset_expr_shape(store, cond)
                && subset_expr_shape(store, then_value)
                && subset_expr_shape(store, else_value)
        }
        FirMatch::FunCall { name, args, .. } => {
            fir::FirMathOp::from_symbol(&name) == Some(fir::FirMathOp::Sin)
                && args.into_iter().all(|x| subset_expr_shape(store, x))
        }
        _ => false,
    }
}

fn declare_compute_stub(
    module_name: &str,
    compute_decl: FirId,
    store: &FirStore,
    jit: &mut JITModule,
) -> Result<(String, usize, bool), CraneliftBackendError> {
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
            match try_lower_compute_body(store, jit, &mut fb, ptr_ty, compute_decl) {
                Ok(lowered) => compute_body_lowered = lowered,
                Err(LoweringError::Unsupported(reason)) => {
                    let _ = reason;
                    emit_return_stub(&mut fb);
                    compute_body_lowered = false;
                }
                Err(LoweringError::Jit(msg)) => {
                    return Err(CraneliftBackendError::jit_failure(msg));
                }
            }
        } else {
            // Early bring-up policy: emit a valid no-op `compute` stub when the
            // FIR body exceeds the currently supported lowering subset.
            emit_return_stub(&mut fb);
            compute_body_lowered = false;
        }
        fb.seal_all_blocks();
        fb.finalize();
    }

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
    Ok((compute_symbol_name, addr, compute_body_lowered))
}

/// Compiles a FIR module to a Cranelift JIT module (early bring-up).
///
/// # Current behavior
/// - Validates FIR root/module shape and locates a `compute` definition.
/// - Emits a real finalized Cranelift JIT function for `compute`, but the
///   generated body is currently a no-op stub.
/// - This is the first backend implementation slice to de-risk Cranelift
///   integration before full FIR body lowering.
pub fn compile_fir_to_cranelift_jit(
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
    let (compute_symbol_name, compute_entry_addr, compute_body_lowered) =
        declare_compute_stub(&module_name, compute_decl, store, &mut jit)?;
    if compute_entry_addr == 0 {
        return Err(CraneliftBackendError::jit_failure(
            "finalized compute symbol address is null",
        ));
    }

    Ok(JitDspModule {
        module_name,
        compute_symbol_name,
        compute_entry_addr,
        compute_body_lowered,
        struct_layout,
        jit_module: jit,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        BACKEND_NAME, CraneliftBackendErrorCode, CraneliftOptions, backend_id,
        compile_fir_to_cranelift_jit,
    };
    use crate::fixtures::build_sine_phasor_test_module;
    use fir::{AccessType, FirBinOp, FirBuilder, FirId, FirType, NamedType};

    #[test]
    fn backend_id_is_stable() {
        assert_eq!(BACKEND_NAME, "cranelift");
        assert_eq!(backend_id(), "cranelift");
    }

    #[test]
    fn compile_rejects_non_module_root() {
        let mut store = fir::FirStore::new();
        let root = {
            let mut b = fir::FirBuilder::new(&mut store);
            b.int32(0)
        };
        let err = compile_fir_to_cranelift_jit(&store, root, &CraneliftOptions::default())
            .expect_err("non-module root should be rejected");
        assert_eq!(err.code, CraneliftBackendErrorCode::UnsupportedModuleShape);
        assert!(err.to_string().contains("FRS-CGEN-CLIF-0002"));
    }

    #[test]
    fn compile_module_emits_real_cranelift_compute_stub() {
        let (store, module) = build_sine_phasor_test_module();
        let compiled = compile_fir_to_cranelift_jit(&store, module, &CraneliftOptions::default())
            .expect("sine phasor fixture should compile to a Cranelift compute stub");
        assert_eq!(compiled.module_name(), "mydsp");
        assert_eq!(compiled.compute_symbol_name(), "mydsp::compute");
        assert!(compiled.has_compute_entry());
        assert_ne!(compiled.compute_entry_addr(), 0);
        assert!(!compiled.compute_body_lowered());
        let layout = compiled.struct_layout();
        assert_eq!(layout.align_bytes(), 8);
        assert_eq!(layout.size_bytes(), 16);
        assert_eq!(layout.fields().len(), 3);
        assert_eq!(
            layout.field("fFreq").expect("fFreq field").offset_bytes,
            0
        );
        assert_eq!(
            layout.field("fGain").expect("fGain field").offset_bytes,
            4
        );
        assert_eq!(
            layout.field("fPhase").expect("fPhase field").offset_bytes,
            8
        );
        assert!(compiled.jit_module_is_alive());
    }

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
        let declarations = b.block(&[compute]);
        let module = b.module("subset_lowerable", dsp_struct, globals, declarations);
        (store, module)
    }

    #[test]
    fn compile_module_lowers_requested_compute_subset_body() {
        let (store, module) = build_subset_lowerable_compute_module();
        let compiled = compile_fir_to_cranelift_jit(&store, module, &CraneliftOptions::default())
            .expect("subset fixture should compile with body lowering");
        assert!(compiled.has_compute_entry());
        assert!(compiled.compute_body_lowered());
    }
}
