# Phase 7 — Additional Backends

> **Crates**: `codegen` (backend modules under `codegen::backends::*`)
> **Estimate**: 53–64 person days (total, highly parallelizable)
> **Prerequisites**: Phase 6 (`fir`, `codegen`)

---

## 1. C++ Inventory

### 1.1 "Simple" text backends (~3–5 files each)

| Backend | Directory | Lines | Files | Priority |
|---------|-----------|--------|----------|----------|
| Rust | `generator/rust/` | 2,191 | 5 | ★★★ |
| Java (**out of scope**) | `generator/java/` | 926 | 3 | N/A |
| Julia | `generator/julia/` | 1,188 | 3 | ★★ |
| C# | `generator/csharp/` | 958 | 3 | ★★ |
| D (Dlang) | `generator/dlang/` | 1,200 | 3 | ★ |
| Cmajor | `generator/cmajor/` | 1,299 | 3 | ★★ |
| Codebox | `generator/codebox/` | 1,009 | 3 | ★ |
| JSFX | `generator/jsfx/` | 1,833 | 3 | ★ |
| JAX | `generator/jax/` | 1,206 | 3 | ★★ |
| VHDL | `generator/vhdl/` | 1,869 | 4 | ★ |
| SDF3 | `generator/sdf3/` | 1,266 | 4 | ★ |

### 1.2 Complex backends

| Backend | Directory | Lines | Files | Priority |
|---------|-----------|--------|----------|----------|
| **Wasm** | `generator/wasm/` | 5,521 | 16 | ★★★ |
| **Interpret** | `generator/interpreter/` | 17,623 | 21 | ★★★ |
| **LLVM** | `generator/llvm/` | 5,072 | 9 | ★★ |

### 1.3 Rust specific backend

| File | Lines | Role |
|---------|--------|------|
| `rust_instructions.hh` | ~600 | `RustInstVisitor`: visitor emitting Rust |
| `rust_code_container.hh/.cpp` | ~1,000 | `RustCodeContainer`: assembly of the complete Rust |
| `dag_instructions_compiler_rust.hh/.cpp` | ~600 | Specialized Rust DAG compiler |

---

## 2. Rust Architecture

### 2.1 Common structure

Each text backend follows the same pattern:

```rust
// codegen/src/backends/<lang>/mod.rs

pub struct <Lang>TypeManager;
impl TypeManager for <Lang>TypeManager { /* ... */ }

pub struct <Lang>Codegen {
    type_manager: <Lang>TypeManager,
    config: BackendConfig,
}

impl TextCodegen for <Lang>Codegen {
    fn emit_type(&self, typ: &FirType) -> String { /* ... */ }
    fn emit_value(&self, val: &FirValue) -> String { /* ... */ }
    fn emit_stmt(&self, stmt: &FirStmt, indent: usize) -> String { /* ... */ }
}

pub fn generate(
    container: &CodeContainer,
    config: &BackendConfig,
    output: &mut dyn Write,
) -> io::Result<()>;
```

Backends live in `codegen` and are toggled through `codegen` feature flags re-exported by the main crate:

```toml
# compiler/Cargo.toml
[features]
default = ["backend-c", "backend-cpp", "backend-wasm"]
backend-c      = ["codegen/backend-c"]
backend-cpp    = ["codegen/backend-cpp"]
backend-rust   = ["codegen/backend-rust"]
backend-wasm   = ["codegen/backend-wasm"]
backend-interp = ["codegen/backend-interp"]
backend-llvm   = ["codegen/backend-llvm"]
backend-julia  = ["codegen/backend-julia"]
backend-csharp = ["codegen/backend-csharp"]
backend-cmajor = ["codegen/backend-cmajor"]
backend-jsfx   = ["codegen/backend-jsfx"]
backend-jax    = ["codegen/backend-jax"]
backend-dlang  = ["codegen/backend-dlang"]
backend-codebox= ["codegen/backend-codebox"]
backend-vhdl   = ["codegen/backend-vhdl"]
all-backends   = ["backend-c", "backend-cpp", "backend-rust", "backend-wasm", ...]
```

### 2.2 Wasm Backend

The Wasm backend is complex because it emits **Wasm binary** directly (no text):

