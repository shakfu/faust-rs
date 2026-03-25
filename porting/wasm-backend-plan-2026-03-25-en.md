# WebAssembly Backend Plan (Faust Rust Port)

**Date:** 2026-03-25
**Status:** Planning
**Target crates:** `codegen`, `compiler`, `wasm-ffi` (new)
**Primary backend module:** `codegen::backends::wasm`
**C++ provenance:** `compiler/generator/wasm/` (5,521 LOC, 16 files)
**Estimated effort:** 8–10 person-days, ~3,500 LOC Rust

---

## 1. Purpose and Positioning

This document defines the implementation plan for the **WebAssembly backend**
in `faust-rs`, porting the existing C++ WASM backend (`generator/wasm/`) to
Rust.

Unlike the Cranelift backend (a Rust-native extension), this backend is a
**direct C++ parity port**. The C++ backend emits valid `.wasm` binary modules
(and optionally `.wat` text) containing a self-contained DSP module that runs
in any WASM runtime (browsers, wasmtime, wasmer, Node.js).

### Why WebAssembly?

- **Primary deployment target for Faust web applications** (Faust IDE, Web Audio API).
- Enables **browser-based DSP** without native compilation.
- Growing adoption in **embedded** and **plugin** contexts (WASM audio worklets).
- Existing ecosystem of Faust architecture files depends on WASM output.

### Mapping status

- **C++ source mapping:** `1:1` parity with `generator/wasm/`.
- **Output format:** WebAssembly binary (`.wasm`), optionally WebAssembly text (`.wat`).
- **Compatibility:** Drop-in replacement — produced modules must be loadable by
  existing Faust JS runtime (`faust2wasm`, `faustwasm` npm package).

---

## 2. C++ Architecture Reference

The C++ WASM backend consists of the following key components:

### 2.1 Binary encoder (`wasm_binary.hh`)

Custom encoder that emits the WASM binary format:
- Module header (magic number, version)
- Type section (function signatures)
- Import section (external math functions, memory)
- Function section (function index → type index mapping)
- Table section (indirect function calls, if needed)
- Memory section (linear memory declaration)
- Export section (DSP API functions, memory)
- Code section (function bodies in WASM bytecode)
- Data section (initial memory contents — tables, constants)

### 2.2 Instruction visitors

- **`WASMInstVisitor`** — FIR → WASM binary bytecode emission
- **`WASTInstVisitor`** — FIR → WAT text format emission (debug/human-readable)

Both implement the FIR instruction visitor pattern, translating each FIR node
to the appropriate WASM instruction sequence.

### 2.3 Code container (`WASMCodeContainer`)

Orchestrates the full module assembly:
- Collects all DSP lifecycle functions
- Manages the linear memory layout (struct fields, tables, I/O buffers)
- Resolves import/export lists
- Emits the complete WASM module

### 2.4 Memory layout

WASM uses a single linear memory. The C++ backend lays it out as:

```
Offset 0:  DSP struct fields (int heap + real heap)
           ├─ iZone: integer state variables
           └─ fZone: float/double state variables
Then:      Static tables (rdtable/rwtable data)
Then:      Audio I/O buffer pointers zone
```

All field accesses are compiled to `i32.load`/`f32.load`/`f64.load` at known
constant offsets from the base of linear memory (the DSP struct lives at
offset 0).

---

## 3. C++ Module Structure Parity Constraints

The Rust WASM backend **must** produce modules that are binary-compatible with
the C++ backend output. The existing Faust JS runtime (`faustwasm`,
`faust2wasm`, `libfaust-wasm`) expects a precise ABI contract. This section
documents every structural constraint extracted from the C++ source
(`compiler/generator/wasm/`).

### 3.1 Module section ordering

The C++ backend emits WASM sections in this exact order
(`WASMCodeContainer::produceClass()` in `wasm_code_container.cpp`):

```
1. Module header       (magic 0x0061736d, version 0x01)
2. Type section   (1)  — all function type signatures
3. Import section (2)  — math imports from "env" + optional memory import
4. Function section (3) — type index for each module-defined function
5. Memory section (5)  — only if internal memory (fInternalMemory == true)
6. Export section (7)  — 11 DSP API functions + optional memory export
7. Code section  (10)  — 14 function bodies
8. Data section  (11)  — JSON string at offset 0
```

No Table (4), Global (6), Start (8), or Element (9) sections are emitted.
The Rust backend must follow this exact section order.

### 3.2 Type section — function type registry

Types are registered by `FunAndTypeCounter` (in `wasm_instructions.hh`) in a
specific order. Each **unique function name** gets one type entry. The order is
determined by the `std::map<std::string, FunTyped*>` iteration (alphabetical
by function name in C++).

Built-in function types (always present, registered in constructor):

