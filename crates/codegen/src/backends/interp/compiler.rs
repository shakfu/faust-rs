//! FIR → FBC bytecode compiler.
//!
//! # Source provenance (C++)
//! - `compiler/generator/interpreter/interpreter_instructions.hh`
//!   (`InterpreterInstVisitor<REAL>`)
//!
//! # Design notes
//! - The C++ visitor/`accept()` pattern is replaced by exhaustive `match`
//!   dispatch on [`FirMatch`] variants obtained from [`match_fir`].
//! - Block switching (save/restore `fCurrentBlock`) is replaced by a
//!   `saved_blocks` stack using [`std::mem::replace`].
//! - The compiler owns the [`FbcBlockArena`]; [`finalize`] moves it into
//!   the result.
//!
//! # API mapping status
//! - `InterpreterInstVisitor<REAL>` → [`FirToFbcCompiler<R>`]: adapted.
//! - `gMathLibTable` → [`math_lib_lookup`]: const fn match.
//! - `gBinOpTable` → [`binop_to_fbc`]: const fn match.
//! - `fFieldTable` → [`FirToFbcCompiler::field_table`]: `HashMap<String, MemoryDesc>`.

use std::collections::HashMap;
use std::fmt;

use fir::{
    AccessType, BargraphType, ButtonType, FirId, FirMatch, FirStore, FirType, SliderType,
    UiBoxType, match_fir,
};

use super::bytecode::{
    BlockId, BlockStoreData, FbcBlock, FbcBlockArena, FbcInstruction, FbcUiInstruction,
};
use super::opcode::FbcOpcode;
use super::real::FbcReal;

// ---------------------------------------------------------------------------
// Helper types
// ---------------------------------------------------------------------------

/// Return type of [`FirToFbcCompiler::into_parts`].
pub type CompilerParts<R> = (
    FbcBlockArena<R>,
    i32,
    i32,
    Vec<FbcUiInstruction<R>>,
    HashMap<String, MemoryDesc>,
);

/// Which heap a variable is allocated in.
///
/// # Source provenance (C++)
/// - `Typed::VarType` (only `kInt32` vs everything-else distinction matters
///   for the interpreter backend).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeapType {
    Int,
    Real,
}

/// Describes a variable's location in the interpreter's dual heaps.
///
/// # Source provenance (C++)
/// - `MemoryDesc` in `struct_manager.hh` (simplified: only the fields
///   used by `InterpreterInstVisitor`).
#[derive(Clone, Debug)]
pub struct MemoryDesc {
    /// Heap offset (index into `int_heap` or `real_heap`).
    pub offset: i32,
    /// Number of elements (1 for scalars, >1 for arrays).
    pub size: i32,
    /// Whether this variable lives in the int heap or the real heap.
    pub heap_type: HeapType,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during FIR → FBC compilation.
///
/// # Source provenance (C++)
/// - `faustassert(false)` and `throw faustexception(...)` in
///   `interpreter_instructions.hh`.
#[derive(Clone, Debug)]
pub enum CompileError {
    /// A variable was used but never declared.
    UndeclaredVariable { name: String },
    /// A math function call references an unknown function.
    UnknownMathFunction { name: String },
    /// A FIR node kind is not supported by the interpreter backend.
    UnsupportedNode { description: String },
    /// `LoadVarAddress` is not supported (mirrors `faustassert(false)` in C++).
    LoadVarAddressNotSupported,
}

impl fmt::Display for CompileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UndeclaredVariable { name } => {
                write!(f, "undeclared variable: {name}")
            }
            Self::UnknownMathFunction { name } => {
                write!(f, "unknown math function: {name}")
            }
            Self::UnsupportedNode { description } => {
                write!(f, "unsupported FIR node: {description}")
            }
            Self::LoadVarAddressNotSupported => {
                write!(
                    f,
                    "LoadVarAddress is not supported by the interpreter backend"
                )
            }
        }
    }
}

impl std::error::Error for CompileError {}

// ---------------------------------------------------------------------------
// Compilation result
// ---------------------------------------------------------------------------

/// Result of a successful FIR → FBC compilation.
pub struct FbcCompileResult<R: FbcReal> {
    /// The block arena containing all compiled blocks.
    pub arena: FbcBlockArena<R>,
    /// The top-level block (entry point).
    pub entry_block: BlockId,
    /// Total int heap slots allocated.
    pub int_heap_size: i32,
    /// Total real heap slots allocated.
    pub real_heap_size: i32,
    /// Variable-to-heap-slot mapping.
    pub field_table: HashMap<String, MemoryDesc>,
    /// Collected UI instructions.
    pub ui_instructions: Vec<FbcUiInstruction<R>>,
}

// ---------------------------------------------------------------------------
// Compiler struct
// ---------------------------------------------------------------------------

/// FIR → FBC bytecode compiler.
///
/// # Source provenance (C++)
/// - `InterpreterInstVisitor<REAL>` in `interpreter_instructions.hh`
///
/// Translates FIR nodes (stored in a [`FirStore`]) into FBC bytecode
/// blocks stored in an [`FbcBlockArena`].
pub struct FirToFbcCompiler<R: FbcReal> {
    /// Block arena — all compiled blocks live here.
    arena: FbcBlockArena<R>,
    /// Block currently being compiled into.
    current_block: FbcBlock<R>,
    /// Stack of saved parent blocks for the block-switching pattern.
    saved_blocks: Vec<FbcBlock<R>>,
    /// Current allocation pointer in the real heap.
    real_heap_offset: i32,
    /// Current allocation pointer in the int heap.
    int_heap_offset: i32,
    /// Maps variable names to their heap locations.
    field_table: HashMap<String, MemoryDesc>,
    /// UI instructions collected during compilation.
    ui_instructions: Vec<FbcUiInstruction<R>>,
}

impl<R: FbcReal> FirToFbcCompiler<R> {
    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// Creates a new compiler with empty state.
    ///
    /// # Source provenance (C++)
    /// - `InterpreterInstVisitor()` constructor.
    #[must_use]
    pub fn new() -> Self {
        Self {
            arena: FbcBlockArena::new(),
            current_block: FbcBlock::new(),
            saved_blocks: Vec::new(),
            real_heap_offset: 0,
            int_heap_offset: 0,
            field_table: HashMap::new(),
            ui_instructions: Vec::new(),
        }
    }

    /// Returns the current real heap offset.
    #[must_use]
    pub fn real_heap_offset(&self) -> i32 {
        self.real_heap_offset
    }

    /// Returns the current int heap offset.
    #[must_use]
    pub fn int_heap_offset(&self) -> i32 {
        self.int_heap_offset
    }

    /// Returns a reference to the field table.
    #[must_use]
    pub fn field_table(&self) -> &HashMap<String, MemoryDesc> {
        &self.field_table
    }

