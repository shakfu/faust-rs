

# Phase 6 — FIR & Main Backends (C, C++)

> **Crates**: `fir`, `codegen` (including `codegen::backends::c` and `codegen::backends::cpp`)
> **Estimate**: 45–65 person days
> **Prerequisites**: Phases 1–5
> **Architecture note**: `porting/faust-rust-fir-architecture-en.md`

---

## 1. C++ Inventory

### 1.1 generator/ (top-level) — 28,554 lines, ~55 files

**FIR Types and Instructions:**

| File | Lines | Role |
|---------|--------|------|
| `instructions_type.hh` | 286 | FIR Types: `Typed`, `VarType` (kInt32…kVoid), `BasicTyped`, `NamedTyped`, `FunTyped`, `ArrayTyped`, `StructTyped`, `VectorTyped` |
| `instructions.hh` | 4,137 | **Complete FIR hierarchy**: ~60 types of instructions (values, statements, loops, functions, UI). Visitor pattern (`InstVisitor`, `CloneVisitor`, `DispatchVisitor`). Builder `IB`. |
| `instructions.cpp` | 667 | Implementation of instructions (print, comparison, utilities) |
| `fir_to_fir.hh/.cpp` | 1,283 | ~20 FIR→FIR transformations: `MoveVariablesInFront`, `FunctionInliner`, `CastRemover`, `ControlExpander`, `ArrayToPointer`, etc. |
| `fir_function_builder.hh` | ~200 | FIR function builder |

**Container code:**

| File | Lines | Role |
|---------|--------|------|
| `code_container.hh/.cpp` | 2,121 | `CodeContainer`: central structure containing all the FIR code of a DSP (fields, init, compute, UI, metadata) |
| `omp_code_container.hh/.cpp` | ~300 | OpenMP variant |
| `vec_code_container.hh/.cpp` | ~300 | Vectorized variant |
| `wss_code_container.hh/.cpp` | ~300 | Work-Stealing Scheduler variant |

**Signal→FIR compilers (old pipeline):**

| File | Lines | Role |
|---------|--------|------|
| `instructions_compiler.hh/.cpp` | 4,455 | `InstructionsCompiler`: signal→FIR compilation (main pipeline) |
| `instructions_compiler1.hh/.cpp` | 116 | Minor variant |
| `instructions_compiler_jax.hh/.cpp` | ~400 | JAX variant |
| `dag_instructions_compiler.hh/.cpp` | 750 | `DAGInstructionsCompiler`: scheduling by DAG |
| `compile.hh/.cpp` | ~700 | Old compiler (`Compiler` base class, klass-based, legacy `-lang ocpp` path now out of scope) |
| `compile_scal.hh/.cpp` | ~1,600 | `ScalarCompiler` (old pipeline) |
| `compile_vect.hh/.cpp` | ~300 | `VectorCompiler` (old pipeline) |
| `compile_sched.hh/.cpp` | ~300 | `SchedulerCompiler` (old pipeline) |

**Utilities:**

| File | Lines | Role |
|---------|--------|------|
| `text_instructions.hh` | 578 | `TextInstVisitor`: base for all text backends |
| `type_manager.hh` | 822 | `TypeManager`: mapping FIR types → target language strings |
| `struct_manager.hh` | 318 | `StructManager`: management of the DSP struct |
| `json_instructions.hh` | 214 | JSON generation (metadata, UI) |
| `typing_instructions.hh` | ~100 | FIR type verification |
| `floats.hh/.cpp` | ~200 | Float/double/quad management |
| `description.hh/.cpp` | ~400 | `Description`: XML description of the DSP |
| `occurrences.hh/.cpp` | ~300 | Occurrence counting (for optimizations) |
| `klass.hh/.cpp` | ~600 | `Klass` (old code generation system) |
| `uitree.hh/.cpp` | ~200 | UI tree for generation |
| `Text.hh/.cpp` | ~300 | Text utilities (T(), number formatting) |
| `tools.hh/.cpp` | ~200 | Miscellaneous utilities |
| `sha_key.hh` | ~50 | SHA-1 calculation of source files |
| `statement.hh` | ~100 | `Statement`: conditions + code |
| `export.cpp` | ~200 | Compiler export function |