| Function name | Params | Return | Notes |
|---------------|--------|--------|-------|
| `classInit` | `(i32, i32)` | `void` | `(dsp, sample_rate)` |
| `compute` | `(i32, i32, i32, i32)` | `void` | `(dsp, count, inputs, outputs)` — inputs/outputs are `kVoid_ptr` → `i32` |
| `getNumInputs` | `(i32)` | `i32` | `(dsp)` |
| `getNumOutputs` | `(i32)` | `i32` | `(dsp)` |
| `getParamValue` | `(i32, i32)` | `f32` or `f64` | `(dsp, index)` — return type depends on `gFloatSize` |
| `getSampleRate` | `(i32)` | `i32` | `(dsp)` |
| `init` | `(i32, i32)` | `void` | `(dsp, sample_rate)` |
| `instanceClear` | `(i32)` | `void` | `(dsp)` |
| `instanceConstants` | `(i32, i32)` | `void` | `(dsp, sample_rate)` |
| `instanceInit` | `(i32, i32)` | `void` | `(dsp, sample_rate)` |
| `instanceResetUserInterface` | `(i32)` | `void` | `(dsp)` |
| `max_i` | `(i32, i32)` | `i32` | Integer max helper |
| `min_i` | `(i32, i32)` | `i32` | Integer min helper |
| `setParamValue` | `(i32, i32, f32)` or `(i32, i32, f64)` | `void` | `(dsp, index, value)` |

Additional types are added for each **math import** discovered during the
global declarations pass (`generateGlobalDeclarations`). These are keyed by
C function name (e.g. `"sinf"`, `"cosf"`, `"expf"` for float; `"sin"`,
`"cos"`, `"exp"` for double).

### 3.3 Import section

Imports come from the `"env"` module. The import list consists of:

1. **(Optional) Memory import** — only when `fInternalMemory == false`
   (external memory mode, used for polyphonic DSP with soundfiles):
   ```
   import "env" "memory" (memory 0 1)
   ```
   Flags = 0 (no max), initial size = 1 page (minimum; actual size set by JS).

2. **Math function imports** — only functions classified as `kExtMath` or
   `kExtWAS` in the `fMathLibTable`. Imported in alphabetical order of their
   C function name. The import base name is prefixed with `"_"` and goes
   through `getMathFunction()` for possible fastmath remapping.

   Example imports for a float DSP using `sin`, `cos`, `exp`:
   ```
   import "env" "_cosf"  (func (type $cosf_type))
   import "env" "_expf"  (func (type $expf_type))
   import "env" "_sinf"  (func (type $sinf_type))
   ```

**Critical constraint**: Function indices are assigned as:
- Imported functions first (indices 0, 1, 2, …)
- Then module-defined functions (continuing the index sequence)

The `getFunctionIndex()` method enforces this ordering.

### 3.4 Math function classification

Each math function falls into one of four categories (`MathFunDesc::Gen`):

| Category | Where implemented | WASM emission | Example |
|----------|-------------------|---------------|---------|
| `kWAS` | Native WASM opcode | Inline opcode (no call) | `fabsf` → `f32.abs`, `sqrtf` → `f32.sqrt`, `ceilf` → `f32.ceil`, `floorf` → `f32.floor`, `min_f` → `f32.min`, `max_f` → `f32.max`, `rintf` → `f32.nearest` |
| `kExtMath` | Host JS `Math.*` | Imported from `"env"` | `sinf`, `cosf`, `expf`, `logf`, `powf`, `atan2f`, `acosf`, `asinf`, `atanf`, `tanf`, `roundf`, `log10f`, `exp10f`, and hyperbolic variants |
| `kIntWAS` | Module-internal function | `call $min_i` / `call $max_i` | `min_i`, `max_i` (implemented as `(lt_s + select)`) |
| `kExtWAS` | Host JS custom function | Imported from `"env"` | `fmodf`, `remainderf`, `isinff`, `copysignf` |

The same classification exists for double variants (`fabs`, `sin`, `cos`, …).

### 3.5 Function section

Lists type indices for **module-defined functions only** (excluding imports),
in alphabetical order of function name (same `std::map` iteration order as
the type section, filtered to exclude imported functions).

The C++ code emits exactly `fFunTypes.size() - fFunImports.size()` entries.

### 3.6 Memory section (internal memory mode)

When `fInternalMemory == true`:

```
memory section:
  1 memory
  flags = 1 (has min AND max)
  min = <computed pages>      ← placeholder, backpatched after code generation
  max = <computed pages + 1000>  ← generous max for soundfile growth
```

Memory size is computed by `genMemSize()`:

```cpp
int genMemSize(int struct_size, int channels, int json_len) {
    return max(1,
        wasm_pow2limit(
            max(json_len,
                struct_size + channels * (audioSampleSize + 8192 * audioSampleSize))
        ) / 65536
    );
}
```

