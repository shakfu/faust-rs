//! FBC bytecode instruction and block types.
//!
//! # Source provenance (C++)
//! - `compiler/generator/interpreter/interpreter_bytecode.hh`
//!   (`FBCBasicInstruction<REAL>`, `FBCBlockInstruction<REAL>`,
//!   `FIRBlockStoreRealInstruction<REAL>`, `FIRBlockStoreIntInstruction<REAL>`,
//!   `FIRUserInterfaceInstruction<REAL>`, `FIRMetaInstruction`)
//!
//! # Design notes
//! - FBC instructions are stored in flat `Vec`s, not in `TreeArena`, because
//!   FBC is a linear instruction stream optimized for sequential execution
//!   (see porting plan §3.1).
//! - C++ raw pointers to `FBCBlockInstruction<REAL>` are replaced by [`BlockId`]
//!   indices into [`FbcBlockArena`].
//! - C++ `template <class REAL>` is replaced by the [`FbcReal`] trait bound.
//!
//! # API mapping status
//! - `FBCBasicInstruction<REAL>` → [`FbcInstruction<R>`]: adapted (fields kept,
//!   pointers replaced by `BlockId`).
//! - `FBCBlockInstruction<REAL>` → [`FbcBlock<R>`]: adapted.
//! - `FIRBlockStoreRealInstruction<REAL>` → [`BlockStoreData::Real`]: merged.
//! - `FIRBlockStoreIntInstruction<REAL>` → [`BlockStoreData::Int`]: merged.
//! - `FIRUserInterfaceInstruction<REAL>` → [`FbcUiInstruction<R>`]: adapted.
//! - `FIRMetaInstruction` → [`FbcMetaInstruction`]: 1:1.

use super::opcode::FbcOpcode;
use super::real::FbcReal;

/// Index into [`FbcBlockArena`].
///
/// Replaces C++ raw `FBCBlockInstruction<REAL>*` pointers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockId(u32);

impl BlockId {
    /// Returns the raw index value.
    #[must_use]
    pub fn as_u32(self) -> u32 {
        self.0
    }

    /// Creates a `BlockId` from a raw index.
    ///
    /// Used by the compiler to predict the next arena allocation index
    /// for `CondBranch` loop-back references.
    ///
    /// # Safety note
    /// This is `pub(crate)` because it creates an ID that may not yet
    /// exist in the arena — the caller must allocate the block immediately
    /// after.
    #[must_use]
    pub(crate) fn from_raw(index: u32) -> Self {
        Self(index)
    }
}

/// A single FBC instruction.
///
/// # Source provenance (C++)
/// - `FBCBasicInstruction<REAL>` in `interpreter_bytecode.hh`
///
/// Generic over `R` (the REAL type: `f32` or `f64`).
///
/// # Field semantics
/// - `opcode`: the instruction type.
/// - `name`: variable/field name for UI and memory instructions (rare).
/// - `int_value`: integer immediate (e.g., constant value, loop bound).
/// - `real_value`: real immediate (e.g., constant value).
/// - `offset1`, `offset2`: heap offsets for memory operations.
/// - `branch1`: branch 1 (if-true / loop-init block).
/// - `branch2`: branch 2 (if-false / loop-body block).
#[derive(Clone, Debug)]
pub struct FbcInstruction<R: FbcReal> {
    pub opcode: FbcOpcode,
    pub name: String,
    pub int_value: i32,
    pub real_value: R,
    pub offset1: i32,
    pub offset2: i32,
    pub branch1: Option<BlockId>,
    pub branch2: Option<BlockId>,
}

impl<R: FbcReal> FbcInstruction<R> {
    /// Creates a new instruction with only an opcode (all other fields default).
    ///
    /// Corresponds to `FBCBasicInstruction(Opcode opcode)` in C++.
    #[must_use]
    pub fn new(opcode: FbcOpcode) -> Self {
        Self {
            opcode,
            name: String::new(),
            int_value: 0,
            real_value: R::default(),
            offset1: -1,
            offset2: -1,
            branch1: None,
            branch2: None,
        }
    }

