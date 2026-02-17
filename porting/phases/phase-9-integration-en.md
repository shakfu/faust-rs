# Phase 9 — Final integration

> **Crates**: `compiler` (binary + lib), `cffi` (C/C++ API)
> **Estimate**: 35–55 person days
> **Prerequisites**: Phases 1–8

---

## 1. C++ Inventory

### 1.1 Top-level files — 9,097 lines

| File | Lines | Role |
|---------|--------|------|
| `main.cpp` | 74 | CLI entry point (`main()`) |
| `global.hh` | 916 | **Declaration from `global`**: ~408 fields (config, tables, counters, status) |
| `global.cpp` | 3,136 | **Implementation**: `processCmdline()`, `initDirectories()`, `parseSourceFiles()`, `reset()`, etc. |
| `libcode.cpp` | 1,541 | **Main orchestrator**: `evaluateBlockDiagram()`, `generateCode()`, `generateOutputFiles()`, and the ~20 functions `compile<Lang>()` |
| `box_signal_api.cpp` | 3,085 | **Public API**: very large C/C++ surface for libfaust (`453` `LIBFAUST_API` declarations) including `DSPToBoxes()`, `boxesToSignals()`, `createCDSPFactoryFromBoxes()`, `createCDSPFactoryFromSignals()`, and hundreds of fine-grained box/signal helpers |
| `dsp_factory.hh` | ~100 | DSP factory interface |
| `garbageable.hh` | ~90 | Base class `Garbageable` (removed in Rust) |
| `lock_api.hh/.cpp` | ~150 | Global mutex for libfaust thread safety |

### 1.2 generator/dsp_aux.hh/.cpp — ~600 lines

Auxiliary DSP runtime: `dsp_factory_base`, dynamic DSP loading.

### 1.3 generator/export.cpp — ~200 lines

Export of libfaust functions.

---

## 2. Rust Architecture

### 2.1 CompilerConfig (replaces gGlobal — config part)

```rust
/// Immutable compiler configuration (replaces ~200 config fields from gGlobal)
#[derive(Clone, Debug)]
pub struct CompilerConfig {
    // Langage cible
    pub output_lang: OutputLang,
    pub float_size: FloatSize,

    // Options de compilation
    pub vectorize: bool,
    pub vector_size: usize,
    pub openmp: bool,
    pub scheduler: bool,
    pub group_tasks: bool,

    // Optimisations
    pub opt_level: u8,           // 0–3
    pub inline_threshold: usize,
    pub math_approximation: bool,

    // Multi-rate
    pub on_demand: bool,

    // Sorties auxiliaires
    pub draw_svg: bool,
    pub draw_ps: bool,
    pub generate_doc: bool,
    pub print_xml: bool,
    pub print_json: bool,
    pub task_graph: bool,

    // Chemins
    pub input_files: Vec<PathBuf>,
    pub output_file: Option<PathBuf>,
    pub architecture_file: Option<PathBuf>,
    pub library_paths: Vec<PathBuf>,
    pub include_paths: Vec<PathBuf>,

    // Metadata
    pub class_name: String,
    pub super_class_name: String,
    pub process_name: String,

    // Debug
    pub details: bool,
    pub trace_mode: u8,
    pub timing: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutputLang {
    C, Cpp, Rust, Julia, CSharp, Dlang,
    Cmajor, Codebox, Jsfx, Jax,
    Wasm, Wast, Llvm, Interp, Fir,
    Vhdl, Sdf3,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FloatSize { Single, Double, Quad }

impl CompilerConfig {
    pub fn from_args(args: &[&str]) -> Result<Self, FaustError>;
    pub fn default() -> Self;
}
```

Scope note: old C++ backend mode `-lang ocpp` is intentionally excluded from the Rust port target scope.

### 2.2 CompileSession (replaces gGlobal — mutable state part)

