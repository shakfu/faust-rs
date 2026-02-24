# Interp Executor Dual-Mode Hardening Plan (Fast + Checked Runtime Paths)

## 1. Goal

Introduce and generalize a **dual execution model** for the Rust `interp`
backend executor:

- **Fast path**: `execute_*` APIs (optimized, minimal checks, panic on invalid
  bytecode/runtime invariants)
- **Checked path**: `try_execute_*` APIs (structured runtime errors, no panics
  for recoverable/interpretable failures)

This enables:

- robust runtime validation workflows (`xtask`, differential trace checks),
- better diagnostics for FIR/bytecode bugs,
- preserved performance for the production/benchmark interpreter path.

## 2. Why This Is Needed

Current issues observed in practice (example: `process = int(_) + 1;`):

- FIR may pass verifier with type warnings only (e.g. `FIR-B03`)
- compiled bytecode then violates runtime stack discipline
- interpreter hits `Option::unwrap()` panic (opaque failure)

We do **not** want to fix such cases by adding implicit runtime casts.
Instead, we want:

- strict semantics
- explicit runtime failure reporting
- actionable diagnostics (`opcode`, `pc`, `block`, stack kind)

## 3. High-Level Design

### 3.1 Two runtime execution API families

#### Fast path (existing public behavior)

- `execute_block(...)`
- `execute_block_io(...)`
- `FbcDspInstance::compute(...)`

Behavior:

- assumes valid bytecode/runtime invariants
- may panic on invalid stacks/indices/branches
- optimized for speed and parity with current executor flow

#### Checked path (new/expanded)

- `try_execute_block(...) -> Result<(), FbcExecError>`
- `try_execute_block_io(...) -> Result<(), FbcExecError>`
- `FbcDspInstance::try_compute(...) -> Result<(), FbcDspRuntimeError>`

Behavior:

- detects runtime failures and returns structured errors
- no implicit semantic repair (no auto-casts, no silent fallback behavior)
- intended for validation, diagnostics, CI, fuzzing, and debugging tools

### 3.2 Compatibility policy

- Keep `execute_*` APIs for backward compatibility and fast mode.
- Checked APIs are additive.
- Existing code can migrate incrementally (`xtask` uses checked mode first).

## 4. Error Model (Structured Runtime Errors)

### 4.1 Existing seed implementation (already started)

- `FbcExecError`
- `FbcStackKind` (`Int`, `Real`)
- `FbcDspRuntimeError` wrapping executor errors at the instance layer

### 4.2 Target error categories

The checked path should cover all opcodes that can fail due to malformed or
inconsistent bytecode/runtime assumptions.

Recommended categories:

- `stack_underflow`
- `stack_overflow` (if logical stack capacity is enforced)
- `heap_oob` (int/real heap)
- `io_oob` (input/output channel or sample index out of bounds)
- `missing_branch_target`
- `invalid_block_id`
- `invalid_pc`
- `unsupported_runtime_feature`
- `internal_invariant`

### 4.3 Required error context

Each structured error should carry enough context to debug FIR/bytecode issues:

- opcode
- block id
- program counter (`pc`)
- optional stack kind (`Int`, `Real`, `Addr`)
- optional heap kind/index
- optional I/O channel/sample

## 5. Hot-Path Aware Generalization Strategy

The key requirement is to improve robustness **without** forcing the fast path
to pay the full cost of pervasive checks.

### 5.1 Principle

- Generalize checks first in the `try_*` path
- Keep `execute_*` fast and panic-based
- Use `try_*` in developer/validation tooling
- Measure before deciding whether additional checks can be promoted into the fast path

### 5.2 Priority rollout (by impact and frequency)

#### Phase A (highest value, immediate)

Cover opcodes and operations that most directly surface real-world failures in
runtime validation:

- Audio I/O:
  - `LoadInput`
  - `StoreOutput`
- Casts / bitcasts:
  - `CastReal`, `CastInt`, bitcast variants
- Control flow:
  - `If`, `Select*`, `Loop`, `Return` (branch target / cond stack pops)
- Indexed memory:
  - `LoadIndexedReal/Int`
  - `StoreIndexedReal/Int`

Why first:

- these failures are common in malformed/under-typed FIR scenarios
- they are high-value for `xtask` runtime trace diagnostics

#### Phase B (broad arithmetic coverage)

Generalize stack underflow checking for:

- int/real binops
- comparisons
- unary ops
- extended math ops
- heap/value hybrid math opcodes

Why second:

- high count of `unwrap()` sites
- mostly mechanical conversion once helpers exist

#### Phase C (rare/edge opcodes + hardening)

- block move/shift ops
- block store table data handling
- UI opcodes (if ever reached in execution path)
- unsupported features (soundfile) -> structured error (instead of `unimplemented!`)

Why third:

- lower operational priority for runtime trace validation
- still important for completeness

## 6. Implementation Approach (Avoiding Excess Duplication)

### 6.1 Preferred approach: helper-based checked operations

In `try_execute_block_io(...)`, replace direct `unwrap()` / unchecked indexing
with small helpers:

- stack helpers:
  - `pop_int_checked(...)`
  - `pop_real_checked(...)`
- branch helpers:
  - `branch1_checked(...)`
  - `branch2_checked(...)`