```rust
/// Wasm binary emission
pub struct WasmBinaryEncoder {
    buffer: Vec<u8>,
    functions: Vec<WasmFunction>,
    memory: WasmMemory,
    imports: Vec<WasmImport>,
    exports: Vec<WasmExport>,
}

impl WasmBinaryEncoder {
    pub fn emit_module(&mut self, container: &CodeContainer) -> Vec<u8>;
    pub fn emit_section_type(&mut self);
    pub fn emit_section_function(&mut self);
    pub fn emit_section_memory(&mut self);
    pub fn emit_section_code(&mut self);
}

/// Wast text emission (for debug)
pub struct WastCodegen;
impl TextCodegen for WastCodegen { /* ... */ }

/// Complete Wasm backend
pub fn generate_wasm(
    container: &CodeContainer,
    config: &WasmConfig,
    output: &mut dyn Write,
) -> io::Result<()>;

pub fn generate_wast(
    container: &CodeContainer,
    config: &WasmConfig,
    output: &mut dyn Write,
) -> io::Result<()>;
```

Note: In native Rust, we could also use the `wasm-encoder` crate (from the wasmtime project) instead of the custom encoder. To be evaluated.

### 2.3 Backend Interpreter

The interpreter backend is the largest (17,623 lines). It generates **FBC bytecode** (Faust Byte Code) executable in interpretive mode:

```rust
/// FBC opcode
#[derive(Clone, Copy, Debug)]
pub enum FbcOpcode {
    // Arithmetic
    RealAdd, RealSub, RealMul, RealDiv,
    IntAdd, IntSub, IntMul, IntDiv,
    // Memory
    LoadInt, StoreInt, LoadReal, StoreReal,
    LoadArrayInt, StoreArrayInt,
    // Control
    Goto, IfGoto, Loop,
    // Functions
    CallFun,
    // ... ~100 opcodes
}

/// Compiled bytecode program
pub struct FbcProgram {
    pub opcodes: Vec<FbcInstruction>,
    pub int_heap: Vec<i32>,
    pub real_heap: Vec<f64>,
    pub inputs: usize,
    pub outputs: usize,
}

/// Interpreter
pub struct FbcInterpreter {
    program: FbcProgram,
    int_stack: Vec<i32>,
    real_stack: Vec<f64>,
}

impl FbcInterpreter {
    pub fn compute(&mut self, count: usize, inputs: &[&[f32]], outputs: &mut [&mut [f32]]);
}

/// FIR → bytecode compilation
pub fn compile_to_bytecode(
    container: &CodeContainer,
) -> Result<FbcProgram, FaustError>;
```

C++ also includes an FBC→native machine compiler (via LLVM or MIR). In Rust, JIT is optional — the pure interpreter is sufficient as a first approximation.

### 2.4 LLVM Backend

```rust
/// LLVM backend (feature-gated, requires llvm-sys)
#[cfg(feature = "codegen/backend-llvm")]
pub mod llvm {
    use inkwell::*;  // or llvm-sys

    pub struct LlvmCodegen<'ctx> {
        context: &'ctx Context,
        module: Module<'ctx>,
        builder: Builder<'ctx>,
    }

    impl<'ctx> LlvmCodegen<'ctx> {
        pub fn compile(container: &CodeContainer) -> Result<Module<'ctx>, FaustError>;
    }

    pub fn generate_llvm_ir(
        container: &CodeContainer,
        output: &mut dyn Write,
    ) -> io::Result<()>;

    pub fn generate_native(
        container: &CodeContainer,
        target: &TargetTriple,
    ) -> Result<Vec<u8>, FaustError>;
}
```

Dependency: `inkwell` (LLVM safe bindings) or `llvm-sys` (raw bindings). Optional feature because LLVM is cumbersome to compile.

### 2.5 Rust Backend (the backend that emits itself!)

```rust
pub struct RustTypeManager;
impl TypeManager for RustTypeManager {
    fn int32_type(&self) -> &str { "i32" }
    fn float_type(&self) -> &str { "f32" }
    fn double_type(&self) -> &str { "f64" }
    // ...
}

pub struct RustCodegen {
    type_manager: RustTypeManager,
}

impl TextCodegen for RustCodegen { /* ... */ }

pub fn generate_rust(
    container: &CodeContainer,
    config: &BackendConfig,
    output: &mut dyn Write,
) -> io::Result<()>;
```

Irony: the Faust compiler in Rust will have a backend that generates Rust!

---

## 3. Recommended order of development

