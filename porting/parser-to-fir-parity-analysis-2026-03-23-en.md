# Parser → FIR Parity Analysis: faust-rs vs Faust C++

> **Project**: Porting the Faust C++ compiler → Rust
> **Date**: 2026-03-23
> **Branch**: `main-dev`
> **Scope**: Full pipeline from parsing to FIR generation

---

## 1. Pipeline overview

| Stage | C++ source | Rust crate | Parity |
|-------|-----------|------------|--------|
| **1. Parsing** | `faustparser.y` / `faustlexer.l` (bison/flex) | `crates/parser/` (lrpar/lrlex) | **~100%** |
| **2. Evaluation (boxes)** | `eval.cpp`, `patternmatcher.cpp` | `crates/eval/` | **~95%** |
| **3. Propagation (box→signal)** | `propagate.cpp` | `crates/propagate/` | **~95%** |
| **4. Signal typing** | `sigtyperules.cpp` | `crates/sigtype/` | **~90%** |
| **5. Normalization** | `simplify.cpp`, `normalize.cpp`, `aterm/mterm` | `crates/normalize/` | **~80%** |
| **6. Interval analysis** | `interval_algebra.hh` | `crates/interval/` | **~100%** |
| **7. Signal → FIR** | `InstructionsCompiler`, `ScalarCompiler` | `crates/transform/signal_fir/` | **~60–70%** |
| **8. FIR representation** | `instructions.hh` (RTTI) | `crates/fir/` (hash-consed) | **~95%** |
| **9. Code generation** | C, C++, LLVM, interp, WASM, etc. | C, C++, interp, Cranelift (+scaffolds) | **~70%** |

---

## 2. Workspace structure (28 crates)

```
tlib            — hash-consing tree arena, properties, recursion helpers
errors          — unified diagnostics (Diagnostic, Stage, codes)
interval        — interval arithmetic (ported, 62 tests)
algebra         — algebraic operations (scaffold)
graph           — graph data structures
boxes           — box AST construction/matching (180+ constructors)
parser          — Faust grammar (lrpar/lrlex), source reader, imports
signals         — signal IR construction/matching (60+ node types)
ui              — UI IR: grouped controls, metadata, layout
eval            — box evaluator: environments, closures, pattern matching
propagate       — box→signal lowering, arity inference, composition algebra
sigtype         — signal type system (Nature×Variability×Computability×Vectorability×Boolean)
normalize       — algebraic simplification (aterm/mterm, simplify, normalform)
transform       — signal_prepare + signal_fir fast-lane lowering
fir             — FIR construction/matching/verification (hash-consed)
codegen         — backend code generation (C, C++, interp, cranelift, +scaffolds)
compiler        — top-level facade, pipeline orchestration
doc             — documentation generation
draw            — signal graph visualization
utils           — shared utilities
xtask           — build-time tasks
interp-ffi      — interpreter backend FFI
cranelift-ffi   — Cranelift JIT backend FFI
box-ffi         — box representation FFI
faust-ffi       — main C FFI interface (libfaust)
```

---

## 3. Stage-by-stage analysis

### 3.1 Parser — parity ~100%

**Covered:**
- Full Faust grammar: composition (`:`, `,`, `<:`, `:>`, `~`), lambda, pattern matching, `with`/`letrec`/`where`
- Iterative forms (`ipar`, `iseq`, `isum`, `iprod`)
- Import/component/library, `environment`, `route`
- Infix/postfix operators, FFI (`ffunction`, `fconst`, `fvar`)
- UI widgets, soundfiles, waveforms, metadata
- Modulation syntax (recent addition)

**Minor differences:**
- Backend: lrpar (Rust LALR) vs bison — same grammar, different engine
- Integer atoms bounded to `i32` at parser boundary (C++ uses native `int`)

### 3.2 Evaluation — parity ~95%

**Covered:**
- Lexical environments with closures and barrier scopes
- Pattern matching via tree automaton (Graef/RTA algorithm)
- Component/library resolution with source cache
- Iterative form unrolling at eval time
- Infinite recursion detection (`LoopDetector`)
- `a2sb` (abstract-to-symbolic-boxes) for post-eval flattening

**Potential gaps:**
- Some obscure multi-variable pattern matching edge cases
- `modulation` syntax — present but lightly tested in production

### 3.3 Propagation — parity ~95%

**Covered:**
- Full composition algebra: seq, par, split, merge, rec
- Arity inference (`box_arity`)
- De Bruijn recursion groups (sigRec/sigProj)
- Grouped UI extraction (`UiProgram` as first-class artifact)
- Post-eval validation via `FlatBoxId`

**Rust advantage:** UI is a first-class IR artifact, not reconstructed heuristically in backends.

### 3.4 Signal representation — 60+ variants, complete coverage