Where:
- `struct_size` = total byte size of DSP struct fields
- `channels` = `numInputs + numOutputs`
- `json_len` = length of JSON metadata string
- `audioSampleSize` = 4 (float) or 8 (double)
- `wasm_pow2limit(x)` = smallest power of 2 ≥ x (minimum 65536)

**Key**: Memory size cannot be determined until after code generation (because
subcontainer inlining and waveform generation affect struct size) AND after
JSON generation (because JSON is written at offset 0 in the data segment).
The C++ backend uses backpatching (`writeAt`) to fill the placeholder.

### 3.7 Export section

Exactly **11 function exports** (constant `EXPORTED_FUNCTION_NUM`), plus
optionally 1 memory export. Exported in this exact order:

```
1.  "compute"
2.  "getNumInputs"
3.  "getNumOutputs"
4.  "getParamValue"
5.  "getSampleRate"
6.  "init"
7.  "instanceClear"
8.  "instanceConstants"
9.  "instanceInit"
10. "instanceResetUserInterface"
11. "setParamValue"
12. "memory"  (only if fInternalMemory == true)
```

Each function export references the function index (imports first, then
module-defined functions).

### 3.8 Code section — function body ordering

The code section contains exactly **14 function bodies**, emitted in this
order (`WASMCodeContainer::produceClass()`):

```
 1. classInit                    — static init (subcontainer inlining + MoveVariablesInFront3)
 2. compute                      — DSP processing loop
 3. getNumInputs                 — returns constant
 4. getNumOutputs                — returns constant
 5. getParamValue                — ad-hoc: dsp[index] load
 6. getSampleRate                — reads fSampleRate from struct
 7. init                         — calls classInit + instanceInit
 8. instanceClear                — zeros delay lines
 9. instanceConstants            — sample-rate-dependent constants
10. instanceInit                 — calls instanceConstants + instanceResetUI + instanceClear
11. instanceResetUserInterface   — resets UI controls to defaults
12. max_i                        — integer max (i32.lt_s + select)
13. min_i                        — integer min (i32.lt_s + select)
14. setParamValue                — ad-hoc: dsp[index] store
```

Each function body is prefixed by a U32LEB size (body length in bytes),
followed by the local variable declaration block, then the instruction
sequence, terminated by `end` (0x0B).

### 3.9 Local variable encoding

Each function body starts with a local variable declaration block:

```
num_groups: U32LEB
  for each group:
    count: U32LEB    — number of locals of this type
    type:  S32LEB    — value type (i32=0x7f, f32=0x7d, f64=0x7c)
```

Groups are emitted in this order: `i32` locals first, then `f32`, then `f64`.
Only non-empty groups are emitted. Function arguments are NOT included in
local declarations (they are implicit in WASM).

Local variable indices are assigned as:
1. Function arguments (indices 0, 1, 2, …)
2. i32 stack/loop locals
3. f32 stack/loop locals
4. f64 stack/loop locals

### 3.10 Memory layout — field offset computation

The DSP struct is laid out in linear memory starting at the address passed as
the `dsp` parameter (first argument of most functions). In **fast memory mode**
(internal memory, `fFastMemory == true`), `dsp` is assumed to be 0, and field
offsets are used directly as constant offsets in load/store instructions.

Field offsets are computed by `FunAndTypeCounter::visit(DeclareVarInst*)` and
`WASMInstVisitor::visit(DeclareVarInst*)`:

**Scalar fields:**
```
offset = fStructOffset
fStructOffset += audioSampleSize()   // always use biggest type size for alignment
```

**Array fields** (size > 1):
```
offset = fStructOffset
fStructOffset += arraySize * audioSampleSize()
```

Where `audioSampleSize()` = 4 for float, 8 for double. This means ALL fields
(even `i32` fields) are padded to the audio sample size for uniform alignment.
This is the critical alignment constraint:

> **Every field slot is `audioSampleSize` bytes wide**, regardless of actual
> type. An `i32` field in a float DSP occupies 4 bytes (coincidentally
> matching), but in a double DSP, an `i32` field still occupies 8 bytes
> (4 bytes used, 4 bytes padding).

### 3.11 Memory access instructions

All struct field accesses use a fixed alignment hint of **2** (i.e., 4-byte
alignment), regardless of the actual data type:

```cpp
void generateMemoryAccess(int offset = 0) {
    *fOut << U32LEB(2);       // alignment = 2 (means 2^2 = 4 bytes)
    *fOut << U32LEB(offset);  // byte offset
}
```

For load/store of struct fields:
- If `fFastMemory == true` and the offset is constant: emit `i32.const 0`
  as base address, use the field offset as the `offset` immediate.
- If `fFastMemory == false`: emit `local.get $dsp` + `i32.const <offset>` +
  `i32.add`, then load/store with offset 0.

### 3.12 Data section — JSON at offset 0