    /// Creates a new instruction with opcode and immediates.
    ///
    /// Corresponds to `FBCBasicInstruction(Opcode, int, REAL)` in C++.
    #[must_use]
    pub fn with_values(opcode: FbcOpcode, int_value: i32, real_value: R) -> Self {
        Self {
            opcode,
            name: String::new(),
            int_value,
            real_value,
            offset1: -1,
            offset2: -1,
            branch1: None,
            branch2: None,
        }
    }

    /// Creates a new instruction with opcode, immediates, and offsets.
    ///
    /// Corresponds to `FBCBasicInstruction(Opcode, int, REAL, int, int)` in C++.
    #[must_use]
    pub fn with_values_and_offsets(
        opcode: FbcOpcode,
        int_value: i32,
        real_value: R,
        offset1: i32,
        offset2: i32,
    ) -> Self {
        Self {
            opcode,
            name: String::new(),
            int_value,
            real_value,
            offset1,
            offset2,
            branch1: None,
            branch2: None,
        }
    }

    /// Creates a new instruction with a name.
    ///
    /// Corresponds to `FBCBasicInstruction(Opcode, string)` in C++.
    #[must_use]
    pub fn with_name(opcode: FbcOpcode, name: impl Into<String>) -> Self {
        Self {
            opcode,
            name: name.into(),
            int_value: 0,
            real_value: R::default(),
            offset1: -1,
            offset2: -1,
            branch1: None,
            branch2: None,
        }
    }

    /// Creates a fully specified instruction.
    ///
    /// Corresponds to the most general `FBCBasicInstruction` constructor in C++.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn full(
        opcode: FbcOpcode,
        name: impl Into<String>,
        int_value: i32,
        real_value: R,
        offset1: i32,
        offset2: i32,
        branch1: Option<BlockId>,
        branch2: Option<BlockId>,
    ) -> Self {
        Self {
            opcode,
            name: name.into(),
            int_value,
            real_value,
            offset1,
            offset2,
            branch1,
            branch2,
        }
    }

    /// Returns branch1 unless this is a `CondBranch` instruction.
    ///
    /// # Source provenance (C++)
    /// - `FBCBasicInstruction::getBranch1()` — returns `nullptr` for `kCondBranch`
    ///   because `fBranch1` is the loop-back pointer (owned by the block, not the
    ///   instruction).
    #[must_use]
    pub fn get_branch1(&self) -> Option<BlockId> {
        if self.opcode == FbcOpcode::CondBranch {
            None
        } else {
            self.branch1
        }
    }

    /// Returns branch2.
    #[must_use]
    pub fn get_branch2(&self) -> Option<BlockId> {
        self.branch2
    }
}

/// Optional bulk-store data attached to `BlockStoreReal` / `BlockStoreInt`
/// instructions.
///
/// # Source provenance (C++)
/// - `FIRBlockStoreRealInstruction<REAL>` and `FIRBlockStoreIntInstruction<REAL>`
///   in `interpreter_bytecode.hh`.
///
/// In C++ these are subclasses carrying a `fNumTable` vector. In Rust we model
/// them as an auxiliary enum stored alongside the instruction in the block.
#[derive(Clone, Debug)]
pub enum BlockStoreData<R: FbcReal> {
    Real(Vec<R>),
    Int(Vec<i32>),
}

/// A block of FBC instructions (linear sequence ending with `Return`).
///
/// # Source provenance (C++)
/// - `FBCBlockInstruction<REAL>` in `interpreter_bytecode.hh`
///
/// # Invariant
/// The last instruction in a well-formed block must have opcode
/// [`FbcOpcode::Return`].
#[derive(Clone, Debug)]
pub struct FbcBlock<R: FbcReal> {
    pub instructions: Vec<FbcInstruction<R>>,
    /// Optional bulk-store data for `BlockStoreReal`/`BlockStoreInt` instructions.
    /// Keyed by instruction index within this block.
    pub block_store_data: Vec<(usize, BlockStoreData<R>)>,
}