- heap helpers:
  - `load_real_heap_checked(...)`
  - `store_real_heap_checked(...)`
  - `load_int_heap_checked(...)`
  - `store_int_heap_checked(...)`
- I/O helpers:
  - `load_input_checked(...)`
  - `store_output_checked(...)`

Benefits:

- keeps opcode match arms readable
- centralizes error formatting/context
- enables incremental rollout by opcode family

### 6.2 Fast path implementation options

#### Option 1 (simple, lower engineering cost)

- `execute_*` wrappers call `try_execute_*` and panic on `Err`

Pros:

- minimal duplication
- easier maintenance

Cons:

- fast path pays check overhead
- weak separation between perf mode and safe mode

#### Option 2 (target architecture, recommended)

- `execute_*` retains direct `unwrap()` / direct indexing path (fast)
- `try_execute_*` uses checked helpers and returns structured errors

Pros:

- clear performance mode vs debug/validation mode
- preserves current performance characteristics

Cons:

- some duplication (or more complex generic core design)

### 6.3 Optional advanced refactor: single generic core

Longer-term option:

- `exec_core<const CHECKS: bool>(...)`
  - `CHECKS = true` => checked helpers / `Result`
  - `CHECKS = false` => fast operations / panic assumptions

This can reduce duplication, but may complicate readability and borrow checker
interaction. Use only if duplication becomes hard to maintain.

## 7. Performance Expectations

### 7.1 Checked path (`try_*`)

Expected behavior:

- slower than fast path due to:
  - stack checks (`Option -> Result`)
  - bounds checks converted into explicit errors
  - error-context construction on failure paths

This is acceptable for:

- `xtask`
- runtime validation / differential tests
- debugging and bug triage

### 7.2 Fast path (`execute_*`)

Target behavior:

- preserve current speed characteristics
- maintain panic semantics for invariant violations in performance-sensitive use

## 8. Where to Use Which Mode

### 8.1 Checked mode (`try_*`) — use by default

- `xtask` runtime trace workflows:
  - `interp-trace-dump`
  - `interp-trace-gen`
  - `interp-trace-check`
  - `interp-trace-diff-*`
- future fuzzing harnesses
- debug tooling
- CI diagnostics workflows

### 8.2 Fast mode (`execute_*`) — keep for performance

- normal interpreter execution path where performance matters
- benchmarks
- parity/perf comparisons
- internal call sites that intentionally rely on bytecode validity invariants

## 9. Test Strategy

### 9.1 Unit tests (executor)

Add focused tests for structured errors by opcode family:

- stack underflow (`LoadInput`, `StoreOutput`, binops, casts)
- missing branch targets
- invalid block/pc transitions (if representable in tests)
- heap/index OOB (if/when converted to structured errors)

Keep existing successful execution tests unchanged to preserve baseline behavior.

### 9.2 Integration tests (xtask/runtime)

Use known repros (e.g. `process = int(_) + 1;`) to verify:

- without `--strict-fir-types`: structured runtime error (no panic)
- with `--strict-fir-types`: early FIR rejection before runtime

### 9.3 Regression tracking

Document known runtime failures and their status in:

- `tests/runtime_corpus_known_failures/`
- `JOURNAL.md`

## 10. Benchmark / Measurement Plan (Before Broad Rollout)

Measure overhead of `try_*` vs `execute_*` on representative DSPs:

- passthrough (I/O-dominated)
- sine/phasor (math + state)
- primitive-heavy constant DSP (extended math)

Suggested metrics:

- total runtime for N blocks
- relative slowdown (`try` / `fast`)

Purpose:

- quantify cost of checked mode
- justify keeping a separate fast path
- decide whether some checks are acceptable in production path

## 11. Incremental Delivery Plan

### Milestone 1 (done/started)

- Introduce structured error types
- Convert `StoreOutput` stack underflow to `Err`
- Add `try_execute_*` + `try_compute(...)`
- Use checked mode in `xtask`

### Milestone 2 (Phase A complete)

- Harden all Phase A opcode families (I/O, casts, control flow, indexed memory)
- Add unit tests for each failure category

### Milestone 3 (Phase B)

- Cover arithmetic + comparisons + extended math opcodes
- Add broad underflow tests

### Milestone 4 (Phase C)

- Cover remaining rare opcodes and `unimplemented!` runtime paths
- Standardize error categories/context fields

### Milestone 5 (perf validation)

- Compare `execute_*` vs `try_*` performance
- Document results in `JOURNAL.md`

## 12. Design Constraints / Non-Goals

- No implicit auto-casts in the runtime executor
- No silent recovery that changes DSP semantics
- No requirement to make malformed FIR “work”
- Errors should be diagnostic, not semantic patches

## 13. Acceptance Criteria

This dual-mode hardening effort is considered successful when:

1. `try_execute_*` covers all opcode families that can currently panic due to
   malformed bytecode/runtime assumptions.
2. `xtask` runtime trace workflows no longer crash the process on known stack
   discipline failures and instead report structured runtime errors.
3. `execute_*` remains available as a fast path.
4. Performance difference between `execute_*` and `try_execute_*` is measured
   and documented.
5. The design and status are documented in `JOURNAL.md`.

