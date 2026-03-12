# UI Group Path Parity Plan (2026-03-12)

Status: proposed execution plan

Scope: converge the Rust UI lowering path toward the C++ model by preserving
explicit UI group hierarchy through the compile pipeline instead of rebuilding
`buildUserInterface` from flat widgets plus a synthetic root-group fallback.
Because the preferred direction is now a fuller architectural convergence
toward the C++ model, implementation must start by writing and freezing a
dedicated architecture note before code changes.

Frozen architecture note:

- `porting/ui-ir-architecture-contract-2026-03-12-en.md`

Reference C++ baseline: `master-dev-ocpp-od-fir-2-FIR19` (`8eebea429`)

Reference C++ source roots:

- `/Users/letz/Developpements/RUST/faust/compiler/propagate/propagate.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/signals/signals.hh`
- `/Users/letz/Developpements/RUST/faust/compiler/signals/signals.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/generator/instructions_compiler.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/generator/compile.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/transform/signalFIRCompiler.hh`

## 1. Problem statement

The current Rust pipeline preserves UI groups at the box/eval level, but loses
them before backend code generation.

Observed on:

- `tests/corpus/rep_58_higher_order_named_direct_apply.dsp`
- `tests/corpus/rep_59_higher_order_named_argument_apply.dsp`

Expected C++ UI:

- `openHorizontalBox("top")`
- `addCheckButton("c", ...)`
- `closeBox()`

Current Rust UI:

- `openVerticalBox("<module_name>")`
- `addCheckButton("c", ...)`
- `closeBox()`

The Rust behavior is wrong even though the functional signal result is correct.

The loss occurs because:

- `eval` still returns `hgroup("top", checkbox("c"))`
- the Rust signal IR currently exposes widgets but not the surrounding UI group
  hierarchy
- the signal-to-FIR lowering path therefore emits only widget statements
- `maybe_wrap_ui_in_root_group(...)` then injects a synthetic root vertical box
  because no explicit groups were emitted

This creates a parity gap in `buildUserInterface`, even when signal semantics
are otherwise correct.

## 2. C++ reference behavior

The relevant C++ behavior is not based on reconstructing UI groups late from
flat widgets.

Instead, C++ carries UI context during propagation:

- the clock environment explicitly stores the UI group path
- widgets are recognized together with their path information
- UI generation later traverses a structured UI tree / element stream and emits
  the exact `open*Box(...)` / widget / `closeBox()` sequence

Relevant reference points:

- `signals.hh` documents the clock environment as carrying `path`
- `signals.cpp` exposes `getClockenvPath(...)`
- `propagate.cpp` threads `path` through `realPropagate(...)`
- `signalFIRCompiler.hh` consumes signal widgets together with `path`
- `instructions_compiler.cpp` and `compile.cpp` emit explicit group open/close
  instructions

This means the C++ compiler does not depend on a fallback heuristic to recover
UI structure.

## 3. Current Rust state

Current Rust behavior is an `adapted` implementation, not `1:1`.

What Rust currently does well:

- parser preserves `vgroup/hgroup/tgroup`
- `eval` preserves group nodes after label interpolation and higher-order
  application
- widgets lower correctly as controls

What Rust currently lacks:

- no explicit UI path/orientation is carried after box lowering
- no dedicated intermediate representation for grouped UI structure
- FIR lowering records widgets directly in `ui_statements`
- a synthetic root group is injected whenever grouped structure is absent from
  those statements

This is sufficient for many corpus cases that only assert widget presence, but
it is insufficient for C++-parity `buildUserInterface`.

## 4. Porting objective

Target objective:

- preserve explicit UI grouping semantics from evaluated boxes through backend
  UI generation
- reproduce the C++ `buildUserInterface` structure for grouped widgets,
  including orientation and nesting
- keep the current synthetic root-group behavior only for cases that truly have
  no explicit group structure

Non-goals for the initial parity step:

- redesign the public signal IR API across the whole workspace
- fully replicate the exact C++ internal data layout
- rewrite all UI lowering around a new top-level subsystem in one pass

## 5. Recommended convergence strategy