| Family | C++ | Rust | Notes |
|--------|-----|------|-------|
| Constants (int, real) | yes | yes | |
| I/O (input, output) | yes | yes | |
| Delays (delay1, delay, prefix) | yes | yes | |
| Casts (int, float, bit) | yes | yes | |
| Tables (rdtbl, wrtbl, gen) | yes | yes | |
| Select2 / Select3 | yes | yes | Select3 via decomposition |
| BinOp (17 operators) | yes | yes | includes LRsh (logical right shift) |
| Unary math (14 functions) | yes | yes | sin, cos, tan, asin, acos, atan, exp, log, log10, sqrt, abs, floor, ceil, rint, round |
| Binary math (pow, min, max, atan2, fmod, remainder) | yes | yes | |
| FFun / FConst / FVar | yes | yes | |
| Rec / Proj | yes | yes | De Bruijn + symbolic conversion |
| UI widgets (7 types) | yes | yes | via ControlId |
| Soundfile (4 variants) | yes | yes | |
| Attach / Enable / Control | yes | yes | |
| Waveform | yes | yes | |
| Clocked / OnDemand / Up-Downsampling | yes | yes | recent syntax |
| AssertBounds / Lowest / Highest | yes | yes | |

### 3.5 Signal typing — parity ~90%

**Covered:**
- Complete lattice: Nature × Variability × Computability × Vectorability × Boolean
- `TypeAnnotator` implements bottom-up inference (mirrors `sigtyperules.cpp`)
- Intervals embedded in `SigType`
- `SimpleType`, `TableType`, `TupletType` hierarchy

**Gap:** Typing is not yet wired into the fast-lane signal→FIR pipeline. Only a reduced type map (`Int | Real | Sound`) is used for FIR lowering. Full typing runs in `signal_prepare` but its results are not fully consumed downstream.

### 3.6 Normalization — parity ~80%

**Covered:**
- `simplify.rs` — memoized algebraic rewriting
- `mterm.rs` / `aterm.rs` — multiplicative/additive term algebra with factorization
- Constant folding, identity elimination
- Delay normalization

**Gaps:**
- Not wired into the fast-lane main pipeline
- Some advanced simplification rules may be incomplete
- C++ has `SignalPromotion` + `SignalAutoDifferentiate` — no visible Rust equivalents

### 3.7 Interval analysis — parity ~100%

- All C++ interval operators ported: arithmetic, casts, logic, trig, math, UI, delay/table
- Bitwise interval operations
- 62 unit tests passing
- `Interval` struct: `[lo: f64, hi: f64, lsb: i32]`

### 3.8 Signal → FIR — parity ~60–70% (main gap)

This is the **most critical parity gap**. The C++ has `InstructionsCompiler` / `ScalarCompiler` / `VectorCompiler` / `WorkStealingCompiler` totaling ~15K lines.

**Covered (fast-lane slices 2A–2H):**

| Slice | Content | Status |
|-------|---------|--------|
| 2A | SIGINPUT, constants, SIGBINOP, SIGOUTPUT | done |
| 2B | Core math (trig, exp, log, sqrt), control/state bootstrap | done |
| 2C | Delay family (SIGDELAY1, fixed/bounded SIGDELAY, SIGPREFIX) | done |
| 2D | Extended primitives (waveform, table, UI) | done |
| 2E | Shim reduction (replace `frs_*` calls with native FIR) | done |
| 2F | Critical shim elimination | done |
| 2G | FIR-native table lowering (SIGWAVEFORM, SIGRDTBL, SIGWRTBL) | done |
| 2H | Non-trivial tables (constant-size SIGWRTBL with deterministic generator) | done |

**Major gaps:**

| C++ feature | Rust status |
|-------------|-------------|
| **VectorCompiler** (block vectorization, `-vec` mode) | absent |
| **WorkStealingCompiler** (parallelism) | absent |
| **Explicit occurrence analysis** (sharing/reuse) | implicit via hash-consing only |
| **Topological scheduling** | simplified in planner |
| **Complex recursive signal expansion** (multi-slot) | partial — unary canonicalization OK |
| **Runtime generator forms** for tables | `UnsupportedSignalNode` for some patterns |
| **Post-typing signal optimization** | absent (C++ does simplify → share → occurrence → compile) |

**Pre-lowering staging (`signal_prepare`):**
1. Clone forest into private arena with sharing preserved
2. `de_bruijn_to_sym` — convert recursive groups to symbolic form
3. Canonicalize unary recursion projections
4. Type annotation via `infer_full_types()`
5. Signal promotion (insert type-driven casts)

### 3.9 FIR representation — parity ~95%