    /// Compiles a single FIR node (and its transitive children) into
    /// FBC bytecode in the current block.
    pub fn compile_node(&mut self, store: &FirStore, id: FirId) -> Result<(), CompileError> {
        match match_fir(store, id) {
            // --- Values ---
            FirMatch::Int32 { value, .. } => self.compile_int32(value),
            FirMatch::Float32 { value, .. } => self.compile_float32(value),
            FirMatch::Float64 { value, .. } => self.compile_float64(value),
            FirMatch::Bool { value, .. } => self.compile_bool(value),

            // --- Variables ---
            FirMatch::LoadVar {
                ref name,
                access,
                ref typ,
            } => self.compile_load_var(store, name, access, typ),
            FirMatch::LoadTable {
                ref name,
                access,
                index,
                ref typ,
            } => self.compile_load_table(store, name, access, index, typ),
            FirMatch::LoadVarAddress { .. } => Err(CompileError::LoadVarAddressNotSupported),
            FirMatch::TeeVar {
                ref name,
                access,
                value,
                ref typ,
            } => self.compile_tee_var(store, name, access, value, typ),

            // --- Declarations ---
            FirMatch::DeclareVar {
                ref name,
                ref typ,
                access,
                init,
            } => self.compile_declare_var(store, name, typ, access, init),
            FirMatch::DeclareFun { .. } | FirMatch::NullDeclareVar => Ok(()),
            FirMatch::DeclareStructType { .. } => Ok(()),

            // --- Storage ---
            FirMatch::StoreVar {
                ref name,
                access,
                value,
            } => self.compile_store_var(store, name, access, value),
            FirMatch::StoreTable {
                ref name,
                access,
                index,
                value,
            } => self.compile_store_table(store, name, access, index, value),
            FirMatch::ShiftArrayVar {
                ref name, delay, ..
            } => self.compile_shift_array(name, delay),
            FirMatch::Drop(inner) => self.compile_node(store, inner),

            // --- Arithmetic ---
            FirMatch::BinOp { op, lhs, rhs, .. } => self.compile_binop(store, op, lhs, rhs),
            FirMatch::Neg { value, ref typ } => self.compile_neg(store, value, typ),

            // --- Cast ---
            FirMatch::Cast { ref typ, value } => self.compile_cast(store, typ, value),
            FirMatch::Bitcast { ref typ, value } => self.compile_bitcast(store, typ, value),

            // --- Control flow ---
            FirMatch::Select2 {
                cond,
                then_value,
                else_value,
                ..
            } => self.compile_select2(store, cond, then_value, else_value),
            FirMatch::If {
                cond,
                then_block,
                else_block,
            } => self.compile_if(store, cond, then_block, else_block),
            FirMatch::ForLoop {
                init,
                end,
                step,
                body,
                ..
            } => self.compile_for_loop(store, init, end, step, body),
            FirMatch::SimpleForLoop {
                ref var,
                upper,
                body,
                ..
            } => self.compile_simple_for_loop(store, var, upper, body),
            FirMatch::Block(ref stmts) => {
                let stmts = stmts.clone();
                self.compile_block(store, &stmts)
            }

            // --- Function calls ---
            FirMatch::FunCall {
                ref name, ref args, ..
            } => {
                let name = name.clone();
                let args = args.clone();
                self.compile_fun_call(store, &name, &args)
            }

            // --- UI ---
            FirMatch::OpenBox { ref typ, ref label } => self.compile_open_box(typ, label),
            FirMatch::CloseBox => self.compile_close_box(),
            FirMatch::AddButton {
                ref typ,
                ref label,
                ref var,
            } => self.compile_add_button(typ, label, var),
            FirMatch::AddSlider {
                ref typ,
                ref label,
                ref var,
                init,
                lo,
                hi,
                step,
            } => self.compile_add_slider(typ, label, var, init, lo, hi, step),
            FirMatch::AddBargraph {
                ref typ,
                ref label,
                ref var,
                lo,
                hi,
            } => self.compile_add_bargraph(typ, label, var, lo, hi),
            FirMatch::AddSoundfile {
                ref label,
                ref url,
                ref var,
            } => self.compile_add_soundfile(label, url, var),
            FirMatch::AddMetaDeclare {
                ref var,
                ref key,
                ref value,
            } => self.compile_add_meta_declare(var, key, value),

            // --- No-ops ---
            FirMatch::NullStatement
            | FirMatch::Label(_)
            | FirMatch::Return(_)
            | FirMatch::Int32Array { .. }
            | FirMatch::Float32Array { .. }
            | FirMatch::Float64Array { .. } => Ok(()),

            // --- Unsupported ---
            other => Err(CompileError::UnsupportedNode {
                description: format!("{other:?}"),
            }),
        }
    }

    /// Finalizes compilation: seals the current block with `kReturn`,
    /// allocates it in the arena, and returns the result.
    pub fn finalize(mut self) -> Result<FbcCompileResult<R>, CompileError> {
        self.current_block
            .push(FbcInstruction::new(FbcOpcode::Return));
        let entry_block = self.arena.alloc(self.current_block);
        Ok(FbcCompileResult {
            arena: self.arena,
            entry_block,
            int_heap_size: self.int_heap_offset,
            real_heap_size: self.real_heap_offset,
            field_table: self.field_table,
            ui_instructions: self.ui_instructions,
        })
    }

    /// Compiles a FIR block node as a new sub-block in the arena and returns
    /// its allocated [`BlockId`].
    ///
    /// If `block_id` does not decode as a [`FirMatch::Block`], an empty block
    /// (containing only `kReturn`) is emitted.
    ///
    /// This is the building block for [`generate_interp_module`] which compiles
    /// each named DSP section (init, compute, …) into a separate arena block.
    pub fn compile_fir_block(
        &mut self,
        store: &FirStore,
        block_id: FirId,
    ) -> Result<BlockId, CompileError> {
        let nodes = match match_fir(store, block_id) {
            FirMatch::Block(ids) => ids,
            _ => vec![],
        };
        self.begin_sub_block();
        for id in &nodes {
            self.compile_node(store, *id)?;
        }
        Ok(self.end_sub_block())
    }

    /// Compiles a list of FIR statements as a new sub-block in the arena.
    ///
    /// This is used by the interpreter backend to split a single FIR `compute`
    /// body into a control prefix block and a DSP loop block without inventing
    /// extra FIR declarations.
    pub fn compile_fir_stmt_list_block(
        &mut self,
        store: &FirStore,
        stmts: &[FirId],
    ) -> Result<BlockId, CompileError> {
        self.begin_sub_block();
        for &id in stmts {
            self.compile_node(store, id)?;
        }
        Ok(self.end_sub_block())
    }

    /// Pre-declares storage nodes from a FIR `Block` into the heap layout.
    ///
    /// This allocates entries in [`Self::field_table`] for top-level module
    /// storage (`dsp_struct`, `globals`) without emitting executable bytecode.
    /// The interpreter backend uses this to make struct/global fields visible
    /// before compiling function bodies that reference them.
    ///
    /// Only direct `DeclareVar` / `DeclareTable` items are accepted; other
    /// nodes are ignored so prototype-only `DeclareFun` entries in `globals`
    /// can coexist with storage declarations.
    pub fn predeclare_storage_block(
        &mut self,
        store: &FirStore,
        block_id: FirId,
    ) -> Result<(), CompileError> {
        let nodes = match match_fir(store, block_id) {
            FirMatch::Block(ids) => ids,
            _ => return Ok(()),
        };
        for id in nodes {
            match match_fir(store, id) {
                FirMatch::DeclareVar {
                    ref name, ref typ, ..
                } => self.predeclare_var_storage(name, typ),
                FirMatch::DeclareTable {
                    ref name,
                    ref elem_type,
                    ref values,
                    ..
                } => self.predeclare_table_storage(name, elem_type, values.len() as i32),
                FirMatch::DeclareFun { .. }
                | FirMatch::NullDeclareVar
                | FirMatch::DeclareStructType { .. } => {}
                _ => {}
            }
        }
        Ok(())
    }

    /// Allocates an empty block (containing only `kReturn`) in the arena.
    ///
    /// Used by [`generate_interp_module`] to fill factory slots for DSP
    /// sections that are not present in the FIR module (e.g. `staticInit`
    /// when the legacy bridge is in use).
    pub fn alloc_empty_block(&mut self) -> BlockId {
        self.begin_sub_block();
        self.end_sub_block()
    }

    /// Destructs the compiler into its arena, heap sizes, UI instructions,
    /// and field table without sealing the outermost block.
    ///
    /// Call this after all function bodies have been compiled via
    /// [`compile_fir_block`].  The outermost (current) block, which should be
    /// empty at that point, is discarded.
    pub fn into_parts(self) -> CompilerParts<R> {
        (
            self.arena,
            self.int_heap_offset,
            self.real_heap_offset,
            self.ui_instructions,
            self.field_table,
        )
    }

    // -----------------------------------------------------------------------
    // Block switching
    // -----------------------------------------------------------------------

    /// Saves the current block and starts building a new empty block.
    ///
    /// # Source provenance (C++)
    /// - The pattern `FBCBlockInstruction<REAL>* current = fCurrentBlock;
    ///   fCurrentBlock = new FBCBlockInstruction<REAL>();` in control-flow
    ///   visitors.
    fn begin_sub_block(&mut self) {
        let current = std::mem::take(&mut self.current_block);
        self.saved_blocks.push(current);
    }

    /// Seals the current block with `kReturn`, allocates it in the arena,
    /// and restores the previously saved block.
    ///
    /// Returns the [`BlockId`] of the newly allocated block.
    fn end_sub_block(&mut self) -> BlockId {
        self.current_block
            .push(FbcInstruction::new(FbcOpcode::Return));
        let finished = std::mem::replace(
            &mut self.current_block,
            self.saved_blocks
                .pop()
                .expect("unbalanced begin/end_sub_block"),
        );
        self.arena.alloc(finished)
    }

