//! FBC interpreter execution engine.
//!
//! # Source provenance (C++)
//! - `compiler/generator/interpreter/fbc_interpreter.hh`
//!   (`FBCInterpreter<REAL, TRACE>::executeBlock`)
//!
//! # Design notes
//! - Replaces C++ computed-goto dispatch (`goto* fDispatchTable[opcode]`) with a
//!   tight `loop { match }` pattern. The `#[repr(u16)]` opcode enum enables LLVM
//!   to generate a jump table, achieving comparable dispatch efficiency.
//! - No `unsafe`: all stack/heap accesses are bounds-checked. The performance
//!   overhead is negligible versus the compute-bound math operations.
//! - Equivalent to C++ `TRACE=0` mode (no tracing / overflow checks).
//!
//! # Control flow model
//! The interpreter uses three stacks local to each `execute_block_io` call:
//! - `real_stack[512]` — computation stack for REAL values
//! - `int_stack[512]` — computation stack for integers
//! - `addr_stack[64]` — return addresses as `(BlockId, pc)` pairs
//!
//! When a branch instruction is encountered (If, Select, Loop), the current
//! execution position is saved on `addr_stack`, and control jumps to the target
//! block. When `Return` is reached and the address stack is non-empty, execution
//! resumes at the saved position.
//!
//! # API mapping status
//! - `FBCInterpreter<REAL, TRACE>::executeBlock` (C++) →
//!   [`FbcExecutor::execute_block_io`] (Rust): adapted (computed goto → match,
//!   raw pointers → `BlockId`, C arrays → `Vec`).

use super::bytecode::{BlockId, BlockStoreData, FbcBlockArena};
use super::opcode::FbcOpcode;
use super::real::FbcReal;

/// Stack sizes matching the C++ interpreter constants.
const REAL_STACK_CAPACITY: usize = 512;
const INT_STACK_CAPACITY: usize = 512;
const ADDR_STACK_CAPACITY: usize = 64;

/// Execution stack kind used in structured runtime errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FbcStackKind {
    /// Integer value stack (`int_stack`).
    Int,
    /// Real value stack (`real_stack`).
    Real,
}

/// Structured runtime execution error for bytecode interpreter failures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FbcExecError {
    /// Human-readable stable error category.
    pub kind: &'static str,
    /// Opcode being executed when the error occurred.
    pub opcode: FbcOpcode,
    /// Block id of the executing bytecode block.
    pub block_id: BlockId,
    /// Program counter within the block.
    pub pc: usize,
    /// Optional stack underflow detail.
    pub stack: Option<FbcStackKind>,
    /// Optional I/O channel index related to the failure.
    pub channel: Option<usize>,
    /// Optional sample index related to the failure.
    pub sample: Option<usize>,
}

impl FbcExecError {
    fn stack_underflow(
        opcode: FbcOpcode,
        block_id: BlockId,
        pc: usize,
        stack: FbcStackKind,
    ) -> Self {
        Self {
            kind: "stack_underflow",
            opcode,
            block_id,
            pc,
            stack: Some(stack),
            channel: None,
            sample: None,
        }
    }

    fn missing_branch_target(opcode: FbcOpcode, block_id: BlockId, pc: usize) -> Self {
        Self {
            kind: "missing_branch_target",
            opcode,
            block_id,
            pc,
            stack: None,
            channel: None,
            sample: None,
        }
    }

    fn unsupported_runtime_feature(opcode: FbcOpcode, block_id: BlockId, pc: usize) -> Self {
        Self {
            kind: "unsupported_runtime_feature",
            opcode,
            block_id,
            pc,
            stack: None,
            channel: None,
            sample: None,
        }
    }

    fn panic_trapped(opcode: FbcOpcode, block_id: BlockId, pc: usize) -> Self {
        Self {
            kind: "panic_trapped",
            opcode,
            block_id,
            pc,
            stack: None,
            channel: None,
            sample: None,
        }
    }

    fn io_oob(
        opcode: FbcOpcode,
        block_id: BlockId,
        pc: usize,
        channel: usize,
        sample: usize,
    ) -> Self {
        Self {
            kind: "io_oob",
            opcode,
            block_id,
            pc,
            stack: None,
            channel: Some(channel),
            sample: Some(sample),
        }
    }
}

impl std::fmt::Display for FbcExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.stack, self.channel, self.sample) {
            (Some(stack), _, _) => write!(
                f,
                "FBC runtime error [{}] opcode={:?} block={:?} pc={} stack={:?}",
                self.kind, self.opcode, self.block_id, self.pc, stack
            ),
            (None, Some(ch), Some(smp)) => write!(
                f,
                "FBC runtime error [{}] opcode={:?} block={:?} pc={} channel={} sample={}",
                self.kind, self.opcode, self.block_id, self.pc, ch, smp
            ),
            _ => write!(
                f,
                "FBC runtime error [{}] opcode={:?} block={:?} pc={}",
                self.kind, self.opcode, self.block_id, self.pc
            ),
        }
    }
}

impl std::error::Error for FbcExecError {}

fn pop_real_stack<R: FbcReal>(
    real_stack: &mut Vec<R>,
    opcode: FbcOpcode,
    block_id: BlockId,
    pc: usize,
) -> Result<R, FbcExecError> {
    real_stack
        .pop()
        .ok_or_else(|| FbcExecError::stack_underflow(opcode, block_id, pc, FbcStackKind::Real))
}

fn pop_int_stack(
    int_stack: &mut Vec<i32>,
    opcode: FbcOpcode,
    block_id: BlockId,
    pc: usize,
) -> Result<i32, FbcExecError> {
    int_stack
        .pop()
        .ok_or_else(|| FbcExecError::stack_underflow(opcode, block_id, pc, FbcStackKind::Int))
}

fn require_branch_target(
    target: Option<BlockId>,
    opcode: FbcOpcode,
    block_id: BlockId,
    pc: usize,
) -> Result<BlockId, FbcExecError> {
    target.ok_or_else(|| FbcExecError::missing_branch_target(opcode, block_id, pc))
}

#[derive(Clone, Copy, Debug)]
struct ExecSite {
    opcode: FbcOpcode,
    block_id: BlockId,
    pc: usize,
}

/// FBC bytecode execution engine.
///
/// Holds mutable state (heaps) and executes bytecode blocks from an
/// [`FbcBlockArena`]. This is the core dispatch loop, ported from C++
/// `FBCInterpreter<REAL, TRACE>::executeBlock`.
///
/// # Source provenance (C++)
/// - `FBCInterpreter<REAL, TRACE>` in `fbc_interpreter.hh`
///
/// # Memory model
/// - **Int heap** (`int_heap`): counters, indices, loop variables.
/// - **Real heap** (`real_heap`): state variables, filter memory, UI zones.
/// - **Execution stacks**: local to each `execute_block_io` call.
pub struct FbcExecutor<R: FbcReal> {
    /// Integer heap (counters, indices, loop variables).
    pub int_heap: Vec<i32>,
    /// Real heap (state variables, filters, UI zones).
    pub real_heap: Vec<R>,
}

impl<R: FbcReal> FbcExecutor<R> {
    /// Creates a new executor with zeroed heaps of the given sizes.
    #[must_use]
    pub fn new(int_heap_size: usize, real_heap_size: usize) -> Self {
        Self {
            int_heap: vec![0; int_heap_size],
            real_heap: vec![R::default(); real_heap_size],
        }
    }

    /// Executes a block without audio I/O (for init, clear, control blocks).
    pub fn execute_block(&mut self, arena: &FbcBlockArena<R>, block_id: BlockId) {
        self.try_execute_block(arena, block_id)
            .unwrap_or_else(|e| panic!("{e}"));
    }

    /// Executes a block without audio I/O (for init, clear, control blocks),
    /// returning a structured runtime error instead of panicking.
    pub fn try_execute_block(
        &mut self,
        arena: &FbcBlockArena<R>,
        block_id: BlockId,
    ) -> Result<(), FbcExecError> {
        self.try_execute_block_io(arena, block_id, &[], &mut [])
    }