### 1.2 generator/fir/ — 1,723 lines, 4 files

| File | Lines | Role |
|---------|--------|------|
| `fir_instructions.hh` | ~500 | Specific FIR instructions (textual FIR backend) |
| `fir_code_container.hh/.cpp` | ~700 | `FirCodeContainer`: backend which issues the FIR in text |
| `fir_code_checker.hh` | ~500 | FIR Consistency Checker |

### 1.3 generator/c/ — 1,727 lines, 3 files

| File | Lines | Role |
|---------|--------|------|
| `c_instructions.hh` | ~500 | `CInstVisitor`: visitor emitting C |
| `c_code_container.hh/.cpp` | ~1,200 | `CCodeContainer`: assembly of the complete C code |

### 1.4 generator/cpp/ — 4,805 lines, 6 files

| File | Lines | Role |
|---------|--------|------|
| `cpp_instructions.hh` | ~600 | `CPPInstVisitor`: visitor sending C++ |
| `cpp_code_container.hh/.cpp` | ~2,500 | `CPPCodeContainer`: assembly of the complete C++ code (scalar, vector, OpenMP, WS) |
| `cpp_gpu_code_container.hh/.cpp` | ~1,100 | GPU variant (OpenCL) |
| `opencl_instructions.hh` | ~600 | OpenCL instructions |

---

## 2. Mapping C++ → Rust

### 2.1 fir — FIR types and instructions (mandatory canonical API)

The FIR hierarchy of 60+ C++ classes → a Rust enum:

```rust
/// FIR types
#[derive(Clone, Debug, PartialEq)]
pub enum FirType {
    Int32,
    Int64,
    Float,
    Double,
    Quad,
    FixedPoint,
    Bool,
    Void,
    Array(Box<FirType>, usize),       // type + size
    Ptr(Box<FirType>),
    Struct(String),
    Vector(Box<FirType>, usize),      // SIMD
    Fun {
        args: Vec<NamedType>,
        ret: Box<FirType>,
    },
}

#[derive(Clone, Debug)]
pub struct NamedType {
    pub name: String,
    pub typ: FirType,
}

/// Memory access
#[derive(Clone, Debug, PartialEq)]
pub enum AccessType {
    Stack,           // local variable
    Struct,          // DSP struct field
    Static,          // static/global variable
    FunArgs,         // function argument
    Loop,            // loop variable
}

/// FIR instructions — Values (expressions)
#[derive(Clone, Debug)]
pub struct FirValue {
    pub typ: FirType,
    pub kind: FirValueKind,
}

#[derive(Clone, Debug)]
pub enum FirValueKind {
    Int32(i32),
    Int64(i64),
    Float32(f32),
    Float64(f64),
    Bool(bool),
    LoadVar { name: String, access: AccessType },
    BinOp { op: BinOp, lhs: Box<FirValue>, rhs: Box<FirValue> },
    Cast { value: Box<FirValue> },
    FunCall { name: String, args: Vec<FirValue> },
}

/// FIR instructions — Statements
#[derive(Clone, Debug)]
pub enum FirStmt {
    DeclareVar {
        name: String,
        typ: FirType,
        access: AccessType,
        init: Option<FirValue>,
    },
    DeclareBufferIterators {
        name: String, typ: FirType, channels: i32, writable: bool,
    },
    StoreVar { name: String, access: AccessType, value: FirValue },
    ShiftArrayVar { name: String, access: AccessType, delay: i32 },
    DeclareFun {
        name: String,
        typ: FirType,
        args: Vec<NamedType>,
        body: FirBlock,
        is_inline: bool,
    },
    Drop(FirValue),
    ForLoop {
        var: String,
        init: FirValue,
        end: FirValue,
        step: FirValue,
        body: FirBlock,
        is_reverse: bool,
    },
    SimpleForLoop {
        var: String, upper: FirValue, body: FirBlock, is_reverse: bool,
    },
    WhileLoop { cond: FirValue, body: FirBlock },
    If { cond: FirValue, then_: FirBlock, else_: Option<FirBlock> },
    Switch { cond: FirValue, cases: Vec<(i32, FirBlock)>, default: Option<FirBlock> },
    Return(Option<FirValue>),
    Block(FirBlock),
    // UI
    OpenBox { typ: BoxType, label: String },
    CloseBox,
    AddButton { typ: ButtonType, label: String, var: String },
    AddSlider { typ: SliderType, label: String, var: String, init: f64, lo: f64, hi: f64, step: f64 },
    AddBargraph { typ: BargraphType, label: String, var: String, lo: f64, hi: f64 },
    AddSoundfile { label: String, var: String },
    AddMetaDeclare { var: String, key: String, value: String },
    Label(String),
}

pub type FirBlock = Vec<FirStmt>;
```