    // -----------------------------------------------------------------------
    // Values
    // -----------------------------------------------------------------------

    /// # Source provenance (C++)
    /// - `visit(Int32NumInst*)` — pushes integer onto int stack.
    fn compile_int32(&mut self, value: i32) -> Result<(), CompileError> {
        self.current_block.push(FbcInstruction::with_values(
            FbcOpcode::Int32Value,
            value,
            R::default(),
        ));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(FloatNumInst*)` — pushes real onto real stack.
    fn compile_float32(&mut self, value: f32) -> Result<(), CompileError> {
        self.current_block.push(FbcInstruction::with_values(
            FbcOpcode::RealValue,
            0,
            R::from_f64(f64::from(value)),
        ));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(DoubleNumInst*)` — pushes real onto real stack.
    fn compile_float64(&mut self, value: f64) -> Result<(), CompileError> {
        self.current_block.push(FbcInstruction::with_values(
            FbcOpcode::RealValue,
            0,
            R::from_f64(value),
        ));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(BoolNumInst*)` — pushes 0 or 1 onto int stack.
    fn compile_bool(&mut self, value: bool) -> Result<(), CompileError> {
        self.current_block.push(FbcInstruction::with_values(
            FbcOpcode::Int32Value,
            i32::from(value),
            R::default(),
        ));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Variable access
    // -----------------------------------------------------------------------

    /// # Source provenance (C++)
    /// - `visit(LoadVarInst*)` — named address path.
    fn compile_load_var(
        &mut self,
        _store: &FirStore,
        name: &str,
        access: AccessType,
        _typ: &FirType,
    ) -> Result<(), CompileError> {
        if access == AccessType::FunArgs && name == "count" && !self.field_table.contains_key(name)
        {
            // Reserve a stable int heap slot for the runtime-set `count` pseudo argument.
            let offset = self.int_heap_offset;
            self.int_heap_offset += 1;
            self.field_table.insert(
                name.to_string(),
                MemoryDesc {
                    offset,
                    size: 1,
                    heap_type: HeapType::Int,
                },
            );
        }
        let desc = self
            .field_table
            .get(name)
            .ok_or_else(|| CompileError::UndeclaredVariable {
                name: name.to_string(),
            })?;
        let opcode = if desc.heap_type == HeapType::Int {
            FbcOpcode::LoadInt
        } else {
            FbcOpcode::LoadReal
        };
        self.current_block
            .push(FbcInstruction::with_values_and_offsets(
                opcode,
                0,
                R::default(),
                desc.offset,
                0,
            ));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(LoadVarInst*)` — indexed address path.
    fn compile_load_table(
        &mut self,
        store: &FirStore,
        name: &str,
        _access: AccessType,
        index: FirId,
        _typ: &FirType,
    ) -> Result<(), CompileError> {
        // Compile the index expression first (pushes onto int stack).
        self.compile_node(store, index)?;

        // Special handling for input channels.
        if let Some(channel) = parse_io_channel(name, "input") {
            self.current_block
                .push(FbcInstruction::with_values_and_offsets(
                    FbcOpcode::LoadInput,
                    0,
                    R::default(),
                    channel,
                    0,
                ));
            return Ok(());
        }

        let desc = self
            .field_table
            .get(name)
            .ok_or_else(|| CompileError::UndeclaredVariable {
                name: name.to_string(),
            })?;
        let opcode = if desc.heap_type == HeapType::Int {
            FbcOpcode::LoadIndexedInt
        } else {
            FbcOpcode::LoadIndexedReal
        };
        self.current_block
            .push(FbcInstruction::with_values_and_offsets(
                opcode,
                0,
                R::default(),
                desc.offset,
                desc.size,
            ));
        Ok(())
    }

    /// Compile `TeeVar` — store to variable and leave value on stack.
    fn compile_tee_var(
        &mut self,
        store: &FirStore,
        name: &str,
        _access: AccessType,
        value: FirId,
        _typ: &FirType,
    ) -> Result<(), CompileError> {
        // Compile value, store it, then reload (store+load = tee).
        self.compile_node(store, value)?;
        let desc = self
            .field_table
            .get(name)
            .ok_or_else(|| CompileError::UndeclaredVariable {
                name: name.to_string(),
            })?;
        let (store_op, load_op) = if desc.heap_type == HeapType::Int {
            (FbcOpcode::StoreInt, FbcOpcode::LoadInt)
        } else {
            (FbcOpcode::StoreReal, FbcOpcode::LoadReal)
        };
        let offset = desc.offset;
        self.current_block
            .push(FbcInstruction::with_values_and_offsets(
                store_op,
                0,
                R::default(),
                offset,
                0,
            ));
        self.current_block
            .push(FbcInstruction::with_values_and_offsets(
                load_op,
                0,
                R::default(),
                offset,
                0,
            ));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Declarations
    // -----------------------------------------------------------------------

    /// # Source provenance (C++)
    /// - `visit(DeclareVarInst*)` — allocates heap slots and optionally
    ///   compiles the initializer.
    fn compile_declare_var(
        &mut self,
        store: &FirStore,
        name: &str,
        typ: &FirType,
        _access: AccessType,
        init: Option<FirId>,
    ) -> Result<(), CompileError> {
        // Skip input/output pseudo-variables.
        if name.starts_with("input") || name.starts_with("output") {
            return Ok(());
        }

        // Determine element type and array size.
        let (elem_type, array_size) = match typ {
            FirType::Array(elem, size) => (elem.as_ref(), *size as i32),
            _ => (typ, 1),
        };
        self.alloc_storage_desc(name, elem_type, array_size);

        // Compile initializer if present.
        if let Some(init_id) = init {
            self.compile_init_store(store, name, typ, init_id)?;
        }
        Ok(())
    }

    /// Compiles the initializer for a `DeclareVar`.
    ///
    /// # Source provenance (C++)
    /// - `visitStore(inst->fAddress, inst->fValue, inst->fType)` called
    ///   from `visit(DeclareVarInst*)`.
    fn compile_init_store(
        &mut self,
        store: &FirStore,
        name: &str,
        typ: &FirType,
        init_id: FirId,
    ) -> Result<(), CompileError> {
        let desc = self.field_table[name].clone();

        // Array waveform store path.
        if let FirType::Array(_, _) = typ {
            match match_fir(store, init_id) {
                FirMatch::Int32Array { values, .. } => {
                    self.current_block.push_block_store(
                        FbcInstruction::with_values_and_offsets(
                            FbcOpcode::BlockStoreInt,
                            0,
                            R::default(),
                            desc.offset,
                            values.len() as i32,
                        ),
                        BlockStoreData::Int(values),
                    );
                    return Ok(());
                }
                FirMatch::Float32Array { values, .. } => {
                    let data: Vec<R> = values.iter().map(|v| R::from_f64(f64::from(*v))).collect();
                    self.current_block.push_block_store(
                        FbcInstruction::with_values_and_offsets(
                            FbcOpcode::BlockStoreReal,
                            0,
                            R::default(),
                            desc.offset,
                            data.len() as i32,
                        ),
                        BlockStoreData::Real(data),
                    );
                    return Ok(());
                }
                FirMatch::Float64Array { values, .. } => {
                    let data: Vec<R> = values.iter().map(|v| R::from_f64(*v)).collect();
                    self.current_block.push_block_store(
                        FbcInstruction::with_values_and_offsets(
                            FbcOpcode::BlockStoreReal,
                            0,
                            R::default(),
                            desc.offset,
                            data.len() as i32,
                        ),
                        BlockStoreData::Real(data),
                    );
                    return Ok(());
                }
                _ => {
                    // Fall through to scalar store.
                }
            }
        }

        // Scalar store path: compile value, then emit StoreInt/StoreReal.
        self.compile_node(store, init_id)?;
        let opcode = if desc.heap_type == HeapType::Int {
            FbcOpcode::StoreInt
        } else {
            FbcOpcode::StoreReal
        };
        self.current_block
            .push(FbcInstruction::with_values_and_offsets(
                opcode,
                0,
                R::default(),
                desc.offset,
                0,
            ));
        Ok(())
    }

    /// Reserves heap storage for a scalar/array variable declaration without
    /// compiling its initializer.
    fn predeclare_var_storage(&mut self, name: &str, typ: &FirType) {
        if name.starts_with("input") || name.starts_with("output") {
            return;
        }
        let (elem_type, array_size) = match typ {
            FirType::Array(elem, size) => (elem.as_ref(), *size as i32),
            _ => (typ, 1),
        };
        let _ = self.alloc_storage_desc(name, elem_type, array_size);
    }

    /// Reserves heap storage for a table declaration without compiling values.
    fn predeclare_table_storage(&mut self, name: &str, elem_type: &FirType, size: i32) {
        if name.starts_with("input") || name.starts_with("output") {
            return;
        }
        let _ = self.alloc_storage_desc(name, elem_type, size.max(0));
    }

    /// Allocates (or reuses) a memory descriptor in the compiler heap layout.
    ///
    /// If the name already exists, the previous descriptor is preserved so
    /// repeated pre-declaration/compilation passes remain idempotent.
    fn alloc_storage_desc(
        &mut self,
        name: &str,
        elem_type: &FirType,
        array_size: i32,
    ) -> MemoryDesc {
        if let Some(existing) = self.field_table.get(name) {
            return existing.clone();
        }
        let heap_type = if is_int_type(elem_type) {
            HeapType::Int
        } else {
            HeapType::Real
        };
        let offset = if heap_type == HeapType::Int {
            let o = self.int_heap_offset;
            self.int_heap_offset += array_size;
            o
        } else {
            let o = self.real_heap_offset;
            self.real_heap_offset += array_size;
            o
        };
        let desc = MemoryDesc {
            offset,
            size: array_size,
            heap_type,
        };
        self.field_table.insert(name.to_string(), desc.clone());
        desc
    }

    // -----------------------------------------------------------------------
    // Storage
    // -----------------------------------------------------------------------

    /// # Source provenance (C++)
    /// - `visit(StoreVarInst*)` / `visitStore()` — named address path.
    fn compile_store_var(
        &mut self,
        store: &FirStore,
        name: &str,
        _access: AccessType,
        value: FirId,
    ) -> Result<(), CompileError> {
        // Compile value (pushes onto stack).
        self.compile_node(store, value)?;

        let desc = self
            .field_table
            .get(name)
            .ok_or_else(|| CompileError::UndeclaredVariable {
                name: name.to_string(),
            })?;
        let opcode = if desc.heap_type == HeapType::Int {
            FbcOpcode::StoreInt
        } else {
            FbcOpcode::StoreReal
        };
        let offset = desc.offset;
        self.current_block
            .push(FbcInstruction::with_values_and_offsets(
                opcode,
                0,
                R::default(),
                offset,
                0,
            ));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visitStore()` — indexed address path.
    fn compile_store_table(
        &mut self,
        store: &FirStore,
        name: &str,
        _access: AccessType,
        index: FirId,
        value: FirId,
    ) -> Result<(), CompileError> {
        // Compile value first, then index (matches C++ order).
        self.compile_node(store, value)?;
        self.compile_node(store, index)?;

        // Special handling for output channels.
        if let Some(channel) = parse_io_channel(name, "output") {
            self.current_block
                .push(FbcInstruction::with_values_and_offsets(
                    FbcOpcode::StoreOutput,
                    0,
                    R::default(),
                    channel,
                    0,
                ));
            return Ok(());
        }

        let desc = self
            .field_table
            .get(name)
            .ok_or_else(|| CompileError::UndeclaredVariable {
                name: name.to_string(),
            })?;
        let opcode = if desc.heap_type == HeapType::Int {
            FbcOpcode::StoreIndexedInt
        } else {
            FbcOpcode::StoreIndexedReal
        };
        self.current_block
            .push(FbcInstruction::with_values_and_offsets(
                opcode,
                0,
                R::default(),
                desc.offset,
                desc.size,
            ));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(ShiftArrayVarInst*)`.
    fn compile_shift_array(&mut self, name: &str, delay: i32) -> Result<(), CompileError> {
        let desc = self
            .field_table
            .get(name)
            .ok_or_else(|| CompileError::UndeclaredVariable {
                name: name.to_string(),
            })?;
        let opcode = if desc.heap_type == HeapType::Int {
            FbcOpcode::BlockShiftInt
        } else {
            FbcOpcode::BlockShiftReal
        };
        // C++: offset1 = tmp.fOffset + inst->fDelay, offset2 = tmp.fOffset
        self.current_block
            .push(FbcInstruction::with_values_and_offsets(
                opcode,
                0,
                R::default(),
                desc.offset + delay,
                desc.offset,
            ));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Arithmetic
    // -----------------------------------------------------------------------

    /// # Source provenance (C++)
    /// - `visit(BinopInst*)` — compiles operands then emits int/real opcode.
    fn compile_binop(
        &mut self,
        store: &FirStore,
        op: fir::FirBinOp,
        lhs: FirId,
        rhs: FirId,
    ) -> Result<(), CompileError> {
        // C++ compiles inst2 (rhs) first, then inst1 (lhs).
        // lhs ends up on TOS for the operator.
        self.compile_node(store, rhs)?;
        let real_t2 = self.current_block.is_real_inst();
        self.compile_node(store, lhs)?;
        let real_t1 = self.current_block.is_real_inst();

        let (int_op, real_op) = binop_to_fbc(op);
        let opcode = if real_t1 || real_t2 { real_op } else { int_op };
        self.current_block.push(FbcInstruction::new(opcode));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(NegInst*)` — multiplies by -1.
    fn compile_neg(
        &mut self,
        store: &FirStore,
        value: FirId,
        typ: &FirType,
    ) -> Result<(), CompileError> {
        if is_int_type(typ) {
            // Push value, push -1, emit MultInt.
            self.compile_node(store, value)?;
            self.current_block.push(FbcInstruction::with_values(
                FbcOpcode::Int32Value,
                -1,
                R::default(),
            ));
            self.current_block
                .push(FbcInstruction::new(FbcOpcode::MultInt));
        } else {
            // Push value, push -1.0, emit MultReal.
            self.compile_node(store, value)?;
            self.current_block.push(FbcInstruction::with_values(
                FbcOpcode::RealValue,
                0,
                R::from_f64(-1.0),
            ));
            self.current_block
                .push(FbcInstruction::new(FbcOpcode::MultReal));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Cast
    // -----------------------------------------------------------------------

    /// # Source provenance (C++)
    /// - `visit(CastInst*)` — emits `kCastInt` or `kCastReal` if type changes.
    fn compile_cast(
        &mut self,
        store: &FirStore,
        typ: &FirType,
        value: FirId,
    ) -> Result<(), CompileError> {
        self.compile_node(store, value)?;
        let real_operand = self.current_block.is_real_inst();

        if is_int_type(typ) {
            // Cast to int — only emit if operand is real.
            if real_operand {
                self.current_block
                    .push(FbcInstruction::new(FbcOpcode::CastInt));
            }
        } else {
            // Cast to real — only emit if operand is int.
            if !real_operand {
                self.current_block
                    .push(FbcInstruction::new(FbcOpcode::CastReal));
            }
        }
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(BitcastInst*)`.
    fn compile_bitcast(
        &mut self,
        store: &FirStore,
        typ: &FirType,
        value: FirId,
    ) -> Result<(), CompileError> {
        self.compile_node(store, value)?;
        let opcode = if is_int_type(typ) {
            FbcOpcode::BitcastInt
        } else {
            FbcOpcode::BitcastReal
        };
        self.current_block.push(FbcInstruction::new(opcode));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Control flow
    // -----------------------------------------------------------------------

    /// # Source provenance (C++)
    /// - `visit(Select2Inst*)` — block-switching for select.
    fn compile_select2(
        &mut self,
        store: &FirStore,
        cond: FirId,
        then_value: FirId,
        else_value: FirId,
    ) -> Result<(), CompileError> {
        // Compile condition into current block.
        self.compile_node(store, cond)?;

        // Compile 'then' in a new sub-block.
        self.begin_sub_block();
        self.compile_node(store, then_value)?;
        let is_real = self.current_block.is_real_inst();
        let then_block_id = self.end_sub_block();

        // Compile 'else' in a new sub-block.
        self.begin_sub_block();
        self.compile_node(store, else_value)?;
        let else_block_id = self.end_sub_block();

        // Emit select instruction referencing both sub-blocks.
        let opcode = if is_real {
            FbcOpcode::SelectReal
        } else {
            FbcOpcode::SelectInt
        };
        self.current_block.push(FbcInstruction::full(
            opcode,
            "",
            0,
            R::default(),
            0,
            0,
            Some(then_block_id),
            Some(else_block_id),
        ));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(IfInst*)` — block-switching for if/else.
    fn compile_if(
        &mut self,
        store: &FirStore,
        cond: FirId,
        then_block: FirId,
        else_block: Option<FirId>,
    ) -> Result<(), CompileError> {
        // Compile condition.
        self.compile_node(store, cond)?;

        // Compile 'then' in a new sub-block.
        self.begin_sub_block();
        self.compile_node(store, then_block)?;
        let then_block_id = self.end_sub_block();

        // Compile 'else' in a (possibly empty) new sub-block.
        self.begin_sub_block();
        if let Some(else_id) = else_block {
            self.compile_node(store, else_id)?;
        }
        let else_block_id = self.end_sub_block();

        // Emit kIf.
        self.current_block.push(FbcInstruction::full(
            FbcOpcode::If,
            "",
            0,
            R::default(),
            0,
            0,
            Some(then_block_id),
            Some(else_block_id),
        ));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(ForLoopInst*)` — block-switching for loop with init + body.
    fn compile_for_loop(
        &mut self,
        store: &FirStore,
        init: FirId,
        end: FirId,
        step: FirId,
        body: FirId,
    ) -> Result<(), CompileError> {
        // Compile init in a new sub-block.
        self.begin_sub_block();
        self.compile_node(store, init)?;
        let init_block_id = self.end_sub_block();

        // Compile loop body in a new sub-block.
        // Order: body → increment → test → kCondBranch → kReturn.
        self.begin_sub_block();
        self.compile_node(store, body)?;
        self.compile_node(store, step)?;
        self.compile_node(store, end)?;

        // Predict the next BlockId for the CondBranch loop-back.
        let next_id = BlockId::from_raw(self.arena.len() as u32);
        self.current_block.push(FbcInstruction::full(
            FbcOpcode::CondBranch,
            "",
            0,
            R::default(),
            0,
            0,
            Some(next_id),
            None,
        ));
        let loop_body_id = self.end_sub_block();
        debug_assert_eq!(loop_body_id.as_u32(), next_id.as_u32());

        // Emit kLoop in the parent block. vec_size = 1 (conservative).
        self.current_block.push(FbcInstruction::full(
            FbcOpcode::Loop,
            "",
            1,
            R::default(),
            0,
            0,
            Some(init_block_id),
            Some(loop_body_id),
        ));
        Ok(())
    }

    /// Compiles `SimpleForLoop(var, upper, body)` as a canonical counting loop:
    /// `for (var = 0; var < upper; var = var + 1)`.
    fn compile_simple_for_loop(
        &mut self,
        store: &FirStore,
        var: &str,
        upper: FirId,
        body: FirId,
    ) -> Result<(), CompileError> {
        // Allocate loop variable if missing (simple pragmatic model: function-scoped slot).
        if !self.field_table.contains_key(var) {
            let offset = self.int_heap_offset;
            self.int_heap_offset += 1;
            self.field_table.insert(
                var.to_string(),
                MemoryDesc {
                    offset,
                    size: 1,
                    heap_type: HeapType::Int,
                },
            );
        }
        let desc =
            self.field_table
                .get(var)
                .cloned()
                .ok_or_else(|| CompileError::UndeclaredVariable {
                    name: var.to_string(),
                })?;

        // Init block: `var = 0`.
        self.begin_sub_block();
        self.current_block
            .push(FbcInstruction::with_values_and_offsets(
                FbcOpcode::StoreIntValue,
                0,
                R::default(),
                desc.offset,
                0,
            ));
        let init_block_id = self.end_sub_block();

        // Body block: body; `var = var + 1`; `var < upper`; cond-branch(loop back).
        self.begin_sub_block();
        self.compile_node(store, body)?;
        self.current_block
            .push(FbcInstruction::with_values_and_offsets(
                FbcOpcode::LoadInt,
                0,
                R::default(),
                desc.offset,
                0,
            ));
        self.current_block.push(FbcInstruction::with_values(
            FbcOpcode::Int32Value,
            1,
            R::default(),
        ));
        self.current_block
            .push(FbcInstruction::new(FbcOpcode::AddInt));
        self.current_block
            .push(FbcInstruction::with_values_and_offsets(
                FbcOpcode::StoreInt,
                0,
                R::default(),
                desc.offset,
                0,
            ));
        self.current_block
            .push(FbcInstruction::with_values_and_offsets(
                FbcOpcode::LoadInt,
                0,
                R::default(),
                desc.offset,
                0,
            ));
        self.compile_node(store, upper)?;
        self.current_block
            .push(FbcInstruction::new(FbcOpcode::LTInt));

        let next_id = BlockId::from_raw(self.arena.len() as u32);
        self.current_block.push(FbcInstruction::full(
            FbcOpcode::CondBranch,
            "",
            0,
            R::default(),
            0,
            0,
            Some(next_id),
            None,
        ));
        let loop_body_id = self.end_sub_block();
        debug_assert_eq!(loop_body_id.as_u32(), next_id.as_u32());

        self.current_block.push(FbcInstruction::full(
            FbcOpcode::Loop,
            "",
            1,
            R::default(),
            0,
            0,
            Some(init_block_id),
            Some(loop_body_id),
        ));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(BlockInst*)` — iterates over block statements.
    fn compile_block(&mut self, store: &FirStore, stmts: &[FirId]) -> Result<(), CompileError> {
        for &stmt in stmts {
            self.compile_node(store, stmt)?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Function calls
    // -----------------------------------------------------------------------

    /// # Source provenance (C++)
    /// - `visit(FunCallInst*)` — compiles args in reverse order, then
    ///   emits the opcode from `gMathLibTable`.
    fn compile_fun_call(
        &mut self,
        store: &FirStore,
        name: &str,
        args: &[FirId],
    ) -> Result<(), CompileError> {
        // Compile args in reverse order (stack discipline).
        for &arg in args.iter().rev() {
            self.compile_node(store, arg)?;
        }

        let opcode = math_lib_lookup(name).ok_or_else(|| CompileError::UnknownMathFunction {
            name: name.to_string(),
        })?;
        self.current_block.push(FbcInstruction::new(opcode));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // UI
    // -----------------------------------------------------------------------

    /// # Source provenance (C++)
    /// - `visit(OpenboxInst*)`.
    fn compile_open_box(&mut self, typ: &UiBoxType, label: &str) -> Result<(), CompileError> {
        let opcode = match typ {
            UiBoxType::Vertical => FbcOpcode::OpenVerticalBox,
            UiBoxType::Horizontal => FbcOpcode::OpenHorizontalBox,
            UiBoxType::Tab => FbcOpcode::OpenTabBox,
        };
        self.ui_instructions
            .push(FbcUiInstruction::open_box(opcode, label));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(CloseboxInst*)`.
    fn compile_close_box(&mut self) -> Result<(), CompileError> {
        self.ui_instructions
            .push(FbcUiInstruction::new(FbcOpcode::CloseBox));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(AddButtonInst*)`.
    fn compile_add_button(
        &mut self,
        typ: &ButtonType,
        label: &str,
        var: &str,
    ) -> Result<(), CompileError> {
        let opcode = match typ {
            ButtonType::Button => FbcOpcode::AddButton,
            ButtonType::Checkbox => FbcOpcode::AddCheckButton,
        };
        let offset = self.get_field_offset(var);
        self.ui_instructions.push(FbcUiInstruction::widget(
            opcode,
            offset,
            label,
            R::default(),
            R::default(),
            R::default(),
            R::default(),
        ));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(AddSliderInst*)`.
    #[allow(clippy::too_many_arguments)]
    fn compile_add_slider(
        &mut self,
        typ: &SliderType,
        label: &str,
        var: &str,
        init: f64,
        lo: f64,
        hi: f64,
        step: f64,
    ) -> Result<(), CompileError> {
        let opcode = match typ {
            SliderType::Horizontal => FbcOpcode::AddHorizontalSlider,
            SliderType::Vertical => FbcOpcode::AddVerticalSlider,
            SliderType::NumEntry => FbcOpcode::AddNumEntry,
        };
        let offset = self.get_field_offset(var);
        self.ui_instructions.push(FbcUiInstruction::widget(
            opcode,
            offset,
            label,
            R::from_f64(init),
            R::from_f64(lo),
            R::from_f64(hi),
            R::from_f64(step),
        ));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(AddBargraphInst*)`.
    fn compile_add_bargraph(
        &mut self,
        typ: &BargraphType,
        label: &str,
        var: &str,
        lo: f64,
        hi: f64,
    ) -> Result<(), CompileError> {
        let opcode = match typ {
            BargraphType::Horizontal => FbcOpcode::AddHorizontalBargraph,
            BargraphType::Vertical => FbcOpcode::AddVerticalBargraph,
        };
        let offset = self.get_field_offset(var);
        self.ui_instructions.push(FbcUiInstruction::bargraph(
            opcode,
            offset,
            label,
            R::from_f64(lo),
            R::from_f64(hi),
        ));
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(AddSoundfileInst*)`.
    fn compile_add_soundfile(
        &mut self,
        label: &str,
        _url: &str,
        _var: &str,
    ) -> Result<(), CompileError> {
        self.ui_instructions
            .push(FbcUiInstruction::new(FbcOpcode::AddSoundfile));
        // Soundfile label is stored but full soundfile support is deferred.
        self.ui_instructions.last_mut().unwrap().label = label.to_string();
        Ok(())
    }

    /// # Source provenance (C++)
    /// - `visit(AddMetaDeclareInst*)`.
    fn compile_add_meta_declare(
        &mut self,
        var: &str,
        key: &str,
        value: &str,
    ) -> Result<(), CompileError> {
        let offset = if var == "0" {
            -1
        } else {
            self.get_field_offset(var)
        };
        self.ui_instructions
            .push(FbcUiInstruction::declare(offset, key, value));
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// # Source provenance (C++)
    /// - `getFieldOffset()`.
    fn get_field_offset(&self, name: &str) -> i32 {
        self.field_table.get(name).map_or(-1, |desc| desc.offset)
    }
}

impl<R: FbcReal> Default for FirToFbcCompiler<R> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Static lookup tables (free functions)
// ---------------------------------------------------------------------------

/// Maps a FIR binary operation to the corresponding FBC opcodes.
///
/// # Source provenance (C++)
/// - `gBinOpTable[opcode]->fInterpIntInst32` / `fInterpFloatInst`.
///
/// Returns `(int_opcode, real_opcode)`.
#[must_use]
pub const fn binop_to_fbc(op: fir::FirBinOp) -> (FbcOpcode, FbcOpcode) {
    use fir::FirBinOp;
    match op {
        FirBinOp::Add => (FbcOpcode::AddInt, FbcOpcode::AddReal),
        FirBinOp::Sub => (FbcOpcode::SubInt, FbcOpcode::SubReal),
        FirBinOp::Mul => (FbcOpcode::MultInt, FbcOpcode::MultReal),
        FirBinOp::Div => (FbcOpcode::DivInt, FbcOpcode::DivReal),
        FirBinOp::Rem => (FbcOpcode::RemInt, FbcOpcode::RemReal),
        FirBinOp::And => (FbcOpcode::ANDInt, FbcOpcode::ANDInt),
        FirBinOp::Or => (FbcOpcode::ORInt, FbcOpcode::ORInt),
        FirBinOp::Xor => (FbcOpcode::XORInt, FbcOpcode::XORInt),
        FirBinOp::Eq => (FbcOpcode::EQInt, FbcOpcode::EQReal),
        FirBinOp::Ne => (FbcOpcode::NEInt, FbcOpcode::NEReal),
        FirBinOp::Lt => (FbcOpcode::LTInt, FbcOpcode::LTReal),
        FirBinOp::Le => (FbcOpcode::LEInt, FbcOpcode::LEReal),
        FirBinOp::Gt => (FbcOpcode::GTInt, FbcOpcode::GTReal),
        FirBinOp::Ge => (FbcOpcode::GEInt, FbcOpcode::GEReal),
    }
}

/// Maps a math function name to its FBC opcode.
///
/// # Source provenance (C++)
/// - `InterpreterInstVisitor::initMathTable()` in `interpreter_instructions.hh`.
///
/// Handles both float-suffix (`sinf`) and double (bare `sin`) forms.
#[must_use]
pub fn math_lib_lookup(name: &str) -> Option<FbcOpcode> {
    match name {
        // Integer
        "abs" => Some(FbcOpcode::Abs),
        "min_i" => Some(FbcOpcode::Min),
        "max_i" => Some(FbcOpcode::Max),
        // Float and double
        "fabsf" | "fabs" => Some(FbcOpcode::Absf),
        "acosf" | "acos" => Some(FbcOpcode::Acosf),
        "asinf" | "asin" => Some(FbcOpcode::Asinf),
        "atanf" | "atan" => Some(FbcOpcode::Atanf),
        "atan2f" | "atan2" => Some(FbcOpcode::Atan2f),
        "ceilf" | "ceil" => Some(FbcOpcode::Ceilf),
        "cosf" | "cos" => Some(FbcOpcode::Cosf),
        "expf" | "exp" => Some(FbcOpcode::Expf),
        "floorf" | "floor" => Some(FbcOpcode::Floorf),
        "fmodf" | "fmod" => Some(FbcOpcode::Fmodf),
        "logf" | "log" => Some(FbcOpcode::Logf),
        "log10f" | "log10" => Some(FbcOpcode::Log10f),
        "min_f" | "min_" => Some(FbcOpcode::Minf),
        "max_f" | "max_" => Some(FbcOpcode::Maxf),
        "powf" | "pow" => Some(FbcOpcode::Powf),
        "remainderf" | "remainder" => Some(FbcOpcode::RemReal),
        "rintf" | "rint" => Some(FbcOpcode::Rintf),
        "roundf" | "round" => Some(FbcOpcode::Roundf),
        "sinf" | "sin" => Some(FbcOpcode::Sinf),
        "sqrtf" | "sqrt" => Some(FbcOpcode::Sqrtf),
        "tanf" | "tan" => Some(FbcOpcode::Tanf),
        // Hyperbolic
        "acoshf" | "acosh" => Some(FbcOpcode::Acoshf),
        "asinhf" | "asinh" => Some(FbcOpcode::Asinhf),
        "atanhf" | "atanh" => Some(FbcOpcode::Atanhf),
        "coshf" | "cosh" => Some(FbcOpcode::Coshf),
        "sinhf" | "sinh" => Some(FbcOpcode::Sinhf),
        "tanhf" | "tanh" => Some(FbcOpcode::Tanhf),
        // Special
        "isnanf" | "isnan" => Some(FbcOpcode::Isnanf),
        "isinff" | "isinf" => Some(FbcOpcode::Isinff),
        "copysignf" | "copysign" => Some(FbcOpcode::Copysignf),
        _ => None,
    }
}

/// Extracts the channel number from `"input0"`, `"output1"`, etc.
fn parse_io_channel(name: &str, prefix: &str) -> Option<i32> {
    name.strip_prefix(prefix)
        .and_then(|suffix| suffix.parse::<i32>().ok())
}

/// Returns `true` if the FIR type maps to the int heap.
fn is_int_type(typ: &FirType) -> bool {
    matches!(typ, FirType::Int32 | FirType::Int64 | FirType::Bool)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use fir::{FirBinOp, FirBuilder};

    /// Helper: compile a single FIR node and finalize.
    fn compile_one<R: FbcReal>(store: &FirStore, id: FirId) -> FbcCompileResult<R> {
        let mut compiler = FirToFbcCompiler::<R>::new();
        compiler.compile_node(store, id).unwrap();
        compiler.finalize().unwrap()
    }

    /// Helper: get the instruction opcodes from a block (excluding final Return).
    fn opcodes<R: FbcReal>(result: &FbcCompileResult<R>, block_id: BlockId) -> Vec<FbcOpcode> {
        result
            .arena
            .get(block_id)
            .instructions
            .iter()
            .map(|i| i.opcode)
            .collect()
    }

    // --- Phase A: Literal values ---

    #[test]
    fn test_compile_int32() {
        let mut store = FirStore::new();
        let id = FirBuilder::new(&mut store).int32(42);
        let result = compile_one::<f32>(&store, id);
        let ops = opcodes(&result, result.entry_block);
        assert_eq!(ops, vec![FbcOpcode::Int32Value, FbcOpcode::Return]);
        assert_eq!(
            result.arena.get(result.entry_block).instructions[0].int_value,
            42
        );
    }

    #[test]
    fn test_compile_float32() {
        let mut store = FirStore::new();
        let id = FirBuilder::new(&mut store).float32(3.125);
        let result = compile_one::<f32>(&store, id);
        let ops = opcodes(&result, result.entry_block);
        assert_eq!(ops, vec![FbcOpcode::RealValue, FbcOpcode::Return]);
        let rv = result.arena.get(result.entry_block).instructions[0].real_value;
        assert!((rv - 3.125).abs() < 1e-6);
    }

    #[test]
    fn test_compile_float64() {
        let mut store = FirStore::new();
        let id = FirBuilder::new(&mut store).float64(2.5);
        let result = compile_one::<f64>(&store, id);
        let ops = opcodes(&result, result.entry_block);
        assert_eq!(ops, vec![FbcOpcode::RealValue, FbcOpcode::Return]);
        let rv = result.arena.get(result.entry_block).instructions[0].real_value;
        assert!((rv - 2.5).abs() < 1e-10);
    }

    #[test]
    fn test_compile_bool() {
        let mut store = FirStore::new();
        let id = FirBuilder::new(&mut store).bool_(true);
        let result = compile_one::<f32>(&store, id);
        let ops = opcodes(&result, result.entry_block);
        assert_eq!(ops, vec![FbcOpcode::Int32Value, FbcOpcode::Return]);
        assert_eq!(
            result.arena.get(result.entry_block).instructions[0].int_value,
            1
        );
    }

    // --- Phase B: Binops ---

    #[test]
    fn test_compile_binop_add_int() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let three = b.int32(3);
        let four = b.int32(4);
        let add = b.binop(FirBinOp::Add, three, four, FirType::Int32);
        let result = compile_one::<f32>(&store, add);
        let ops = opcodes(&result, result.entry_block);
        // rhs (4) compiled first, then lhs (3), then AddInt
        assert_eq!(
            ops,
            vec![
                FbcOpcode::Int32Value, // 4 (rhs)
                FbcOpcode::Int32Value, // 3 (lhs)
                FbcOpcode::AddInt,
                FbcOpcode::Return
            ]
        );
        assert_eq!(
            result.arena.get(result.entry_block).instructions[0].int_value,
            4
        );
        assert_eq!(
            result.arena.get(result.entry_block).instructions[1].int_value,
            3
        );
    }

    #[test]
    fn test_compile_binop_add_real() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let one = b.float32(1.0);
        let two = b.float32(2.0);
        let add = b.binop(FirBinOp::Add, one, two, FirType::Float32);
        let result = compile_one::<f32>(&store, add);
        let ops = opcodes(&result, result.entry_block);
        assert_eq!(
            ops,
            vec![
                FbcOpcode::RealValue, // 2.0 (rhs)
                FbcOpcode::RealValue, // 1.0 (lhs)
                FbcOpcode::AddReal,
                FbcOpcode::Return
            ]
        );
    }

    // --- Phase C: Declare + Load + Store ---

    #[test]
    fn test_compile_declare_and_load_int() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let init_val = b.int32(42);
        let decl = b.declare_var("x", FirType::Int32, AccessType::Struct, Some(init_val));
        let load = b.load_var("x", AccessType::Struct, FirType::Int32);

        let mut compiler = FirToFbcCompiler::<f32>::new();
        compiler.compile_node(&store, decl).unwrap();
        compiler.compile_node(&store, load).unwrap();
        let result = compiler.finalize().unwrap();

        let ops = opcodes(&result, result.entry_block);
        assert_eq!(
            ops,
            vec![
                FbcOpcode::Int32Value, // init value 42
                FbcOpcode::StoreInt,   // store to heap[0]
                FbcOpcode::LoadInt,    // load from heap[0]
                FbcOpcode::Return,
            ]
        );
        // Verify offset = 0 on store and load.
        assert_eq!(
            result.arena.get(result.entry_block).instructions[1].offset1,
            0
        );
        assert_eq!(
            result.arena.get(result.entry_block).instructions[2].offset1,
            0
        );
        assert_eq!(result.int_heap_size, 1);
    }

    #[test]
    fn test_compile_declare_real() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let init_val = b.float32(1.5);
        let decl = b.declare_var("y", FirType::Float32, AccessType::Struct, Some(init_val));

        let mut compiler = FirToFbcCompiler::<f32>::new();
        compiler.compile_node(&store, decl).unwrap();
        let result = compiler.finalize().unwrap();

        assert_eq!(result.real_heap_size, 1);
        assert_eq!(result.int_heap_size, 0);
        let ops = opcodes(&result, result.entry_block);
        assert_eq!(
            ops,
            vec![
                FbcOpcode::RealValue,
                FbcOpcode::StoreReal,
                FbcOpcode::Return
            ]
        );
    }

    // --- Phase D: Cast ---

    #[test]
    fn test_compile_cast_int_to_real() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let val = b.int32(7);
        let cast = b.cast(FirType::Float32, val);
        let result = compile_one::<f32>(&store, cast);
        let ops = opcodes(&result, result.entry_block);
        assert_eq!(
            ops,
            vec![
                FbcOpcode::Int32Value,
                FbcOpcode::CastReal,
                FbcOpcode::Return
            ]
        );
    }

    #[test]
    fn test_compile_cast_real_to_int() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let val = b.float32(7.5);
        let cast = b.cast(FirType::Int32, val);
        let result = compile_one::<f32>(&store, cast);
        let ops = opcodes(&result, result.entry_block);
        assert_eq!(
            ops,
            vec![FbcOpcode::RealValue, FbcOpcode::CastInt, FbcOpcode::Return]
        );
    }

    #[test]
    fn test_compile_cast_same_type_no_op() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let val = b.int32(5);
        let cast = b.cast(FirType::Int32, val);
        let result = compile_one::<f32>(&store, cast);
        let ops = opcodes(&result, result.entry_block);
        // No CastInt emitted because operand is already int.
        assert_eq!(ops, vec![FbcOpcode::Int32Value, FbcOpcode::Return]);
    }

    // --- Phase E: Select2 ---

    #[test]
    fn test_compile_select2() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let cond = b.int32(1);
        let then_v = b.int32(10);
        let else_v = b.int32(20);
        let sel = b.select2(cond, then_v, else_v, FirType::Int32);
        let result = compile_one::<f32>(&store, sel);

        // Entry block: cond + kSelectInt.
        let ops = opcodes(&result, result.entry_block);
        assert_eq!(
            ops,
            vec![
                FbcOpcode::Int32Value,
                FbcOpcode::SelectInt,
                FbcOpcode::Return
            ]
        );

        // Check sub-blocks exist.
        let select_instr = &result.arena.get(result.entry_block).instructions[1];
        let then_id = select_instr.branch1.unwrap();
        let else_id = select_instr.branch2.unwrap();
        let then_ops = opcodes(&result, then_id);
        let else_ops = opcodes(&result, else_id);
        assert_eq!(then_ops, vec![FbcOpcode::Int32Value, FbcOpcode::Return]);
        assert_eq!(else_ops, vec![FbcOpcode::Int32Value, FbcOpcode::Return]);
    }

    // --- Phase F: ForLoop ---

    #[test]
    fn test_compile_for_loop_structure() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        // for (i = 0; i < 10; i++) { /* empty body */ }
        let init_val = b.int32(0);
        let init_decl = b.declare_var("i", FirType::Int32, AccessType::Loop, Some(init_val));
        let load_i = b.load_var("i", AccessType::Loop, FirType::Int32);
        let ten = b.int32(10);
        let cond = b.binop(FirBinOp::Lt, load_i, ten, FirType::Bool);
        let load_i2 = b.load_var("i", AccessType::Loop, FirType::Int32);
        let one = b.int32(1);
        let incr_val = b.binop(FirBinOp::Add, load_i2, one, FirType::Int32);
        let step = b.store_var("i", AccessType::Loop, incr_val);
        let body = b.block(&[]);
        let loop_node = b.for_loop("i", init_decl, cond, step, body, false);

        let result = compile_one::<f32>(&store, loop_node);

        // Entry block should have kLoop.
        let ops = opcodes(&result, result.entry_block);
        assert_eq!(ops, vec![FbcOpcode::Loop, FbcOpcode::Return]);

        // kLoop instruction should reference init and loop-body blocks.
        let loop_instr = &result.arena.get(result.entry_block).instructions[0];
        assert!(loop_instr.branch1.is_some());
        assert!(loop_instr.branch2.is_some());

        // Init block should contain init declaration + Return.
        let init_id = loop_instr.branch1.unwrap();
        let init_ops = opcodes(&result, init_id);
        assert!(init_ops.contains(&FbcOpcode::StoreInt));
        assert_eq!(*init_ops.last().unwrap(), FbcOpcode::Return);

        // Loop body block should end with CondBranch + Return.
        let body_id = loop_instr.branch2.unwrap();
        let body_ops = opcodes(&result, body_id);
        assert!(body_ops.contains(&FbcOpcode::CondBranch));
        assert_eq!(*body_ops.last().unwrap(), FbcOpcode::Return);
    }

    // --- Phase G: Function calls ---

    #[test]
    fn test_compile_fun_call_sin() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let arg = b.float32(0.5);
        let call = b.fun_call("sinf", &[arg], FirType::Float32);
        let result = compile_one::<f32>(&store, call);
        let ops = opcodes(&result, result.entry_block);
        assert_eq!(
            ops,
            vec![FbcOpcode::RealValue, FbcOpcode::Sinf, FbcOpcode::Return]
        );
    }

    #[test]
    fn test_unknown_function_error() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);
        let arg = b.float32(1.0);
        let call = b.fun_call("bogus_fn", &[arg], FirType::Float32);
        let mut compiler = FirToFbcCompiler::<f32>::new();
        let err = compiler.compile_node(&store, call).unwrap_err();
        match err {
            CompileError::UnknownMathFunction { name } => {
                assert_eq!(name, "bogus_fn");
            }
            _ => panic!("expected UnknownMathFunction, got: {err:?}"),
        }
    }