    /// Executes a block with audio I/O (for compute blocks).
    ///
    /// # Arguments
    /// - `arena`: the block arena containing all compiled blocks.
    /// - `block_id`: the block to execute.
    /// - `inputs`: audio input buffers (`inputs[channel][sample]`).
    /// - `outputs`: audio output buffers (`outputs[channel][sample]`).
    ///
    /// # Panics
    /// Panics on stack underflow, out-of-bounds heap access, or missing branch
    /// targets. These indicate bugs in bytecode generation, not user errors.
    #[allow(clippy::too_many_lines)]
    pub fn execute_block_io(
        &mut self,
        arena: &FbcBlockArena<R>,
        block_id: BlockId,
        inputs: &[&[R]],
        outputs: &mut [&mut [R]],
    ) {
        self.try_execute_block_io(arena, block_id, inputs, outputs)
            .unwrap_or_else(|e| panic!("{e}"));
    }

    /// Executes a block with audio I/O and returns a structured runtime error
    /// for detected stack-discipline failures instead of panicking.
    pub fn try_execute_block_io(
        &mut self,
        arena: &FbcBlockArena<R>,
        block_id: BlockId,
        inputs: &[&[R]],
        outputs: &mut [&mut [R]],
    ) -> Result<(), FbcExecError> {
        let mut last_site = ExecSite {
            opcode: FbcOpcode::Nop,
            block_id,
            pc: 0,
        };
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.try_execute_block_io_inner(arena, block_id, inputs, outputs, &mut last_site)
        })) {
            Ok(result) => result,
            Err(_payload) => Err(FbcExecError::panic_trapped(
                last_site.opcode,
                last_site.block_id,
                last_site.pc,
            )),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn try_execute_block_io_inner(
        &mut self,
        arena: &FbcBlockArena<R>,
        block_id: BlockId,
        inputs: &[&[R]],
        outputs: &mut [&mut [R]],
        last_site: &mut ExecSite,
    ) -> Result<(), FbcExecError> {
        use FbcOpcode::*;

        // Execution stacks (local to this call, matching C++ local arrays).
        let mut real_stack: Vec<R> = Vec::with_capacity(REAL_STACK_CAPACITY);
        let mut int_stack: Vec<i32> = Vec::with_capacity(INT_STACK_CAPACITY);
        let mut addr_stack: Vec<(BlockId, usize)> = Vec::with_capacity(ADDR_STACK_CAPACITY);

        // Current execution position.
        let mut cur_block = block_id;
        let mut pc: usize = 0;

        loop {
            let block = arena.get(cur_block);
            let instr = &block.instructions[pc];
            *last_site = ExecSite {
                opcode: instr.opcode,
                block_id: cur_block,
                pc,
            };

            // Pre-extract commonly used instruction fields.
            let o1 = instr.offset1 as usize;
            let o2 = instr.offset2 as usize;
            let iv = instr.int_value;
            let rv = instr.real_value;

            match instr.opcode {
                // ── Numbers ─────────────────────────────────────────────
                RealValue => {
                    real_stack.push(rv);
                    pc += 1;
                }
                Int32Value => {
                    int_stack.push(iv);
                    pc += 1;
                }

                // ── Memory: load/store ──────────────────────────────────
                LoadReal => {
                    real_stack.push(self.real_heap[o1]);
                    pc += 1;
                }
                LoadInt => {
                    int_stack.push(self.int_heap[o1]);
                    pc += 1;
                }
                LoadSoundFieldInt => {
                    return Err(FbcExecError::unsupported_runtime_feature(
                        instr.opcode,
                        cur_block,
                        pc,
                    ));
                }
                LoadSoundFieldReal => {
                    return Err(FbcExecError::unsupported_runtime_feature(
                        instr.opcode,
                        cur_block,
                        pc,
                    ));
                }
                StoreReal => {
                    self.real_heap[o1] =
                        pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    pc += 1;
                }
                StoreInt => {
                    self.int_heap[o1] = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    pc += 1;
                }
                StoreRealValue => {
                    self.real_heap[o1] = rv;
                    pc += 1;
                }
                StoreIntValue => {
                    self.int_heap[o1] = iv;
                    pc += 1;
                }
                LoadIndexedReal => {
                    let offset = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(self.real_heap[o1.wrapping_add(offset as usize)]);
                    pc += 1;
                }
                LoadIndexedInt => {
                    let offset = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(self.int_heap[o1.wrapping_add(offset as usize)]);
                    pc += 1;
                }
                StoreIndexedReal => {
                    let offset = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let val = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    self.real_heap[o1.wrapping_add(offset as usize)] = val;
                    pc += 1;
                }
                StoreIndexedInt => {
                    let offset = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let val = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    self.int_heap[o1.wrapping_add(offset as usize)] = val;
                    pc += 1;
                }
                BlockStoreReal => {
                    if let Some((_, BlockStoreData::Real(table))) =
                        block.block_store_data.iter().find(|(idx, _)| *idx == pc)
                    {
                        let count = o2;
                        self.real_heap[o1..(count + o1)].copy_from_slice(&table[..count]);
                    }
                    pc += 1;
                }
                BlockStoreInt => {
                    if let Some((_, BlockStoreData::Int(table))) =
                        block.block_store_data.iter().find(|(idx, _)| *idx == pc)
                    {
                        let count = o2;
                        self.int_heap[o1..(count + o1)].copy_from_slice(&table[..count]);
                    }
                    pc += 1;
                }
                MoveReal => {
                    self.real_heap[o1] = self.real_heap[o2];
                    pc += 1;
                }
                MoveInt => {
                    self.int_heap[o1] = self.int_heap[o2];
                    pc += 1;
                }
                PairMoveReal => {
                    self.real_heap[o1] = self.real_heap[o1 - 1];
                    self.real_heap[o2] = self.real_heap[o2 - 1];
                    pc += 1;
                }
                PairMoveInt => {
                    self.int_heap[o1] = self.int_heap[o1 - 1];
                    self.int_heap[o2] = self.int_heap[o2 - 1];
                    pc += 1;
                }
                BlockPairMoveReal => {
                    let mut i = o1;
                    while i < o2 {
                        self.real_heap[i + 1] = self.real_heap[i];
                        i += 2;
                    }
                    pc += 1;
                }
                BlockPairMoveInt => {
                    let mut i = o1;
                    while i < o2 {
                        self.int_heap[i + 1] = self.int_heap[i];
                        i += 2;
                    }
                    pc += 1;
                }
                BlockShiftReal => {
                    let mut i = o1;
                    while i > o2 {
                        self.real_heap[i] = self.real_heap[i - 1];
                        i -= 1;
                    }
                    pc += 1;
                }
                BlockShiftInt => {
                    let mut i = o1;
                    while i > o2 {
                        self.int_heap[i] = self.int_heap[i - 1];
                        i -= 1;
                    }
                    pc += 1;
                }

                // ── I/O ─────────────────────────────────────────────────
                LoadInput => {
                    let sample_idx =
                        pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)? as usize;
                    let channel = inputs.get(o1).ok_or_else(|| {
                        FbcExecError::io_oob(instr.opcode, cur_block, pc, o1, sample_idx)
                    })?;
                    let sample = channel.get(sample_idx).ok_or_else(|| {
                        FbcExecError::io_oob(instr.opcode, cur_block, pc, o1, sample_idx)
                    })?;
                    real_stack.push(*sample);
                    pc += 1;
                }
                StoreOutput => {
                    let sample_idx =
                        pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)? as usize;
                    let val = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let channel = outputs.get_mut(o1).ok_or_else(|| {
                        FbcExecError::io_oob(instr.opcode, cur_block, pc, o1, sample_idx)
                    })?;
                    let slot = channel.get_mut(sample_idx).ok_or_else(|| {
                        FbcExecError::io_oob(instr.opcode, cur_block, pc, o1, sample_idx)
                    })?;
                    *slot = val;
                    pc += 1;
                }

                // ── Cast / Bitcast ──────────────────────────────────────
                CastReal => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(R::from_i32(v));
                    pc += 1;
                }
                CastInt => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v.to_i32());
                    pc += 1;
                }
                CastRealHeap => {
                    real_stack.push(R::from_i32(self.int_heap[o1]));
                    pc += 1;
                }
                CastIntHeap => {
                    int_stack.push(self.real_heap[o1].to_i32());
                    pc += 1;
                }
                BitcastInt => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v.to_bits_i32());
                    pc += 1;
                }
                BitcastReal => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(R::from_bits_i32(v));
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Standard math: stack OP stack
                // ═══════════════════════════════════════════════════════

                // ── Real arithmetic ─────────────────────────────────────
                AddReal => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v1 + v2);
                    pc += 1;
                }
                SubReal => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v1 - v2);
                    pc += 1;
                }
                MultReal => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v1 * v2);
                    pc += 1;
                }
                DivReal => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v1 / v2);
                    pc += 1;
                }
                RemReal => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v1.fbc_remainder(v2));
                    pc += 1;
                }

                // ── Int arithmetic ──────────────────────────────────────
                AddInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v1.wrapping_add(v2));
                    pc += 1;
                }
                SubInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v1.wrapping_sub(v2));
                    pc += 1;
                }
                MultInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v1.wrapping_mul(v2));
                    pc += 1;
                }
                DivInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(if v2 != 0 { v1.wrapping_div(v2) } else { 0 });
                    pc += 1;
                }
                RemInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(if v2 != 0 { v1.wrapping_rem(v2) } else { 0 });
                    pc += 1;
                }

                // ── Int shifts ──────────────────────────────────────────
                LshInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v1.wrapping_shl(v2 as u32));
                    pc += 1;
                }
                ARshInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v1.wrapping_shr(v2 as u32));
                    pc += 1;
                }
                LRshInt => {
                    // Logical right shift: cast to unsigned, shift, cast back.
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((v1 as u32).wrapping_shr(v2 as u32) as i32);
                    pc += 1;
                }

                // ── Int comparisons ─────────────────────────────────────
                GTInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((v1 > v2) as i32);
                    pc += 1;
                }
                LTInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((v1 < v2) as i32);
                    pc += 1;
                }
                GEInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((v1 >= v2) as i32);
                    pc += 1;
                }
                LEInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((v1 <= v2) as i32);
                    pc += 1;
                }
                EQInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((v1 == v2) as i32);
                    pc += 1;
                }
                NEInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((v1 != v2) as i32);
                    pc += 1;
                }

                // ── Real comparisons → int ──────────────────────────────
                GTReal => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((v1 > v2) as i32);
                    pc += 1;
                }
                LTReal => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((v1 < v2) as i32);
                    pc += 1;
                }
                GEReal => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((v1 >= v2) as i32);
                    pc += 1;
                }
                LEReal => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((v1 <= v2) as i32);
                    pc += 1;
                }
                EQReal => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((v1 == v2) as i32);
                    pc += 1;
                }
                NEReal => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((v1 != v2) as i32);
                    pc += 1;
                }

                // ── Int logical ─────────────────────────────────────────
                ANDInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v1 & v2);
                    pc += 1;
                }
                ORInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v1 | v2);
                    pc += 1;
                }
                XORInt => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v1 ^ v2);
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Standard math: heap OP heap
                // ═══════════════════════════════════════════════════════
                AddRealHeap => {
                    real_stack.push(self.real_heap[o1] + self.real_heap[o2]);
                    pc += 1;
                }
                SubRealHeap => {
                    real_stack.push(self.real_heap[o1] - self.real_heap[o2]);
                    pc += 1;
                }
                MultRealHeap => {
                    real_stack.push(self.real_heap[o1] * self.real_heap[o2]);
                    pc += 1;
                }
                DivRealHeap => {
                    real_stack.push(self.real_heap[o1] / self.real_heap[o2]);
                    pc += 1;
                }
                RemRealHeap => {
                    real_stack.push(self.real_heap[o1].fbc_remainder(self.real_heap[o2]));
                    pc += 1;
                }

                AddIntHeap => {
                    int_stack.push(self.int_heap[o1].wrapping_add(self.int_heap[o2]));
                    pc += 1;
                }
                SubIntHeap => {
                    int_stack.push(self.int_heap[o1].wrapping_sub(self.int_heap[o2]));
                    pc += 1;
                }
                MultIntHeap => {
                    int_stack.push(self.int_heap[o1].wrapping_mul(self.int_heap[o2]));
                    pc += 1;
                }
                DivIntHeap => {
                    let d = self.int_heap[o2];
                    int_stack.push(if d != 0 {
                        self.int_heap[o1].wrapping_div(d)
                    } else {
                        0
                    });
                    pc += 1;
                }
                RemIntHeap => {
                    let d = self.int_heap[o2];
                    int_stack.push(if d != 0 {
                        self.int_heap[o1].wrapping_rem(d)
                    } else {
                        0
                    });
                    pc += 1;
                }

                LshIntHeap => {
                    int_stack.push(self.int_heap[o1].wrapping_shl(self.int_heap[o2] as u32));
                    pc += 1;
                }
                ARshIntHeap => {
                    int_stack.push(self.int_heap[o1].wrapping_shr(self.int_heap[o2] as u32));
                    pc += 1;
                }
                LRshIntHeap => {
                    int_stack.push(
                        (self.int_heap[o1] as u32).wrapping_shr(self.int_heap[o2] as u32) as i32,
                    );
                    pc += 1;
                }

                GTIntHeap => {
                    int_stack.push((self.int_heap[o1] > self.int_heap[o2]) as i32);
                    pc += 1;
                }
                LTIntHeap => {
                    int_stack.push((self.int_heap[o1] < self.int_heap[o2]) as i32);
                    pc += 1;
                }
                GEIntHeap => {
                    int_stack.push((self.int_heap[o1] >= self.int_heap[o2]) as i32);
                    pc += 1;
                }
                LEIntHeap => {
                    int_stack.push((self.int_heap[o1] <= self.int_heap[o2]) as i32);
                    pc += 1;
                }
                EQIntHeap => {
                    int_stack.push((self.int_heap[o1] == self.int_heap[o2]) as i32);
                    pc += 1;
                }
                NEIntHeap => {
                    int_stack.push((self.int_heap[o1] != self.int_heap[o2]) as i32);
                    pc += 1;
                }

                GTRealHeap => {
                    int_stack.push((self.real_heap[o1] > self.real_heap[o2]) as i32);
                    pc += 1;
                }
                LTRealHeap => {
                    int_stack.push((self.real_heap[o1] < self.real_heap[o2]) as i32);
                    pc += 1;
                }
                GERealHeap => {
                    int_stack.push((self.real_heap[o1] >= self.real_heap[o2]) as i32);
                    pc += 1;
                }
                LERealHeap => {
                    int_stack.push((self.real_heap[o1] <= self.real_heap[o2]) as i32);
                    pc += 1;
                }
                EQRealHeap => {
                    int_stack.push((self.real_heap[o1] == self.real_heap[o2]) as i32);
                    pc += 1;
                }
                NERealHeap => {
                    int_stack.push((self.real_heap[o1] != self.real_heap[o2]) as i32);
                    pc += 1;
                }

                ANDIntHeap => {
                    int_stack.push(self.int_heap[o1] & self.int_heap[o2]);
                    pc += 1;
                }
                ORIntHeap => {
                    int_stack.push(self.int_heap[o1] | self.int_heap[o2]);
                    pc += 1;
                }
                XORIntHeap => {
                    int_stack.push(self.int_heap[o1] ^ self.int_heap[o2]);
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Standard math: heap OP stack
                // ═══════════════════════════════════════════════════════
                AddRealStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(self.real_heap[o1] + v);
                    pc += 1;
                }
                SubRealStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(self.real_heap[o1] - v);
                    pc += 1;
                }
                MultRealStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(self.real_heap[o1] * v);
                    pc += 1;
                }
                DivRealStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(self.real_heap[o1] / v);
                    pc += 1;
                }
                RemRealStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(self.real_heap[o1].fbc_remainder(v));
                    pc += 1;
                }

                AddIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(self.int_heap[o1].wrapping_add(v));
                    pc += 1;
                }
                SubIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(self.int_heap[o1].wrapping_sub(v));
                    pc += 1;
                }
                MultIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(self.int_heap[o1].wrapping_mul(v));
                    pc += 1;
                }
                DivIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(if v != 0 {
                        self.int_heap[o1].wrapping_div(v)
                    } else {
                        0
                    });
                    pc += 1;
                }
                RemIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(if v != 0 {
                        self.int_heap[o1].wrapping_rem(v)
                    } else {
                        0
                    });
                    pc += 1;
                }

                LshIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(self.int_heap[o1].wrapping_shl(v as u32));
                    pc += 1;
                }
                ARshIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(self.int_heap[o1].wrapping_shr(v as u32));
                    pc += 1;
                }
                LRshIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((self.int_heap[o1] as u32).wrapping_shr(v as u32) as i32);
                    pc += 1;
                }

                GTIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((self.int_heap[o1] > v) as i32);
                    pc += 1;
                }
                LTIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((self.int_heap[o1] < v) as i32);
                    pc += 1;
                }
                GEIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((self.int_heap[o1] >= v) as i32);
                    pc += 1;
                }
                LEIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((self.int_heap[o1] <= v) as i32);
                    pc += 1;
                }
                EQIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((self.int_heap[o1] == v) as i32);
                    pc += 1;
                }
                NEIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((self.int_heap[o1] != v) as i32);
                    pc += 1;
                }

                GTRealStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((self.real_heap[o1] > v) as i32);
                    pc += 1;
                }
                LTRealStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((self.real_heap[o1] < v) as i32);
                    pc += 1;
                }
                GERealStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((self.real_heap[o1] >= v) as i32);
                    pc += 1;
                }
                LERealStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((self.real_heap[o1] <= v) as i32);
                    pc += 1;
                }
                EQRealStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((self.real_heap[o1] == v) as i32);
                    pc += 1;
                }
                NERealStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((self.real_heap[o1] != v) as i32);
                    pc += 1;
                }

                ANDIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(self.int_heap[o1] & v);
                    pc += 1;
                }
                ORIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(self.int_heap[o1] | v);
                    pc += 1;
                }
                XORIntStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(self.int_heap[o1] ^ v);
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Standard math: value OP stack
                // ═══════════════════════════════════════════════════════
                AddRealStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(rv + v);
                    pc += 1;
                }
                SubRealStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(rv - v);
                    pc += 1;
                }
                MultRealStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(rv * v);
                    pc += 1;
                }
                DivRealStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(rv / v);
                    pc += 1;
                }
                RemRealStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(rv.fbc_remainder(v));
                    pc += 1;
                }

                AddIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(iv.wrapping_add(v));
                    pc += 1;
                }
                SubIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(iv.wrapping_sub(v));
                    pc += 1;
                }
                MultIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(iv.wrapping_mul(v));
                    pc += 1;
                }
                DivIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(if v != 0 { iv.wrapping_div(v) } else { 0 });
                    pc += 1;
                }
                RemIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(if v != 0 { iv.wrapping_rem(v) } else { 0 });
                    pc += 1;
                }

                LshIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(iv.wrapping_shl(v as u32));
                    pc += 1;
                }
                ARshIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(iv.wrapping_shr(v as u32));
                    pc += 1;
                }
                LRshIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((iv as u32).wrapping_shr(v as u32) as i32);
                    pc += 1;
                }

                GTIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((iv > v) as i32);
                    pc += 1;
                }
                LTIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((iv < v) as i32);
                    pc += 1;
                }
                GEIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((iv >= v) as i32);
                    pc += 1;
                }
                LEIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((iv <= v) as i32);
                    pc += 1;
                }
                EQIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((iv == v) as i32);
                    pc += 1;
                }
                NEIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((iv != v) as i32);
                    pc += 1;
                }

                GTRealStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((rv > v) as i32);
                    pc += 1;
                }
                LTRealStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((rv < v) as i32);
                    pc += 1;
                }
                GERealStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((rv >= v) as i32);
                    pc += 1;
                }
                LERealStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((rv <= v) as i32);
                    pc += 1;
                }
                EQRealStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((rv == v) as i32);
                    pc += 1;
                }
                NERealStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push((rv != v) as i32);
                    pc += 1;
                }

                ANDIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(iv & v);
                    pc += 1;
                }
                ORIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(iv | v);
                    pc += 1;
                }
                XORIntStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(iv ^ v);
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Standard math: value OP heap
                // ═══════════════════════════════════════════════════════
                AddRealValue => {
                    real_stack.push(rv + self.real_heap[o1]);
                    pc += 1;
                }
                SubRealValue => {
                    real_stack.push(rv - self.real_heap[o1]);
                    pc += 1;
                }
                MultRealValue => {
                    real_stack.push(rv * self.real_heap[o1]);
                    pc += 1;
                }
                DivRealValue => {
                    real_stack.push(rv / self.real_heap[o1]);
                    pc += 1;
                }
                RemRealValue => {
                    real_stack.push(rv.fbc_remainder(self.real_heap[o1]));
                    pc += 1;
                }

                AddIntValue => {
                    int_stack.push(iv.wrapping_add(self.int_heap[o1]));
                    pc += 1;
                }
                SubIntValue => {
                    int_stack.push(iv.wrapping_sub(self.int_heap[o1]));
                    pc += 1;
                }
                MultIntValue => {
                    int_stack.push(iv.wrapping_mul(self.int_heap[o1]));
                    pc += 1;
                }
                DivIntValue => {
                    let d = self.int_heap[o1];
                    int_stack.push(if d != 0 { iv.wrapping_div(d) } else { 0 });
                    pc += 1;
                }
                RemIntValue => {
                    let d = self.int_heap[o1];
                    int_stack.push(if d != 0 { iv.wrapping_rem(d) } else { 0 });
                    pc += 1;
                }

                LshIntValue => {
                    int_stack.push(iv.wrapping_shl(self.int_heap[o1] as u32));
                    pc += 1;
                }
                ARshIntValue => {
                    int_stack.push(iv.wrapping_shr(self.int_heap[o1] as u32));
                    pc += 1;
                }
                LRshIntValue => {
                    int_stack.push((iv as u32).wrapping_shr(self.int_heap[o1] as u32) as i32);
                    pc += 1;
                }

                GTIntValue => {
                    int_stack.push((iv > self.int_heap[o1]) as i32);
                    pc += 1;
                }
                LTIntValue => {
                    int_stack.push((iv < self.int_heap[o1]) as i32);
                    pc += 1;
                }
                GEIntValue => {
                    int_stack.push((iv >= self.int_heap[o1]) as i32);
                    pc += 1;
                }
                LEIntValue => {
                    int_stack.push((iv <= self.int_heap[o1]) as i32);
                    pc += 1;
                }
                EQIntValue => {
                    int_stack.push((iv == self.int_heap[o1]) as i32);
                    pc += 1;
                }
                NEIntValue => {
                    int_stack.push((iv != self.int_heap[o1]) as i32);
                    pc += 1;
                }

                GTRealValue => {
                    int_stack.push((rv > self.real_heap[o1]) as i32);
                    pc += 1;
                }
                LTRealValue => {
                    int_stack.push((rv < self.real_heap[o1]) as i32);
                    pc += 1;
                }
                GERealValue => {
                    int_stack.push((rv >= self.real_heap[o1]) as i32);
                    pc += 1;
                }
                LERealValue => {
                    int_stack.push((rv <= self.real_heap[o1]) as i32);
                    pc += 1;
                }
                EQRealValue => {
                    int_stack.push((rv == self.real_heap[o1]) as i32);
                    pc += 1;
                }
                NERealValue => {
                    int_stack.push((rv != self.real_heap[o1]) as i32);
                    pc += 1;
                }

                ANDIntValue => {
                    int_stack.push(iv & self.int_heap[o1]);
                    pc += 1;
                }
                ORIntValue => {
                    int_stack.push(iv | self.int_heap[o1]);
                    pc += 1;
                }
                XORIntValue => {
                    int_stack.push(iv ^ self.int_heap[o1]);
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Standard math: value OP heap inverted (non-commutative)
                // ═══════════════════════════════════════════════════════
                SubRealValueInvert => {
                    real_stack.push(self.real_heap[o1] - rv);
                    pc += 1;
                }
                SubIntValueInvert => {
                    int_stack.push(self.int_heap[o1].wrapping_sub(iv));
                    pc += 1;
                }
                DivRealValueInvert => {
                    real_stack.push(self.real_heap[o1] / rv);
                    pc += 1;
                }
                DivIntValueInvert => {
                    int_stack.push(if iv != 0 {
                        self.int_heap[o1].wrapping_div(iv)
                    } else {
                        0
                    });
                    pc += 1;
                }
                RemRealValueInvert => {
                    real_stack.push(self.real_heap[o1].fbc_remainder(rv));
                    pc += 1;
                }
                RemIntValueInvert => {
                    int_stack.push(if iv != 0 {
                        self.int_heap[o1].wrapping_rem(iv)
                    } else {
                        0
                    });
                    pc += 1;
                }

                LshIntValueInvert => {
                    int_stack.push(self.int_heap[o1].wrapping_shl(iv as u32));
                    pc += 1;
                }
                ARshIntValueInvert => {
                    int_stack.push(self.int_heap[o1].wrapping_shr(iv as u32));
                    pc += 1;
                }
                LRshIntValueInvert => {
                    int_stack.push((self.int_heap[o1] as u32).wrapping_shr(iv as u32) as i32);
                    pc += 1;
                }

                GTIntValueInvert => {
                    int_stack.push((self.int_heap[o1] > iv) as i32);
                    pc += 1;
                }
                LTIntValueInvert => {
                    int_stack.push((self.int_heap[o1] < iv) as i32);
                    pc += 1;
                }
                GEIntValueInvert => {
                    int_stack.push((self.int_heap[o1] >= iv) as i32);
                    pc += 1;
                }
                LEIntValueInvert => {
                    int_stack.push((self.int_heap[o1] <= iv) as i32);
                    pc += 1;
                }

                GTRealValueInvert => {
                    int_stack.push((self.real_heap[o1] > rv) as i32);
                    pc += 1;
                }
                LTRealValueInvert => {
                    int_stack.push((self.real_heap[o1] < rv) as i32);
                    pc += 1;
                }
                GERealValueInvert => {
                    int_stack.push((self.real_heap[o1] >= rv) as i32);
                    pc += 1;
                }
                LERealValueInvert => {
                    int_stack.push((self.real_heap[o1] <= rv) as i32);
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Extended unary math (stack)
                // ═══════════════════════════════════════════════════════
                Abs => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v.wrapping_abs());
                    pc += 1;
                }
                Absf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_absf());
                    pc += 1;
                }
                Acosf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_acos());
                    pc += 1;
                }
                Acoshf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_acosh());
                    pc += 1;
                }
                Asinf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_asin());
                    pc += 1;
                }
                Asinhf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_asinh());
                    pc += 1;
                }
                Atanf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_atan());
                    pc += 1;
                }
                Atanhf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_atanh());
                    pc += 1;
                }
                Ceilf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_ceil());
                    pc += 1;
                }
                Cosf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_cos());
                    pc += 1;
                }
                Coshf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_cosh());
                    pc += 1;
                }
                Expf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_exp());
                    pc += 1;
                }
                Floorf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_floor());
                    pc += 1;
                }
                Logf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_log());
                    pc += 1;
                }
                Log10f => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_log10());
                    pc += 1;
                }
                Rintf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_rint());
                    pc += 1;
                }
                Roundf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_round());
                    pc += 1;
                }
                Sinf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_sin());
                    pc += 1;
                }
                Sinhf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_sinh());
                    pc += 1;
                }
                Sqrtf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_sqrt());
                    pc += 1;
                }
                Tanf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_tan());
                    pc += 1;
                }
                Tanhf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v.fbc_tanh());
                    pc += 1;
                }
                Isnanf => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v.fbc_is_nan() as i32);
                    pc += 1;
                }
                Isinff => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v.fbc_is_infinite() as i32);
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Extended unary math (heap)
                // ═══════════════════════════════════════════════════════
                AbsHeap => {
                    int_stack.push(self.int_heap[o1].wrapping_abs());
                    pc += 1;
                }
                AbsfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_absf());
                    pc += 1;
                }
                AcosfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_acos());
                    pc += 1;
                }
                AcoshfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_acosh());
                    pc += 1;
                }
                AsinfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_asin());
                    pc += 1;
                }
                AsinhfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_asinh());
                    pc += 1;
                }
                AtanfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_atan());
                    pc += 1;
                }
                AtanhfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_atanh());
                    pc += 1;
                }
                CeilfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_ceil());
                    pc += 1;
                }
                CosfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_cos());
                    pc += 1;
                }
                CoshfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_cosh());
                    pc += 1;
                }
                ExpfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_exp());
                    pc += 1;
                }
                FloorfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_floor());
                    pc += 1;
                }
                LogfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_log());
                    pc += 1;
                }
                Log10fHeap => {
                    real_stack.push(self.real_heap[o1].fbc_log10());
                    pc += 1;
                }
                RintfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_rint());
                    pc += 1;
                }
                RoundfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_round());
                    pc += 1;
                }
                SinfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_sin());
                    pc += 1;
                }
                SinhfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_sinh());
                    pc += 1;
                }
                SqrtfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_sqrt());
                    pc += 1;
                }
                TanfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_tan());
                    pc += 1;
                }
                TanhfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_tanh());
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Extended binary math (stack OP stack)
                // ═══════════════════════════════════════════════════════
                Atan2f => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v1.fbc_atan2(v2));
                    pc += 1;
                }
                Fmodf => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v1.fbc_fmod(v2));
                    pc += 1;
                }
                Powf => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v1.fbc_pow(v2));
                    pc += 1;
                }
                Max => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v1.max(v2));
                    pc += 1;
                }
                Maxf => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    // Match C++ std::max semantics: (a < b) ? b : a
                    real_stack.push(if v1 < v2 { v2 } else { v1 });
                    pc += 1;
                }
                Min => {
                    let v1 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(v1.min(v2));
                    pc += 1;
                }
                Minf => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    // Match C++ std::min semantics: (b < a) ? b : a
                    real_stack.push(if v2 < v1 { v2 } else { v1 });
                    pc += 1;
                }
                Copysignf => {
                    let v1 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let v2 = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(v1.fbc_copysign(v2));
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Extended binary math (heap OP heap)
                // ═══════════════════════════════════════════════════════
                Atan2fHeap => {
                    real_stack.push(self.real_heap[o1].fbc_atan2(self.real_heap[o2]));
                    pc += 1;
                }
                FmodfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_fmod(self.real_heap[o2]));
                    pc += 1;
                }
                PowfHeap => {
                    real_stack.push(self.real_heap[o1].fbc_pow(self.real_heap[o2]));
                    pc += 1;
                }
                MaxHeap => {
                    int_stack.push(self.int_heap[o1].max(self.int_heap[o2]));
                    pc += 1;
                }
                MaxfHeap => {
                    let (a, b) = (self.real_heap[o1], self.real_heap[o2]);
                    real_stack.push(if a < b { b } else { a });
                    pc += 1;
                }
                MinHeap => {
                    int_stack.push(self.int_heap[o1].min(self.int_heap[o2]));
                    pc += 1;
                }
                MinfHeap => {
                    let (a, b) = (self.real_heap[o1], self.real_heap[o2]);
                    real_stack.push(if b < a { b } else { a });
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Extended binary math (heap OP stack)
                // ═══════════════════════════════════════════════════════
                Atan2fStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(self.real_heap[o1].fbc_atan2(v));
                    pc += 1;
                }
                FmodfStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(self.real_heap[o1].fbc_fmod(v));
                    pc += 1;
                }
                PowfStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(self.real_heap[o1].fbc_pow(v));
                    pc += 1;
                }
                MaxStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(self.int_heap[o1].max(v));
                    pc += 1;
                }
                MaxfStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let a = self.real_heap[o1];
                    real_stack.push(if a < v { v } else { a });
                    pc += 1;
                }
                MinStack => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(self.int_heap[o1].min(v));
                    pc += 1;
                }
                MinfStack => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    let a = self.real_heap[o1];
                    real_stack.push(if v < a { v } else { a });
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Extended binary math (value OP stack)
                // ═══════════════════════════════════════════════════════
                Atan2fStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(rv.fbc_atan2(v));
                    pc += 1;
                }
                FmodfStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(rv.fbc_fmod(v));
                    pc += 1;
                }
                PowfStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(rv.fbc_pow(v));
                    pc += 1;
                }
                MaxStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(iv.max(v));
                    pc += 1;
                }
                MaxfStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(if rv < v { v } else { rv });
                    pc += 1;
                }
                MinStackValue => {
                    let v = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    int_stack.push(iv.min(v));
                    pc += 1;
                }
                MinfStackValue => {
                    let v = pop_real_stack(&mut real_stack, instr.opcode, cur_block, pc)?;
                    real_stack.push(if v < rv { v } else { rv });
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Extended binary math (value OP heap)
                // ═══════════════════════════════════════════════════════
                Atan2fValue => {
                    real_stack.push(rv.fbc_atan2(self.real_heap[o1]));
                    pc += 1;
                }
                FmodfValue => {
                    real_stack.push(rv.fbc_fmod(self.real_heap[o1]));
                    pc += 1;
                }
                PowfValue => {
                    real_stack.push(rv.fbc_pow(self.real_heap[o1]));
                    pc += 1;
                }
                MaxValue => {
                    int_stack.push(iv.max(self.int_heap[o1]));
                    pc += 1;
                }
                MaxfValue => {
                    let h = self.real_heap[o1];
                    real_stack.push(if rv < h { h } else { rv });
                    pc += 1;
                }
                MinValue => {
                    int_stack.push(iv.min(self.int_heap[o1]));
                    pc += 1;
                }
                MinfValue => {
                    let h = self.real_heap[o1];
                    real_stack.push(if h < rv { h } else { rv });
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Extended binary math (value OP heap inverted)
                // ═══════════════════════════════════════════════════════
                Atan2fValueInvert => {
                    real_stack.push(self.real_heap[o1].fbc_atan2(rv));
                    pc += 1;
                }
                FmodfValueInvert => {
                    real_stack.push(self.real_heap[o1].fbc_fmod(rv));
                    pc += 1;
                }
                PowfValueInvert => {
                    real_stack.push(self.real_heap[o1].fbc_pow(rv));
                    pc += 1;
                }

                // ═══════════════════════════════════════════════════════
                // Control flow
                // ═══════════════════════════════════════════════════════
                Return => {
                    if let Some((saved_block, saved_pc)) = addr_stack.pop() {
                        cur_block = saved_block;
                        pc = saved_pc;
                    } else {
                        // Empty address stack = end of execution.
                        return Ok(());
                    }
                }

                If => {
                    // Save return address (instruction after If).
                    addr_stack.push((cur_block, pc + 1));
                    let cond = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    if cond != 0 {
                        cur_block =
                            require_branch_target(instr.branch1, instr.opcode, cur_block, pc)?;
                    } else {
                        cur_block =
                            require_branch_target(instr.branch2, instr.opcode, cur_block, pc)?;
                    }
                    pc = 0;
                }

                SelectReal | SelectInt => {
                    // Save return address.
                    addr_stack.push((cur_block, pc + 1));
                    let cond = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    if cond != 0 {
                        cur_block =
                            require_branch_target(instr.branch1, instr.opcode, cur_block, pc)?;
                    } else {
                        cur_block =
                            require_branch_target(instr.branch2, instr.opcode, cur_block, pc)?;
                    }
                    pc = 0;
                }

                CondBranch => {
                    let cond = pop_int_stack(&mut int_stack, instr.opcode, cur_block, pc)?;
                    if cond != 0 {
                        // Loop back: jump to branch1 (loop body start).
                        cur_block =
                            require_branch_target(instr.branch1, instr.opcode, cur_block, pc)?;
                        pc = 0;
                    } else {
                        // Exit loop: advance to next instruction (typically Return).
                        pc += 1;
                    }
                }

                Loop => {
                    // Save return address (instruction after Loop).
                    addr_stack.push((cur_block, pc + 1));
                    // Push loop body (branch2) onto address stack.
                    let body = require_branch_target(instr.branch2, instr.opcode, cur_block, pc)?;
                    addr_stack.push((body, 0));
                    // Jump to init block (branch1).
                    cur_block = require_branch_target(instr.branch1, instr.opcode, cur_block, pc)?;
                    pc = 0;
                }

                // ═══════════════════════════════════════════════════════
                // UI instructions — never appear in execution blocks.
                // Handled separately by executeBuildUserInterface.
                // ═══════════════════════════════════════════════════════
                OpenVerticalBox
                | OpenHorizontalBox
                | OpenTabBox
                | CloseBox
                | AddButton
                | AddCheckButton
                | AddHorizontalSlider
                | AddVerticalSlider
                | AddNumEntry
                | AddSoundfile
                | AddHorizontalBargraph
                | AddVerticalBargraph
                | Declare => {
                    pc += 1;
                }

                Nop => {
                    pc += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::interp::bytecode::{FbcBlock, FbcInstruction};

    /// Helper: build a block from a list of instructions, appending Return.
    fn make_block(instrs: Vec<FbcInstruction<f32>>) -> FbcBlock<f32> {
        let mut block = FbcBlock::new();
        for i in instrs {
            block.push(i);
        }
        block.push(FbcInstruction::new(FbcOpcode::Return));
        block
    }

    #[test]
    fn push_and_store_real() {
        let mut arena = FbcBlockArena::<f32>::new();
        let block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::RealValue, 0, 3.125),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(0, 4);
        exec.execute_block(&arena, bid);
        assert!((exec.real_heap[0] - 3.125).abs() < 1e-6);
    }

    #[test]
    fn push_and_store_int() {
        let mut arena = FbcBlockArena::<f32>::new();
        let block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::Int32Value, 42, 0.0),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(4, 0);
        exec.execute_block(&arena, bid);
        assert_eq!(exec.int_heap[0], 42);
    }

    #[test]
    fn add_real() {
        let mut arena = FbcBlockArena::<f32>::new();
        let block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::RealValue, 0, 1.5),
            FbcInstruction::with_values(FbcOpcode::RealValue, 0, 2.5),
            FbcInstruction::new(FbcOpcode::AddReal),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(0, 4);
        exec.execute_block(&arena, bid);
        assert!((exec.real_heap[0] - 4.0).abs() < 1e-6);
    }

    #[test]
    fn sub_int() {
        let mut arena = FbcBlockArena::<f32>::new();
        let block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::Int32Value, 10, 0.0),
            FbcInstruction::with_values(FbcOpcode::Int32Value, 3, 0.0),
            FbcInstruction::new(FbcOpcode::SubInt),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(4, 0);
        exec.execute_block(&arena, bid);
        // v1=3 (top), v2=10 (second), result = v1 - v2 = 3 - 10 = -7
        assert_eq!(exec.int_heap[0], -7);
    }

    #[test]
    fn heap_load_store() {
        let mut arena = FbcBlockArena::<f32>::new();
        // Load real from heap[0], store to heap[1]
        let block = make_block(vec![
            FbcInstruction::with_values_and_offsets(FbcOpcode::LoadReal, 0, 0.0, 0, -1),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 1, -1),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(0, 4);
        exec.real_heap[0] = 99.0;
        exec.execute_block(&arena, bid);
        assert!((exec.real_heap[1] - 99.0).abs() < 1e-6);
    }

    #[test]
    fn heap_op_heap() {
        let mut arena = FbcBlockArena::<f32>::new();
        // AddRealHeap: push real_heap[0] + real_heap[1], store to heap[2]
        let block = make_block(vec![
            FbcInstruction::with_values_and_offsets(FbcOpcode::AddRealHeap, 0, 0.0, 0, 1),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 2, -1),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(0, 4);
        exec.real_heap[0] = 10.0;
        exec.real_heap[1] = 20.0;
        exec.execute_block(&arena, bid);
        assert!((exec.real_heap[2] - 30.0).abs() < 1e-6);
    }

    #[test]
    fn comparison_real() {
        let mut arena = FbcBlockArena::<f32>::new();
        // Push 5.0, push 3.0, GTReal → 3.0 > 5.0 = false (0)
        let block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::RealValue, 0, 5.0),
            FbcInstruction::with_values(FbcOpcode::RealValue, 0, 3.0),
            FbcInstruction::new(FbcOpcode::GTReal),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(4, 0);
        exec.execute_block(&arena, bid);
        // v1=3.0 (top), v2=5.0, v1 > v2 = 3.0 > 5.0 = false = 0
        assert_eq!(exec.int_heap[0], 0);
    }

    #[test]
    fn cast_real_to_int() {
        let mut arena = FbcBlockArena::<f32>::new();
        let block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::RealValue, 0, 3.7),
            FbcInstruction::new(FbcOpcode::CastInt),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(4, 0);
        exec.execute_block(&arena, bid);
        assert_eq!(exec.int_heap[0], 3); // truncation
    }

    #[test]
    fn cast_int_to_real() {
        let mut arena = FbcBlockArena::<f32>::new();
        let block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::Int32Value, 7, 0.0),
            FbcInstruction::new(FbcOpcode::CastReal),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(0, 4);
        exec.execute_block(&arena, bid);
        assert!((exec.real_heap[0] - 7.0).abs() < 1e-6);
    }

    #[test]
    fn extended_unary_sin() {
        let mut arena = FbcBlockArena::<f32>::new();
        let block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::RealValue, 0, std::f32::consts::FRAC_PI_2),
            FbcInstruction::new(FbcOpcode::Sinf),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(0, 4);
        exec.execute_block(&arena, bid);
        assert!((exec.real_heap[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn if_branch_true() {
        let mut arena = FbcBlockArena::<f32>::new();

        // then-block: store 1.0 to heap[0]
        let then_block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::RealValue, 0, 1.0),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
        ]);
        let then_id = arena.alloc(then_block);

        // else-block: store 2.0 to heap[0]
        let else_block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::RealValue, 0, 2.0),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
        ]);
        let else_id = arena.alloc(else_block);

        // main block: push 1 (true), If
        let main_block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::Int32Value, 1, 0.0),
            FbcInstruction::full(
                FbcOpcode::If,
                "",
                0,
                0.0,
                -1,
                -1,
                Some(then_id),
                Some(else_id),
            ),
        ]);
        let main_id = arena.alloc(main_block);

        let mut exec = FbcExecutor::new(0, 4);
        exec.execute_block(&arena, main_id);
        assert!((exec.real_heap[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn if_branch_false() {
        let mut arena = FbcBlockArena::<f32>::new();

        let then_block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::RealValue, 0, 1.0),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
        ]);
        let then_id = arena.alloc(then_block);

        let else_block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::RealValue, 0, 2.0),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
        ]);
        let else_id = arena.alloc(else_block);

        let main_block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0), // false
            FbcInstruction::full(
                FbcOpcode::If,
                "",
                0,
                0.0,
                -1,
                -1,
                Some(then_id),
                Some(else_id),
            ),
        ]);
        let main_id = arena.alloc(main_block);

        let mut exec = FbcExecutor::new(0, 4);
        exec.execute_block(&arena, main_id);
        assert!((exec.real_heap[0] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn simple_loop() {
        // Implement: for (i = 0; i < 5; i++) { heap[1] += 10; }
        let mut arena = FbcBlockArena::<f32>::new();

        // Init block: set int_heap[0] = 0  (loop counter)
        let init_block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
        ]);
        let init_id = arena.alloc(init_block);

        // Body block: heap[1] += 10; i++; CondBranch(i < 5)
        // We need to allocate first to get the ID for CondBranch's branch1.
        let body_placeholder = FbcBlock::new(); // placeholder
        let body_id = arena.alloc(body_placeholder);

        // Now build the real body
        let mut body = FbcBlock::new();
        // heap[1] += 10
        body.push(FbcInstruction::with_values_and_offsets(
            FbcOpcode::LoadInt,
            0,
            0.0,
            1,
            -1,
        ));
        body.push(FbcInstruction::with_values(FbcOpcode::Int32Value, 10, 0.0));
        body.push(FbcInstruction::new(FbcOpcode::AddInt));
        body.push(FbcInstruction::with_values_and_offsets(
            FbcOpcode::StoreInt,
            0,
            0.0,
            1,
            -1,
        ));
        // i++
        body.push(FbcInstruction::with_values_and_offsets(
            FbcOpcode::LoadInt,
            0,
            0.0,
            0,
            -1,
        ));
        body.push(FbcInstruction::with_values(FbcOpcode::Int32Value, 1, 0.0));
        body.push(FbcInstruction::new(FbcOpcode::AddInt));
        body.push(FbcInstruction::with_values_and_offsets(
            FbcOpcode::StoreInt,
            0,
            0.0,
            0,
            -1,
        ));
        // condition: i < 5
        // Stack convention: LTInt pops v1 (TOS), v2 (second), computes v1 < v2.
        // To get i < 5: push 5 first (v2), then i (v1 = TOS), so v1 < v2 = i < 5.
        body.push(FbcInstruction::with_values(FbcOpcode::Int32Value, 5, 0.0));
        body.push(FbcInstruction::with_values_and_offsets(
            FbcOpcode::LoadInt,
            0,
            0.0,
            0,
            -1,
        ));
        body.push(FbcInstruction::new(FbcOpcode::LTInt));
        // CondBranch: if true → loop back (branch1 = body_id)
        body.push(FbcInstruction::full(
            FbcOpcode::CondBranch,
            "",
            0,
            0.0,
            -1,
            -1,
            Some(body_id),
            None,
        ));
        body.push(FbcInstruction::new(FbcOpcode::Return));
        // Replace placeholder with real body
        *arena.get_mut(body_id) = body;

        // Main block: Loop with init and body
        let main_block = make_block(vec![FbcInstruction::full(
            FbcOpcode::Loop,
            "",
            0,
            0.0,
            -1,
            -1,
            Some(init_id),
            Some(body_id),
        )]);
        let main_id = arena.alloc(main_block);

        let mut exec = FbcExecutor::new(4, 0);
        exec.execute_block(&arena, main_id);
        // After 5 iterations: i=5, heap[1] = 50
        assert_eq!(exec.int_heap[0], 5);
        assert_eq!(exec.int_heap[1], 50);
    }

    #[test]
    fn move_real() {
        let mut arena = FbcBlockArena::<f32>::new();
        let block = make_block(vec![FbcInstruction::with_values_and_offsets(
            FbcOpcode::MoveReal,
            0,
            0.0,
            1,
            0,
        )]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(0, 4);
        exec.real_heap[0] = 42.0;
        exec.execute_block(&arena, bid);
        assert!((exec.real_heap[1] - 42.0).abs() < 1e-6);
    }

    #[test]
    fn store_real_value() {
        let mut arena = FbcBlockArena::<f32>::new();
        let block = make_block(vec![FbcInstruction::with_values_and_offsets(
            FbcOpcode::StoreRealValue,
            0,
            7.77,
            2,
            -1,
        )]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(0, 4);
        exec.execute_block(&arena, bid);
        assert!((exec.real_heap[2] - 7.77).abs() < 1e-5);
    }

    #[test]
    fn io_load_store() {
        let mut arena = FbcBlockArena::<f32>::new();
        // Load sample 0 from input channel 0, store to output channel 0 at index 0
        let block = make_block(vec![
            // Push sample index 0
            FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
            // LoadInput channel 0
            FbcInstruction::with_values_and_offsets(FbcOpcode::LoadInput, 0, 0.0, 0, -1),
            // Push sample index 0 for output
            FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
            // StoreOutput channel 0 (pops index then value)
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreOutput, 0, 0.0, 0, -1),
        ]);
        let bid = arena.alloc(block);

        let input_data = [1.0_f32, 2.0, 3.0];
        let inputs: &[&[f32]] = &[&input_data];
        let mut output_data = [0.0_f32; 3];
        let mut exec = FbcExecutor::new(0, 0);
        exec.execute_block_io(&arena, bid, inputs, &mut [&mut output_data]);
        assert!((output_data[0] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn load_input_oob_returns_structured_io_error() {
        let mut arena = FbcBlockArena::<f32>::new();
        let block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
            // Request input channel 1 while only channel 0 is provided.
            FbcInstruction::with_values_and_offsets(FbcOpcode::LoadInput, 0, 0.0, 1, -1),
        ]);
        let bid = arena.alloc(block);

        let input_data = [1.0_f32];
        let inputs: &[&[f32]] = &[&input_data];
        let mut exec = FbcExecutor::new(0, 0);
        let err = exec
            .try_execute_block_io(&arena, bid, inputs, &mut [])
            .expect_err("LoadInput with missing channel should return io_oob");

        assert_eq!(err.kind, "io_oob");
        assert_eq!(err.opcode, FbcOpcode::LoadInput);
        assert_eq!(err.channel, Some(1));
        assert_eq!(err.sample, Some(0));
    }

    #[test]
    fn store_output_stack_underflow_returns_structured_error() {
        let mut arena = FbcBlockArena::<f32>::new();
        // Push only an int sample index, then attempt StoreOutput. This leaves
        // no value on the real stack and should report a structured underflow.
        let block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreOutput, 0, 0.0, 0, -1),
        ]);
        let bid = arena.alloc(block);

        let mut output_data = [0.0_f32; 1];
        let mut exec = FbcExecutor::new(0, 0);
        let err = exec
            .try_execute_block_io(&arena, bid, &[], &mut [&mut output_data])
            .expect_err("StoreOutput with empty real stack should not panic");

        assert_eq!(err.kind, "stack_underflow");
        assert_eq!(err.opcode, FbcOpcode::StoreOutput);
        assert_eq!(err.block_id, bid);
        assert_eq!(err.pc, 1);
        assert_eq!(err.stack, Some(FbcStackKind::Real));
    }

    #[test]
    fn unchecked_heap_oob_is_trapped_as_structured_panic_error_in_try_mode() {
        let mut arena = FbcBlockArena::<f32>::new();
        // StoreReal into heap[4] while heap size is 1 -> indexing panic in the
        // current unchecked fast-style implementation path, which try-mode must
        // trap and report structurally.
        let block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::RealValue, 0, 1.0),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 4, -1),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(0, 1);
        let err = exec
            .try_execute_block(&arena, bid)
            .expect_err("heap OOB should be trapped as a structured runtime error");

        assert_eq!(err.kind, "panic_trapped");
        assert_eq!(err.opcode, FbcOpcode::StoreReal);
        assert_eq!(err.block_id, bid);
        assert_eq!(err.pc, 1);
    }

    #[test]
    fn div_int_by_zero() {
        let mut arena = FbcBlockArena::<f32>::new();
        let block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::Int32Value, 10, 0.0),
            FbcInstruction::with_values(FbcOpcode::Int32Value, 0, 0.0),
            FbcInstruction::new(FbcOpcode::DivInt),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(4, 0);
        exec.execute_block(&arena, bid);
        // Division by zero → 0
        assert_eq!(exec.int_heap[0], 0);
    }

    #[test]
    fn bitwise_and() {
        let mut arena = FbcBlockArena::<f32>::new();
        let block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::Int32Value, 0b1100, 0.0),
            FbcInstruction::with_values(FbcOpcode::Int32Value, 0b1010, 0.0),
            FbcInstruction::new(FbcOpcode::ANDInt),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(4, 0);
        exec.execute_block(&arena, bid);
        // v1=0b1010, v2=0b1100, result = 0b1010 & 0b1100 = 0b1000 = 8
        assert_eq!(exec.int_heap[0], 0b1000);
    }

    #[test]
    fn select_real() {
        let mut arena = FbcBlockArena::<f32>::new();

        // Branch1: push 100.0
        let b1 = make_block(vec![FbcInstruction::with_values(
            FbcOpcode::RealValue,
            0,
            100.0,
        )]);
        let b1_id = arena.alloc(b1);

        // Branch2: push 200.0
        let b2 = make_block(vec![FbcInstruction::with_values(
            FbcOpcode::RealValue,
            0,
            200.0,
        )]);
        let b2_id = arena.alloc(b2);

        // Main: push cond=1, SelectReal, StoreReal
        let main_block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::Int32Value, 1, 0.0),
            FbcInstruction::full(
                FbcOpcode::SelectReal,
                "",
                0,
                0.0,
                -1,
                -1,
                Some(b1_id),
                Some(b2_id),
            ),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreReal, 0, 0.0, 0, -1),
        ]);
        let main_id = arena.alloc(main_block);

        let mut exec = FbcExecutor::new(0, 4);
        exec.execute_block(&arena, main_id);
        assert!((exec.real_heap[0] - 100.0).abs() < 1e-6);
    }

    #[test]
    fn nop_and_return() {
        let mut arena = FbcBlockArena::<f32>::new();
        let block = make_block(vec![
            FbcInstruction::new(FbcOpcode::Nop),
            FbcInstruction::new(FbcOpcode::Nop),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(0, 0);
        exec.execute_block(&arena, bid);
        // Just verify it doesn't crash.
    }

    #[test]
    fn extended_binary_max_min() {
        let mut arena = FbcBlockArena::<f32>::new();
        let block = make_block(vec![
            FbcInstruction::with_values(FbcOpcode::Int32Value, 3, 0.0),
            FbcInstruction::with_values(FbcOpcode::Int32Value, 7, 0.0),
            FbcInstruction::new(FbcOpcode::Max),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 0, -1),
            FbcInstruction::with_values(FbcOpcode::Int32Value, 3, 0.0),
            FbcInstruction::with_values(FbcOpcode::Int32Value, 7, 0.0),
            FbcInstruction::new(FbcOpcode::Min),
            FbcInstruction::with_values_and_offsets(FbcOpcode::StoreInt, 0, 0.0, 1, -1),
        ]);
        let bid = arena.alloc(block);

        let mut exec = FbcExecutor::new(4, 0);
        exec.execute_block(&arena, bid);
        // v1=7 (top), v2=3, max(7,3)=7, min(7,3)=3
        assert_eq!(exec.int_heap[0], 7);
        assert_eq!(exec.int_heap[1], 3);
    }
}