```rust
/// Compilation session: full state of one compilation
/// Each compilation creates an independent session
pub struct CompileSession {
    pub config: Arc<CompilerConfig>,
    pub arena: TreeArena,
    pub diagnostics: DiagnosticCollector,
    pub timer: PassTimer,
    pub name_gen: NameGenerator,
    pub source_reader: SourceReader,
}

impl CompileSession {
    pub fn new(config: CompilerConfig) -> Self;

    /// Full pipeline: source → generated code
    pub fn compile(&mut self) -> Result<CompileResult, FaustError>;

    /// Sub-steps exposed individually (for the API)
    pub fn parse(&mut self) -> Result<TreeId, FaustError>;
    pub fn evaluate(&mut self, defs: TreeId) -> Result<(TreeId, usize, usize), FaustError>;
    pub fn propagate(&mut self, process: TreeId, n_in: usize) -> Result<Vec<TreeId>, FaustError>;
    pub fn normalize(&mut self, signals: Vec<TreeId>) -> Result<Vec<TreeId>, FaustError>;
    pub fn generate_fir(&mut self, signals: &[TreeId], n_in: usize, n_out: usize) -> Result<CodeContainer, FaustError>;
    pub fn generate_code(&self, container: &CodeContainer, output: &mut dyn Write) -> Result<(), FaustError>;
}

pub struct CompileResult {
    pub code: String,
    pub json: String,
    pub num_inputs: usize,
    pub num_outputs: usize,
    pub sha_key: String,
}
```

### 2.3 Main pipeline

```rust
impl CompileSession {
    pub fn compile(&mut self) -> Result<CompileResult, FaustError> {
        // 1. Parse
        self.timer.start("parsing");
        let defs = self.parse()?;
        self.timer.stop("parsing");

        // 2. Evaluate (boxes → process box)
        self.timer.start("evaluation");
        let (process, n_in, n_out) = self.evaluate(defs)?;
        self.timer.stop("evaluation");

        // 3. Draw (optional)
        if self.config.draw_svg {
            faust_draw::draw_schema(&self.arena, process, &svg_path, DrawFormat::Svg)?;
        }

        // 4. Propagate (boxes → signals)
        self.timer.start("propagation");
        let signals = self.propagate(process, n_in)?;
        self.timer.stop("propagation");

        // 5. Normalize + Transform
        self.timer.start("normalization");
        let signals = self.normalize(signals)?;
        self.timer.stop("normalization");

        // 6. Generate FIR
        self.timer.start("fir_generation");
        let container = self.generate_fir(&signals, n_in, n_out)?;
        self.timer.stop("fir_generation");

        // 7. Generate target code
        self.timer.start("code_generation");
        let mut output = Vec::new();
        self.generate_code(&container, &mut output)?;
        self.timer.stop("code_generation");

        // 8. Timing report
        if self.config.timing {
            eprintln!("{}", self.timer.report());
        }

        Ok(CompileResult {
            code: String::from_utf8(output)?,
            json: container.json.to_string(),
            num_inputs: n_in,
            num_outputs: n_out,
            sha_key: compute_sha_key(&self.config.input_files)?,
        })
    }
}
```

### 2.4 CLI binary (main.rs)

```rust
// compiler/src/main.rs
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let args_str: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    let config = CompilerConfig::from_args(&args_str[1..])?;
    let mut session = CompileSession::new(config);
    let result = session.compile()?;

    // Write output
    if let Some(ref output_path) = session.config.output_file {
        std::fs::write(output_path, &result.code)?;
    } else {
        print!("{}", result.code);
    }

    Ok(())
}
```

### 2.5 cffi — C/C++ API

```rust
// cffi/src/lib.rs
// C API exposed via cbindgen

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

/// Compile a DSP string into target code
#[no_mangle]
pub extern "C" fn createDSPFactoryFromString(
    name_app: *const c_char,
    dsp_content: *const c_char,
    argc: c_int,
    argv: *const *const c_char,
    target: *const c_char,
    error_msg: *mut *mut c_char,
) -> *mut DspFactory {
    // ... C→Rust conversion, CompileSession call, return
}

/// API Box
#[no_mangle]
pub extern "C" fn DSPToBoxes(
    name_app: *const c_char,
    dsp_content: *const c_char,
    argc: c_int,
    argv: *const *const c_char,
    inputs: *mut c_int,
    outputs: *mut c_int,
    error_msg: *mut *mut c_char,
) -> *mut CTree { /* ... */ }

/// API Signal
#[no_mangle]
pub extern "C" fn boxesToSignals(
    box_: *mut CTree,
    error_msg: *mut *mut c_char,
) -> *mut CTree { /* ... */ }

// ... many additional C functions (hundreds on the current branch)
```

For generating C/C++ headers:
- `cbindgen` for the C header
- `cxx` (optional) for an ergonomic C++ API

### 2.6 API migration strategy (recommended)

Given the observed API size, migrate in tiers:

1. **Tier 1 (must-have)**: factory lifecycle + compile-from-string/file + `DSPToBoxes`/`boxesToSignals` used by major tools.
2. **Tier 2 (high-value)**: cache/introspection helpers and frequently used signal/box constructors.
3. **Tier 3 (full parity)**: long tail of fine-grained `Cbox*` / `Csig*` exports.