**Covered:**
- Types: `Int32`, `Int64`, `Float32`, `Float64`, `FaustFloat`, `Quad`, `FixedPoint`, `Bool`, `Void`, `Array`, `Vector`, `Struct`, `Ptr`, `Fun`
- Access classification: `Stack`, `Struct`, `Static`, `FunArgs`, `Loop`, `Global`
- Values: Load/Store/Tee, BinOp (16), Neg, Cast, Bitcast, Select2, FunCall, MathCall (21)
- Statements: DeclareVar/Table/Fun/StructType, If/Switch/ForLoop/WhileLoop, Block, Return
- UI instructions: OpenBox/CloseBox, AddButton/Slider/Bargraph/Soundfile/Meta
- Module structure: globals, functions, static declarations, DSP struct

**Rust advantages:**
- Hash-consed storage (`FirStore` backed by `TreeArena`) — structural sharing
- Exhaustive pattern matching via `FirMatch` — no RTTI, no silent defaults
- Explicit type embedding on value nodes — no separate type-reconstruction phase

**Minor gap:** No `WavFile`/`SndFile` loading for table initialization from audio files.

### 3.10 Code generation — parity ~70%

| Backend | C++ | Rust | Status |
|---------|-----|------|--------|
| **C** | production | production | ~95% parity |
| **C++** | production | production | ~95% parity |
| **Interpreter** | production | production | FBC complete, 6-level peephole optimizer, `.fbc` serialization |
| **Cranelift JIT** | — | bring-up | Rust-native alternative to LLVM |
| **LLVM** | production | scaffold | not implemented |
| **WASM** | production | scaffold | not implemented |
| **Rust** | experimental | scaffold | not implemented |
| **Others** (Julia, JAX, VHDL…) | various | scaffold | not implemented |

---

## 4. Architectural differences and Rust advantages

### 4.1 Hash-consing throughout
All IRs (boxes, signals, FIR) use `TreeArena` with automatic deduplication. Identical subtrees share memory. This gives implicit structural sharing without explicit occurrence analysis.

### 4.2 UI as first-class artifact
`UiProgram` is extracted during propagation and threaded through to codegen. C++ reconstructs UI layout heuristically in each backend.

### 4.3 Exhaustive pattern matching
`BoxMatch`, `SigMatch`, `FirMatch` enums replace C++ RTTI/virtual dispatch. No silent fall-through on unhandled cases.

### 4.4 Typed error boundaries
Each stage returns `Result<Output, StageError>`. The compiler facade aggregates into `CompilerError` with variants for Parse, Import, Eval, Propagate, SignalFir, Codegen.

### 4.5 Dual compilation lanes
`SignalFirLane::LegacyBridge` (temporary C++ bridge) and `SignalFirLane::TransformFastLane` (new Rust-native lowering) can be selected at the compiler facade level.

### 4.6 Modular crate architecture
25+ crates with explicit dependency boundaries vs. C++ monolithic compilation model.

---

## 5. Critical gaps summary

| Priority | Gap | Impact |
|----------|-----|--------|
| **P0** | Normalization not wired into fast-lane | Signals reach FIR without full algebraic simplification — suboptimal generated code |
| **P0** | Occurrence analysis absent | No explicit sharing/reuse analysis — redundant computations in generated code, no CSE |
| **P1** | VectorCompiler absent | No block-vectorized compilation (`-vec` mode unavailable) |
| **P1** | Complex recursive signal expansion (multi-slot) | Some recursive patterns hit `UnsupportedSignalNode` |
| **P1** | Tables with complex runtime generators | Subset of SIGWRTBL patterns unsupported |
| **P2** | LLVM/WASM backends | Scaffolds only — no native LLVM IR or WASM emission |
| **P2** | WorkStealingCompiler | No parallel compute mode |
| **P2** | Post-typing signal optimization | C++ pipeline does simplify→share→occurrence→compile; Rust skips middle steps |

---

## 6. Test infrastructure

- **Parser tests**: grammar coverage validated against Faust corpus
- **Eval tests**: pattern matching, closures, iterative forms
- **Propagation tests**: composition algebra, arity inference
- **Interval tests**: 62 unit tests (full port)
- **Signal→FIR tests**: slice-by-slice validation (2A–2H)
- **Codegen tests**: C/C++ output differential against reference compiler
- **Interpreter tests**: FBC execution with `.fbc` serialization round-trip
- **Integration tests**: `crates/compiler/tests/` — corpus-based end-to-end validation

---

## 7. Conclusion

The Rust port covers the **full Faust compilation pipeline** from parsing to code generation with high fidelity in the front-end stages (parser, eval, propagation: 95–100%) and solid coverage of the IR layers (signals, FIR: 90–95%). The main parity gap is in the **signal→FIR lowering** stage (~60–70%), where the C++ scalar/vector/work-stealing compilers represent ~15K lines of mature optimization logic. Closing this gap requires:

1. Wiring normalization + occurrence analysis into the fast-lane
2. Completing multi-slot recursive signal expansion
3. Implementing the VectorCompiler equivalent (if `-vec` mode is a target)

The Rust implementation brings architectural improvements (hash-consing, exhaustive matching, first-class UI, typed errors) that should make the remaining work more tractable than in the original C++ codebase.