The data section contains exactly **1 data segment**:

```
data section:
  1 segment
  memory index = 0
  offset = i32.const 0; end   (initializer expression)
  data = <JSON string bytes>
```

The JSON string is written **raw** (not null-terminated in the binary, though
the JS runtime reads it as a string up to the length). The JSON is placed at
offset 0 in linear memory.

**Critical implication**: The DSP struct fields also start at offset 0 in
linear memory. The JSON is **overwritten** when the DSP is initialized — the
runtime must read and convert the JSON string **before** calling `init()` on
the DSP instance.

### 3.13 `compute` function signature

```wasm
(func $compute (param $dsp i32) (param $count i32)
                (param $inputs i32) (param $outputs i32))
```

- `$dsp` (local 0): base address of DSP struct in linear memory
- `$count` (local 1): number of frames to process
- `$inputs` (local 2): pointer to array of input buffer pointers (i32[])
- `$outputs` (local 3): pointer to array of output buffer pointers (i32[])

Input/output buffer access:
- `inputs[n]` = `i32.load(inputs + n*4)` → gives base address of buffer n
- `output[n]` = `i32.load(outputs + n*4)` → gives base address of buffer n
- Sample access: `f32.load(buffer_ptr + frame_index * audioSampleSize)`

The C++ backend has a `gLoopVarInBytes` optimization where the loop index
variable increments by bytes rather than frames, eliminating the
`frame_index * audioSampleSize` multiply in inner loop buffer access.

### 3.14 `setParamValue` / `getParamValue` — ad-hoc generation

These two functions are generated with **ad-hoc code** (not from FIR), because
FIR doesn't model index-based parameter access.

**`setParamValue(dsp, index, value)`**:
```wasm
local.get 0       ;; dsp base address
local.get 1       ;; index (byte offset from dsp base)
i32.add
local.get 2       ;; value (f32 or f64)
f32.store align=2 offset=0    ;; (or f64.store for double)
end
```

**`getParamValue(dsp, index)`**:
```wasm
local.get 0       ;; dsp base address
local.get 1       ;; index (byte offset from dsp base)
i32.add
f32.load align=2 offset=0     ;; (or f64.load for double)
return
end
```

The `index` parameter is a **byte offset**, not a field index. The JSON
metadata maps parameter paths to byte offsets using the `fFieldTable`.

### 3.15 `max_i` / `min_i` — integer helpers

These are always generated as module-internal functions:

**`max_i(a, b)`**:
```wasm
local.get 0   ;; a
local.get 1   ;; b
local.get 0   ;; a
local.get 1   ;; b
i32.lt_s
select        ;; returns b if a < b, else a
end
```

**`min_i(a, b)`**: same but with `i32.gt_s` instead of `i32.lt_s`.

### 3.16 Soundfile handling

When the DSP uses soundfiles (`AddSoundfileInst` present in UI):
- `fInternalMemory` is **forced to `false`** (external memory mode)
- Soundfile pointers are moved to the **beginning** of the DSP struct
- The Soundfile struct is flattened in memory:
  ```
  struct Soundfile {
      fBuffers:   i32   // pointer to float**/double** array
      fLength:    i32   // pointer to int array (length per part)
      fSR:        i32   // pointer to int array (sample rate per part)
      fOffset:    i32   // pointer to int array (offset per part)
      fChannels:  i32   // max channels
      fParts:     i32   // total number of parts
      fIsDouble:  i32   // 0 = float, 1 = double
  }
  ```
- These pointers are filled by JS code before DSP initialization

### 3.17 `wasm-i` vs `wasm-e` modes

- **`wasm-i`** (or just `wasm`): Internal memory mode. Memory is declared
  inside the WASM module and exported. This is the default for monophonic DSP.
- **`wasm-e`**: External memory mode. Memory is imported from `"env"`.
  Required for polyphonic DSP and any DSP with soundfiles.

### 3.18 Subcontainer inlining

The C++ backend **inlines subcontainers** (e.g., table generators, waveform
initializers) into `classInit` and `instanceConstants`. This means:
- Subcontainer fields are merged into the main DSP struct
- Subcontainer init code is inlined using `MoveVariablesInFront3`
- The struct size grows to accommodate inlined waveform data

The Rust backend must replicate this inlining behavior to produce identical
memory layouts.

### 3.19 Summary of hard parity requirements