    // --- Phase I: UI ---

    #[test]
    fn test_ui_slider() {
        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        // Declare the zone variable first.
        let init_val = b.float32(0.5);
        let decl = b.declare_var(
            "fGain",
            FirType::Float32,
            AccessType::Struct,
            Some(init_val),
        );
        let slider = b.add_slider(
            SliderType::Horizontal,
            "Gain",
            "fGain",
            fir::SliderRange {
                init: 0.5,
                lo: 0.0,
                hi: 1.0,
                step: 0.01,
            },
        );

        let mut compiler = FirToFbcCompiler::<f32>::new();
        compiler.compile_node(&store, decl).unwrap();
        compiler.compile_node(&store, slider).unwrap();
        let result = compiler.finalize().unwrap();

        assert_eq!(result.ui_instructions.len(), 1);
        assert_eq!(
            result.ui_instructions[0].opcode,
            FbcOpcode::AddHorizontalSlider
        );
        assert_eq!(result.ui_instructions[0].label, "Gain");
        assert_eq!(result.ui_instructions[0].offset, 0);
    }

    // --- Phase J: Integration tests (compile + execute roundtrip) ---

    #[test]
    fn test_roundtrip_int32() {
        use super::super::executor::FbcExecutor;

        let mut store = FirStore::new();
        let id = FirBuilder::new(&mut store).int32(42);
        let result = compile_one::<f32>(&store, id);

        let mut exec = FbcExecutor::<f32>::new(16, 16);
        exec.execute_block(&result.arena, result.entry_block);
        // After execution, 42 should be on the int stack.
        // Since execute_block returns normally after kReturn,
        // we verify by compiling a store+check.
    }

