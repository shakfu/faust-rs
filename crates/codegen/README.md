# codegen

Backend code-generation from FIR (Faust Intermediate Representation).

Consumes a `FirStore` produced by the `transform` / `compiler` pipeline and
emits target-language source text, bytecode, or JIT-compiled machine code.
All backend option structs, typed errors, and signature-validation helpers
live here.

## Position in the pipeline

``` 
parser → boxes → eval → propagate → signals → transform → fir → [codegen]
                                                                → C source
                                                                → C++ source
                                                                → .fbc bytecode
                                                                → native C++ (AOT from .fbc)
                                                                → Cranelift JIT
                                                                → … (scaffolded)
```

## C++ provenance

| Rust path | C++ origin |
|---|---|
| `backends::c` | `compiler/generator/c/` |
| `backends::cpp` | `compiler/generator/cpp/` |
| `backends::interp` | `compiler/generator/interpreter/` |
| `backends::cranelift` | *(new — no C++ equivalent)* |
| Other backends | `compiler/generator/<backend>/` *(planned)* |

---

## Backend status

| Backend | Status | Entry point |
|---|---|---|
| `c` | ✅ Implemented | `generate_c_module` |
| `cpp` | ✅ Implemented | `generate_cpp_module` |
| `interp` | ✅ Implemented | `generate_interp_module` |
| `interp::fbc_to_cpp` | ✅ Implemented | `generate_cpp_from_fbc` |
| `cranelift` | 🔧 Bring-up | `generate_cranelift_module` |
| `cmajor` | 🗂 Scaffolded | — |
| `codebox` | 🗂 Scaffolded | — |
| `csharp` | 🗂 Scaffolded | — |
| `dlang` | 🗂 Scaffolded | — |
| `jax` | 🗂 Scaffolded | — |
| `jsfx` | 🗂 Scaffolded | — |
| `julia` | 🗂 Scaffolded | — |
| `llvm` | 🗂 Scaffolded | — |
| `rust` | 🗂 Scaffolded | — |
| `sdf3` | 🗂 Scaffolded | — |
| `vhdl` | 🗂 Scaffolded | — |
| `wasm` | 🗂 Scaffolded | — |

---

## Public API

### C backend — `backends::c`

Emits a C header with a `typedef struct` DSP state container and the full
Faust C-style functional API (`new*`, `delete*`, `init*`, `buildUserInterface*`,
`compute*`, `metadata*`).

```rust
use codegen::backends::c::{COptions, generate_c_module};

let opts = COptions {
    class_name: Some("mydsp".to_owned()),
    ..Default::default()
};
let c_source = generate_c_module(&store, root_id, &opts)?;
```

| Item | Description |
|---|---|
| `COptions` | `class_name`, `quad_type_name`, `fixed_type_name` |
| `generate_c_module` | `(&FirStore, FirId, &COptions) → Result<String, CodegenError>` |
| `CodegenError` | Codes `FRS-CGEN-C-0001..0003` |

---

### C++ backend — `backends::cpp`

Emits a C++ class (`class <name> : public dsp`) with the full Faust
object-oriented lifecycle.

```rust
use codegen::backends::cpp::{CppOptions, generate_cpp_module};

let opts = CppOptions {
    class_name: Some("MySynth".to_owned()),
    namespace: Some("faust".to_owned()),
    ..Default::default()
};
let cpp_source = generate_cpp_module(&store, root_id, &opts)?;
```

| Item | Description |
|---|---|
| `CppOptions` | `class_name`, `namespace`, `quad_type_name`, `fixed_type_name` |
| `generate_cpp_module` | `(&FirStore, FirId, &CppOptions) → Result<String, CodegenError>` |
| `CodegenError` | Codes `FRS-CGEN-CPP-0001..0003` |

---

### Interpreter backend — `backends::interp`

Compiles FIR to Faust Bytecode (FBC), runs it in a stack-machine interpreter,
and serializes/deserializes `.fbc` text files. Also includes an AOT C++
emitter (see below).

