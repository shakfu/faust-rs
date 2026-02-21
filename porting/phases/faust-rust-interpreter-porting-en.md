# Interpreter Backend Porting Plan (C++ → Rust)

**Status**: design document for Phase 7 (Priority 1 backend)
**C++ source**: `compiler/generator/interpreter/`
**Rust target**: `crates/codegen/src/backends/interp/`
**Date**: February 2026

---

## 1. Scope

Port the Faust Byte Code (FBC) interpreter backend from C++ to Rust. This covers:

1. **FBC bytecode representation** — opcodes, instructions, blocks
2. **FIR → FBC compiler** — visitor translating FIR nodes to bytecode
3. **Interpreter engine** — dispatch loop with "computed goto" equivalent
4. **DSP factory/instance** — memory model, heaps, I/O, UI binding
5. **Bytecode optimizer** — peephole and algebraic passes
6. **Serialization** — `.fbc` file format read/write

The interpreter backend is activated in the C++ compiler with:

```bash
faust -lang interp foo.dsp
```

This produces a `.fbc` bytecode file that can be loaded and executed at
runtime by the interpreter engine. The Rust port must support the same
`-lang interp` flag in the CLI and produce/consume `.fbc` files that are
byte-for-byte compatible with the C++ compiler (see §8.1).

Out of scope (deferred): LLVM JIT backend (`fbc_llvm_compiler.hh`), MIR backend
(`fbc_mir_compiler.hh`), C++ codegen from bytecode (`fbc_cpp_compiler.hh`),
template-based compiler (`fbc_template_compiler.hh`), vectorized interpreter
(`fbc_vec_interpreter.hh`).

---

## 2. C++ Architecture Summary

### 2.1 File inventory

| C++ file | LOC | Role |
|----------|-----|------|
| `fbc_opcode.hh` | 515 | Opcode enum (~367 variants) with helpers |
| `interpreter_bytecode.hh` | 678 | `FBCBasicInstruction<REAL>`, `FBCBlockInstruction<REAL>` |
| `fbc_interpreter.hh` | 5011 | Main dispatch loop (switch + computed goto) |
| `interpreter_instructions.hh` | 709 | FIR → FBC visitor (`InterpreterInstVisitor`) |
| `interpreter_optimizer.hh` | 1471 | Bytecode optimization passes |
| `interpreter_dsp_aux.hh` | 993 | DSP instance management, memory layout |
| `interpreter_dsp_aux.cpp` | 496 | Runtime DSP factory |
| `interpreter_code_container.hh` | 153 | Code container interface |
| `interpreter_code_container.cpp` | 381 | Code generation orchestration |
| `fbc_executor.hh` | 92 | Abstract executor interface |
| `fbc_compiler.hh` | 92 | Abstract compiler interface |
| `interpreter_dsp.hh` | 230 | DSP interface |
| `fbc_vec_interpreter.hh` | 2571 | Vectorized SIMD interpreter (deferred) |
| `fbc_llvm_compiler.hh` | 1109 | LLVM JIT backend (deferred) |
| `fbc_mir_compiler.hh` | 1144 | MIR backend (deferred) |
| `fbc_cpp_compiler.hh` | 981 | C++ codegen from bytecode (deferred) |

**In-scope total**: ~9,023 LOC
**Full directory total**: ~15,625 LOC

### 2.2 Bytecode instruction model

```cpp
template <class REAL>
struct FBCBasicInstruction {
    Opcode      fOpcode;       // Instruction type
    std::string fName;         // Variable/field name (optional)
    int         fIntValue;     // Integer immediate
    REAL        fRealValue;    // Real immediate
    int         fOffset1;      // Heap offset 1
    int         fOffset2;      // Heap offset 2
    FBCBlockInstruction<REAL>* fBranch1;  // Branch 1 (if-true / loop-init)
    FBCBlockInstruction<REAL>* fBranch2;  // Branch 2 (if-false / loop-body)
};

template <class REAL>
struct FBCBlockInstruction {
    std::vector<FBCBasicInstruction<REAL>*> fInstructions;
};
```

### 2.3 Memory model

- **Int heap** (`fIntHeap`): `int[]` — counters, indices, loop variables
- **Real heap** (`fRealHeap`): `REAL[]` — state variables, filters, UI zones
- **Execution stacks** (local to `executeBlock`):
  - `real_stack[512]` — computation stack for REAL values
  - `int_stack[512]` — computation stack for integers
  - `address_stack[64]` — return addresses for control flow
- **Special offsets**: `fSROffset` (sample rate), `fCountOffset` (buffer size),
  `fIOTAOffset` (index counter)

### 2.4 Dispatch: "computed goto"

The C++ interpreter uses GCC's label-address extension (`&&label`, `goto*`):

```cpp
static void* fDispatchTable[] = {
    &&do_kRealValue, &&do_kInt32Value, &&do_kLoadReal, ...
};
#define dispatchFirstScal() { goto* fDispatchTable[(*it)->fOpcode]; }
#define dispatchNextScal()  { it++; dispatchFirstScal(); }
```

Each opcode handler is a labeled block that ends with `dispatchNextScal()`,
jumping directly to the next instruction's handler without returning to a
central loop. This gives 15-30% better performance than a switch-based loop.

---

## 3. Rust Representation Strategy

### 3.1 Design decision: flat bytecode, not TreeArena

**FBC bytecode should NOT use `TreeArena` hash-consing.** Unlike boxes, signals,
and FIR, FBC is:

- A **flat, linear instruction stream** designed for sequential execution
- **Never structurally compared** (no need for O(1) equality)
- **Never deduplicated** (each instruction in a block is positionally meaningful)
- **Never transformed functionally** (no FBC→FBC rewriting via hash-consing)
- **Performance-critical at runtime** — the dispatch loop is the hot path

TreeArena adds indirection (read `NodeKind` + tag dispatch) that would be
counterproductive in a tight interpreter loop. FBC needs:

- Contiguous memory layout for cache efficiency
- Direct field access (no arena lookup per instruction)
- Enum-based opcode dispatch (integer match, not tag string comparison)