This tiering avoids blocking CLI/compiler parity on full API completion.

API status policy for migration tracking:
- `1:1`: exported contract kept compatible with legacy API semantics/signature expectations.
- `adapted`: Rust-internal API shape changed (ownership/types/context modeling) with preserved documented behavior.
- `deferred`: not yet migrated (owner + milestone required).

For each touched API surface, keep a mapping record with:
- C++ symbol + file reference,
- Rust symbol/module,
- status (`1:1` / `adapted` / `deferred`),
- compatibility impact and required wrapper/shim notes,
- test coverage (unit/integration/differential) proving expected behavior.

### 2.7 Integration into the Faust repository

```
faust/
├── compiler/          ← REPLACED by the Rust workspace
│   ├── Cargo.toml     (workspace)
│   ├── crates/
│   │   ├── tlib/
│   │   ├── boxes/
│   │   ├── parser/
│   │   ├── signals/
│   │   ├── eval/
│   │   ├── propagate/
│   │   ├── normalize/
│   │   ├── transform/
│   │   ├── interval/
│   │   ├── algebra/
│   │   ├── graph/
│   │   ├── errors/
│   │   ├── utils/
│   │   ├── fir/
│   │   ├── codegen/
│   │   │   └── src/backends/
│   │   │       ├── c/
│   │   │       ├── cpp/
│   │   │       ├── rust/
│   │   │       ├── wasm/
│   │   │       ├── interp/
│   │   │       ├── llvm/
│   │   │       └── .../
│   │   ├── draw/
│   │   ├── doc/
│   │   ├── compiler/     (bin + lib)
│   │   └── cffi/         (API C/C++)
│   ├── docs/
│   │   ├── faust-rust-porting-plan.md
│   │   ├── phases/
│   │   └── JOURNAL.md
│   └── tests/
│       ├── integration/
│       └── differential/
├── Makefile           ← adapted to call `cargo build`
├── architecture/      (unchanged)
├── libraries/         (unchanged)
├── examples/          (unchanged)
└── ...
```

Adaptation of the Makefile:
```makefile
compiler:
	cd compiler && cargo build --release
	cp compiler/target/release/faust $(prefix)/bin/

libfaust:
	cd compiler && cargo build --release --lib
	cp compiler/target/release/libfaust.so $(prefix)/lib/

install: compiler libfaust
```

---

## 3. Parallelization of compilations

```rust
/// Parallel compilation of multiple DSP files
pub fn compile_batch(
    configs: Vec<CompilerConfig>,
) -> Vec<Result<CompileResult, FaustError>> {
    configs.into_par_iter()  // rayon
        .map(|config| {
            let mut session = CompileSession::new(config);
            session.compile()
        })
        .collect()
}

/// Multi-backend compilation (one DSP → multiple languages)
pub fn compile_multi_target(
    config: CompilerConfig,
    targets: &[OutputLang],
) -> Vec<Result<CompileResult, FaustError>> {
    let mut session = CompileSession::new(config);

    // Pipeline commun (parse → eval → propagate → normalize → FIR)
    let defs = session.parse().unwrap();
    let (process, n_in, n_out) = session.evaluate(defs).unwrap();
    let signals = session.propagate(process, n_in).unwrap();
    let signals = session.normalize(signals).unwrap();
    let container = session.generate_fir(&signals, n_in, n_out).unwrap();
    let container = Arc::new(container);  // shared read-only

    // Backends in parallel
    targets.par_iter()
        .map(|&target| {
            let mut output = Vec::new();
            let mut cfg = (*session.config).clone();
            cfg.output_lang = target;
            generate_code_for_lang(target, &container, &cfg, &mut output)?;
            Ok(CompileResult { code: String::from_utf8(output)?, /* ... */ })
        })
        .collect()
}
```

---

## 4. Dependencies

```
compiler → all previous crates
cffi     → compiler, tlib, boxes, signals
```

Additional external dependencies:
- `clap`: default parser for CLI arguments (fallback alternatives only with documented justification)
- `rayon`: parallelization (compile_batch, multi-target)
- `cbindgen`: generation of the C header (build.rs)
- `sha1`: calculation of SHA for cache keys
- `serde` + `serde_json`: JSON serialization

---

## 5. Known pitfalls