| Constraint | Specification |
|-----------|---------------|
| Section order | Type → Import → Function → Memory → Export → Code → Data |
| Export names | Exactly 11 functions in alphabetical order + optional memory |
| Export order | `compute`, `getNumInputs`, `getNumOutputs`, `getParamValue`, `getSampleRate`, `init`, `instanceClear`, `instanceConstants`, `instanceInit`, `instanceResetUserInterface`, `setParamValue`, `memory` |
| Code section | 14 function bodies in alphabetical order (classInit…setParamValue) |
| Import module | Always `"env"` |
| Import base name | `"_" + mathFunctionName` (e.g. `_sinf`, `_cosf`) |
| Field alignment | Every field slot = `audioSampleSize` bytes (4 for float, 8 for double) |
| Memory access alignment | Always 2 (= 4-byte aligned) in alignment immediate |
| Data segment | 1 segment, JSON at offset 0, memory index 0 |
| Memory sizing | `genMemSize(struct_size, numInputs+numOutputs, json_len)` formula |
| Param access | `index` = byte offset from dsp base, not field ordinal |
| Soundfiles | Force external memory; soundfile pointers first in struct |

---

## 4. Rust Architecture

> The Rust architecture described below must respect all constraints from
> Section 3. The `wasm-encoder` API is used for binary emission, but the
> **section order, function ordering, field layout, and export list** must
> match the C++ output byte-for-byte where structurally meaningful.

### 3.1 Crate and module layout

```
crates/codegen/src/backends/wasm/
├── mod.rs              # Public API: generate_wasm_module(), WasmOptions, errors
├── encoder.rs          # WasmBinaryEncoder — low-level binary format writer
├── compiler.rs         # FirToWasmCompiler — FIR → WASM instruction lowering
├── layout.rs           # WasmMemoryLayout — DSP struct → linear memory mapping
├── sections.rs         # Module section builders (type, import, export, code…)
├── wat.rs              # Optional WAT text emitter (for debug/testing)
└── tests.rs            # Unit + integration tests
```

### 3.2 Key decision: `wasm-encoder` vs custom encoder

The C++ backend has a custom binary encoder (`wasm_binary.hh`). Two options:

| Approach | Pros | Cons |
|----------|------|------|
| **`wasm-encoder` crate** (wasmtime) | Battle-tested, maintained, correct by construction, handles LEB128/section encoding | Extra dependency, API may not map 1:1 to C++ patterns |
| **Custom encoder** | Direct port from C++, no external dependency, full control | Must re-implement LEB128, section framing, validation |

**Recommendation:** Use **`wasm-encoder`** from the wasmtime project.

Rationale:
- Eliminates an entire class of binary encoding bugs.
- Widely used and actively maintained.
- Keeps the Rust backend focused on FIR→WASM *semantics*, not binary plumbing.
- The `wasm-encoder` API maps well to the section-based emission model.
- Small, no-std-compatible crate with zero transitive dependencies.

### 3.3 Public API

```rust
// crates/codegen/src/backends/wasm/mod.rs

pub const BACKEND_NAME: &str = "wasm";

/// WASM backend compilation options.
#[derive(Clone, Debug, Default)]
pub struct WasmOptions {
    /// Emit f64 (double) instead of f32 (float) for FAUSTFLOAT.
    pub double_precision: bool,
    /// Also produce WAT text alongside the binary (for debug).
    pub emit_wat: bool,
    /// Internal memory size in WASM pages (64 KiB each). 0 = auto-size.
    pub memory_pages: u32,
    /// Enable internal memory (true) or import memory from host (false).
    pub internal_memory: bool,
}

/// Compiled WASM module output.
pub struct WasmModule {
    /// WASM binary (valid `.wasm` file).
    pub wasm_binary: Vec<u8>,
    /// Optional WAT text (if `emit_wat` was true).
    pub wat_text: Option<String>,
    /// JSON metadata (UI description, parameter paths, I/O count).
    pub dsp_json: String,
    /// Memory layout descriptor (for runtime/FFI integration).
    pub memory_layout: WasmMemoryLayout,
}

/// Stable error codes for the WASM backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WasmBackendErrorCode {
    UnsupportedModuleShape,
    MissingCompute,
    UnsupportedFirNode,
    EncodingFailure,
    MemoryLayoutOverflow,
}

pub fn generate_wasm_module(
    store: &FirStore,
    module: FirId,
    options: &WasmOptions,
) -> Result<WasmModule, WasmBackendError>;
```

### 3.4 Memory layout engine

```rust
// crates/codegen/src/backends/wasm/layout.rs

/// Maps FIR struct globals to WASM linear memory offsets.
pub struct WasmMemoryLayout {
    /// Byte offset for each struct field, keyed by FIR variable name.
    pub field_offsets: IndexMap<String, FieldLayout>,
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

pub struct FieldLayout {
    pub offset: u32,
    pub typ: WasmValType,
    pub size: u32,
}
```

This is derived from FIR `DeclareVar` nodes with `AccessType::Struct`, similar
to the `StructLayoutPlan` used by the Cranelift backend.

### 3.5 FIR → WASM instruction compiler

The compiler walks the FIR tree via `match_fir()` and emits WASM instructions.
WASM is a **stack machine**, so the compilation model is:

- **Value nodes** (expressions) push results onto the WASM operand stack.
- **Statement nodes** consume values from the stack and produce side effects.
- **Control flow** uses WASM structured control (`block`, `loop`, `if`, `br`).

```rust
// crates/codegen/src/backends/wasm/compiler.rs

pub struct FirToWasmCompiler<'a> {
    store: &'a FirStore,
    layout: &'a WasmMemoryLayout,
    options: &'a WasmOptions,
    /// Current function being built.
    func: wasm_encoder::Function,
    /// Local variable slots (FIR Stack/Loop vars → WASM local indices).
    locals: HashMap<String, u32>,
    /// Imported function index mapping (math intrinsics).
    imports: &'a ImportMap,
}
```

FIR node mapping to WASM instructions:

| FIR Node | WASM Instructions |
|----------|-------------------|
| `Int32 { value }` | `i32.const value` |
| `Float32 { value }` | `f32.const value` |
| `Float64 { value }` | `f64.const value` |
| `BinOp(Add, Int32, a, b)` | `compile(a); compile(b); i32.add` |
| `BinOp(Add, Float32, a, b)` | `compile(a); compile(b); f32.add` |
| `LoadVar(Struct, name, Int32)` | `i32.const offset; i32.load` |
| `LoadVar(Struct, name, Float32)` | `i32.const offset; f32.load` |
| `StoreVar(Struct, name, val)` | `i32.const offset; compile(val); i32/f32.store` |
| `LoadTable(name, idx)` | `compile(idx); i32.const table_base; i32.add; *.load` |
| `StoreTable(name, idx, val)` | similar with `*.store` |
| `Cast(Int32→Float32)` | `f32.convert_i32_s` |
| `Cast(Float32→Int32)` | `i32.trunc_f32_s` |
| `Select2(cond, then, else)` | `compile(then); compile(else); compile(cond); select` |
| `SimpleForLoop` | `block { loop { ... br_if ... br ... } }` |
| `If(cond, then, else)` | `compile(cond); if ... else ... end` |
| `FunCall(math_fn)` | `call $imported_fn_index` |
| `Block(stmts)` | compile each statement sequentially |

### 3.6 Exported DSP functions

The WASM module exports the canonical Faust DSP API:

| Export name | WASM signature | FIR source |
|-------------|---------------|------------|
| `getNumInputs` | `() → i32` | `Module.num_inputs` |
| `getNumOutputs` | `() → i32` | `Module.num_outputs` |
| `getSampleRate` | `(dsp: i32) → i32` | reads SR from struct |
| `init` | `(dsp: i32, sr: i32) → ()` | `staticInit` + `instanceInit` |
| `instanceInit` | `(dsp: i32, sr: i32) → ()` | `instanceConstants` + `instanceResetUserInterface` + `instanceClear` |
| `instanceConstants` | `(dsp: i32, sr: i32) → ()` | from FIR `init_block` |
| `instanceResetUserInterface` | `(dsp: i32) → ()` | from FIR `reset_ui_block` |
| `instanceClear` | `(dsp: i32) → ()` | from FIR `clear_block` |
| `compute` | `(dsp: i32, count: i32, inputs: i32, outputs: i32) → ()` | FIR `compute` function |
| `getParamValue` | `(dsp: i32, index: i32) → f32/f64` | parameter access |
| `setParamValue` | `(dsp: i32, index: i32, value: f32/f64) → ()` | parameter mutation |

### 3.7 Imported functions (math intrinsics)

WASM has limited built-in math (`abs`, `sqrt`, `ceil`, `floor`, `trunc`,
`nearest`, `min`, `max` are native). Other math functions must be imported
from the host environment:

```
env.exp, env.log, env.log10, env.pow, env.sin, env.cos, env.tan,
env.asin, env.acos, env.atan, env.atan2, env.sinh, env.cosh, env.tanh,
env.fmod, env.remainder, env.round
```

The compiler maintains an `ImportMap` that assigns indices to these imports
and emits the corresponding import section entries.

### 3.8 JSON metadata

The WASM backend must also produce a companion **JSON metadata** string
describing:
- DSP name, SHA key, compile options
- Number of inputs/outputs
- UI description tree (sliders, buttons, bargraphs, groups)
- Parameter paths and ranges
- Memory layout information (struct size, zones)

This JSON is consumed by the Faust JS runtime to build the web UI and
correctly allocate/configure the WASM instance.

---

## 5. Implementation Steps

### Step 1: Scaffold and infrastructure (1 day)

- [ ] Set up `crates/codegen/src/backends/wasm/` module structure.
- [ ] Define `WasmOptions`, `WasmBackendError`, `WasmBackendErrorCode`.
- [ ] Add `wasm-encoder` dependency to `codegen/Cargo.toml` (feature-gated under `backend-wasm`).
- [ ] Wire `generate_wasm_module()` stub into the compiler facade (`crates/compiler/src/lib.rs`):
  add `compile_source_to_wasm()` / `compile_file_to_wasm()`.