Typed-value policy:

- unlike C++ `ValueInst`, Rust FIR values carry their result type directly
  (`FirValue.typ`);
- this removes the need for a separate type-reconstruction pass equivalent to
  `typing_instructions.hh` for backend dispatch/lowering.

### 2.1.1 Canonical construction/matching API (mandatory)

To stay aligned with the Rust architecture already used for boxes/signals, FIR must use one
canonical API pair:

- construction: `FirBuilder`
- inspection/dispatch: `FirMatch` + `match_fir`

Reference shape:

```rust
pub struct FirStore { /* typed nodes arena */ }
pub struct FirBuilder<'a> { /* wraps &mut FirStore */ }

pub enum FirMatch<'a> {
    Int32(i32),
    BinOp { op: FirBinOp, lhs: FirId, rhs: FirId },
    DeclareVar { name: &'a str, /* ... */ },
    // ... exhaustive over canonical FIR nodes
    Unknown,
}

pub fn match_fir(store: &FirStore, id: FirId) -> FirMatch<'_>;
```

Mapping rule from C++:

- `IB::gen*` construction entry points map to `FirBuilder::*`.
- Visitor/RTTI dispatch (`accept`, `DispatchVisitor`, `dynamic_cast` families) maps to
  exhaustive `match_fir` decoding + Rust `match`.
- No parallel constructor/matcher ladders are allowed in production paths.

This is a phase-level invariant for backend work. C/C++/Rust/Wasm/LLVM emitters should consume FIR
through this canonical dispatch surface to keep behavior deterministic and auditable.

### 2.1.2 C++ anchors for the post-signal FIR path

Source-of-truth files to mirror during migration:

- `compiler/generator/instructions.hh`:
  - class hierarchy for `ValueInst`/`StatementInst`,
  - central static factory `IB::gen*`,
  - visitor dispatch (`InstVisitor`, `DispatchVisitor`).
- `compiler/generator/instructions_type.hh`:
  - `Typed::VarType` and helper conversions (`getPtrFromType`, `getVecFromType`, ...).
- `compiler/generator/instructions_compiler.hh/.cpp`:
  - currently effective production path (`InstructionsCompiler`/`DAGInstructionsCompiler`)
    that lowers signals to FIR blocks in `CodeContainer`.
- `compiler/transform/signalFIRCompiler.hh/.cpp`:
  - alternative direct signal->FIR path (secondary for current parity target).
- `compiler/generator/code_container.hh/.cpp`:
  - sectioned FIR ownership and lifecycle (`init`, `clear`, `compute`, UI, metadata).

Rust architectural interpretation:

- `IB::gen*` surface is represented by `FirBuilder` methods.
- C++ visitor/RTTI dispatch is represented by one `match_fir` + Rust `match`.
- `CodeContainer` and backend emitters consume stable `FirId`/`FirNode` references,
  not ad hoc backend-specific node wrappers.

### 2.2 fir — FIR→FIR transformations

