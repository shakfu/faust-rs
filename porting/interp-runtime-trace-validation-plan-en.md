## Interp Runtime Trace Validation Plan (Continuous Compiler Validation)

### Purpose

Use the Rust `interp` backend as a continuous execution oracle for DSP corpus
tests by running compiled DSPs and comparing generated audio sample traces.

This complements existing compile-time checks (`cargo test`, FIR verifier,
golden text outputs) with runtime semantic validation.

### Why This Is Valuable

- Detects regressions that compile-only checks miss (wrong values, state update
  ordering, control-flow mistakes, cast bugs, table indexing errors).
- Validates the integrated path:
  `parse -> boxes -> eval -> propagate -> normalize -> transform -> FIR -> interp compiler -> FBC executor`.
- Provides a fast, deterministic regression harness suitable for CI.
- Creates a reusable oracle for validating future FIR passes (for example the
  `FunctionInliner`) using pre/post-transform execution equivalence.

### Important Limitation (Explicitly Accepted)

This is not an independent reference oracle because it uses the Rust pipeline
and Rust interpreter backend. A shared bug may still pass.

Mitigation:
- keep existing golden checks,
- add differential modes (`legacy` vs `fast-lane`),
- optionally compare selected cases to C++ reference traces later.

---

## Goals

### Primary Goals (v1)

1. Execute selected `tests/corpus/rep_*.dsp` programs through the `interp`
   backend with deterministic inputs.
2. Capture output sample traces in a stable machine-readable format.
3. Compare traces against stored golden traces with numeric tolerances.
4. Integrate into `xtask` and CI as a repeatable validation stage.

### Secondary Goals (v2+)

1. Differential trace mode:
   - Rust `legacy` lane vs Rust `fast-lane`
2. C++ reference trace generation/checking for a small curated subset
3. Property/scenario expansion (impulse, step, ramp, sine, noise-seeded)
4. Performance budgets (execution time ceilings)

### Non-Goals (v1)

- Replacing text golden output checks
- Real-time audio performance benchmarking
- Full UI event automation across all controls
- Cross-platform bit-identical traces (tolerance-based comparison is expected)

---

## Scope and Coverage Strategy

### Initial Corpus Scope (v1)

Use a curated subset of `tests/corpus/rep_*.dsp` that is:
- supported by `interp`,
- deterministic,
- stable under fixed sample rate/block size,
- representative of key semantics:
  - arithmetic
  - feedback/state
  - conditionals/select
  - tables
  - math functions

Recommended initial groups:
- simple arithmetic / routing (`rep_02`, `rep_03`, `rep_21`, `rep_22`)
- feedback/state (`rep_05`, `rep_06` if supported end-to-end)
- nonlinear/math (`rep_07`, `rep_31`, `rep_38`)
- tables (`rep_35`, `rep_37`) once FIR/interp support is stable

### Execution Scenarios (v1)

Each DSP runs with one or more deterministic input scenarios:

- `zeros`: all input samples = 0
- `impulse`: first sample = 1, then 0
- `ramp`: normalized ramp sequence
- `sine`: deterministic sine input (fixed freq, phase)

Not every DSP needs all scenarios initially. Start with:
- `zeros` + `impulse` for most DSPs
- add `ramp`/`sine` for math-heavy cases

### Runtime Parameters (fixed defaults, v1)

- sample rate: `48000`
- block size: `64`
- num blocks: `4` (256 samples total)
- input amplitude conventions documented in metadata

---

## Architecture Overview

### Core Principle

Do not parse CLI text output. Build traces through Rust APIs directly.

### Proposed Layers

1. `compiler` crate/API:
   - compile DSP source to `interp` factory (`FbcDspFactory`)
2. `codegen::backends::interp` runtime:
   - instantiate `FbcDspInstance`
   - run `compute()`
3. new trace harness (likely in `xtask` first, later optional library helper):
   - generate deterministic inputs
   - collect outputs
   - serialize/compare traces

### Integration Point Recommendation

Implement the orchestration in `crates/xtask` first:
- easiest place for corpus walking, snapshot IO, diff summaries
- keeps runtime validation tooling out of user-facing CLI

Optional later extraction:
- `crates/testing` helper module or `compiler::testing` APIs for reuse in unit
  tests without duplicating harness logic

---

## Data Model and Trace Format

### Trace File Format (v1)

Use a simple, explicit JSON format for ease of review/debugging.

Suggested schema:

```json
{
  "schema_version": 1,
  "dsp": "tests/corpus/rep_31_extended_primitives.dsp",
  "backend": "interp",
  "pipeline": {
    "signal_fir_lane": "fast-lane"
  },
  "runtime": {
    "sample_rate": 48000,
    "block_size": 64,
    "num_blocks": 4
  },
  "scenario": {
    "name": "impulse",
    "inputs": 1,
    "outputs": 1
  },
  "outputs": [
    [0.0, 1.0, 0.5, 0.25]
  ]
}
```

Notes:
- Store full output traces (not hashes) in v1 for debuggability.
- JSON is acceptable at this scale; optimize later only if CI becomes slow/heavy.

### Snapshot Storage Layout