### 5.1 gGlobal has ~408 fields
The biggest challenge is to decompose `global` into targeted structures. Categorization:
- **Config (~150)** → `CompilerConfig`
- **Cache tables (~80)** → `TreeProperty<T>` per pass
- **Counters (~30)** → `DiagnosticCollector`, `NameGenerator`
- **Pre-recorded symbols (~100)** → in `TreeArena.init_symbols()`
- **Parsing state (~20)** → in `CompileSession`
- **I/O Status (~30)** → in `CompilerConfig` + `SourceReader`

### 5.2 Thread safety of libfaust
In C++, `lock_api.cpp` uses a global mutex. In Rust, each `CompileSession` is independent → no need for global mutex. Thread-safety by construction.

### 5.3 API C and ownership
The C API exposes `CTree*` pointers. Strategy: return an opaque pointer to the session, with functions to query/manipulate trees via `TreeId`.

### 5.4 CLI compatibility
The `faust` Rust binary must accept exactly the same CLI options as C++. Exhaustive differential test:
```bash
for f in examples/*.dsp; do
    faust-cpp -lang c "$f" > output-cpp.c
    faust-rust -lang c "$f" > output-rust.c
    diff output-cpp.c output-rust.c
done
```

### 5.5 WebAssembly of the compiler itself
Rust advantage: compile the compiler in Wasm via `cargo build --target wasm32-unknown-unknown`, replacing the current Emscripten version.

### 5.6 Fixed-size `argv` staging hazards in legacy API paths
Some C++ entry paths stage CLI arguments in fixed-size temporary arrays. Rust integration should always normalize arguments into dynamic validated vectors, with explicit error handling for oversized input.

### 5.7 Stack-size thread trampoline in legacy orchestration
The C++ flow uses thread trampolines with custom stack sizes (`callFun`) to protect deep recursion. Rust should keep stack usage explicit: prefer iterative passes where possible and enforce recursion-depth guards in recursive stages.

### 5.8 CLI/backend option-validation drift
Legacy C++ validates backend-option compatibility through long imperative condition chains. Rust should encode these constraints in a declarative capability matrix and validate it with automated consistency tests.

### 5.9 Early-return backend lifecycle asymmetry
Some legacy backend paths return early and bypass parts of common orchestration. Rust sessions must guarantee deterministic teardown/state reset independent of backend path.

### 5.10 Output mode/capability mismatches
Legacy code mixes text/binary output behavior via backend-specific branches. Rust should make writer mode explicit in sink types/capabilities so invalid combinations are impossible.

### 5.11 Legacy non-target backend residue
Excluded backends (`-lang ocpp`) and template/scaffold paths should remain outside the core migration target and be isolated from default command validation/help/profile generation.

---

## 6. Testing

### 6.1 CLI regression testing
- Every documented CLI option is tested
- `faust --help` and `faust --version`
- Stress tests for long argument vectors (including >64 args) and invalid C-API argument payloads

### 6.2 Exhaustive differential tests
- Compile the ~200 Faust examples with both compilers (C++ and Rust)
- Compare outputs for each backend
- Accept cosmetic, not structural, differences
- Add a **status differential gate** on the full local corpus (`tests/corpus/*.dsp`) before backend-level parity:
  - run C++ reference compiler on each case (`faust <case>.dsp`),
  - run Rust pipeline on each case (`cargo run -p compiler -- --dump-sig <case>.dsp`),
  - classify `OK/OK`, `ERR/ERR`, `OK/ERR`, `ERR/OK`,
  - treat `OK/ERR` and `ERR/OK` as parity mismatches that must be triaged and tracked.

#### 6.2.1 Operational protocol (mandatory)

1. Use `/Users/letz/Developpements/RUST/faust` as the source-of-truth C++ compiler tree.
2. Produce/update a persistent mismatch report in `porting/phases/` with:
   - case name,
   - C++ status and short reason/output class,
   - Rust status and short reason/output class,
   - owner crate (`parser` / `eval` / `propagate` / other),
   - next action.
   - recommended automation command:
     - `cargo run -p xtask -- corpus-status-report`
     - output: `porting/phases/phase-4-corpus-status-diff-report-en.md`
3. Re-run the full status differential after each parity fix touching parser/eval/propagate.
4. Only reclassify corpus fixtures (`err_*` vs `rep_*`) after C++ status is verified.

### 6.3 C API testing
- Call `createDSPFactoryFromString()` from a C program
- Check API Box and API Signal
- Validate unified context lifecycle behavior across all C entry points (no divergent init/teardown contracts)