```rust
/// Trait for FIR→FIR transformations
pub trait FirTransform {
    fn transform_value(&mut self, v: FirValue) -> FirValue { v }
    fn transform_stmt(&mut self, s: FirStmt) -> FirStmt { s }
    fn transform_block(&mut self, b: FirBlock) -> FirBlock {
        b.into_iter().map(|s| self.transform_stmt(s)).collect()
    }
}

// Concrete transformations
pub struct MoveVariablesInFront;
impl FirTransform for MoveVariablesInFront { /* ... */ }

pub struct FunctionInliner;
impl FirTransform for FunctionInliner { /* ... */ }

pub struct CastRemover;
impl FirTransform for CastRemover { /* ... */ }

pub struct ControlExpander;
impl FirTransform for ControlExpander { /* ... */ }

pub struct ArrayToPointer;
impl FirTransform for ArrayToPointer { /* ... */ }

// FIR checker
pub struct FirTypeChecker;
impl FirTypeChecker {
    pub fn check(&self, block: &FirBlock) -> Result<(), Vec<FirTypeError>>;
}
```

### 2.3 codegen — CodeContainer and generation framework

```rust
/// Central structure: all FIR code for a DSP
pub struct CodeContainer {
    pub name: String,
    pub num_inputs: usize,
    pub num_outputs: usize,
    pub num_inputs_channels: usize,
    pub num_outputs_channels: usize,

    // FIR blocks
    pub global_declarations: FirBlock,
    pub struct_fields: Vec<NamedType>,
    pub init_code: FirBlock,
    pub reset_ui_code: FirBlock,
    pub clear_code: FirBlock,
    pub static_init_code: FirBlock,
    pub static_destroy_code: FirBlock,
    pub compute_code: FirBlock,
    pub post_compute_code: FirBlock,
    pub metadata: Vec<(String, String)>,
    pub ui_tree: UiTree,

    // Sub-containers (tables, etc.)
    pub sub_containers: Vec<CodeContainer>,

    // JSON
    pub json: JsonMeta,
}

/// JSON generation
pub struct JsonMeta {
    pub name: String,
    pub inputs: usize,
    pub outputs: usize,
    pub meta: Vec<(String, String)>,
    pub ui: serde_json::Value,
}

/// Base text visitor (for text backends)
pub trait TextCodegen {
    fn emit_type(&self, typ: &FirType) -> String;
    fn emit_value(&self, val: &FirValue) -> String;
    fn emit_stmt(&self, stmt: &FirStmt, indent: usize) -> String;
    fn emit_block(&self, block: &FirBlock, indent: usize) -> String;
}

/// TypeManager: mapping FIR types → target language types
pub trait TypeManager {
    fn int32_type(&self) -> &str;
    fn int64_type(&self) -> &str;
    fn float_type(&self) -> &str;
    fn double_type(&self) -> &str;
    fn bool_type(&self) -> &str;
    fn ptr_type(&self, inner: &str) -> String;
    fn array_type(&self, inner: &str, size: usize) -> String;
}
```

### 2.4 codegen::backends::c

```rust
pub struct CTypeManager;
impl TypeManager for CTypeManager { /* int, float, double, ... */ }

pub struct CCodegen {
    type_manager: CTypeManager,
    float_size: FloatSize,
}

impl TextCodegen for CCodegen { /* ... */ }

/// Generates the complete C file
pub fn generate_c(
    container: &CodeContainer,
    config: &BackendConfig,
    output: &mut dyn Write,
) -> io::Result<()>;
```

### 2.5 codegen::backends::cpp

```rust
pub struct CppTypeManager;
impl TypeManager for CppTypeManager { /* ... */ }

pub struct CppCodegen {
    type_manager: CppTypeManager,
    float_size: FloatSize,
    use_virtual: bool,
}

impl TextCodegen for CppCodegen { /* ... */ }

pub fn generate_cpp(
    container: &CodeContainer,
    config: &BackendConfig,
    output: &mut dyn Write,
) -> io::Result<()>;
```

### 2.6 Recommended FIR/codegen restructuring during the Rust port

The audit of `instructions.hh/.cpp`, `instructions_type.hh`, `type_manager.hh`, and `struct_manager.hh` shows several high-value restructuring opportunities that should be integrated into Phase 6:

1. Replace C++ class hierarchy + RTTI with enum-based FIR nodes and `match`-based passes.
2. Replace raw-pointer instruction ownership with arena IDs and contiguous Rust containers (`Vec`/`SmallVec`) for stable traversal and simpler cloning.
3. Split the current `IB` responsibilities into:
   - a pure node factory
   - a canonicalization/folding pass
   - a lowering/target adaptation pass