The FBC representation uses **Rust enums + Vec** — the natural fit for a bytecode
instruction set.

### 3.2 Opcode representation

```rust
/// FBC opcode — complete instruction set.
///
/// # Source provenance (C++)
/// - `compiler/generator/interpreter/fbc_opcode.hh`
///
/// Uses `#[repr(u16)]` to guarantee dense integer discriminants
/// suitable for jump-table dispatch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum FbcOpcode {
    // === Numbers ===
    RealValue = 0,
    Int32Value,

    // === Memory ===
    LoadReal,
    LoadInt,
    LoadSoundFieldInt,
    LoadSoundFieldReal,
    StoreReal,
    StoreInt,
    StoreRealValue,
    StoreIntValue,
    LoadIndexedReal,
    LoadIndexedInt,
    StoreIndexedReal,
    StoreIndexedInt,
    BlockStoreReal,
    BlockStoreInt,
    MoveReal,
    MoveInt,
    PairMoveReal,
    PairMoveInt,
    BlockPairMoveReal,
    BlockPairMoveInt,
    BlockShiftReal,
    BlockShiftInt,
    LoadInput,
    StoreOutput,

    // === Cast/Bitcast ===
    CastReal,
    CastInt,
    CastRealHeap,
    CastIntHeap,
    BitcastInt,
    BitcastReal,

    // === Standard math (stack OP stack) ===
    AddReal, AddInt,
    SubReal, SubInt,
    MultReal, MultInt,
    DivReal, DivInt,
    RemReal, RemInt,
    LshInt, ARshInt, LRshInt,
    GTInt, LTInt, GEInt, LEInt, EQInt, NEInt,
    GTReal, LTReal, GEReal, LEReal, EQReal, NEReal,
    ANDInt, ORInt, XORInt,

    // === Standard math (heap OP heap) ===
    AddRealHeap, AddIntHeap,
    // ... (all ~29 heap OP heap variants)

    // === Standard math (heap OP stack) ===
    AddRealStack, AddIntStack,
    // ... (all ~29 heap OP stack variants)

    // === Standard math (value OP stack) ===
    AddRealStackValue, AddIntStackValue,
    // ... (all ~29 value OP stack variants)

    // === Standard math (value OP heap) ===
    AddRealValue, AddIntValue,
    // ... (all ~29 value OP heap variants)

    // === Standard math (value OP heap inverted, non-commutative) ===
    SubRealValueInvert, SubIntValueInvert,
    DivRealValueInvert, DivIntValueInvert,
    RemRealValueInvert, RemIntValueInvert,
    LshIntValueInvert, ARshIntValueInvert, LRshIntValueInvert,
    GTIntValueInvert, LTIntValueInvert,
    GEIntValueInvert, LEIntValueInvert,
    GTRealValueInvert, LTRealValueInvert,
    GERealValueInvert, LERealValueInvert,

    // === Extended unary math (stack) ===
    Absf, Abs,
    Acosf, Asinf, Atanf,
    Ceilf, Cosf, Coshf,
    Expf, Floorf,
    Logf, Log10f,
    Rintf, Roundf,
    Sinf, Sinhf, Sqrtf,
    Tanf, Tanhf,
    Isnanf, Isinff,
    Acoshf, Asinhf, Atanhf,
    Copysignf,

    // === Extended unary math (heap) ===
    AbsfHeap, AbsHeap,
    // ... (all heap variants)

    // === Extended binary math (stack) ===
    Atan2f, Fmodf, Powf,
    Max, Maxf, Min, Minf,

    // === Extended binary math (heap OP heap) ===
    Atan2fHeap, FmodfHeap, PowfHeap,
    MaxHeap, MaxfHeap, MinHeap, MinfHeap,

    // === Extended binary math (heap OP stack, value OP stack, value OP heap) ===
    // ... (all variants)

    // === Control flow ===
    Loop,
    Return,
    If,
    CondBranch,

    // === Select ===
    SelectReal,
    SelectInt,

    // === User Interface ===
    OpenVerticalBox,
    OpenHorizontalBox,
    OpenTabBox,
    CloseBox,
    AddButton,
    AddCheckButton,
    AddHorizontalSlider,
    AddVerticalSlider,
    AddNumEntry,
    AddSoundfile,
    AddHorizontalBargraph,
    AddVerticalBargraph,
    Declare,

    // === Misc ===
    Nop,
}
```

The full enum is generated mechanically from the C++ `fbc_opcode.hh` to ensure
exact 1:1 parity. The `#[repr(u16)]` attribute guarantees dense discriminants
that the compiler can optimize into a jump table.

### 3.3 Instruction representation

```rust
/// Index into a `FbcBlock`'s instruction Vec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InstrId(u32);

/// Index into the block arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockId(u32);

/// A single FBC instruction.
///
/// # Source provenance (C++)
/// - `compiler/generator/interpreter/interpreter_bytecode.hh`
///   (`FBCBasicInstruction<REAL>`)
///
/// Generic over `R` (the REAL type: f32 or f64).
#[derive(Clone, Debug)]
pub struct FbcInstruction<R: FbcReal> {
    pub opcode: FbcOpcode,
    pub name: Option<Box<str>>,     // Variable/field name (rare)
    pub int_value: i32,             // Integer immediate
    pub real_value: R,              // Real immediate
    pub offset1: i32,               // Heap offset 1
    pub offset2: i32,               // Heap offset 2
    pub branch1: Option<BlockId>,   // Branch 1 (if-true / loop-init)
    pub branch2: Option<BlockId>,   // Branch 2 (if-false / loop-body)
}

/// A block of FBC instructions (linear sequence ending with Return).
///
/// # Source provenance (C++)
/// - `FBCBlockInstruction<REAL>` in `interpreter_bytecode.hh`
#[derive(Clone, Debug)]
pub struct FbcBlock<R: FbcReal> {
    pub instructions: Vec<FbcInstruction<R>>,
}

/// Arena-like storage for all blocks in a DSP factory.
///
/// Blocks reference each other via `BlockId` indices, avoiding raw pointers.
#[derive(Clone, Debug)]
pub struct FbcBlockArena<R: FbcReal> {
    blocks: Vec<FbcBlock<R>>,
}

impl<R: FbcReal> FbcBlockArena<R> {
    pub fn new() -> Self {
        Self { blocks: Vec::new() }
    }

    pub fn alloc(&mut self, block: FbcBlock<R>) -> BlockId {
        let id = BlockId(self.blocks.len() as u32);
        self.blocks.push(block);
        id
    }

    pub fn get(&self, id: BlockId) -> &FbcBlock<R> {
        &self.blocks[id.0 as usize]
    }
}
```