- [ ] Add `-lang wasm` CLI support in `crates/compiler/src/main.rs`.

### Step 2: Memory layout engine (1 day)

- [ ] Implement `WasmMemoryLayout` — walk FIR module `globals` and compute byte offsets.
- [ ] Handle int fields (i32), real fields (f32/f64 based on `double_precision`).
- [ ] Handle array fields (static tables: `DeclareTable`).
- [ ] Handle I/O buffer pointer zone.
- [ ] Compute total memory size and WASM page count.
- [ ] Unit tests: verify layout matches C++ output for reference DSP programs.

### Step 3: Module skeleton — sections without code bodies (1–2 days)

- [ ] Emit WASM module with correct sections using `wasm-encoder`:
  - Type section (function signatures for all DSP API functions + math imports).
  - Import section (math intrinsics from `"env"` module).
  - Function section (index-to-type mapping).
  - Memory section (linear memory declaration, `internal_memory` option).
  - Export section (all DSP API functions + memory).
  - Data section (initial memory: table data, constant values).
- [ ] Emit trivial function bodies (empty/no-op) for all exported functions.
- [ ] Validation: output passes `wasm-validate` (wabt) or `wasmparser` crate.

### Step 4: Simple DSP function bodies (1–2 days)

- [ ] Implement `FirToWasmCompiler` core — walk FIR via `match_fir()`.
- [ ] Lower `getNumInputs`, `getNumOutputs` (constant returns).
- [ ] Lower `getSampleRate` (struct load).
- [ ] Lower `instanceConstants` (struct stores of sample-rate-dependent values).
- [ ] Lower `instanceResetUserInterface` (struct stores of UI defaults).
- [ ] Lower `instanceClear` (zero-fill delay lines and state).
- [ ] Lower `init` (call `instanceConstants` + `instanceResetUserInterface` + `instanceClear`).
- [ ] Test: compile `process = 0;` and `process = _;` — validate module structure.

### Step 5: Compute body lowering (2–3 days)

This is the core of the backend — lowering the FIR `compute` function body.

- [ ] Value nodes: `Int32`, `Float32`, `Float64`, `Bool`.
- [ ] Arithmetic: `BinOp` (all FIR ops → WASM arithmetic instructions).
- [ ] Comparisons: `BinOp` comparison ops → WASM `i32.eq`, `f32.lt`, etc.
- [ ] Casts: `Cast` between int/float types.
- [ ] Memory access: `LoadVar(Struct)`, `StoreVar(Struct)` → WASM load/store at layout offset.
- [ ] Stack/loop locals: `DeclareVar(Stack/Loop)`, `LoadVar(Stack/Loop)`, `StoreVar(Stack/Loop)` → WASM locals.
- [ ] Table access: `LoadTable`, `StoreTable` → offset arithmetic + load/store.
- [ ] Math calls: `FunCall` for math intrinsics → `call $import_index`.
- [ ] Control flow: `If`, `Select2` → WASM `if/else/end`, `select`.
- [ ] Loops: `SimpleForLoop`, `ForLoop` → WASM `block/loop/br_if` pattern.
- [ ] Block: `Block` → sequential emission.
- [ ] Shift arrays: `ShiftArrayVar` → memory copy/shift pattern.
- [ ] Soundfile access: `LoadSoundfileBuffer`, `LoadSoundfileLength`, `LoadSoundfileRate`.

Test matrix:
- `process = _;` (passthrough)
- `process = + ~ _;` (integrator — tests recursion/delay)
- `process = os.osc(440);` (oscillator — tests math imports, tables)
- `process = fi.lowpass(2, 1000);` (filter — tests complex compute body)

### Step 6: JSON metadata and `setParamValue`/`getParamValue` (1 day)

- [ ] Walk FIR UI block to build JSON metadata tree.
- [ ] Emit `setParamValue` / `getParamValue` function bodies (index-based struct access).
- [ ] Produce complete `WasmModule` with `dsp_json`.
- [ ] Validate JSON matches C++ output for reference programs.

### Step 7: WAT text emitter (0.5 days)

- [ ] Implement optional WAT text emission (using `wasmprinter` crate or manual).
- [ ] Useful for debugging and differential testing against C++ `.wat` output.

### Step 8: Compiler integration and CLI (0.5 days)

- [ ] Wire `compile_source_to_wasm()` in `Compiler`.
- [ ] CLI: `-lang wasm` writes `.wasm` binary + `.json` metadata.
- [ ] CLI: `-lang wast` writes `.wat` text.
- [ ] Feature flag: `backend-wasm` in `codegen/Cargo.toml` and `compiler/Cargo.toml`.

### Step 9: Testing and validation (1–2 days)