4. Remove `gGlobal` dependencies from FIR construction by introducing an explicit `CompilerContext` passed through the pipeline.
5. Replace `VarType` variant explosion with a compositional type model (`BaseType`, `Pointer(Type)`, `Vector { elem, lanes }`, etc.).
6. Replace `TypeManager` inheritance tree with traits plus backend-specific formatting tables to eliminate repeated casting logic.
7. Separate DSP struct concerns:
   - field/layout computation
   - memory/usage metadata
   - backend emission
8. Replace repeated field-name linear scans with indexed lookups (symbol IDs and maps) in struct layout code.

Recommended rollout:

1. Preserve current semantics first (MVP parity path).
2. Lock behavior with golden/differential tests.
3. Apply the restructuring incrementally in this order:
   - FIR representation and ownership
   - type system and type managers
   - struct/memory layout subsystem
   - backend-specific lowering cleanup

### 2.7 Recommended CodeContainer machinery restructuring during the Rust port

The audit of `code_container.hh/.cpp`, `vec_code_container.hh/.cpp`, `omp_code_container.hh/.cpp`, `wss_code_container.hh/.cpp`, and related backend container classes shows additional high-value restructuring opportunities:

1. Split `CodeContainer` into explicit data sections (declarations, init/static-init, UI, compute/control, metadata/memory) instead of one broad mutable holder.
2. Replace option-driven in-place orchestration in `processFIR()` with an explicit pass pipeline and typed pass contexts.
3. Move zone rewriting (`iZone`/`fZone`) into dedicated transforms operating on explicit pass inputs/outputs.
4. Replace side-effectful subcontainer merge-and-clear logic with deterministic merge results.
5. Avoid rebuilding flattened FIR snapshots repeatedly during checks; compute once and reuse per phase.
6. Replace pointer/set-based loop graph handling with stable loop IDs and deterministic scheduling views.
7. Replace scalar/vector/OpenMP/WSS inheritance specialization with strategy composition (`ComputeStrategy`, `ParallelStrategy`).
8. Replace backend mode `if/else` factories with a registry-driven backend/strategy selector.
9. Deduplicate repeated local input/output address setup logic shared by vector/OpenMP/WSS containers.
10. Represent OpenMP and work-stealing behavior as structured IR effects/annotations instead of textual labels in IR blocks.
11. Isolate memory-layout/access accounting into analysis modules independent from text code generation.
12. Remove residual global-state dependencies in container machinery by passing an explicit compilation context.

Recommended rollout:

1. Keep current behavior and output parity on the effective production path.
2. Add golden/differential tests around container flattening, scheduling, and emitted code.
3. Apply architecture changes incrementally:
   - sectioned container model
   - pass manager for container transformations
   - loop DAG/model stabilization
   - backend strategy extraction and emitter cleanup

### 2.8 Recommended `libcode.cpp` orchestration restructuring during the Rust port

The audit of `libcode.cpp` (current backend entry and orchestration layer) shows additional high-value simplifications to integrate into Phase 6:

1. Replace mutable global compile state (`gGlobal` usage and static globals in orchestration paths) with explicit request/session objects.
2. Replace many backend-specific `compileX` wrappers with a backend registry and one shared compile template.
3. Replace long backend dispatch chains with table-driven selection returning structured backend profiles.
4. Move architecture/enrobage assembly out of the main compile routine into dedicated post-processing stages.
5. Replace stream downcasts (`dynamic_cast<ostringstream*>`) with typed output sinks and explicit output capabilities.
6. Unify API entry points (`expandDSP`, `DSPToBoxes`, and factory creation) around one lifecycle model to avoid divergent behavior.
7. Keep orchestration compilation units explicit (no `.cpp` includes in `.cpp` orchestration layer).
8. Make timing/teardown scope-safe so early-return paths cannot skip finalization.
9. Replace fixed-size temporary `argv` arrays in API entry code with dynamic validated vectors.
10. Move backend/option compatibility checks to a declarative capability matrix with automated consistency tests.
11. Ensure orchestration pointers/state are reset per request so early-return backends cannot leak stale state.
12. Normalize output writer mode handling (text vs binary) in one sink abstraction to avoid backend-specific file mode drift.
13. Isolate legacy/excluded backend residues (`ocpp`, template-only scaffolding) from the core compilation path.
14. Replace stack-size thread trampoline patterns with explicit recursion-depth limits and iterative rewrites where possible.