impl<R: FbcReal> FbcBlock<R> {
    /// Creates an empty block.
    #[must_use]
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            block_store_data: Vec::new(),
        }
    }

    /// Appends an instruction to the block.
    pub fn push(&mut self, instr: FbcInstruction<R>) {
        self.instructions.push(instr);
    }

    /// Appends a block-store instruction with associated data table.
    pub fn push_block_store(&mut self, instr: FbcInstruction<R>, data: BlockStoreData<R>) {
        let idx = self.instructions.len();
        self.instructions.push(instr);
        self.block_store_data.push((idx, data));
    }

    /// Returns `true` if the block ends with a `Return` instruction.
    ///
    /// # Source provenance (C++)
    /// - `FBCBlockInstruction::check()` in `interpreter_bytecode.hh`
    #[must_use]
    pub fn is_well_formed(&self) -> bool {
        self.instructions
            .last()
            .is_some_and(|instr| instr.opcode == FbcOpcode::Return)
    }

    /// Returns `true` if the last instruction operates on real values.
    ///
    /// # Source provenance (C++)
    /// - `FBCBlockInstruction::isRealInst()` in `interpreter_bytecode.hh`
    #[must_use]
    pub fn is_real_inst(&self) -> bool {
        self.instructions
            .last()
            .is_some_and(|instr| instr.opcode.is_real_type())
    }

    /// Returns the number of instructions in the block.
    #[must_use]
    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    /// Returns `true` if the block is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }
}

impl<R: FbcReal> Default for FbcBlock<R> {
    fn default() -> Self {
        Self::new()
    }
}

/// Arena-like storage for all blocks in a DSP factory.
///
/// Blocks reference each other via [`BlockId`] indices, replacing C++ raw
/// `FBCBlockInstruction<REAL>*` pointers. This provides safe, index-based
/// ownership without `unsafe` pointer arithmetic.
///
/// # Source provenance (C++)
/// - Implicit in `interpreter_dsp_aux.hh` (blocks are owned by the factory
///   and referenced by pointer).
#[derive(Clone, Debug)]
pub struct FbcBlockArena<R: FbcReal> {
    blocks: Vec<FbcBlock<R>>,
}

impl<R: FbcReal> FbcBlockArena<R> {
    /// Creates an empty block arena.
    #[must_use]
    pub fn new() -> Self {
        Self { blocks: Vec::new() }
    }

    /// Allocates a block in the arena and returns its [`BlockId`].
    pub fn alloc(&mut self, block: FbcBlock<R>) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(block);
        id
    }

    /// Returns a reference to the block at the given ID.
    ///
    /// # Panics
    /// Panics if `id` is out of range.
    #[must_use]
    pub fn get(&self, id: BlockId) -> &FbcBlock<R> {
        &self.blocks[id.0 as usize]
    }

    /// Returns a reference to the block at the given ID, or `None` if out of range.
    #[must_use]
    pub fn try_get(&self, id: BlockId) -> Option<&FbcBlock<R>> {
        self.blocks.get(id.0 as usize)
    }

    /// Returns a mutable reference to the block at the given ID.
    ///
    /// # Panics
    /// Panics if `id` is out of range.
    pub fn get_mut(&mut self, id: BlockId) -> &mut FbcBlock<R> {
        &mut self.blocks[id.0 as usize]
    }

    /// Returns a mutable reference to the block at the given ID, or `None` if out of range.
    pub fn try_get_mut(&mut self, id: BlockId) -> Option<&mut FbcBlock<R>> {
        self.blocks.get_mut(id.0 as usize)
    }

    /// Returns the number of blocks in the arena.
    #[must_use]
    pub fn len(&self) -> usize {
        self.blocks.len()
    }

    /// Returns `true` if the arena contains no blocks.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

impl<R: FbcReal> Default for FbcBlockArena<R> {
    fn default() -> Self {
        Self::new()
    }
}

/// A UI instruction in the FBC bytecode.
///
/// # Source provenance (C++)
/// - `FIRUserInterfaceInstruction<REAL>` in `interpreter_bytecode.hh`
///
/// # API mapping status
/// - adapted: C++ overloaded constructors are replaced by a single struct
///   with `Option` / default fields.
#[derive(Clone, Debug)]
pub struct FbcUiInstruction<R: FbcReal> {
    pub opcode: FbcOpcode,
    pub offset: i32,
    pub label: String,
    pub key: String,
    pub value: String,
    pub init: R,
    pub min: R,
    pub max: R,
    pub step: R,
}