#### FIR → FBC pipeline

1. `FirToFbcCompiler<R>` — compiles each FIR function body into a shared
   `FbcBlockArena`.
2. `generate_interp_module` — maps the FIR DSP lifecycle functions into
   `FbcDspFactory` code blocks, splitting `compute` into `compute_block` and
   `compute_dsp_block` when possible.
3. `FbcDspFactory::optimize(level)` — runs peephole bytecode optimizer
   (levels 0–6; `MAX_OPT_LEVEL = 6`).
4. `write_fbc` / `read_fbc` — serialize/deserialize to/from `.fbc` text.
5. `FbcDspInstance` — in-process DSP runtime (`init`, `compute`).

```rust
use codegen::backends::interp::{InterpOptions, generate_interp_module, write_fbc};

let opts = InterpOptions { opt_level: 4, module_name: None };
let factory = generate_interp_module(&store, root_id, &opts)?;
let mut buf = Vec::new();
write_fbc(&factory, &mut buf)?;
```

#### Function-to-block mapping

| FIR function name | `FbcDspFactory` block slot |
|---|---|
| `"staticInit"` | `static_init_block` |
| `"instanceConstants"` | `init_block` |
| `"instanceResetUserInterface"` | `reset_ui_block` |
| `"instanceClear"` | `clear_block` |
| `"compute"` | `compute_dsp_block` or `compute_block` + `compute_dsp_block` |

#### Key re-exports

| Item | Description |
|---|---|
| `InterpOptions` | `opt_level: i32`, `module_name: Option<String>` |
| `generate_interp_module` | `(&FirStore, FirId, &InterpOptions) → Result<FbcDspFactory<f32>, CodegenError>` |
| `FbcDspFactory<R>` | Compiled bytecode program with lifecycle/data blocks |
| `FbcDspInstance` | Runtime DSP state; provides `init` and `compute` |
| `FbcBlockArena` | Arena of `FbcBlock`s indexed by `BlockId` |
| `FbcInstruction<R>` | Single FBC instruction (`opcode + offsets + branches`) |
| `FbcOpcode` | 294-variant enum of all interpreter opcodes |
| `FbcReal` | Trait for `f32`/`f64` dispatch |
| `write_fbc` / `read_fbc` | `.fbc` text serialization |
| `optimize_block` | Peephole optimizer |
| `MAX_OPT_LEVEL` | Maximum optimizer level (`6`) |
| `INTERP_FILE_VERSION` | Current `.fbc` format version |
| `FbcCppOptions` | Options for the AOT C++ generator |
| `generate_cpp_from_fbc` | AOT C++ generator entry point |

---

### AOT C++ generator — `backends::interp::fbc_to_cpp`

Reads an `FbcDspFactory<R>` (from `generate_interp_module` or `read_fbc`)
and emits a **self-contained C++ header** — no interpreter runtime dependency
at the output side.

The generator performs a single pass over each of the 6 code blocks,
maintaining a **virtual stack** of named C++ temporaries (`fRN` for reals,
`iIN` for integers). All 294 FBC opcodes are covered.

#### Control-flow translation

| FBC instruction | Generated C++ |
|---|---|
| `Loop(init, body)` | `{ <init>; while (true) { <body> } }` |
| `CondBranch` | `if (!<cond>) { break; }` |
| `If(b1, b2)` | `if (<cond>) { <b1> } else { <b2> }` |
| `SelectReal/Int(b1, b2)` | pre-declared merge variable + `if/else` |
| `Return` | end of block (no explicit `return` emitted) |

#### Generated class structure

