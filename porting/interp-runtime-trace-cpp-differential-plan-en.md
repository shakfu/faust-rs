# Interp Runtime Trace Differential Validation vs Faust C++ (Plan)

## 1. Goal

Validate the observable DSP runtime semantics of the Rust pipeline:

- `parse -> eval -> propagate -> signal->FIR (fast-lane) -> interp backend -> runtime execution`

against the **current Faust C++ compiler** by comparing output sample traces on a
small, curated DSP corpus with explicit types in source code.

This plan intentionally avoids blocking on current fast-lane FIR typing gaps
(`FIR-B01/B03/C01`) by using DSP fixtures whose source code already makes types
explicit.

## 2. Why This Workflow (and Why Now)

Current status:

- Rust `interp` runtime trace harness exists (`xtask interp-trace-dump`).
- Rust runtime trace snapshot gen/check exists.
- Lane diff scaffold exists, but `legacy` FIR bridge is currently non-semantic
  (label-only `compute`) and therefore not a valid runtime oracle.

Therefore, the next meaningful oracle is:

- **Rust interp (fast-lane)** vs **Faust C++ generated runtime**.

This gives a stronger semantic signal than Rust-vs-Rust comparisons while
avoiding immediate fast-lane typing refactors.

## 3. Non-Goals (v1)

- Full corpus parity (`tests/corpus/rep_*`) on day one.
- Replacing golden text snapshots (`golden-check`) with runtime traces.
- Proving full interpreter correctness across all DSP features.
- Fixing fast-lane typing issues in this phase.
- Designing a generic multi-backend runtime benchmark framework.

## 4. Scope (v1)

### In scope

- Curated DSP fixtures with explicit types (`tests/runtime_corpus_cpp_ref/` or a
  dedicated subset in `tests/runtime_corpus/`).
- Deterministic input scenarios (zeros / impulse / ramp / sine).
- Trace generation for:
  - Rust `interp` fast-lane
  - Faust C++ compiled executable (or equivalent C++ runtime path)
- Tolerant float comparison (`abs_tol`, `rel_tol`)
- `xtask` commands for generation/check/diff against C++
- CI-friendly small suite

### Out of scope (v1)

- Differential testing against C and C++ source backends emitted by Rust
- Performance benchmarking / throughput measurement
- Randomized fuzzing inputs
- Stateful UI automation beyond deterministic defaults (unless a fixture is
  explicitly designed for it)

## 5. Reference Model Options (Choose One First)

The plan supports two possible C++ reference execution routes.

### Option A (recommended): `faust` -> native code executable -> run trace harness

Workflow:

1. Use `FAUST_CPP_BIN` (`faust`) to emit C++ from DSP
2. Compile emitted C++ with a host C++ compiler into an executable
3. Run executable with deterministic input/output trace protocol
4. Compare against Rust `interp` trace

Pros:

- Strong end-to-end oracle (matches real production reference path)
- Independent from Rust interpreter implementation

Cons:

- Requires host C++ toolchain + build harness
- Need a stable executable wrapper protocol for I/O traces

### Option B (fallback): Faust C++ interpreter backend (`-lang interp`) -> `.fbc` + C++ runner

Workflow:

1. Use C++ `faust` to emit interpreter bytecode (`.fbc` or equivalent)
2. Run via reference C++ interpreter runtime wrapper
3. Capture traces

Pros:

- Closer conceptual match to Rust `interp`

Cons:

- More tooling/setup complexity in this repo
- May require linking/reference runtime not currently integrated in `faust-rs`

### Decision (v1)

Start with **Option A** unless local/tooling constraints block it.

## 6. DSP Fixture Strategy (Type-Explicit Source)

Create and maintain a dedicated runtime differential corpus with explicit typing.

### Proposed folder

- `tests/runtime_corpus_cpp_ref/`

Keep it separate from:

- `tests/corpus/rep_*` (porting/parity structure corpus)
- `tests/runtime_corpus/` (current Rust runtime trace fixtures)

### Fixture design rules

1. Prefer explicit floating literals:
   - `1.0`, `2.0`, `-3.0` instead of `1`, `2`, `-3`
2. Use explicit casts in DSP source when ambiguity exists:
   - especially around `abs/fabs`, `min/fmin`, `max/fmax`, comparisons/selects
