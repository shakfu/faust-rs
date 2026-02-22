# Step 5: Factory, Serialization, DSP Interface

## Context

Port C++ `interpreter_dsp_factory_aux<REAL, TRACE>` (993 lines in `interpreter_dsp_aux.hh`)
and `interpreter_dsp_aux<REAL, TRACE>` + the `read()`/`write()` serialization from
`interpreter_dsp.hh` and `interpreter_bytecode.hh` into three new Rust files:
`factory.rs`, `instance.rs`, and `serial.rs`.

This is the **final integration step** of the interpreter backend, bringing together
all previous modules (opcodes, bytecode, executor, compiler, optimizer) into a
complete pipeline: factory creation → optimization → serialization → instantiation
→ DSP compute.

---

## Files to create/modify

| File | Action | Description |
|------|--------|-------------|
| `crates/codegen/src/backends/interp/factory.rs` | **NEW** (~200 lines) | `FbcDspFactory<R>` — compiled bytecode program |
| `crates/codegen/src/backends/interp/instance.rs` | **NEW** (~250 lines) | `FbcDspInstance<R>` — runtime DSP state with compute() |
| `crates/codegen/src/backends/interp/serial.rs` | **NEW** (~600 lines) | `.fbc` read/write serialization |
| `crates/codegen/src/backends/interp/real.rs` | **EDIT** | Add `std::str::FromStr` bound to `FbcReal` trait |
| `crates/codegen/src/backends/interp/mod.rs` | **EDIT** | Register 3 new modules + re-exports |

---

## C++ Architecture (what we're porting)

### Factory (`interpreter_dsp_factory_aux<REAL, TRACE>`)

Holds the compiled bytecode program with 8 named blocks + metadata:

```
Fields:
  fVersion, fNumInputs, fNumOutputs
  fIntHeapSize, fRealHeapSize
  fSROffset, fCountOffset, fIOTAOffset
  fOptLevel, fOptimized, fCompileOptions
  fMetaBlock            → Vec<FbcMetaInstruction>
  fUserInterfaceBlock   → Vec<FbcUiInstruction<R>>
  fStaticInitBlock      → BlockId  (8 bytecode blocks in the arena)
  fInitBlock            → BlockId
  fResetUIBlock         → BlockId
  fClearBlock           → BlockId
  fComputeBlock         → BlockId  (control block)
  fComputeDSPBlock      → BlockId  (DSP block)
```

Key methods:
- `optimize()` — applies optimizer levels 1..fOptLevel to all 6 code blocks (once)
- `metadata()` — iterates fMetaBlock, calling `meta->declare(key, value)`
- `write()` — serializes to `.fbc` text format (normal or small mode)
- `read()` — static factory method to deserialize from `.fbc`

### Instance (`interpreter_dsp_aux<REAL, TRACE>`)

Runtime state for one DSP instance:

```
Fields:
  fFactory      → &FbcDspFactory<R> (shared, immutable)
  fFBCExecutor  → FbcExecutor<R> (owns the heaps)
  fInitialized  → bool
  fCycle        → usize (compute call counter)
```

Key lifecycle:
1. **Construction**: `new(factory)` → calls `factory.optimize()`, creates executor with factory heap sizes
2. **init(sample_rate)**: sets `fInitialized = true`, calls `instanceInit(sample_rate)`
3. **instanceInit(sr)**: `classInit(sr)` → `instanceConstants(sr)` → `instanceResetUserInterface()` → `instanceClear()`
4. **classInit(sr)**: executes `fStaticInitBlock`
5. **instanceConstants(sr)**: sets `int_heap[fSROffset] = sr`, executes `fInitBlock`
6. **instanceResetUserInterface()**: executes `fResetUIBlock`
7. **instanceClear()**: executes `fClearBlock`
8. **compute(count, inputs, outputs)**:
   - Guard: `if count == 0 { return; }`
   - Set input/output buffer pointers on executor
   - Set `int_heap[fCountOffset] = count`
   - Execute `fComputeBlock` (control)
   - Execute `fComputeDSPBlock` (DSP)
   - Increment `fCycle`