Recommended approach: treat grouped UI as a first-class compilation artifact
instead of a late backend heuristic.

This means:

- writing a proper architecture note first
- freezing the ownership and lifecycle of grouped UI information across phases
- then implementing the chosen design under explicit phase contracts

The architecture note must freeze at least these decisions:

- where the Rust UI IR is introduced
- which phase owns the UI IR as source of truth
- whether `signals` also carries UI information or UI remains a parallel-only
  artifact
- what each phase consumes and produces once UI is explicit
- whether grouped UI remains internal-only or is promoted into shared IR/public
  APIs
- when the synthetic root-group fallback is still allowed

This is intentionally closer to the C++ semantics than the current fallback
design, while avoiding an ad hoc local fix that would later constrain a fuller
rewrite.

### 5.1 Target model

Each widget that reaches the FIR lowerer should carry:

- its leaf widget kind and label
- its ordered enclosing UI group path
- each group's orientation (`vertical`, `horizontal`, `tab`)
- already-evaluated labels, not raw unevaluated box syntax

This is semantically equivalent to the C++ idea of carrying `path` through the
pipeline, but the Rust architecture note may choose either:

- a shared UI-aware IR crossing several phases
- or a parallel UI artifact explicitly paired with DSP lowering outputs

Both are acceptable if the phase boundaries and invariants are frozen first.

### 5.2 Why the architecture note is mandatory first

Without a frozen architecture note, the implementation would risk:

- introducing UI data too late and depending again on reconstruction heuristics
- widening the wrong public/internal APIs
- duplicating UI context in both signals and transform without a clear owner
- making later convergence toward the C++ model harder, not easier

The architecture note should therefore be treated as a Phase 0 gate for this
subsystem work.

### 5.3 Why this convergence direction is still pragmatic

This direction:

- restores the missing parity behavior
- keeps runtime cost negligible because UI reconstruction happens at compile
  time only
- removes the current backend heuristic as the source of truth
- creates a clean foundation for grouped UI parity across backends

## 6. Representation plan

Recommended first design space to evaluate in the architecture note:

- Option A: shared UI-aware IR introduced before `signals`
- Option B: `signals` carries widget + path information explicitly
- Option C: UI remains parallel to `signals`, with paired lowering products

Default recommendation for the architecture note:

- prefer one explicit UI artifact with a clearly documented owner
- avoid making flat FIR UI statements the earliest point where grouping exists
- avoid heuristic reconstruction from widget leaves alone

Suggested internal types:

```rust
enum UiGroupKind {
    Vertical,
    Horizontal,
    Tab,
}

struct UiGroupFrame {
    kind: UiGroupKind,
    label: String,
}

struct UiPath {
    groups: Vec<UiGroupFrame>,
}
```

One widget-side descriptor can then carry:

- `UiPath`
- leaf widget FIR payload
- stable source order index when needed for deterministic reconstruction

This representation is illustrative only. The architecture note may instead
place a structurally similar type in a new module/crate if that produces
clearer ownership.

## 7. Implementation stages

### Stage 0: write and freeze the architecture note

Before adding implementation code:

- write a dedicated architecture note under `porting/`
- freeze:
  - where UI IR is introduced
  - which phase owns it
  - whether `signals` also carries UI or UI is parallel-only
  - what each affected phase consumes and produces
  - what remains internal vs potentially public/shared
  - when synthetic root-group fallback is permitted
- record mapping status (`1:1`, `adapted`, `deferred`) for each touched phase
  boundary

Pass criteria:

- no unresolved ownership ambiguity remains
- the chosen design is explicit enough to code against without guessing
- the note identifies the crates/files expected to change

### Stage 1: pin exact parity targets

Before changing lowering logic:

- freeze the current divergence with tests on:
  - `rep_58_higher_order_named_direct_apply.dsp`
  - `rep_59_higher_order_named_argument_apply.dsp`
- add at least one nested-group corpus case
- record the expected C++ UI event order for each case

Pass criteria:

- Rust tests fail today for the right reason and describe exact target UI
  structure

### Stage 2: implement the frozen carry point and UI owner

Implement internal collection of grouped UI information:

- descend through explicit `vgroup/hgroup/tgroup`
- accumulate a path stack
- when a widget is reached, record the widget plus its full path

Important constraints:

- preserve nesting order
- preserve orientation
- preserve already-evaluated label text
- keep deterministic emission order

Pass criteria:

- grouped widgets can be observed internally with full path information
- ownership matches the architecture note rather than an ad hoc shortcut

### Stage 3: reconstruct FIR UI box events from explicit UI data

Replace direct flat widget emission with grouped emission:

- compare the next widget path against the current open-group stack
- emit `CloseBox` events until the common prefix is reached
- emit `OpenBox` events for newly-entered groups
- emit the widget statement
- close remaining groups at the end

This mirrors standard tree-from-path reconstruction and should match the C++
observable behavior if the path data is correct.

Pass criteria:

- FIR dump for grouped corpus cases contains explicit `OpenBox` / `CloseBox`
  with the right labels and orientations

### Stage 4: restrict the synthetic root-group fallback

Once explicit grouped FIR UI exists:

- keep `maybe_wrap_ui_in_root_group(...)` only for truly group-less widget sets
- ensure it does not trigger when explicit group paths were reconstructed

Pass criteria:

- `rep_58` and `rep_59` no longer receive synthetic root vertical boxes
- group-less fixtures still receive the synthetic root group if required by the
  current Rust backend contract

### Stage 5: differential validation against C++

Add or extend differential checks so that Rust and C++ agree on UI structure.

Checks should cover:

- open/close group order
- orientation (`vgroup` vs `hgroup` vs `tgroup`)
- group labels
- widget labels
- absence of spurious synthetic root groups

Pass criteria:

- Rust/C++ UI event streams match for targeted corpus cases

### Stage 6: promote the chosen architecture consistently

After the first implementation is validated:

- extend the chosen UI architecture to other affected backends/phases as needed
- remove now-redundant local heuristics
- document final ownership and invariants in code comments and porting docs

Decision criteria:

- number of backends needing grouped UI parity
- whether the chosen owner boundary remains stable in practice
- maintenance burden of any remaining duplicated UI logic
- future parity work around metadata, empty labels, and nested path handling

## 8. Recommended test matrix

Minimum required corpus coverage:

- `tests/corpus/rep_58_higher_order_named_direct_apply.dsp`
- `tests/corpus/rep_59_higher_order_named_argument_apply.dsp`
- `tests/corpus/rep_17_ui_groups.dsp`
- `tests/corpus/rep_28_nested_ui_groups.dsp`

Additional focused cases to add if missing:

- one empty-label group case
- one group-label-with-metadata case
- one mixed sibling-group case with shared path prefix
- one widget set with no explicit groups to preserve fallback coverage

## 9. Risks and failure modes

Main semantic risks:

- wrong nesting order when sibling widgets share only part of the path
- losing orientation while preserving labels
- unstable event ordering across hash-consed/shared nodes
- incorrect handling of empty group labels
- incorrect interaction with label metadata stripping

Main implementation risks:

- placing the carry point too late and having to reconstruct groups from
  incomplete data
- placing it too early and duplicating too much traversal logic already handled
  elsewhere

## 10. Performance expectations

Expected performance impact is negligible:

- UI path reconstruction is compile-time only
- `buildUserInterface` generation is not part of the audio-rate runtime path
- widget/group counts are small compared with the rest of the compile pipeline

This plan is therefore parity-driven, not performance-sensitive.

## 11. Recommended commit slicing

Suggested small coherent commits:

1. add the architecture note freezing UI-IR ownership and phase contracts
2. add failing UI parity tests for grouped widgets
3. implement the chosen UI owner/carry point
4. reconstruct FIR `OpenBox` / `CloseBox` from explicit UI data
5. narrow synthetic root-group fallback
6. add/expand C++ differential UI validation

## 12. Migration status classification

Current status:

- UI grouping after `eval`: `adapted`
- `buildUserInterface` grouped parity: `deferred`

Target status after this plan:

- grouped UI behavior in the active Rust compile path: `1:1` behaviorally
- internal representation: still `adapted`, unless later promoted closer to
  the C++ clockenv/path architecture