impl<R: FbcReal> FbcUiInstruction<R> {
    /// Creates a UI instruction with all fields defaulted.
    #[must_use]
    pub fn new(opcode: FbcOpcode) -> Self {
        Self {
            opcode,
            offset: -1,
            label: String::new(),
            key: String::new(),
            value: String::new(),
            init: R::default(),
            min: R::default(),
            max: R::default(),
            step: R::default(),
        }
    }

    /// Creates a box-open UI instruction (vertical/horizontal/tab).
    #[must_use]
    pub fn open_box(opcode: FbcOpcode, label: impl Into<String>) -> Self {
        Self {
            opcode,
            offset: -1,
            label: label.into(),
            key: String::new(),
            value: String::new(),
            init: R::default(),
            min: R::default(),
            max: R::default(),
            step: R::default(),
        }
    }

    /// Creates a widget UI instruction (button/slider/etc.) with range.
    #[must_use]
    pub fn widget(
        opcode: FbcOpcode,
        offset: i32,
        label: impl Into<String>,
        init: R,
        min: R,
        max: R,
        step: R,
    ) -> Self {
        Self {
            opcode,
            offset,
            label: label.into(),
            key: String::new(),
            value: String::new(),
            init,
            min,
            max,
            step,
        }
    }

    /// Creates a bargraph UI instruction with range.
    #[must_use]
    pub fn bargraph(
        opcode: FbcOpcode,
        offset: i32,
        label: impl Into<String>,
        min: R,
        max: R,
    ) -> Self {
        Self {
            opcode,
            offset,
            label: label.into(),
            key: String::new(),
            value: String::new(),
            init: R::default(),
            min,
            max,
            step: R::default(),
        }
    }

    /// Creates a metadata declare UI instruction.
    #[must_use]
    pub fn declare(offset: i32, key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            opcode: FbcOpcode::Declare,
            offset,
            label: String::new(),
            key: key.into(),
            value: value.into(),
            init: R::default(),
            min: R::default(),
            max: R::default(),
            step: R::default(),
        }
    }
}

/// A metadata key-value pair.
///
/// # Source provenance (C++)
/// - `FIRMetaInstruction` in `interpreter_bytecode.hh`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FbcMetaInstruction {
    pub key: String,
    pub value: String,
}

impl FbcMetaInstruction {
    /// Creates a new metadata instruction.
    #[must_use]
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_arena_alloc_and_get() {
        let mut arena = FbcBlockArena::<f32>::new();
        assert!(arena.is_empty());

        let mut block = FbcBlock::new();
        block.push(FbcInstruction::new(FbcOpcode::Int32Value));
        block.push(FbcInstruction::new(FbcOpcode::Return));

        let id = arena.alloc(block);
        assert_eq!(id.as_u32(), 0);
        assert_eq!(arena.len(), 1);
        assert!(!arena.is_empty());

        let retrieved = arena.get(id);
        assert_eq!(retrieved.len(), 2);
        assert_eq!(retrieved.instructions[0].opcode, FbcOpcode::Int32Value);
        assert_eq!(retrieved.instructions[1].opcode, FbcOpcode::Return);
    }

    #[test]
    fn block_well_formed() {
        let mut block = FbcBlock::<f64>::new();
        assert!(!block.is_well_formed());

        block.push(FbcInstruction::new(FbcOpcode::AddReal));
        assert!(!block.is_well_formed());

        block.push(FbcInstruction::new(FbcOpcode::Return));
        assert!(block.is_well_formed());
    }

    #[test]
    fn instruction_with_values() {
        let instr = FbcInstruction::<f32>::with_values(FbcOpcode::RealValue, 0, 3.125);
        assert_eq!(instr.opcode, FbcOpcode::RealValue);
        assert_eq!(instr.int_value, 0);
        assert!((instr.real_value - 3.125).abs() < 1e-6);
        assert_eq!(instr.offset1, -1);
        assert_eq!(instr.offset2, -1);
        assert!(instr.branch1.is_none());
        assert!(instr.branch2.is_none());
    }