Recommended rollout:

1. Freeze behavior with differential tests across representative in-scope backends (`c`, `cpp`, `codebox`, `rust`, `wasm`, `llvm` as available).
2. Introduce backend descriptors and registry dispatch while preserving current emitted outputs.
3. Move global state and output handling behind explicit session/sink abstractions, then simplify API surface.

---

## 3. Dependencies

```
fir         → errors  (pure FIR types, no dependency on signals)
codegen     → fir, errors, utils
codegen::backends::c   → codegen, fir
codegen::backends::cpp → codegen, fir
```

**Important**: `fir` does NOT depend on `tlib` nor `signals`. It is an independent intermediate representation. The signal→FIR translation is in `transform` (Phase 5).

---

## 4. Known pitfalls

### 4.1 Deep inheritance hierarchy in C++
FIR instructions form a deep inheritance hierarchy (3–4 levels) with visitor pattern. In Rust, we replace with enums + pattern matching. The advantage: guaranteed completeness, no vtable, no casting.

### 4.2 IB (Instruction Builder) — global factory
In C++, `IB` is a static class with factory methods (`IB::genLoadVar(...)`, etc.). In Rust, this
must be represented as one canonical `FirBuilder` API (not ad hoc enum-constructor calls spread
across crates), so construction paths stay uniform and testable.

### 4.3 Old vs new pipeline
There are **two** signal→FIR compilation pipelines:
- **Former**: `InstructionsCompiler` → `CodeContainer` (via `instructions_compiler.cpp`)
- **New**: `SignalFIRCompiler` → `FirBlocks` (via `transform/signalFIRCompiler.cpp`)

**Audit correction**:
- On the current branch, `libcode.cpp` backend dispatch still relies on the former pipeline (`InstructionsCompiler` / `DAGInstructionsCompiler`) for the main end-to-end flow.
- `SignalFIRCompiler` exists but is not currently the default production path for C/C++ backend generation.

→ In Rust, carry the **former pipeline first** for MVP parity, then evaluate whether `SignalFIRCompiler` should be ported as a second step.

### 4.4 Garbageable for FIR instructions
In C++, all FIR instructions inherit from `Garbageable`. In Rust, we use `Vec<FirStmt>` owning — no need for GC.

### 4.5 JSON and SHA
JSON generation uses dedicated structures. In Rust, use `serde_json` for serialization. For SHA, use `sha1` crate.

### 4.6 Struct layout complexity and search costs
`struct_manager.hh` currently mixes layout and metadata concerns and performs repeated field lookup scans. During the Rust port, this should be split and indexed to keep complexity predictable.

### 4.7 TypeManager duplication
`type_manager.hh` contains repeated backend-specialized logic with parallel class hierarchies. Rust traits plus backend lookup tables should replace this pattern.

### 4.8 Global-state coupling in FIR builders
`instructions.cpp` currently depends on global compiler state for type and memory decisions. Rust implementation should route all such data through explicit context objects.

### 4.9 Monolithic CodeContainer state
`code_container.hh/.cpp` currently centralizes many mutable responsibilities (sections, loops, metadata, memory, UI, backend hooks). Splitting this state is key to maintainability in Rust.

### 4.10 Backend specialization explosion
Current C/C++ codegen combines backend language concerns with scalar/vector/OpenMP/WSS specialization via inheritance layers. Rust should use composition and strategy traits.

### 4.11 Text-label encoded parallel semantics
OpenMP/work-stealing code paths inject behavior via textual labels/directives in IR emission paths. Rust should keep these semantics as explicit structures until final text emission.

### 4.12 `libcode.cpp` global lifecycle coupling
Some `libcode.cpp` API paths manage `gGlobal` lifecycle differently, increasing the risk of divergent behavior and hard-to-track bugs. Rust should centralize this lifecycle in one session model.