    #[test]
    fn test_roundtrip_add() {
        use super::super::executor::FbcExecutor;

        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        // Declare x, compute 3 + 4, store in x.
        let init_val = b.int32(0);
        let decl = b.declare_var("x", FirType::Int32, AccessType::Struct, Some(init_val));
        let three = b.int32(3);
        let four = b.int32(4);
        let add = b.binop(FirBinOp::Add, three, four, FirType::Int32);
        let st = b.store_var("x", AccessType::Struct, add);

        let mut compiler = FirToFbcCompiler::<f32>::new();
        compiler.compile_node(&store, decl).unwrap();
        compiler.compile_node(&store, st).unwrap();
        let result = compiler.finalize().unwrap();

        let mut exec = FbcExecutor::<f32>::new(
            result.int_heap_size as usize,
            result.real_heap_size as usize,
        );
        exec.execute_block(&result.arena, result.entry_block);
        assert_eq!(exec.int_heap[0], 7);
    }

    #[test]
    fn test_roundtrip_store_load() {
        use super::super::executor::FbcExecutor;

        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        // Declare x = 42, then store 99 into x.
        let init_val = b.int32(42);
        let decl = b.declare_var("x", FirType::Int32, AccessType::Struct, Some(init_val));
        let ninety_nine = b.int32(99);
        let st = b.store_var("x", AccessType::Struct, ninety_nine);

        let mut compiler = FirToFbcCompiler::<f32>::new();
        compiler.compile_node(&store, decl).unwrap();
        compiler.compile_node(&store, st).unwrap();
        let result = compiler.finalize().unwrap();

        let mut exec = FbcExecutor::<f32>::new(
            result.int_heap_size as usize,
            result.real_heap_size as usize,
        );
        exec.execute_block(&result.arena, result.entry_block);
        assert_eq!(exec.int_heap[0], 99);
    }