    #[test]
    fn instruction_with_values_and_offsets() {
        let instr =
            FbcInstruction::<f64>::with_values_and_offsets(FbcOpcode::LoadReal, 0, 0.0, 42, -1);
        assert_eq!(instr.opcode, FbcOpcode::LoadReal);
        assert_eq!(instr.offset1, 42);
        assert_eq!(instr.offset2, -1);
    }

    #[test]
    fn instruction_full() {
        let instr = FbcInstruction::<f32>::full(
            FbcOpcode::If,
            "",
            0,
            0.0,
            -1,
            -1,
            Some(BlockId(1)),
            Some(BlockId(2)),
        );
        assert_eq!(instr.opcode, FbcOpcode::If);
        assert_eq!(instr.branch1, Some(BlockId(1)));
        assert_eq!(instr.branch2, Some(BlockId(2)));
    }

    #[test]
    fn get_branch1_cond_branch_returns_none() {
        let instr = FbcInstruction::<f32>::full(
            FbcOpcode::CondBranch,
            "",
            0,
            0.0,
            -1,
            -1,
            Some(BlockId(0)),
            None,
        );
        // CondBranch's branch1 is the loop-back pointer, not semantically owned.
        assert!(instr.get_branch1().is_none());
    }

    #[test]
    fn block_is_real_inst() {
        let mut block = FbcBlock::<f32>::new();
        block.push(FbcInstruction::new(FbcOpcode::AddReal));
        assert!(block.is_real_inst());

        let mut block2 = FbcBlock::<f32>::new();
        block2.push(FbcInstruction::new(FbcOpcode::AddInt));
        assert!(!block2.is_real_inst());
    }

    #[test]
    fn block_store_data() {
        let mut block = FbcBlock::<f32>::new();
        let instr =
            FbcInstruction::with_values_and_offsets(FbcOpcode::BlockStoreReal, 0, 0.0, 0, 4);
        let data = BlockStoreData::Real(vec![1.0, 2.0, 3.0, 4.0]);
        block.push_block_store(instr, data);
        block.push(FbcInstruction::new(FbcOpcode::Return));

        assert_eq!(block.len(), 2);
        assert_eq!(block.block_store_data.len(), 1);
        assert_eq!(block.block_store_data[0].0, 0); // index of the block-store instruction
    }

    #[test]
    fn ui_instruction_constructors() {
        let open = FbcUiInstruction::<f32>::open_box(FbcOpcode::OpenVerticalBox, "main");
        assert_eq!(open.opcode, FbcOpcode::OpenVerticalBox);
        assert_eq!(open.label, "main");

        let slider = FbcUiInstruction::<f64>::widget(
            FbcOpcode::AddHorizontalSlider,
            10,
            "gain",
            0.5,
            0.0,
            1.0,
            0.01,
        );
        assert_eq!(slider.opcode, FbcOpcode::AddHorizontalSlider);
        assert_eq!(slider.offset, 10);
        assert_eq!(slider.label, "gain");
        assert!((slider.init - 0.5).abs() < 1e-10);

        let meta = FbcUiInstruction::<f32>::declare(5, "unit", "dB");
        assert_eq!(meta.opcode, FbcOpcode::Declare);
        assert_eq!(meta.key, "unit");
        assert_eq!(meta.value, "dB");
    }

    #[test]
    fn meta_instruction() {
        let meta = FbcMetaInstruction::new("name", "sine");
        assert_eq!(meta.key, "name");
        assert_eq!(meta.value, "sine");
    }

    #[test]
    fn multiple_blocks_in_arena() {
        let mut arena = FbcBlockArena::<f32>::new();

        let mut b1 = FbcBlock::new();
        b1.push(FbcInstruction::new(FbcOpcode::Return));
        let id1 = arena.alloc(b1);

        let mut b2 = FbcBlock::new();
        b2.push(FbcInstruction::with_values(FbcOpcode::Int32Value, 42, 0.0));
        b2.push(FbcInstruction::new(FbcOpcode::Return));
        let id2 = arena.alloc(b2);

        assert_eq!(arena.len(), 2);
        assert_eq!(arena.get(id1).len(), 1);
        assert_eq!(arena.get(id2).len(), 2);
        assert_eq!(arena.get(id2).instructions[0].int_value, 42);
    }
}