1. **Backend Rust** (2,191 LOC) — Most useful for testing the Rust compiler
2. **Backend Wasm** (5,521 LOC) — Essential for Faust web applications
3. **Backend Interpreter** (17,623 LOC) — Essential for libfaust
4. **Simple text backends** (Julia, C#, etc.; Java excluded) — Parallelizable, low risk
5. **Backend LLVM** (5,072 LOC) — Last because it requires heavy dependencies

Simple text backends are **highly parallelizable**: one person per backend, each following the same template.

---

## 4. Dependencies

```
codegen::backends::rust    → codegen, fir
codegen::backends::julia   → codegen, fir
codegen::backends::csharp  → codegen, fir
codegen::backends::dlang   → codegen, fir
codegen::backends::cmajor  → codegen, fir
codegen::backends::codebox → codegen, fir
codegen::backends::jsfx    → codegen, fir
codegen::backends::jax     → codegen, fir
codegen::backends::vhdl    → codegen, fir
codegen::backends::wasm    → codegen, fir
codegen::backends::interp  → codegen, fir
codegen::backends::llvm    → codegen, fir, inkwell (or llvm-sys)
```

External dependencies:
- `wasm-encoder` (optional, for the Wasm backend)
- `inkwell` / `llvm-sys` (optional, feature-gated, for LLVM)
- `serde_json` (for all backends, already required in Phase 6)

Scope note: `backend-java` is intentionally excluded from the Rust port target scope.

---

## 5. Known pitfalls

### 5.1 Interpreter: huge API surface area
The C++ interpreter is also a complete runtime (`interpreter_dsp_aux`, `interpreter_dynamic_dsp_aux`). In Rust, you have to decide: carry the complete runtime or only the bytecode compiler?

→ Recommendation: port the bytecode compiler + minimal interpreter first. The `dsp_aux` runtime (dynamic loading, etc.) will come in Phase 9.

### 5.2 Wasm: custom binary encoding
C++ has its own Wasm binary encoder (`wasm_binary.hh`). In Rust, evaluate if wasmtime's `wasm-encoder` crate can replace it — that would simplify a lot.

### 5.3 LLVM: pinning version
The LLVM backend depends on a specific version of LLVM. In Rust, `inkwell` manages this via features (`llvm17-0`, `llvm18-0`, etc.).

### 5.4 Rust backend: specifics
The Rust backend has a specialized DAG compiler (`dag_instructions_compiler_rust`) which differs from the standard version. We need to understand why and if it is still necessary.

### 5.5 SDF3 and VHDL are special cases
These backends don't go through `CodeContainer` in the same way — they have their own build pipeline. They can be worn last.

---

## 6. Testing

### By backend:
- **Compilation**: The generated code compiles into the target language
- **Functional**: A simple DSP (`+ ~ _`) produces the correct audio result
- **Differential**: Compare with C++ compiler output (bit-exact for text backends)
- **Round-trip Wasm**: The Wasm produced works in a Wasm runtime (wasmtime or browser)

### Cross-sectional tests:
- All backends produce the same JSON
- Standard Faust examples compile with each backend

---

## 7. "Done" criteria

- [ ] Rust backend: code compilable with `rustc`
- [ ] Backend Wasm: valid Wasm module (verified by `wasm-validate`)
- [ ] Backend Interpreter: executable bytecode, correct results
- [ ] Each text backend: code compilable in its target language
- [ ] LLVM backend: Valid LLVM IR (verified by `llvm-as`)
- [ ] Functional feature flags (compilation without unselected backends)
- [ ] Java backend intentionally excluded (not required for Phase 7 completion)

---

## 8. Detailed Effort

| Backend | LOC C++ | Estimated LOC Rust | Days | Parallelizable |
|---------|---------|-----------------|-------|----------------|
| Rust | 2,191 | 1,500 | 4–5 | Yes |
| Wasm (binary + wast) | 5,521 | 3,500 | 8–10 | No |
| Interpret (bytecode + interp) | 17,623 | 10,000 | 15–20 | No |
| LLVM | 5,072 | 3,000 | 8–10 | Yes |
| Julia | 1,188 | 800 | 2 | Yes |
| C# | 958 | 600 | 2 | Yes |
| Cmajor | 1,299 | 800 | 2–3 | Yes |
| JAX | 1,206 | 800 | 2–3 | Yes |
| D | 1,200 | 800 | 2 | Yes |
| Codebox | 1,009 | 700 | 2 | Yes |
| JSFX | 1,833 | 1,200 | 3 | Yes |
| VHDL | 1,869 | 1,200 | 3 | Yes |
| SDF3 | 1,266 | 800 | 2 | Yes |
| **Total Phase 7** (excluding Java) | **42,235** | **24,700** | **53–64** |  |

**With parallelization** (3 developers): simple text backends are done in parallel → effective duration **25–35 days**.