Recommended path:

- `tests/runtime_traces/`
  - `METADATA.toml`
  - `rust/`
  - `cpp/` (future optional)

Example:

- `tests/runtime_traces/rust/rep_31_extended_primitives/impulse.json`
- `tests/runtime_traces/rust/rep_31_extended_primitives/zeros.json`

This mirrors the spirit of `tests/golden/` while keeping runtime traces separate
from codegen text goldens.

### Metadata File (`tests/runtime_traces/METADATA.toml`)

Track:
- schema version
- default runtime parameters
- tolerance defaults
- per-case overrides
- enabled scenarios per case
- exclusions and reasons

Example fields:

```toml
schema_version = 1
default_sample_rate = 48000
default_block_size = 64
default_num_blocks = 4
abs_tol = 1e-6
rel_tol = 1e-5

[cases.rep_31_extended_primitives]
scenarios = ["zeros", "impulse", "sine"]
```

---

## Numeric Comparison Rules

### Comparison Strategy

Use tolerant float comparison per sample:

- pass if `abs(a - b) <= abs_tol + rel_tol * max(abs(a), abs(b))`

Default tolerances (v1):
- `abs_tol = 1e-6`
- `rel_tol = 1e-5`

### Additional Rules

- `NaN` handling:
  - exact class check (`NaN` expected vs not)
  - optionally reject mismatched signaling/quiet distinctions (not required v1)
- `Inf` handling:
  - sign must match
- length mismatch:
  - hard failure
- channel count mismatch:
  - hard failure

### Diff Reporting

On mismatch, report:
- file + scenario
- channel index
- sample index
- expected / actual
- absolute error / relative error
- first mismatch only (default) plus optional `--all-mismatches`

---

## Test Input Generation

### Deterministic Scenario Generators (v1)

Implement pure functions that generate `Vec<Vec<f32>>` input blocks:

- `zeros(num_inputs, total_samples)`
- `impulse(num_inputs, total_samples, amplitude=1.0)`
- `ramp(num_inputs, total_samples)` (per-channel phase offset optional)
- `sine(num_inputs, total_samples, sr, freq_hz, phase)`

### Future Scenarios (v2+)

- seeded pseudo-random noise (fixed seed)
- square wave
- multi-tone
- per-channel distinct stimuli for routing diagnostics
- UI event scripted scenario (requires control API support)

---

## Execution Harness Design

### Proposed `xtask` Commands

Add new commands to `crates/xtask`:

1. `interp-trace-gen`
   - generate Rust runtime trace snapshots
2. `interp-trace-check`
   - compare current Rust runtime traces against stored Rust snapshots
3. `interp-trace-check-cpp` (future)
   - compare current Rust runtime traces against C++ reference snapshots
4. `interp-trace-gen-cpp` (future)
   - generate C++ reference traces (if/when a C++ trace runner exists)

Optional convenience:
- `interp-trace-check --lane legacy`
- `interp-trace-check --lane fast`
- `interp-trace-diff-lanes` (legacy vs fast-lane, no snapshot writes)

### Suggested CLI Options

For `interp-trace-check` / `gen`:

- `--filter <glob-or-regex>` (subset of corpus)
- `--case <path>` (single DSP)
- `--scenario <name>` (repeatable)
- `--sample-rate <hz>`
- `--block-size <n>`
- `--num-blocks <n>`
- `--lane <legacy|fast>`
- `--update` (for `gen`, optional overwrite behavior)
- `--report-json <path>` (machine-readable CI report)
- `--fail-fast`

### Execution Flow (per case)

1. Load DSP source from `tests/corpus/...`
2. Compile to `interp` with configured lane
3. Instantiate DSP instance
4. Initialize runtime (sample rate, instance init flow)
5. Generate deterministic input buffers
6. Run `compute()` over N blocks
7. Collect output samples
8. Compare or write snapshot
9. Emit structured result

---

## Differential Modes (Recommended)

### Mode A: Snapshot Validation (Rust vs Rust snapshots)

Purpose:
- catch regressions after accepted behavior is snapshotted

### Mode B: Lane Differential (Rust legacy vs Rust fast-lane)

Purpose:
- validate migration correctness continuously
- useful before/while C++ parity is incomplete

Behavior:
- compile same DSP twice (legacy/fast)
- run same scenario and runtime params
- compare traces with same tolerance policy

### Mode C: Reference Differential (Rust vs C++ traces) (future)

Purpose:
- stronger oracle for semantic parity

Note:
- requires a C++ trace runner or export path (not part of v1)

---

## FIR Checker Integration (Key for Future Passes)

This trace harness should support validating semantic-preserving FIR passes
such as the `FunctionInliner`.

Recommended optional hooks:

1. `verify_fir_before = true` (default in compiler pipeline if enabled)
2. `apply_fir_passes = [...]` (future)
3. `verify_fir_after = true`
4. run `interp` and compare traces

This gives a strong workflow:
- structural validity (`checker.rs`)
- semantic equivalence (runtime traces)

---

## CI Integration Plan

### CI Stage Layout (Recommended)