### 3.4 The `FbcReal` trait

```rust
/// Trait bound for the interpreter's REAL type parameter.
///
/// Replaces the C++ `template <class REAL>` pattern.
pub trait FbcReal:
    Copy + Default + PartialOrd
    + std::ops::Add<Output = Self>
    + std::ops::Sub<Output = Self>
    + std::ops::Mul<Output = Self>
    + std::ops::Div<Output = Self>
    + std::ops::Rem<Output = Self>
    + Into<f64> + From<f64>
    + Send + Sync
    + 'static
{
    fn from_i32(v: i32) -> Self;
    fn to_i32(self) -> i32;
    fn sin(self) -> Self;
    fn cos(self) -> Self;
    fn tan(self) -> Self;
    fn exp(self) -> Self;
    fn log(self) -> Self;
    fn log10(self) -> Self;
    fn sqrt(self) -> Self;
    fn floor(self) -> Self;
    fn ceil(self) -> Self;
    fn round(self) -> Self;
    fn rint(self) -> Self;
    fn abs(self) -> Self;
    fn atan2(self, other: Self) -> Self;
    fn pow(self, exp: Self) -> Self;
    fn fmod(self, other: Self) -> Self;
    fn copysign(self, sign: Self) -> Self;
    fn is_nan(self) -> bool;
    fn is_infinite(self) -> bool;
    fn to_bits_i32(self) -> i32;
    fn from_bits_i32(v: i32) -> Self;
    fn acos(self) -> Self;
    fn asin(self) -> Self;
    fn atan(self) -> Self;
    fn cosh(self) -> Self;
    fn sinh(self) -> Self;
    fn tanh(self) -> Self;
    fn acosh(self) -> Self;
    fn asinh(self) -> Self;
    fn atanh(self) -> Self;
}

impl FbcReal for f32 { /* delegate to std */ }
impl FbcReal for f64 { /* delegate to std */ }
```

### 3.5 DSP factory and instance

```rust
/// Compiled FBC program ready for instantiation.
///
/// # Source provenance (C++)
/// - `interpreter_dsp_factory_aux<REAL, TRACE>` in `interpreter_dsp_aux.hh`
pub struct FbcDspFactory<R: FbcReal> {
    pub name: String,
    pub sha_key: String,

    // Memory layout
    pub int_heap_size: usize,
    pub real_heap_size: usize,
    pub sr_offset: usize,
    pub count_offset: usize,
    pub iota_offset: usize,
    pub num_inputs: usize,
    pub num_outputs: usize,

    // Compiled bytecode blocks
    pub blocks: FbcBlockArena<R>,
    pub static_init_block: BlockId,
    pub init_block: BlockId,
    pub reset_ui_block: BlockId,
    pub clear_block: BlockId,
    pub control_block: BlockId,
    pub compute_block: BlockId,

    // UI
    pub ui_block: Vec<FbcUiInstruction<R>>,
    pub meta_block: Vec<(String, String)>,
}

/// Runtime DSP instance with its own heaps.
///
/// # Source provenance (C++)
/// - `interpreter_dsp_aux<REAL, TRACE>` in `interpreter_dsp_aux.hh`
pub struct FbcDspInstance<R: FbcReal> {
    factory: Arc<FbcDspFactory<R>>,
    int_heap: Vec<i32>,
    real_heap: Vec<R>,
    inputs: Vec<*const R>,
    outputs: Vec<*mut R>,
    // soundfile table
    sound_table: HashMap<String, Soundfile<R>>,
}
```

---

## 4. Computed Goto in Rust

### 4.1 The problem

Rust has no `computed goto` language feature. The standard approach is
`match opcode { ... }`, which compiles to a jump table when the discriminant is
dense (as guaranteed by `#[repr(u16)]`). However, this still requires a single
branch back to the `match` statement between instructions.

### 4.2 Strategy: `match` with `#[repr(u16)]` + profile-guided optimization

The primary strategy is a tight `loop { match }`:

```rust
pub fn execute_block(&mut self, block_id: BlockId) {
    let block = self.factory.blocks.get(block_id);
    let instrs = &block.instructions;
    let mut pc: usize = 0;

    let mut real_stack = SmallVec::<[R; 512]>::new();
    let mut int_stack = SmallVec::<[i32; 512]>::new();
    let mut addr_stack = SmallVec::<[usize; 64]>::new();

    loop {
        // SAFETY: pc is bounds-checked by block construction
        // (every block ends with Return)
        let instr = unsafe { instrs.get_unchecked(pc) };

        match instr.opcode {
            FbcOpcode::RealValue => {
                real_stack.push(instr.real_value);
                pc += 1;
            }
            FbcOpcode::Int32Value => {
                int_stack.push(instr.int_value);
                pc += 1;
            }
            FbcOpcode::LoadReal => {
                real_stack.push(self.real_heap[instr.offset1 as usize]);
                pc += 1;
            }
            FbcOpcode::AddReal => {
                let v1 = real_stack.pop().unwrap();
                let v2 = real_stack.pop().unwrap();
                real_stack.push(v1 + v2);
                pc += 1;
            }
            // ... all ~367 opcodes ...
            FbcOpcode::Return => {
                if let Some(return_pc) = addr_stack.pop() {
                    pc = return_pc;
                } else {
                    return; // exit block
                }
            }
            FbcOpcode::Loop => {
                // Execute init block (branch1)
                if let Some(init_block) = instr.branch1 {
                    self.execute_block(init_block);
                }
                // Execute loop body (branch2)
                if let Some(body_block) = instr.branch2 {
                    self.execute_block(body_block);
                }
                pc += 1;
            }
            FbcOpcode::If => {
                let cond = int_stack.pop().unwrap();
                if cond != 0 {
                    if let Some(then_block) = instr.branch1 {
                        self.execute_block(then_block);
                    }
                } else {
                    if let Some(else_block) = instr.branch2 {
                        self.execute_block(else_block);
                    }
                }
                pc += 1;
            }
            FbcOpcode::CondBranch => {
                let cond = int_stack.pop().unwrap();
                if cond != 0 {
                    pc = 0; // Loop back to beginning of block
                } else {
                    return; // Exit loop
                }
            }
            _ => {
                pc += 1;
            }
        }
    }
}
```