### Serialization (`.fbc` text format, version 8)

**Normal mode header:**
```
interpreter_dsp_factory float|double
file_version 8
Faust version X.Y.Z
compile_options <options>
name <name>
sha_key <sha>
opt_level <level>
inputs N outputs M
int_heap_size H real_heap_size R sr_offset S count_offset C iota_offset I
```

**Blocks** (8 total, each preceded by a label line):
```
meta_block / user_interface_block / static_init_block / constants_block /
reset_ui / clear_block / control_block / dsp_block
```

**Block format:**
```
block_size N
<N instruction lines>
```

**Instruction line (normal mode):**
```
opcode NUM kName int V real R offset1 O1 offset2 O2 [name N]
```

**Special: BlockStoreReal / BlockStoreInt:**
```
opcode NUM kBlockStoreReal offset1 O1 offset2 O2 size S
V1 V2 V3 ... VS
```

**UI instruction line:**
```
opcode NUM kName offset O label "L" key K value "V" init I min MN max MX step ST
```

**Meta instruction line:**
```
meta key "K" value "V"
```

**Sub-blocks:** If/Select/Loop instructions have branch1/branch2 written inline
as recursive `readCodeBlock()` / `block.write()`.

---

## Design

### `factory.rs` — `FbcDspFactory<R>`

```rust
pub struct FbcDspFactory<R: FbcReal> {
    pub name: String,
    pub sha_key: String,
    pub compile_options: String,
    pub version: u32,
    pub num_inputs: i32,
    pub num_outputs: i32,
    pub int_heap_size: i32,
    pub real_heap_size: i32,
    pub sr_offset: i32,
    pub count_offset: i32,
    pub iota_offset: i32,
    pub opt_level: i32,
    optimized: bool,

    // Data blocks
    pub arena: FbcBlockArena<R>,
    pub meta_block: Vec<FbcMetaInstruction>,
    pub ui_block: Vec<FbcUiInstruction<R>>,

    // 6 code block IDs in the arena
    pub static_init_block: BlockId,
    pub init_block: BlockId,
    pub reset_ui_block: BlockId,
    pub clear_block: BlockId,
    pub compute_block: BlockId,
    pub compute_dsp_block: BlockId,
}

impl<R: FbcReal> FbcDspFactory<R> {
    pub fn new(...) -> Self;
    pub fn optimize(&mut self);  // applies optimizer levels 1..opt_level once
}
```

### `instance.rs` — `FbcDspInstance<R>`

```rust
pub struct FbcDspInstance<'a, R: FbcReal> {
    factory: &'a FbcDspFactory<R>,
    executor: FbcExecutor<R>,
    initialized: bool,
    cycle: usize,
}

impl<'a, R: FbcReal> FbcDspInstance<'a, R> {
    pub fn new(factory: &'a mut FbcDspFactory<R>) -> Self;
    pub fn init(&mut self, sample_rate: i32);
    pub fn instance_init(&mut self, sample_rate: i32);
    pub fn class_init(&mut self, sample_rate: i32);
    pub fn instance_constants(&mut self, sample_rate: i32);
    pub fn instance_reset_user_interface(&mut self);
    pub fn instance_clear(&mut self);
    pub fn compute(&mut self, count: i32, inputs: &[&[R]], outputs: &mut [&mut [R]]);
    pub fn get_sample_rate(&self) -> i32;
    pub fn get_num_inputs(&self) -> i32;
    pub fn get_num_outputs(&self) -> i32;
}
```

Lifetime `'a` ties the instance to its factory reference. The factory must be
optimized before creating an instance (handled in `new()`).

### `serial.rs` — `.fbc` read/write

```rust
pub fn write_fbc<R: FbcReal>(
    factory: &FbcDspFactory<R>,
    writer: &mut dyn Write,
    small: bool,
) -> io::Result<()>;

pub fn read_fbc<R: FbcReal>(
    reader: &mut dyn BufRead,
) -> Result<FbcDspFactory<R>, FbcSerialError>;
```