- [ ] **Structural**: All generated modules pass `wasmparser::validate()`.
- [ ] **Functional**: Run generated WASM in `wasmtime` crate, feed audio samples,
  compare output with interpreter backend (bit-exact for integer DSPs, epsilon
  for float).
- [ ] **Differential**: Compare WASM binary output with C++ compiler output for
  the standard test corpus.
- [ ] **Regression**: Add WASM backend to CI test matrix.
- [ ] **Browser smoke test**: Load generated module in a minimal Web Audio
  AudioWorklet test page.

### Step 10 (optional): `wasm-ffi` crate (deferred)

Similar to `interp-ffi` and `cranelift-ffi`, a `wasm-ffi` crate could expose
a C API for loading/running WASM DSP factories from host applications. This is
lower priority since the primary WASM use case is browser deployment where the
JS runtime handles instantiation.

---

## 6. Dependencies

```toml
# codegen/Cargo.toml
[dependencies]
wasm-encoder = { version = "0.225", optional = true }  # WASM binary emission
wasmparser = { version = "0.225", optional = true }     # Validation (tests)

[dev-dependencies]
wasmtime = "29"        # Runtime execution for functional tests
wasmprinter = "0.225"  # WAT text for debug/differential tests

[features]
backend-wasm = ["wasm-encoder", "wasmparser"]
```

No LLVM, no native toolchain required — pure Rust dependencies.

---

## 7. Known Pitfalls

### 6.1 Memory model differences

WASM linear memory is byte-addressed from offset 0 with no protection. The
backend must ensure:
- Correct alignment for f32/f64 loads/stores (4/8-byte aligned offsets).
- No overlap between struct fields, tables, and I/O zones.
- Memory growth handling if DSP uses dynamic-size tables.

### 6.2 Math import signatures must match host

The imported math functions (sin, cos, exp, etc.) must have signatures matching
what the Faust JS runtime provides. Both `f32` and `f64` variants may be
needed depending on `double_precision`.

### 6.3 Endianness

WASM is always little-endian. The memory layout must match, which is natural
on most development hosts but must be explicit in the layout engine.

### 6.4 `FAUSTFLOAT` mapping

By convention in the C++ WASM backend:
- **Single precision** (default): `FAUSTFLOAT = f32`, audio buffers are `f32`.
- **Double precision**: `FAUSTFLOAT = f64`, audio buffers are `f64`.

The Rust backend must support both via `WasmOptions::double_precision`.

### 6.5 Compatibility with existing Faust JS runtime

The generated WASM module must be API-compatible with the existing
`faustwasm` / `faust2wasm` toolchain. This means:
- Same exported function names and signatures.
- Same memory layout conventions.
- Same JSON metadata format.

Testing against the existing JS runtime is the ultimate validation.

---

## 8. "Done" Criteria

- [ ] `generate_wasm_module()` produces valid WASM for the full Faust test corpus.
- [ ] Output passes `wasmparser::validate()`.
- [ ] Functional tests pass in `wasmtime` runtime (correct audio output).
- [ ] JSON metadata matches expected format.
- [ ] CLI `-lang wasm` produces `.wasm` + `.json` files.
- [ ] Feature flag `backend-wasm` compiles cleanly when enabled/disabled.
- [ ] Generated modules load and run in the Faust JS web runtime (manual browser test).
- [ ] Differential parity with C++ WASM output for reference programs.

---

## 9. Relation to Other Backends

| Aspect | C/C++ Backend | Interpreter | Cranelift | **WASM** |
|--------|--------------|-------------|-----------|----------|
| Output | Text source | FBC bytecode | Native JIT | WASM binary |
| Runtime | External compiler | FBC executor | In-process | WASM VM / browser |
| FIR consumption | `match_fir` | `match_fir` | `match_fir` | `match_fir` |
| Memory model | Struct fields | Int/real heaps | `StructLayoutPlan` | Linear memory |
| Compilation model | Text emission | Stack-based bytecode | Register-based IR | Stack-based bytecode |
| Primary use case | Native builds | Dynamic loading | Fast JIT | Web deployment |

The WASM backend is architecturally closest to the **interpreter backend**
(both emit stack-based bytecode), but targets an external VM rather than an
internal executor. The **memory layout** logic shares patterns with the
**Cranelift backend** (`StructLayoutPlan` → byte offsets).

---

## 10. Future Extensions (Post-v1)

- **WASI support**: Emit WASI-compatible modules for server-side / CLI usage.
- **SIMD**: Use WASM SIMD128 instructions for vectorized audio processing.
- **Threads**: Leverage WASM threads for multi-channel parallel processing.
- **Streaming compilation**: Support `WebAssembly.compileStreaming()` friendly output.
- **Poly DSP**: Multi-voice polyphonic DSP in a single WASM module.
- **AudioWorklet glue**: Optionally embed the AudioWorklet wrapper JS in the output.