```cpp
class MySynth final : public dsp {
    int   iVec[<int_heap_size>];
    float fVec[<real_heap_size>];
    int   fSampleRate;
public:
    void classInit(int sample_rate);             // static_init_block
    void instanceConstants(int sample_rate) override; // init_block
    void instanceResetUserInterface() override;       // reset_ui_block
    void instanceClear() override;                    // clear_block
    void instanceInit(int sample_rate) override;      // orchestrates the above
    void init(int sample_rate) override;
    void buildUserInterface(UI* ui_interface) override;
    void metadata(Meta* m) override;
    void compute(int count, FAUSTFLOAT** inputs, FAUSTFLOAT** outputs) override;
    dsp* clone() override;
    int  getNumInputs() override;
    int  getNumOutputs() override;
    int  getSampleRate() override;
};
```

#### Usage

```rust
use codegen::backends::interp::{
    FbcCppOptions, generate_cpp_from_fbc, read_fbc,
};
use std::io::BufReader;

// From a .fbc file:
let text = std::fs::read_to_string("my.fbc")?;
let factory = read_fbc::<f32>(&mut BufReader::new(text.as_bytes()))?;

let opts = FbcCppOptions {
    class_name: Some("MySynth".to_owned()),
    pragma_once: true,
    namespace: Some("faust".to_owned()),
};
let header = generate_cpp_from_fbc(&factory, &opts)?;
std::fs::write("my.h", header)?;
```

Or directly from the CLI:

```sh
# Step 1 — compile .dsp to .fbc
cargo run -p compiler -- --lang interp my.dsp -o my.fbc

# Step 2 — emit native C++ from .fbc
cargo run -p compiler -- --dump-cpp-from-fbc my.fbc -o my.h
```

| Item | Description |
|---|---|
| `FbcCppOptions` | `class_name`, `pragma_once`, `namespace` |
| `FbcCppError` | `MissingBranchTarget`, `InvalidBlockId`, `Unsupported` |
| `generate_cpp_from_fbc` | `(&FbcDspFactory<R>, &FbcCppOptions) → Result<String, FbcCppError>` |

---

### Cranelift backend — `backends::cranelift`

JIT-compiles a FIR module to native machine code via Cranelift. Prioritizes
compile-path coverage and diagnosability; falls back to a no-op `compute` stub
for FIR nodes outside the current lowering subset.

```rust
use codegen::backends::cranelift::{CraneliftOptions, generate_cranelift_module};

let opts = CraneliftOptions::default(); // opt_level: Speed
let jit = generate_cranelift_module(&store, root_id, &opts)?;
```

| Item | Description |
|---|---|
| `CraneliftOptions` | `opt_level`, `target_triple`, `enable_nan_canonicalization`, `fail_on_subset_gap` |
| `CraneliftOptLevel` | `None`, `Speed` (default), `SpeedAndSize` |
| `generate_cranelift_module` | Main entry point; returns compiled JIT module |
| `diagnose_cranelift_compute_subset_gap` | Reports unsupported FIR nodes |

---

### Fixtures — `fixtures`

Shared FIR modules for backend-agnostic parity testing. All backends are
exercised against the same 8 canonical fixtures, preventing test drift.

```rust
use codegen::fixtures::backend_test_fixtures;

for (name, build) in backend_test_fixtures() {
    let (store, root) = build();
    // run backend against (store, root) …
}
```

| Fixture name | What it covers |
|---|---|
| `"sine_phasor"` | Phasor-driven sine oscillator, UI controls, persistent state |
| `"heavy_bench"` | Stress test for backend coverage |
| `"passthrough"` | Minimal audio pass-through |
| `"gain_bias_ui_meta"` | Gain/bias with UI and `metadata` |
| `"table_state_delay"` | Table initialization and stateful delay |
| `"control_flow"` | Conditional branching and loops |
| `"math_intrinsics"` | Mathematical function coverage |
| `"ir_coverage"` | Low-level FIR node coverage |

---

## Scaffolded backends

The following backends expose a stable `backend_id()` identifier and are
otherwise empty. They reserve a place in the roadmap and prevent accidental
namespace collisions as parity work proceeds.

`cmajor` · `codebox` · `csharp` · `dlang` · `jax` · `jsfx` · `julia` ·
`llvm` · `rust` · `sdf3` · `vhdl` · `wasm`