3. Keep fixtures small and focused:
   - one semantic feature per file where possible
4. Keep deterministic behavior:
   - no time-varying randomness
   - no filesystem dependencies
5. Document expected scenario(s) in a fixture README or metadata

### Initial fixture set (v1)

- `cppref_01_passthrough.dsp`
- `cppref_02_gain_bias_typed.dsp`
- `cppref_03_stereo_mix_typed.dsp`
- `cppref_09_ui_slider_typed.dsp` (if UI defaults are deterministic)
- `cppref_22_parallel_mix_typed.dsp`
- `cppref_31_extended_primitives_typed.dsp`
- `cppref_38_sine_phasor_typed.dsp` (only if runtime-stable)

## 7. Trace Format and Comparison Contract

Reuse the existing JSON trace schema already used by:

- `xtask interp-trace-dump`

This keeps:

- one trace parser
- one tolerance comparator
- one snapshot model

### Trace metadata requirements

The compared traces must match exactly on:

- DSP fixture id/path (or normalized case id)
- scenario name
- sample rate
- block size
- number of blocks
- channel counts
- sample counts per channel

### Numeric comparison

Use tolerant float comparison:

- `abs_tol`
- `rel_tol`

Defaults can reuse current `tests/runtime_traces/METADATA.toml` values or define
CPP-reference-specific tolerances if needed.

## 8. Proposed `xtask` Command Structure

The commands below are intentionally parallel to the existing Rust-only trace
workflow (`interp-trace-dump/gen/check/diff-lanes`).

### 8.1 `interp-trace-dump-cppref`

Purpose:

- Generate one runtime trace from the Faust C++ reference path for a DSP case.

Example:

```bash
cargo run -p xtask -- interp-trace-dump-cppref \
  --case tests/runtime_corpus_cpp_ref/cppref_01_passthrough.dsp \
  --scenario impulse \
  --sample-rate 48000 \
  --block-size 64 \
  --num-blocks 1 \
  --out /tmp/cppref_trace.json
```

Inputs:

- `--case <path>`
- `--scenario zeros|impulse|ramp|sine`
- `--sample-rate <N>`
- `--block-size <N>`
- `--num-blocks <N>`
- `--out <path>` (optional, stdout fallback)

Environment:

- `FAUST_CPP_BIN=/path/to/faust`
- `CXX=/path/to/c++` (optional, default host compiler)

### 8.2 `interp-trace-diff-cpp`

Purpose:

- Compare Rust `interp` fast-lane trace vs Faust C++ reference trace for one or
  more fixtures/scenarios.

Example:

```bash
cargo run -p xtask -- interp-trace-diff-cpp \
  --case tests/runtime_corpus_cpp_ref/cppref_31_extended_primitives_typed.dsp \
  --sample-rate 48000 \
  --block-size 64 \
  --num-blocks 1
```

Behavior:

- Generates Rust trace in-memory (`interp-trace-dump` logic reuse)
- Generates C++ ref trace in-memory (`interp-trace-dump-cppref` logic reuse)
- Compares with tolerant float comparator
- Prints `match` / `skip` / `mismatch` with clear reasons

### 8.3 `interp-trace-gen-cppref` (optional v1.1)

Purpose:

- Generate reference C++ trace snapshots under:
  - `tests/runtime_traces/cpp/<case>/<scenario>.json`

This is optional in v1 if `interp-trace-diff-cpp` computes both traces on the
fly.

### 8.4 `interp-trace-check-cppref` (optional v1.1)

Purpose:

- Compare Rust traces against stored C++ snapshots (faster CI mode, less toolchain
  dependence at check time)

## 9. `xtask` Implementation Plan (Phases)

### Phase A — Reference execution design spike

Deliverable:

- Decide and document reference route (Option A vs B)
- Prove one fixture runs end-to-end and produces a trace (`cppref_01_passthrough`)

Acceptance:

- One successful C++ trace JSON generated by `xtask`

### Phase B — C++ trace dump command (`interp-trace-dump-cppref`)

Deliverables:

- CLI parsing
- deterministic input generation reuse
- C++ compile/run wrapper
- JSON trace emission matching Rust schema

Acceptance:

- `cppref_01_passthrough` and `cppref_31_extended_primitives_typed` produce valid
  JSON traces

### Phase C — Differential command (`interp-trace-diff-cpp`)

Deliverables:

- run Rust trace + C++ trace
- tolerant comparison reuse
- structured mismatch reporting
- scenario mapping for `runtime_corpus_cpp_ref`

Acceptance:

- reports at least one `match`
- reports meaningful mismatch details on a forced divergence

### Phase D — Fixture corpus expansion and CI subset

Deliverables:

- initial `cppref_*` corpus committed
- CI-safe subset and runtime budget documented

Acceptance:

- small subset runs reliably in CI/local script

### Phase E — Snapshot mode (optional follow-up)

Deliverables:

- `interp-trace-gen-cppref` and `interp-trace-check-cppref`
- metadata for C++ reference snapshots

Acceptance:

- can run trace differential checks without invoking `faust`/C++ compiler each time

## 10. Execution Wrapper Design (Option A)

For Option A (`faust` -> C++ -> executable), define a tiny deterministic wrapper
program contract.

### Wrapper responsibilities

- instantiate generated DSP
- init with sample rate
- allocate input/output buffers
- feed deterministic inputs from scenario generator
- run `compute()` across `num_blocks`
- print/output JSON trace with same schema as Rust trace harness

### Wrapper integration strategy

Two approaches:

1. Generate a temporary C++ wrapper source from `xtask` and compile it alongside
   the Faust-generated C++ file
2. Keep a checked-in reusable wrapper template under `tests/runtime_traces/cpp_wrapper/`

Recommendation:

- **Checked-in wrapper template** for debuggability and reproducibility

## 11. Error Classification and Reporting

`xtask interp-trace-diff-cpp` should distinguish:

- `match`
- `skip` (known unsupported fixture/scenario, missing tools, explicit policy)
- `tool-error` (Faust C++ invocation failed, compile failed, runtime wrapper failed)
- `mismatch` (semantic output divergence)

This keeps CI and local triage actionable.

## 12. CI Strategy (Incremental)

### Local developer mode (default)

- Runs live C++ toolchain if `FAUST_CPP_BIN` is present
- Otherwise reports `skip` with clear message

### CI mode (later)

Option 1:

- Install Faust C++ binary and host C++ compiler in CI job and run live diffs

Option 2:

- Check C++ reference trace snapshots into repo and compare Rust traces to snapshots
  in standard CI

Recommendation:

- Start with **local live mode**
- Move to **snapshot-backed CI** once fixture set stabilizes

## 13. Risks and Mitigations

### Risk: C++ toolchain variability causes noisy diffs

Mitigation:

- compare runtime samples (not generated source)
- use fixed sample rate/block sizes
- use float tolerances
- keep fixtures simple

### Risk: wrapper introduces its own semantic bug

Mitigation:

- keep wrapper minimal and deterministic
- test wrapper on `passthrough` / `gain`
- cross-check one case manually

### Risk: typed fixtures drift from real-world DSPs

Mitigation:

- document they are **validation fixtures**, not a replacement for corpus coverage
- keep links/comments referencing the original `rep_*` inspiration

## 14. Acceptance Criteria (v1)

This plan is considered implemented (v1) when:

1. A dedicated type-explicit DSP runtime differential corpus exists.
2. `xtask` can generate a C++ reference trace for one fixture.
3. `xtask interp-trace-diff-cpp` compares Rust `interp` fast-lane vs C++ on at
   least 3 fixtures/scenarios.
4. Comparison uses tolerant float matching and reports clear mismatch locations.
5. Results and limitations are documented in `JOURNAL.md`.

## 15. Open Questions (to resolve before Phase A/B completion)

1. Which C++ reference execution route is easiest to make reliable in this repo
   (Option A native executable vs Option B interpreter backend)?
2. Should C++ reference traces be generated live only, or snapshot-backed from
   the start?
3. Where should the `cppref_*` corpus live:
   - `tests/runtime_corpus_cpp_ref/`
   - or merged into `tests/runtime_corpus/` with metadata tags?
4. What default tolerances are acceptable for trigonometric-heavy fixtures?
5. Should UI fixtures be included in v1, or deferred until a stable UI init
   contract is documented for the wrapper?
