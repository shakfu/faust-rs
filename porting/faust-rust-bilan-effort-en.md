# Porting the Faust C++ compiler → Rust — Effort report

> **Date**: February 2026
> **Source**: branch `master-dev-ocpp-od-fir-2-FIR19`
> **Clarification**: branch name includes `ocpp`, but old C++ mode `-lang ocpp` is out of scope.
> **C++ code base**: 159,012 LOC (`.cpp` + `.hh`), 162,315 LOC including `.h/.hpp/.l/.y` — **Estimated Rust base**: ~96,800–101,700 LOC

---

## 1. Effort per phase

| Phase | Description | LOC C++ | LOC Rust | Person days |
|:-----:|-------------|--------:|---------:|:--------------:|
| 0 | Validation sprint (parser/gGlobal/API/pipeline checks) | — | — | 5–10 |
| 1 | Foundations (tlib, errors, utils, interval, algebra, graph) | 13,151 | 9,000 | 33–40 |
| 2 | Block Diagrams (boxes) | 3,231 | 2,700 | 13–16 |
| 3 | Parser (lrlex / lrpar) | 4,100 | 4,400 | 19–22 |
| 4 | Signals / Evaluation / Propagation | 18,044 | 13,200 | 34–42 |
| 5 | Normalization / Transformations | 15,470 | 12,800 | 39–49 |
| 6 | FIR & C/C++ Backends (effective production path first) | 20,546 | 15,000–18,000 | 45–65 |
| 7 | Additional backends (Wasm, Interp, LLVM, Rust, etc.; Java excluded) | 42,235 | 24,700 | 53–64 |
| 8 | Draw (SVG) & Documentator (LaTeX) | 10,606 | 7,100 | 19–22 |
| 9 | Final integration (CLI, API C, repo) | 7,000 (+ broad API parity) | 7,900–9,900 | 35–55 |
| | **Total** | **~134,400** | **~96,800–101,700** | **295–385** |

**Median value: ~340 person days ≈ 1.55 person years.**

---

## 2. Pure human effort

| Team | Calendar duration | Comment |
|--------|:----------------:|-------------|
| 1 developer | 16–18 months | Sequential with full API parity |
| 2 developers | 10–12 months | Phases 7/8 in parallel |
| 3 developers | 7–9 months | Highly parallelizable text backends |

Assumption: 220 working days per year, experienced Rust + DSP developer.

---

## 3. Human effort + AI (tandem mode)

### 3.1 What AI accelerates

| Stain | Estimated gain | Explanation |
|-------|:-----------:|-------------|
| Writing Rust code | ×5–8 | Almost instantaneous generation, humans validate and adjust |
| Mechanical porting (constructors, enums, pattern matching) | ×8–10 | Repetitive task ideal for AI |
| Rustdoc Documentation | ×10+ | Automatic generation from C++ |
| Unit testing | ×4–6 | Generation of test cases, the human checks the invariants |
| Rust API/Syntax Search | ×∞ | Eliminated — AI knows the ecosystem |
| Debug borrow checker | ×2–3 | AI offers solutions, humans choose |

### 3.2 What AI does not accelerate (or only slightly)

| Stain | Gain | Explanation |
|-------|:----:|-------------|
| Architectural decisions | ×1 | The human decides, the AI ​​offers options |
| Semantic debug (bad audio result) | ×1–2 | Requires deep understanding of DSP |
| Differential testing (C++ vs Rust) | ×1 | Both versions must be compiled and run |
| Ecosystem integration (faust2jack, etc.) | ×1 | Depends on actual environment |

### 3.3 Tandem assessment

| Scenario | Person days | Calendar duration | Reduction |
|----------|:--------------:|:----------------:|:---------:|
| Pure human (1 dev) | ~345 | 16–18 months | — |
| Human + general AI | ~180–240 | 9–12 months | ×1.4–1.9 |
| Human expert Faust + AI | ~130–180 | 6–9 months | ×1.9–2.6 |
| Expert human + AI + 2nd dev tandem | ~95–140 | 4–7 months | ×2.5–3.6 |

---

