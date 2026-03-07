//! `FbcDspFactory<R>` — compiled FBC bytecode program ready for instantiation.
//!
//! # Source provenance (C++)
//! - `interpreter_dsp_factory_aux<REAL, TRACE>` in `interpreter_dsp_aux.hh`
//!
//! # Design notes
//! - Holds 6 bytecode blocks (as [`BlockId`]s into the [`FbcBlockArena`]),
//!   plus metadata and UI instruction lists.
//! - The `optimize()` method applies the bytecode optimizer (levels 1..opt_level)
//!   to all 6 code blocks, guarded by a one-shot flag.
//! - No `TRACE` template parameter — tracing is a future runtime option.

use super::bytecode::{BlockId, FbcBlockArena, FbcMetaInstruction, FbcUiInstruction};
use super::optimizer::optimize_block;
use super::real::FbcReal;

/// Compiled FBC program ready for instantiation.
///
/// # Source provenance (C++)
/// - `interpreter_dsp_factory_aux<REAL, TRACE>` in `interpreter_dsp_aux.hh`
///
/// Generic over `R` (the REAL type: `f32` or `f64`).
///
/// # Fields
/// - `name`, `sha_key`, `compile_options`: factory identity/metadata.
/// - `version`: `.fbc` file format version (must match [`INTERP_FILE_VERSION`]).
/// - `num_inputs`, `num_outputs`: audio channel counts.
/// - `int_heap_size`, `real_heap_size`: heap allocation sizes for instances.
/// - `sr_offset`, `count_offset`, `iota_offset`: well-known heap slot indices.
/// - `opt_level`: maximum optimizer level (0 = no optimization).
/// - `arena`: block arena owning all bytecode blocks.
/// - `meta_block`, `ui_block`: metadata and UI instruction lists.
/// - 6 `BlockId`s referencing code blocks in the arena.
///
/// The factory is intentionally immutable after construction except for the
/// optional one-shot optimizer pass. Runtime heaps and DSP execution state live
/// in `FbcDspInstance`, not here.
///
/// [`INTERP_FILE_VERSION`]: super::opcode::INTERP_FILE_VERSION
#[derive(Debug)]
pub struct FbcDspFactory<R: FbcReal> {
    // ── Identity / metadata ────────────────────────────────────────────
    pub name: String,
    pub sha_key: String,
    pub compile_options: String,
    pub version: u32,

    // ── Audio layout ───────────────────────────────────────────────────
    pub num_inputs: i32,
    pub num_outputs: i32,

    // ── Memory layout ──────────────────────────────────────────────────
    pub int_heap_size: i32,
    pub real_heap_size: i32,
    pub sr_offset: i32,
    pub count_offset: i32,
    pub iota_offset: i32,

    // ── Optimizer ──────────────────────────────────────────────────────
    pub opt_level: i32,
    optimized: bool,

    // ── Block arena ────────────────────────────────────────────────────
    pub arena: FbcBlockArena<R>,

    // ── Data blocks ────────────────────────────────────────────────────
    pub meta_block: Vec<FbcMetaInstruction>,
    pub ui_block: Vec<FbcUiInstruction<R>>,

    // ── Code block IDs ─────────────────────────────────────────────────
    pub static_init_block: BlockId,
    pub init_block: BlockId,
    pub reset_ui_block: BlockId,
    pub clear_block: BlockId,
    pub compute_block: BlockId,
    pub compute_dsp_block: BlockId,
}