1. **Fast CI job** (mandatory)
   - curated subset (small)
   - `interp-trace-check`
   - 1–2 scenarios per DSP
   - completes quickly

2. **Extended CI job** (optional/nightly)
   - larger subset
   - more scenarios
   - lane differential checks

3. **Reference parity job** (future)
   - C++ trace comparisons on selected cases

### Failure Reporting in CI

Output should include:
- summary counts
- per-case status
- first mismatch details
- path to diff report JSON/artifacts if generated

---

## Implementation Plan (Phased)

### Phase 0 — Design Validation and Inventory

Deliverables:
- confirm `interp` API path for programmatic compile + execute
- choose trace snapshot directory structure
- choose JSON schema v1
- select initial corpus subset and scenarios

Pass criteria:
- written design decisions in this document (or follow-up update)
- 5–10 candidate DSPs listed with rationale

### Phase 1 — Minimal Trace Harness (No Snapshots Yet)

Deliverables:
- `xtask` command prototype (single DSP, single scenario)
- compile DSP to `interp`, run, print/serialize trace
- deterministic inputs

Pass criteria:
- can run `rep_31_extended_primitives.dsp` and dump trace
- repeated runs produce identical trace files

### Phase 2 — Snapshot Store + Comparator

Deliverables:
- JSON trace writer/reader
- tolerant comparator
- mismatch reporter
- `interp-trace-gen` and `interp-trace-check`

Pass criteria:
- generate snapshots for curated subset
- check passes on clean tree
- intentional perturbation causes readable failure

### Phase 3 — Lane Differential Mode (`legacy` vs `fast-lane`)

Deliverables:
- mode/command to compare traces without snapshots
- configurable lane pair
- CI-friendly summary

Pass criteria:
- selected corpus subset passes lane diff
- failures identify DSP/scenario/sample index

### Phase 4 — CI Integration

Deliverables:
- CI job definition(s)
- documentation in `crates/xtask/README.md` and/or root docs
- stable runtime trace metadata file

Pass criteria:
- CI green with runtime trace checks enabled
- execution time acceptable

### Phase 5 — Expansion and Hardening (Follow-up)

Deliverables:
- larger corpus subset
- more scenarios
- optional C++ reference trace path
- optional FIR pass equivalence hooks

Pass criteria:
- sustained low flakiness
- clear triage workflow for runtime mismatches

---

## Testing Strategy for the Harness Itself

### Unit Tests

- scenario generators (`zeros`, `impulse`, `ramp`, `sine`)
- float comparator (normal values, NaN, Inf, tolerance edges)
- trace JSON parse/serialize roundtrip
- metadata parsing with overrides

### Integration Tests

- single known DSP trace generation + check
- mismatch diagnostics formatting
- lane differential on a simple DSP (`process = +;`)

### Regression Tests

Add targeted regression entries when runtime bugs are found:
- sample index mismatch due to state update order
- cast bug in FIR lowering
- table index normalization issue

---

## Risks and Mitigations

### Risk 1: Flaky Floating-Point Comparisons

Mitigation:
- tolerance-based comparison
- fixed runtime parameters
- avoid platform-dependent non-deterministic inputs in v1

### Risk 2: Harness Tests Backend + Harness Bug Together

Mitigation:
- keep comparator and input generators unit-tested
- add differential mode (legacy vs fast-lane)
- later add C++ reference traces for a subset

### Risk 3: CI Runtime Cost

Mitigation:
- curated subset in mandatory CI
- nightly extended suite
- scenario count tuning

### Risk 4: Unsupported DSP Features in `interp`

Mitigation:
- metadata-driven exclusions with explicit reasons
- track unsupported cases and close gaps incrementally

---

## Acceptance Criteria (v1)

The plan is considered implemented when all of the following are true:

1. `xtask` can generate and check runtime traces for a curated corpus subset
   using the `interp` backend.
2. Traces are deterministic and stored in a documented snapshot format.
3. Comparisons use explicit numeric tolerances and produce actionable diffs.
4. CI runs at least one runtime-trace validation job.
5. Documentation explains how to regenerate/check traces and how to triage
   failures.

---

## Suggested Follow-Up Work (After v1)

1. Add `legacy` vs `fast-lane` differential mode to CI for migration safety.
2. Add C++ reference trace generation/checking for a small parity subset.
3. Add FIR-pass equivalence mode (pre/post inliner traces).
4. Add scripted UI-control event scenarios once control injection is exposed.
5. Add performance baselines (execution time budgets) for the interpreter path.

---

## Open Questions (To Resolve Before Implementation Starts)

1. Snapshot location:
   - reuse `tests/golden/` subtree or create dedicated `tests/runtime_traces/`?
   - recommendation: dedicated `tests/runtime_traces/`
2. JSON vs compact binary trace format:
   - recommendation: JSON first for debuggability
3. Default lane for snapshots:
   - recommendation: `fast-lane` for forward progress, plus optional lane-diff
4. How much corpus to include in mandatory CI:
   - recommendation: start with 8–12 DSPs
5. Should `xtask` support per-case runtime parameter overrides in v1?
   - recommendation: yes (via metadata), but only implement if needed by the
     initial subset