## 4. Cost in tokens (API estimate)

### 4.1 Ventilation

| Job | Entry tokens | Output tokens | Subtotal |
|-------|:-------------:|:-------------:|:----------:|
| Analysis of C++ (reading, understanding) | 7M | 1M | 8M |
| Generation of Rust (first draft) | 3M | 4M | 7M |
| Iterations and corrections (×3–5) | 8M | 12M | 20M |
| Testing and documentation | 2M | 3M | 5M |
| **Total** | **~20M** | **~20M** | **~40M** |

### 4.2 Estimated monetary cost

| Model | Entrance price | Exit price | Total cost |
|--------|:-----------:|:-----------:|:----------:|
| Claude Sonnet 4 | $3/M | $15/M | **~$360** |
| Claude Opus 4 | $15/M | $75/M | **~$1,800** |
| Mix Opus (archi) + Sonnet (code) | — | — | **~$500–800** |

Note: these costs only count API calls. The Claude Pro/Team subscription may be more economical for intensive use.

---

## 5. Milestones and progression

```
Month 1  ██████████████████████████████  Phase 0 + Phase 1 kickoff
Month 2  ██████████████████████████████  Phases 1–2
Month 3  ██████████████████████████████  Phase 3
Month 4  ██████████████████████████████  Phase 4
Month 5  ████████████████░░░░░░░░░░░░░░  Phase 5 (part 1)
Month 6  ████████████████░░░░░░░░░░░░░░  Phase 5 (part 2)
Month 7  ████████████████░░░░░░░░░░░░░░  Phase 6
          ░░░░░░░░░░░░░░██████████████  Phase 8 (draw + doc) — in parallel
Month 8  ████████████████░░░░░░░░░░░░░░  Phase 6 (stabilization)
Month 9  ██████████████████████████████  Phase 7 (backends)
Month 10 ██████████████████████████████  Phase 9 (integration + API parity)
```

### Verifiable Milestones

| Milestone | End of phase | Validation criterion |
|-------|:------------:|----------------------|
| **M1 — Parse OK** | 1–3 | `process = _;` parses itself and produces a box tree |
| **M2 — First signal** | 1–4 | `process = + ~ _;` produces correct signals |
| **M3 — First .c** | 1–6 | `faust -lang c noise.dsp` → C code compilable by gcc |
| **M4 — Multi-backend** | 1–7 | Functional C, C++, Rust, Wasm |
| **M5 — Full parity** | 1–9 | 200 examples pass, compatible C API, identical CLI |

---

## 6. Risk factors

| Risk | Impact | Probability | Mitigation |
|--------|:------:|:-----------:|------------|
| lrpar incompatible with Faust grammar | Strong | AVERAGE | Rapid prototype in phase 3, fallback lalrpop |
| Porting `signalFIRCompiler` first while production flow uses `InstructionsCompiler` | Strong | AVERAGE | Port the effective path first, keep `signalFIRCompiler` as secondary |
| C API surface larger than expected (`box_signal_api.cpp`) | Strong | AVERAGE | Deliver API in tiers (must-have subset, then full parity) |
| Insufficient TreeArena performance | AVERAGE | Weak | Benchmark criterion from phase 1 |
| LLVM backend: inkwell does not support the required version | AVERAGE | AVERAGE | Report LLVM, use text backends first |
| AI generates code that compiles but is semantically wrong | Strong | AVERAGE | Systematic differential tests at each phase |

---

## 7. Conclusion

| Metric | Pure human | Human + AI |
|----------|:----------:|:-----------:|
| Person days | ~340 | ~130–180 |
| Duration (1 person) | 16–18 months | 6–9 months |
| Human cost (100 €/h) | ~€272,000 | ~€104,000–144,000 |
| AI cost (tokens) | — | ~€500–800 |
| **Total cost** | **~€272,000** | **~€105,000–145,000** |
| **Reduction** | — | **~1.9–2.6x** |

Porting the Faust compiler to Rust remains feasible, but with a broader and more realistic scope than the initial estimate. With an expert developer and AI, a practical expectation is **6–9 months** for high-confidence parity including significant API coverage.