### 4.3 Why this is competitive with computed goto

1. **Dense `#[repr(u16)]` enum** → LLVM generates a jump table (same as computed
   goto's dispatch table)

2. **`unsafe { get_unchecked(pc) }`** → eliminates bounds checking in the hot
   loop (safety guaranteed by block construction invariant: every block ends
   with `Return`)

3. **Single match point** → LLVM's jump threading pass can often eliminate the
   branch back to the `match`, effectively creating computed-goto-like dispatch

4. **Profile-guided optimization (PGO)** → with `cargo pgo`, LLVM optimizes
   branch prediction at the match point based on real workloads

5. **`#[inline(never)]` on `execute_block`** → prevents excessive inlining that
   would bloat the instruction cache

### 4.4 Alternative: function-pointer dispatch table

If benchmarks show the `match` approach is insufficient, an alternative is a
manually constructed function-pointer dispatch table:

```rust
type Handler<R> = fn(&mut FbcInterpreter<R>, &FbcInstruction<R>) -> DispatchResult;

static DISPATCH_TABLE_F32: [Handler<f32>; OPCODE_COUNT] = [
    handle_real_value,
    handle_int32_value,
    handle_load_real,
    // ... one entry per opcode
];

fn execute_block_fn_ptr(&mut self, block_id: BlockId) {
    let block = self.factory.blocks.get(block_id);
    let instrs = &block.instructions;
    let mut pc: usize = 0;
    loop {
        let instr = unsafe { instrs.get_unchecked(pc) };
        let handler = DISPATCH_TABLE[instr.opcode as usize];
        match handler(self, instr) {
            DispatchResult::Next => pc += 1,
            DispatchResult::Jump(target) => pc = target,
            DispatchResult::Return => return,
        }
    }
}
```

This is closer to computed goto semantics (indirect call through table) but
adds function call overhead. Benchmarking will determine which approach wins.

### 4.5 Alternative: flattened bytecode with threaded code

A more aggressive optimization flattens nested blocks into a single instruction
array with explicit jump targets (replacing `BlockId` branches with `pc`
offsets). This eliminates recursive `execute_block` calls:

```rust
/// Flattened instruction with resolved jump targets.
pub struct FlatInstruction<R: FbcReal> {
    pub opcode: FbcOpcode,
    pub int_value: i32,
    pub real_value: R,
    pub offset1: i32,
    pub offset2: i32,
    pub jump1: u32,  // PC offset for branch1
    pub jump2: u32,  // PC offset for branch2
}
```

This is the ultimate threaded-code representation and eliminates all recursion
in the interpreter. It can be built as an optimization pass on top of the
block-based representation.

### 4.6 Benchmark plan

All three approaches (match, fn-ptr, flattened) will be benchmarked on the
standard Faust test suite:

- Metric: samples/second for `compute(256, ...)` on representative DSPs
- Baseline: C++ computed-goto interpreter from the same test programs
- Tool: `criterion` benchmarks in `crates/codegen/benches/`

---

## 5. FIR → FBC Compilation

### 5.1 The visitor pattern in C++

The C++ `InterpreterInstVisitor<REAL>` inherits from `DispatchVisitor` and
visits all FIR instruction types, emitting FBC instructions into a
`FBCBlockInstruction`. Key methods:

- `visit(BinopInst*)` → emit appropriate `kAddReal`/`kMulInt`/etc. based on
  operand locations (stack, heap, immediate)
- `visit(StoreVarInst*)` → emit `kStoreReal`/`kStoreInt` with heap offset
- `visit(LoadVarInst*)` → emit `kLoadReal`/`kLoadInt` with heap offset
- `visit(LoopInst*)` → emit `kLoop` with init/body sub-blocks
- `visit(IfInst*)` → emit `kIf` with then/else sub-blocks

### 5.2 Rust design: `FirToFbc` compiler

```rust
/// Compiles FIR nodes into FBC bytecode.
///
/// # Source provenance (C++)
/// - `compiler/generator/interpreter/interpreter_instructions.hh`
///   (`InterpreterInstVisitor<REAL>`)
pub struct FirToFbc<'a, R: FbcReal> {
    arena: &'a TreeArena,
    blocks: &'a mut FbcBlockArena<R>,
    current_block: Vec<FbcInstruction<R>>,

    // Heap offset tracking
    int_heap_offset: usize,
    real_heap_offset: usize,

    // Variable → heap offset mapping
    int_var_table: HashMap<String, (usize, AccessType)>,
    real_var_table: HashMap<String, (usize, AccessType)>,

    // UI accumulator
    ui_instructions: Vec<FbcUiInstruction<R>>,
}

impl<'a, R: FbcReal> FirToFbc<'a, R> {
    /// Compile a FIR statement list into a bytecode block.
    pub fn compile_block(&mut self, fir_id: FirId) -> BlockId {
        // Walk FIR cons-list, compile each statement
        self.current_block.clear();
        self.compile_stmt(fir_id);
        self.current_block.push(FbcInstruction::new(FbcOpcode::Return));
        let block = FbcBlock { instructions: std::mem::take(&mut self.current_block) };
        self.blocks.alloc(block)
    }

    /// Compile a FIR value expression, leaving result on stack.
    fn compile_value(&mut self, fir_id: FirId) { ... }

    /// Compile a FIR statement.
    fn compile_stmt(&mut self, fir_id: FirId) { ... }
}
```

This uses `match_fir` exhaustive dispatch instead of C++ visitor/RTTI.

---

## 6. Trace / Debug Modes

### 6.1 C++ TRACE template parameter

In C++, `TRACE` is a compile-time `int` template parameter (0-6) that controls
bounds checking and NaN/Inf/overflow detection.

### 6.2 Rust design: runtime trace level

Instead of compile-time template specialization (which would require 7 copies
of the entire interpreter), use a runtime trace level with conditional checks:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceLevel {
    None = 0,
    Subnormal = 1,
    InfNan = 2,
    Full = 3,
    FailFast = 4,
    Continue = 5,
}

impl<R: FbcReal> FbcInterpreter<R> {
    #[inline(always)]
    fn check_real(&self, value: R) -> R {
        if self.trace_level >= TraceLevel::InfNan {
            if value.is_nan() { self.report_error(FbcError::Nan); }
            if value.is_infinite() { self.report_error(FbcError::Infinite); }
        }
        value
    }

    #[inline(always)]
    fn assert_heap_real(&self, index: usize) {
        if self.trace_level >= TraceLevel::Full {
            assert!(index < self.real_heap.len(),
                "heap overflow: index={} size={}", index, self.real_heap.len());
        }
    }
}
```

The `#[inline(always)]` + branch on constant-like field allows LLVM to
effectively eliminate checks when trace is disabled (the branch predictor
learns quickly that trace_level is always 0).

If benchmarks show measurable overhead, a feature-flag alternative
(`#[cfg(feature = "fbc-trace")]`) can compile out all checks entirely.

---

## 7. Bytecode Optimizer

### 7.1 C++ optimizer passes

`FBCInstructionOptimizer<REAL>` in `interpreter_optimizer.hh` implements:

1. **Heap operation fusion**: `LoadReal(off) + BinOp` → fused `BinOpHeap(off)`
2. **Value fusion**: `RealValue(v) + BinOp` → fused `BinOpValue(v)`
3. **Identity elimination**: `x + 0 → x`, `x * 1 → x`, `x * 0 → 0`
4. **Dead store elimination**: remove stores to unused heap locations
5. **Constant folding**: `2.0 + 3.0 → 5.0`

### 7.2 Rust design

```rust
/// Bytecode optimization pipeline.
///
/// # Source provenance (C++)
/// - `compiler/generator/interpreter/interpreter_optimizer.hh`
pub struct FbcOptimizer<R: FbcReal> {
    opt_level: u8,  // 0-4
}

impl<R: FbcReal> FbcOptimizer<R> {
    /// Optimize a block in place.
    pub fn optimize(&self, block: &mut FbcBlock<R>) {
        if self.opt_level >= 1 { self.fuse_heap_ops(block); }
        if self.opt_level >= 2 { self.fuse_value_ops(block); }
        if self.opt_level >= 3 { self.fold_constants(block); }
        if self.opt_level >= 4 { self.eliminate_dead_stores(block); }
    }

    fn fuse_heap_ops(&self, block: &mut FbcBlock<R>) { ... }
    fn fuse_value_ops(&self, block: &mut FbcBlock<R>) { ... }
    fn fold_constants(&self, block: &mut FbcBlock<R>) { ... }
    fn eliminate_dead_stores(&self, block: &mut FbcBlock<R>) { ... }
}
```

Optimization works on `FbcBlock` directly (mutable in-place transformation on
Vec), which is appropriate since bytecode blocks are built once and optimized
before execution.

---

## 8. Serialization (`.fbc` format)

### 8.1 Cross-compiler compatibility invariant

**The `.fbc` file format produced by the Rust interpreter must be byte-for-byte
compatible with the C++ compiler's reader, and vice versa.** This is a hard
requirement because:

- `.fbc` files produced by the C++ `faust` compiler (`-lang interp`) must be
  loadable by the Rust interpreter runtime.
- `.fbc` files produced by the Rust compiler must be loadable by the C++
  `libfaust` interpreter runtime (e.g. in existing applications using
  `createInterpreterDSPFactoryFromFile`).
- Existing `.fbc` files in production or in test suites must remain valid.

This means the Rust serializer/deserializer must reproduce the **exact text
format** defined in the C++ source
(`interpreter_dsp_aux.hh:write()`/`read()` and
`interpreter_bytecode.hh:write()`), including:

- Header tokens and field ordering
- Opcode names as printed by `gFBCInstructionTable[]` (string names, not
  numeric codes)
- Floating-point precision (`std::numeric_limits<REAL>::digits10 + 1`, i.e.
  9 digits for float, 17 for double)
- Quoted string encoding for UI labels and metadata
- Block size prefix for each section
- The `INTERP_FILE_VERSION` constant (currently `8`)

### 8.2 Format details

The `.fbc` file format is a text-based bytecode serialization (version 8).
Two modes exist in C++: normal and small (compact). The Rust port must
support both.

**Normal mode** (used by `-lang interp`):

```
interpreter_dsp_factory float|double
file_version 8
Faust version X.Y.Z
compile_options <options>
name <dsp_name>
sha_key <hash>
opt_level <0-4>
inputs N outputs M
int_heap_size X real_heap_size Y sr_offset Z count_offset W iota_offset V
meta_block
  block_size <N>
  <N meta instructions: key "k" value "v">
user_interface_block
  block_size <N>
  <N UI instructions>
static_init_block
  block_size <N>
  <N bytecode instructions>
constants_block
  block_size <N>
  <N bytecode instructions>
reset_ui
  block_size <N>
  <N bytecode instructions>
clear_block
  block_size <N>
  <N bytecode instructions>
control_block
  block_size <N>
  <N bytecode instructions>
dsp_block
  block_size <N>
  <N bytecode instructions>
```

**Bytecode instruction text format** (from `FBCBasicInstruction::write`):

```
int <opcode_name> <int_value> <real_value> <offset1> <offset2>
```

Where `<opcode_name>` is the string from `gFBCInstructionTable` (e.g.
`"kAddReal"`, `"kLoadInt"`, `"kStoreOutput"`).

For instructions with branches (Loop, If):
```
int <opcode_name> <int_value> <real_value> <offset1> <offset2>
  <branch1 block>
  <branch2 block>
```

### 8.3 Validation strategy

- **Differential tests**: compile the same `.dsp` with C++ `faust -lang interp`
  and with Rust, diff the `.fbc` output byte-for-byte.
- **Cross-load tests**: load a C++-produced `.fbc` into the Rust interpreter,
  load a Rust-produced `.fbc` into the C++ interpreter, compare `compute()`
  output sample-by-sample.
- **Round-trip tests**: write → read → write, verify second write is identical
  to first.
- **Version check**: the Rust reader must reject files with
  `file_version != INTERP_FILE_VERSION` with a clear error message (matching
  the C++ behavior in `interpreter_dsp.hh`).

### 8.4 Opcode name table parity

The Rust code must maintain a `&str` table indexed by `FbcOpcode` that matches
the C++ `gFBCInstructionTable[]` exactly. This table is used only for
serialization (not for dispatch). A compile-time or unit-test assertion must
verify that the Rust table has the same length and contents as the C++ table.

```rust
/// Opcode string names for `.fbc` serialization.
/// Must match C++ `gFBCInstructionTable` in `fbc_opcode.hh` exactly.
const FBC_INSTRUCTION_NAMES: &[&str] = &[
    "kRealValue",
    "kInt32Value",
    "kLoadReal",
    "kLoadInt",
    // ... all 367+ entries in C++ order
    "kNop",
];

#[cfg(test)]
mod tests {
    #[test]
    fn opcode_name_table_parity() {
        assert_eq!(FBC_INSTRUCTION_NAMES.len(), FbcOpcode::COUNT);
        // Verify each name matches the C++ table
        assert_eq!(FBC_INSTRUCTION_NAMES[FbcOpcode::RealValue as usize], "kRealValue");
        assert_eq!(FBC_INSTRUCTION_NAMES[FbcOpcode::Nop as usize], "kNop");
    }
}
```

### 8.5 Rust implementation

```rust
/// Serialize FbcDspFactory to `.fbc` format (C++-compatible).
pub fn write_fbc<R: FbcReal>(
    factory: &FbcDspFactory<R>,
    writer: &mut dyn Write,
    small: bool,
) -> io::Result<()> { ... }

/// Deserialize FbcDspFactory from `.fbc` format (C++-compatible).
pub fn read_fbc<R: FbcReal>(
    reader: &mut dyn BufRead,
) -> Result<FbcDspFactory<R>, InterpError> { ... }
```

---

## 9. Module Structure

```
crates/codegen/src/backends/interp/
├── mod.rs              // Public API, backend_id()
├── opcode.rs           // FbcOpcode enum (generated/maintained from C++)
├── instruction.rs      // FbcInstruction, FbcBlock, FbcBlockArena
├── real_trait.rs        // FbcReal trait + f32/f64 impls
├── interpreter.rs      // FbcInterpreter — dispatch loop
├── compiler.rs         // FirToFbc — FIR → bytecode compilation
├── optimizer.rs        // FbcOptimizer — bytecode optimization
├── factory.rs          // FbcDspFactory — compiled program
├── instance.rs         // FbcDspInstance — runtime state
├── ui.rs               // FbcUiInstruction, UI block handling
├── serial.rs           // .fbc read/write
└── trace.rs            // TraceLevel, error reporting
```

---

## 10. Execution Plan

Each step must pass the mandatory quality gate before commit (AGENTS.md §3):
- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`

If a gate cannot be passed, the reason and follow-up must be recorded in
`JOURNAL.md` (AGENTS.md §5).

### Step 1 — Opcode and instruction types (foundation)

- Port `fbc_opcode.hh` → `opcode.rs`: complete `FbcOpcode` enum with `#[repr(u16)]`
- Port `interpreter_bytecode.hh` → `instruction.rs`: `FbcInstruction`, `FbcBlock`, `FbcBlockArena`
- Implement `FbcReal` trait in `real_trait.rs`
- Unit tests: opcode count parity, instruction construction, block operations
- Rustdoc: `///` on every `pub` item with C++ source provenance and parity notes

**Deliverable**: compile-ready FBC data model with exact opcode parity.

**Pass criteria**:
- `FbcOpcode` variant count == C++ `gFBCInstructionTable` length (assertion test)
- `FBC_INSTRUCTION_NAMES` table matches C++ (assertion test)
- `FbcInstruction` round-trip: construct → read fields → assert equality
- `FbcBlockArena`: alloc → get → verify block contents
- `FbcReal` impls for f32/f64: all math operations match `std` results
- Quality gate green

### Step 2 — Interpreter dispatch loop (core engine)

- Port `fbc_interpreter.hh` → `interpreter.rs`
- Implement `execute_block` with `match`-based dispatch
- Implement all ~367 opcode handlers
- Implement `FbcDspInstance` with int/real heaps, I/O buffers
- Port trace/debug infrastructure to `trace.rs`
- Unit tests: hand-crafted bytecode blocks, stack operations, control flow
- Rustdoc: document dispatch strategy rationale and safety invariants for
  `unsafe` blocks (AGENTS.md §3: avoid `unsafe` unless strictly required and
  documented)

**Deliverable**: working interpreter that can execute hand-built FBC blocks.

**Pass criteria**:
- Arithmetic: `push(3.0) + push(4.0) + AddReal → pop == 7.0` (f32 and f64)
- All binary ops: verify result for each opcode on representative inputs
- Control flow: If/Loop/CondBranch/Return sequences execute correctly
- Heap: LoadReal/StoreReal round-trip through heap
- I/O: LoadInput/StoreOutput read/write audio buffers
- Trace level None: no overhead measurable (benchmark)
- Trace level FailFast: NaN triggers error report
- Quality gate green

### Step 3 — FIR → FBC compiler

- Port `interpreter_instructions.hh` → `compiler.rs`
- Implement `FirToFbc` using `match_fir` dispatch
- Port heap offset allocation and variable tracking
- Port UI instruction compilation to `ui.rs`
- Rustdoc: document each `compile_*` method with source provenance and
  the FIR node types it handles
- Integration tests: compile FIR programs → execute → verify output

**Deliverable**: end-to-end FIR → bytecode → execution.

**Pass criteria**:
- Compile a FIR `Int32(42)` → execute → verify stack contains 42
- Compile a FIR `BinOp(Add, Int32(3), Int32(4))` → execute → verify result == 7
- Compile a FIR `ForLoop` → execute → verify loop iteration count
- Compile a FIR `StoreVar/LoadVar` round-trip → verify heap state
- Quality gate green

### Step 4 — Bytecode optimizer

- Port `interpreter_optimizer.hh` → `optimizer.rs`
- Implement heap/value fusion, identity elimination, constant folding
- Benchmark optimized vs unoptimized execution
- Unit tests: verify each optimization preserves semantics
- Rustdoc: document each pass with before/after instruction examples

**Deliverable**: optimization pipeline with measurable speedup.

**Pass criteria**:
- Heap fusion: `LoadReal(off) + AddReal` → `AddRealHeap(off)` (assertion test)
- Value fusion: `RealValue(v) + MultReal` → `MultRealStackValue(v)` (assertion test)
- Identity: `x + 0 → x` (block instruction count reduced)
- Constant fold: `RealValue(2.0) + RealValue(3.0) → RealValue(5.0)`
- Semantic preservation: optimized block produces same `compute()` output as
  unoptimized on 10+ test programs
- Quality gate green

### Step 5 — Factory, serialization, DSP interface

- Port `interpreter_dsp_aux.hh/.cpp` → `factory.rs`, `instance.rs`
- Implement `.fbc` serialization → `serial.rs`
- Implement `compute()` entry point (sample loop)
- Port UI building and metadata handling
- Integration tests: round-trip `.fbc` files, DSP `compute` output parity
- Rustdoc: document `.fbc` format and C++ compatibility contract

**Deliverable**: full interpreter backend usable as a Faust target.

**Pass criteria**:
- `.fbc` round-trip: write → read → write produces identical output
- `.fbc` cross-compatibility: C++-produced `.fbc` loads in Rust reader without error
- `.fbc` cross-compatibility: Rust-produced `.fbc` loads in C++ reader without error
- `compute()` parity: same `.dsp` compiled by C++ and Rust produces
  sample-identical output (tolerance: 0 ULP for same REAL type)
- Version mismatch: Rust reader rejects `file_version != 8` with clear error
- Quality gate green

### Step 6 — Benchmarks and dispatch tuning

- Implement `criterion` benchmarks comparing:
  - Rust match dispatch vs C++ computed goto
  - Rust fn-ptr dispatch table variant
  - Rust flattened threaded-code variant
- Profile with real Faust DSPs from the test suite
- Select optimal dispatch strategy based on results
- Apply PGO if beneficial

**Deliverable**: performance-validated interpreter with data-driven dispatch choice.

**Pass criteria**:
- Benchmark results documented in `JOURNAL.md`
- Dispatch strategy choice justified by data
- No performance regression vs previous step on `compute()` throughput

---

## 11. Testing Strategy

### 11.1 Test matrix

| Level | What | How |
|-------|------|-----|
| Unit | Opcode enum parity | Assert variant count == C++ count |
| Unit | Opcode name table parity | Assert each `FBC_INSTRUCTION_NAMES[i]` matches C++ `gFBCInstructionTable[i]` |
| Unit | Stack operations | Hand-built bytecode: push/pop/arithmetic on f32 and f64 |
| Unit | Control flow | If/loop/return/cond-branch bytecode sequences |
| Unit | Heap operations | Load/store/indexed/block-shift/move patterns |
| Unit | Cast/bitcast | CastReal/CastInt/BitcastInt/BitcastReal correctness |
| Unit | Extended math | Each unary/binary math opcode vs `std` reference |
| Unit | Optimizer passes | Before/after instruction sequences for each pass |
| Unit | `.fbc` round-trip | write → read → write identity |
| Unit | `.fbc` version check | Reject wrong version with `InterpError` |
| Integration | FIR → FBC → execute | Compile FIR programs, verify output values |
| Integration | `.fbc` cross-load | C++ `.fbc` in Rust, Rust `.fbc` in C++ |
| Differential | C++ vs Rust output | Same `.dsp` → `compute()` sample-by-sample comparison |
| Differential | `.fbc` file diff | Same `.dsp` → `.fbc` byte-for-byte comparison |
| Performance | Benchmark suite | `criterion` on representative DSPs vs C++ baseline |

### 11.2 Negative tests

- Stack underflow on empty stack → panic or error (not undefined behavior)
- Heap out-of-bounds at trace level >= Full → `FbcError::HeapOverflow`
- Division by zero (int) at trace level >= Full → `FbcError::DivByZero`
- NaN/Inf detection at trace level >= InfNan → `FbcError::Nan`/`FbcError::Infinite`
- Invalid opcode in `.fbc` file → `InterpError::UnknownOpcode`
- Malformed `.fbc` header → `InterpError::ParseError`

### 11.3 Differential test procedure (AGENTS.md §5)

For critical compiler behavior, prefer differential tests against C++ reference
outputs:

1. Compile `.dsp` with C++ `faust -lang interp -o test.fbc`
2. Compile same `.dsp` with Rust backend → `test_rust.fbc`
3. Diff `.fbc` files byte-for-byte
4. Load both into respective interpreters
5. Run `compute(256, inputs, outputs)` with identical input buffers
6. Compare output samples (exact match for same REAL type)

---

## 12. Rustdoc Requirements (AGENTS.md §5)

All `pub` items must include Rustdoc comments (`///`) with:

1. **C++ source provenance**: file path(s) and function/class name(s)
2. **Parity invariants**: semantic constraints preserved from C++
3. **Adaptation rationale**: if the Rust API differs from C++ signature

Module-level documentation (`//!`) must include:

1. **Source provenance block**: which C++ files this module maps to
2. **Public API mapping status**: `1:1`, `adapted`, or `deferred` for each
   exported item
3. **Parity invariants**: structural and behavioral guarantees

Example:

```rust
//! FBC opcode definitions.
//!
//! # Source provenance (C++)
//! - `compiler/generator/interpreter/fbc_opcode.hh`
//!   (`FBCInstruction::Opcode` enum, `gFBCInstructionTable[]`)
//!
//! # Public API mapping status
//! - `FbcOpcode` enum: **1:1** — same variant set and ordering as C++
//!   `FBCInstruction::Opcode`
//! - `FBC_INSTRUCTION_NAMES`: **1:1** — same string table as C++
//!   `gFBCInstructionTable`
//! - `FbcOpcode::name()`: **adapted** — method on enum instead of free
//!   array lookup
//!
//! # Parity invariants
//! - Variant count must equal C++ opcode count (assertion test).
//! - Discriminant ordering must match C++ enum ordering for `.fbc`
//!   serialization compatibility.

/// FBC opcode — complete instruction set.
///
/// # Source provenance (C++)
/// - `compiler/generator/interpreter/fbc_opcode.hh`
///   (`FBCInstruction::Opcode`)
///
/// # Parity invariants
/// - `#[repr(u16)]` discriminants match C++ enum values.
/// - Variant count validated by `opcode_count_parity` test.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum FbcOpcode { ... }
```

---

## 13. Code Formatting and Linting Contract

All code in `crates/codegen/src/backends/interp/` must comply with
(AGENTS.md §3):

- `cargo fmt --all` — standard Rust formatting, no local overrides
- `cargo clippy --workspace --all-targets -- -D warnings` — zero warnings
- No `#[allow(clippy::...)]` without documented justification
- `unsafe` blocks require:
  - A `// SAFETY:` comment explaining the invariant that makes the operation
    sound
  - A corresponding debug-mode assertion or bounds check that validates the
    invariant at runtime when `cfg(debug_assertions)` is true
  - Documentation in Rustdoc on the enclosing function

Example:

```rust
/// Execute a bytecode block.
///
/// # Safety invariants
/// - Every `FbcBlock` must end with a `Return` instruction, guaranteeing
///   that `pc` never exceeds `instrs.len() - 1`.
/// - This invariant is enforced by `FbcBlock::new()` which panics if the
///   last instruction is not `Return`.
pub fn execute_block(&mut self, block_id: BlockId) {
    // ...
    loop {
        // SAFETY: pc is bounded by the Return-termination invariant.
        // Debug builds verify this with an explicit bounds check.
        debug_assert!(pc < instrs.len());
        let instr = unsafe { instrs.get_unchecked(pc) };
        // ...
    }
}
```

---

## 14. Public API Mapping Table

Each public item must be tracked with its C++ counterpart and mapping status.
This table is updated as implementation progresses.

| Rust item | C++ counterpart | Status | Notes |
|-----------|----------------|--------|-------|
| `FbcOpcode` | `FBCInstruction::Opcode` | 1:1 | Same variants, same order |
| `FBC_INSTRUCTION_NAMES` | `gFBCInstructionTable[]` | 1:1 | Same strings |
| `FbcInstruction<R>` | `FBCBasicInstruction<REAL>` | adapted | `Option<BlockId>` instead of raw pointer |
| `FbcBlock<R>` | `FBCBlockInstruction<REAL>` | adapted | `Vec<FbcInstruction>` instead of `Vec<FBCBasicInstruction*>` |
| `FbcBlockArena<R>` | N/A (raw pointers in C++) | adapted | New safe ownership model |
| `FbcReal` trait | `template <class REAL>` | adapted | Trait instead of template parameter |
| `FbcInterpreter<R>` | `FBCInterpreter<REAL, TRACE>` | adapted | Runtime trace level instead of template |
| `FirToFbc<R>` | `InterpreterInstVisitor<REAL>` | adapted | `match_fir` instead of visitor pattern |
| `FbcOptimizer<R>` | `FBCInstructionOptimizer<REAL>` | 1:1 | Same passes, same semantics |
| `FbcDspFactory<R>` | `interpreter_dsp_factory_aux<REAL, TRACE>` | adapted | `Arc` sharing, no TRACE template |
| `FbcDspInstance<R>` | `interpreter_dsp_aux<REAL, TRACE>` | adapted | `Vec` heaps instead of raw arrays |
| `write_fbc` / `read_fbc` | `write()` / `read()` | 1:1 | Byte-for-byte `.fbc` format compatibility |
| `TraceLevel` | `TRACE` template parameter (0-6) | adapted | Runtime enum instead of compile-time int |

---

## 15. Key Differences from C++

| Aspect | C++ | Rust |
|--------|-----|------|
| REAL type | Template parameter | `FbcReal` trait with f32/f64 impls |
| TRACE level | Compile-time int template | Runtime enum (or feature flag) |
| Dispatch | `goto*` computed goto (GCC extension) | `match` on `#[repr(u16)]` enum (LLVM jump table) |
| Block ownership | Raw pointers (`FBCBlockInstruction*`) | `BlockId` indices into `FbcBlockArena` |
| Instruction storage | Heap-allocated `vector<FBCBasicInstruction*>` | Contiguous `Vec<FbcInstruction>` (no pointer indirection) |
| Heap memory | Raw `new int[]` / `new REAL[]` | `Vec<i32>` / `Vec<R>` (bounds-checked in debug) |
| FIR dispatch | Visitor pattern + virtual dispatch | `match_fir` exhaustive enum match |
| Not TreeArena | N/A | Deliberate: FBC is flat bytecode, not a functional tree |

---

## 16. Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| `match` dispatch slower than computed goto | 10-20% performance gap | Benchmark early; fn-ptr table fallback; PGO |
| 367 opcode handlers = large function | I-cache pressure | `#[cold]` on rare handlers; split hot/cold paths |
| TRACE runtime overhead | Measurable even at level 0 | `#[inline(always)]` + branch prediction; feature flag fallback |
| Opcode enum drift from C++ | Silently missing opcodes | Mechanical generation script + count assertion test |
| Recursive `execute_block` stack depth | Stack overflow on deep nesting | Iterative flattened variant (Step 6); explicit stack limit |
| `unsafe { get_unchecked }` in hot loop | Memory safety regression | Invariant: every block ends with Return; debug-mode bounds checks |

---

*This document is part of the faust-rs porting plan. It will be updated as
implementation progresses and benchmarks become available.*