### 6.4 Performance tests
- Benchmark on the 20 largest files in the example suite
- Compare with C++ compiler
- Objective: ≥ as fast

### 6.5 Ecosystem integration tests
- `faust2jack`, `faust2caqt` work with the Rust compiler
- `FaustLive` can use the Rust libfaust
- The Faust web IDE works with the Rust compiler Wasm

---

## 7. “Done” criteria

- [ ] The `faust` Rust binary accepts all C++ CLI options
- [ ] CLI/backend option compatibility is capability-matrix-driven and contradiction-tested
- [ ] The ~200 Faust examples compile with all backends enabled
- [ ] Full `tests/corpus/*.dsp` C++ vs Rust status matrix is generated and all `OK/ERR` + `ERR/OK` mismatches are either fixed or explicitly waived with rationale
- [ ] The C API (`libfaust.so`/`.dylib`/`.dll`) is compatible with existing tools
- [ ] C API argument normalization safely handles long argument vectors (no fixed temporary staging limits)
- [ ] C entry points follow one lifecycle contract (no divergent context init/teardown paths)
- [ ] The C header generated by cbindgen is compatible with `faust/dsp/libfaust-c.h`
- [ ] Compiler Wasm compilation working
- [ ] Linux/macOS/Windows cross compilation via `cargo build`
- [ ] Compile orchestration is backend-path deterministic (no stale per-request state leakage)
- [ ] Performance ≥ C++ compiler on the benchmark suite
- [ ] `cargo fmt --all` passes on all 3 platforms
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes on all 3 platforms
- [ ] `cargo test --workspace --all-targets` passes on all 3 platforms
- [ ] CI/CD configured (GitHub Actions)
- [ ] Updated user documentation

---

## 8. Detailed Effort

| Sub-module | LOC C++ | Estimated LOC Rust | Days |
|-------------|---------|-----------------|-------|
| CompilerConfig (CLI parsing) | ~1,500 | 800 | 3–4 |
| CompileSession (pipeline) | ~1,500 | 1,000 | 4–5 |
| main.rs (CLI) | 74 | 100 | 1 |
| cffi (C API) | ~3,500 core file + very large export surface | 3,000–5,000 | 15–25 |
| Repo integration (Makefile, CI) | — | 500 | 2–3 |
| Differential testing | — | 2,000 | 5–7 |
| Documentation | — | 500 | 2 |
| **Total Phase 9** | **~7,000 (+ broad API parity work)** | **7,900–9,900** | **35–55** |

---

## 9. Overall Summary — All Phases

| Phase | Description | LOC C++ | Estimated LOC Rust | Person days |
|-------|-------------|---------|-----------------|----------------|
| 1 | Foundations (tlib, errors, utils, interval, algebra, graph) | 13,151 | 9,000 | 33–40 |
| 2 | Block Diagrams (boxes) | 3,231 | 2,700 | 13–16 |
| 3 | Parser (lrlex/lrpar) | 4,100 | 4,400 | 19–22 |
| 4 | Signals / Evaluation / Propagation | 18,044 | 13,200 | 34–42 |
| 5 | Normalization / Transformations | 15,470 | 12,800 | 39–49 |
| 6 | FIR & C/C++ Backends | 20,546 | 15,000–18,000 | 45–65 |
| 7 | Additional backends (excluding Java) | 42,235 | 24,700 | 53–64 |
| 8 | Draw (SVG) & Documentator (LaTeX) | 10,606 | 7,100 | 19–22 |
| 9 | Final integration | 7,000 (+ large C API parity scope) | 7,900–9,900 | 35–55 |
| **TOTAL** | | **~134,400** | **~96,800–101,700** | **290–375** |

### Calendar estimate

- **1 developer**: ~320 working days ≈ **16–18 months**
- **2 developers**: ~210 days ≈ **10–12 months** (phases 7/8 in parallel)
- **3 developers**: ~150 days ≈ **7–9 months** (parallelizable backends)

### Key milestones

| Milestone | Phases | Result |
|-------|--------|----------|
| **M1 — Hello World** | 1–3 | `process = _;` is parsed correctly |
| **M2 — First signal** | 1–4 | `process = + ~ _;` produces signals |
| **M3 — First C code** | 1–6 | `faust -lang c noise.dsp` produces a compilable .c |
| **M4 — Multi-backend** | 1–7 | Functional C, C++, Rust, Wasm |
| **M5 — Parity** | 1–9 | Rust compiler passes all C++ tests |