    #[test]
    fn test_roundtrip_for_loop() {
        use super::super::executor::FbcExecutor;

        let mut store = FirStore::new();
        let mut b = FirBuilder::new(&mut store);

        // Declare x = 0 (at struct level, offset 0 in int heap).
        let x_init = b.int32(0);
        let x_decl = b.declare_var("x", FirType::Int32, AccessType::Struct, Some(x_init));

        // for (i = 0; i < 10; i++) { x = x + 1; }
        let i_init = b.int32(0);
        let i_decl = b.declare_var("i", FirType::Int32, AccessType::Loop, Some(i_init));

        // Condition: i < 10
        let load_i = b.load_var("i", AccessType::Loop, FirType::Int32);
        let ten = b.int32(10);
        let cond = b.binop(FirBinOp::Lt, load_i, ten, FirType::Bool);

        // Step: i = i + 1
        let load_i2 = b.load_var("i", AccessType::Loop, FirType::Int32);
        let one = b.int32(1);
        let incr = b.binop(FirBinOp::Add, load_i2, one, FirType::Int32);
        let step = b.store_var("i", AccessType::Loop, incr);

        // Body: x = x + 1
        let load_x = b.load_var("x", AccessType::Struct, FirType::Int32);
        let one2 = b.int32(1);
        let add_x = b.binop(FirBinOp::Add, load_x, one2, FirType::Int32);
        let store_x = b.store_var("x", AccessType::Struct, add_x);
        let body = b.block(&[store_x]);

        let loop_node = b.for_loop("i", i_decl, cond, step, body, false);

        let mut compiler = FirToFbcCompiler::<f32>::new();
        compiler.compile_node(&store, x_decl).unwrap();
        compiler.compile_node(&store, loop_node).unwrap();
        let result = compiler.finalize().unwrap();

        let mut exec = FbcExecutor::<f32>::new(
            result.int_heap_size as usize,
            result.real_heap_size as usize,
        );
        exec.execute_block(&result.arena, result.entry_block);

        // x should be 10 after 10 iterations.
        let x_offset = result.field_table["x"].offset as usize;
        assert_eq!(exec.int_heap[x_offset], 10);
    }

    // --- Lookup table tests ---

    #[test]
    fn test_math_lib_lookup() {
        assert_eq!(math_lib_lookup("sinf"), Some(FbcOpcode::Sinf));
        assert_eq!(math_lib_lookup("sin"), Some(FbcOpcode::Sinf));
        assert_eq!(math_lib_lookup("abs"), Some(FbcOpcode::Abs));
        assert_eq!(math_lib_lookup("unknown"), None);
    }

    #[test]
    fn test_binop_to_fbc() {
        let (int_op, real_op) = binop_to_fbc(fir::FirBinOp::Add);
        assert_eq!(int_op, FbcOpcode::AddInt);
        assert_eq!(real_op, FbcOpcode::AddReal);
    }

    #[test]
    fn test_parse_io_channel() {
        assert_eq!(parse_io_channel("input0", "input"), Some(0));
        assert_eq!(parse_io_channel("input3", "input"), Some(3));
        assert_eq!(parse_io_channel("output1", "output"), Some(1));
        assert_eq!(parse_io_channel("fGain", "input"), None);
    }
}