impl<R: FbcReal> FbcDspFactory<R> {
    /// Creates a new factory with all fields provided.
    ///
    /// # Source provenance (C++)
    /// - `interpreter_dsp_factory_aux` constructor in `interpreter_dsp_aux.hh`
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: impl Into<String>,
        sha_key: impl Into<String>,
        compile_options: impl Into<String>,
        version: u32,
        num_inputs: i32,
        num_outputs: i32,
        int_heap_size: i32,
        real_heap_size: i32,
        sr_offset: i32,
        count_offset: i32,
        iota_offset: i32,
        opt_level: i32,
        arena: FbcBlockArena<R>,
        meta_block: Vec<FbcMetaInstruction>,
        ui_block: Vec<FbcUiInstruction<R>>,
        static_init_block: BlockId,
        init_block: BlockId,
        reset_ui_block: BlockId,
        clear_block: BlockId,
        compute_block: BlockId,
        compute_dsp_block: BlockId,
    ) -> Self {
        Self {
            name: name.into(),
            sha_key: sha_key.into(),
            compile_options: compile_options.into(),
            version,
            num_inputs,
            num_outputs,
            int_heap_size,
            real_heap_size,
            sr_offset,
            count_offset,
            iota_offset,
            opt_level,
            optimized: false,
            arena,
            meta_block,
            ui_block,
            static_init_block,
            init_block,
            reset_ui_block,
            clear_block,
            compute_block,
            compute_dsp_block,
        }
    }

    /// Applies bytecode optimization (levels 1..opt_level) to all 6 code blocks.
    ///
    /// This is idempotent: the first call optimizes, subsequent calls are no-ops.
    /// Optimization is factory-wide on purpose so every instance created from
    /// the same factory sees the same stabilized bytecode layout.
    ///
    /// # Source provenance (C++)
    /// - `interpreter_dsp_factory_aux::optimize()` in `interpreter_dsp.hh`
    ///   (only runs when `TRACE == 0` and `!fOptimized`).
    pub fn optimize(&mut self) {
        if self.optimized {
            return;
        }
        self.optimized = true;

        if self.opt_level <= 0 {
            return;
        }

        let max = self.opt_level as u8;
        optimize_block::<R>(&mut self.arena, self.static_init_block, 1, max);
        optimize_block::<R>(&mut self.arena, self.init_block, 1, max);
        optimize_block::<R>(&mut self.arena, self.reset_ui_block, 1, max);
        optimize_block::<R>(&mut self.arena, self.clear_block, 1, max);
        optimize_block::<R>(&mut self.arena, self.compute_block, 1, max);
        optimize_block::<R>(&mut self.arena, self.compute_dsp_block, 1, max);
    }

    /// Returns whether this factory has already been optimized.
    #[must_use]
    pub fn is_optimized(&self) -> bool {
        self.optimized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::interp::bytecode::FbcBlock;
    use crate::backends::interp::opcode::FbcOpcode;
    use crate::backends::interp::{FbcInstruction, INTERP_FILE_VERSION};

    /// Helper: creates a trivial block containing only a `Return` instruction.
    fn trivial_block(arena: &mut FbcBlockArena<f32>) -> BlockId {
        let mut block = FbcBlock::new();
        block.push(FbcInstruction::new(FbcOpcode::Return));
        arena.alloc(block)
    }

    /// Helper: creates a factory with trivial (empty) blocks.
    fn make_trivial_factory() -> FbcDspFactory<f32> {
        let mut arena = FbcBlockArena::new();
        let b1 = trivial_block(&mut arena);
        let b2 = trivial_block(&mut arena);
        let b3 = trivial_block(&mut arena);
        let b4 = trivial_block(&mut arena);
        let b5 = trivial_block(&mut arena);
        let b6 = trivial_block(&mut arena);

        FbcDspFactory::new(
            "test",
            "sha123",
            "-lang interp",
            INTERP_FILE_VERSION,
            1,
            1,
            16,
            16,
            0, // sr_offset
            1, // count_offset
            2, // iota_offset
            4, // opt_level
            arena,
            vec![FbcMetaInstruction::new("name", "test")],
            vec![],
            b1,
            b2,
            b3,
            b4,
            b5,
            b6,
        )
    }

    #[test]
    fn factory_construction() {
        let factory = make_trivial_factory();
        assert_eq!(factory.name, "test");
        assert_eq!(factory.sha_key, "sha123");
        assert_eq!(factory.num_inputs, 1);
        assert_eq!(factory.num_outputs, 1);
        assert_eq!(factory.int_heap_size, 16);
        assert_eq!(factory.real_heap_size, 16);
        assert_eq!(factory.opt_level, 4);
        assert!(!factory.is_optimized());
        assert_eq!(factory.meta_block.len(), 1);
        assert_eq!(factory.meta_block[0].key, "name");
    }

    #[test]
    fn factory_optimize_idempotent() {
        let mut factory = make_trivial_factory();
        assert!(!factory.is_optimized());

        factory.optimize();
        assert!(factory.is_optimized());

        // Second call is a no-op.
        factory.optimize();
        assert!(factory.is_optimized());
    }

    #[test]
    fn factory_optimize_reduces_instructions() {
        // Build a block with LoadReal(0) + StoreReal(1) that should fuse to MoveReal.
        let mut arena = FbcBlockArena::<f32>::new();

        // Optimizable block.
        let mut block = FbcBlock::new();
        block.push(FbcInstruction::with_values_and_offsets(
            FbcOpcode::LoadReal,
            0,
            0.0,
            0,
            -1,
        ));
        block.push(FbcInstruction::with_values_and_offsets(
            FbcOpcode::StoreReal,
            0,
            0.0,
            1,
            -1,
        ));
        block.push(FbcInstruction::new(FbcOpcode::Return));
        let opt_block = arena.alloc(block);

        // Trivial blocks for the rest.
        let b2 = trivial_block(&mut arena);
        let b3 = trivial_block(&mut arena);
        let b4 = trivial_block(&mut arena);
        let b5 = trivial_block(&mut arena);
        let b6 = trivial_block(&mut arena);

        let mut factory = FbcDspFactory::new(
            "test",
            "",
            "",
            INTERP_FILE_VERSION,
            0,
            0,
            4,
            4,
            0,
            1,
            -1,
            4, // opt_level 4 includes level 2 (Move fusion)
            arena,
            vec![],
            vec![],
            opt_block,
            b2,
            b3,
            b4,
            b5,
            b6,
        );

        // Before optimization: 3 instructions (Load, Store, Return).
        assert_eq!(factory.arena.get(factory.static_init_block).len(), 3);

        factory.optimize();

        // After optimization: 2 instructions (MoveReal, Return).
        assert_eq!(factory.arena.get(factory.static_init_block).len(), 2);
        assert_eq!(
            factory.arena.get(factory.static_init_block).instructions[0].opcode,
            FbcOpcode::MoveReal
        );
    }
}