Internal helpers:
- `write_instruction()` — formats a single FbcInstruction (normal + small)
- `write_block_store_instruction()` — formats BlockStoreReal/Int with data line
- `write_ui_instruction()` — formats FbcUiInstruction
- `write_meta_instruction()` — formats FbcMetaInstruction
- `write_code_block()` — writes `block_size N` then N instruction lines
- `read_code_block()` — reads block_size + instructions, handles sub-blocks
- `read_code_instruction()` — reads one instruction, detects BlockStore/branch
- `read_ui_block()` — reads UI block
- `read_meta_block()` — reads meta block
- `quote1()` / `unquote1()` — string quoting for labels/keys/values

### `real.rs` change

Add `std::str::FromStr` to the `FbcReal` trait supertraits:

```rust
pub trait FbcReal: ... + std::str::FromStr + ...
```

Both `f32` and `f64` already implement `FromStr`, so no new code needed in impls.
This is required by the deserializer to parse real values from `.fbc` text.

---

## Implementation order

| Phase | Contents | Tests enabled |
|-------|----------|---------------|
| **A** | `real.rs`: Add `FromStr` bound to `FbcReal` | Existing tests pass |
| **B** | `factory.rs`: `FbcDspFactory<R>` struct + `new()` + `optimize()` | Factory construction test |
| **C** | `instance.rs`: `FbcDspInstance<R>` struct + lifecycle + `compute()` | Init + compute smoke test |
| **D** | `serial.rs` write path: `write_fbc()` + all write helpers | Write round-trip test |
| **E** | `serial.rs` read path: `read_fbc()` + all read helpers | Read round-trip test |
| **F** | `mod.rs`: Register modules + re-exports | Compilation check |
| **G** | Integration tests: full pipeline + parity tests | End-to-end |
| **H** | Quality gate: fmt + clippy + all tests | Pass criteria |

---

## Tests

### Unit tests:

1. **`test_factory_construction`** — Create a factory with trivial blocks, verify all field accessors.
2. **`test_factory_optimize`** — Create factory with unoptimized blocks, call `optimize()`, verify blocks were optimized (instruction count reduced).
3. **`test_factory_optimize_idempotent`** — Call `optimize()` twice, verify it only runs once (`fOptimized` guard).
4. **`test_instance_lifecycle`** — Create instance from factory, call `init(44100)`, verify `get_sample_rate() == 44100`.
5. **`test_instance_compute_passthrough`** — Build a simple passthrough DSP (copy input[0] → output[0]), verify `compute()` produces correct output.
6. **`test_instance_compute_gain`** — Build a gain DSP (input × 0.5), verify output samples.

### Serialization tests:

7. **`test_write_empty_factory`** — Write a factory with empty blocks, verify header format matches C++ output.
8. **`test_write_read_roundtrip`** — Write a factory with various instructions, read it back, verify all fields match.
9. **`test_write_small_mode`** — Write in small mode, verify compact format tokens.
10. **`test_read_block_store`** — Verify BlockStoreReal/Int instructions with data lines round-trip correctly.
11. **`test_read_branching_instructions`** — Verify If/Select/Loop instructions with sub-blocks round-trip.
12. **`test_read_ui_block`** — Verify UI instructions (slider, button, box open/close) round-trip.
13. **`test_read_meta_block`** — Verify meta instructions round-trip.
14. **`test_version_check`** — Attempt to read a `.fbc` with wrong version, verify error.
15. **`test_quoted_strings`** — Verify labels with special characters survive quote/unquote.

### Integration tests:

16. **`test_full_pipeline`** — FIR → compile → optimize → serialize → deserialize → instantiate → compute → verify output.

### Pass criteria (from porting plan):

- `.fbc` round-trip: write → read produces identical factory ✓
- `compute()` output parity with executor-only test ✓
- Version check: wrong version → error ✓
- Factory optimization runs exactly once ✓
- Quality gate green (fmt + clippy + all tests) ✓