### 4.13 Backend dispatch and wrapper duplication in `libcode.cpp`
The orchestration layer currently duplicates compile flow across many backend wrappers and dispatch branches. Rust should eliminate this with registry-driven backend descriptors.

### 4.14 Output stream downcasts in orchestration paths
Output handling currently depends on stream type checks/downcasts in `libcode.cpp`. Rust should use typed sink interfaces to avoid hidden output-mode branching.

### 4.15 FIR dispatch drift
If FIR construction/inspection is reimplemented independently in several modules, semantic drift
between backends is likely. Keep one shared `FirBuilder` + `match_fir` implementation in `crates/fir`.

### 4.16 Late value-type reconstruction
Reconstructing value types in separate backend-specific passes (C++ style `typing_instructions.hh`)
creates drift and debugging overhead. Rust FIR should keep type on value nodes directly and treat
type reconstruction as a validation fallback, not a primary mechanism.

---

## 5. Testing

- **Unit**: Construction of each type of FIR instruction
- **Unit**: FIR→FIR transformations (MoveVariablesInFront, CastRemover, etc.)
- **Unit**: FirTypeChecker on valid and invalid blocks
- **Unit**: C generation of a simple DSP (check text output)
- **Unit**: C++ generation of a simple DSP
- **Integration**: Complete pipeline signal→FIR→C on `process = + ~ _;`
- **Compilation**: The generated C/C++ code compiles with gcc/clang
- **Differential**: Compare C/C++ output with C++ compiler on 20+ examples

---

## 6. "Done" criteria

- [ ] All representable FIR types
- [ ] All FIR→FIR transformations carried
- [ ] Working C backend: `faust -lang c` produces compilable code
- [ ] Working C++ backend: `faust -lang cpp` produces compilable code
- [ ] Correct JSON (check with existing Faust tools)
- [ ] The generated code is bit-identical or functionally equivalent to C++
- [ ] FIR nodes use enum + typed IDs (no RTTI/dynamic_cast patterns)
- [ ] FIR construction goes through `FirBuilder` (single canonical entrypoint)
- [ ] FIR inspection/dispatch goes through `FirMatch` + `match_fir` (no parallel ladders)
- [ ] FIR building no longer depends on mutable global state
- [ ] Type mapping uses trait-based backends with shared core type model
- [ ] Code container uses explicit sectioned model and pass pipeline
- [ ] Loop DAG/scheduling model is deterministic and ID-based
- [ ] Backend compute variants use strategy composition (not inheritance matrix)
- [ ] Orchestration layer uses explicit compile sessions (no mutable global lifecycle in backend entry paths)
- [ ] Backend selection is registry-driven with shared compile template flow
- [ ] Output writing in orchestration paths uses typed sinks (no stream downcasts)
- [ ] API compile entry points share one lifecycle model (no divergent init/teardown behavior)
- [ ] CLI/backend option compatibility is driven by a declarative capability matrix with consistency tests
- [ ] API argument normalization uses dynamic vectors (no fixed-size temporary `argv` staging)
- [ ] Orchestration stack handling avoids hidden thread-trampoline behavior in core compile flow

---

## 7. Detailed Effort

| Sub-module | LOC C++ | Estimated LOC Rust | Days |
|-------------|---------|-----------------|-------|
| Types + FIR instructions | 5,090 | 3,000 | 8–10 |
| FIR→FIR transformations | 1,283 | 800 | 3–4 |
| CodeContainer + infrastructure | 4,200 | 2,500 | 6–8 |
| TextCodegen + TypeManager | 1,718 | 1,000 | 3–4 |
| Backend C | 1,727 | 1,200 | 3–4 |
| C++ Backend | 4,805 | 3,000 | 6–8 |
| FIR backend (debug) | 1,723 | 1,000 | 2–3 |
| Tests + docs | — | 1,500 | 4–5 |
| **Total Phase 6** | **20,546** | **15,000–18,000** | **45–65** |

**Note**: For branch parity, Phase 6 includes the currently used path around `InstructionsCompiler`/`DAGInstructionsCompiler`. The `SignalFIRCompiler` path is treated as optional/future unless upstream flow changes.
