# UI IR Architecture Contract (Rust)

**Status**: active design contract for grouped UI parity rewrite.

**Reference C++ baseline**: `master-dev-ocpp-od-fir-2-FIR19` (`8eebea429`)

**Primary C++ anchors**:

- `/Users/letz/Developpements/RUST/faust/compiler/propagate/propagate.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/signals/signals.hh`
- `/Users/letz/Developpements/RUST/faust/compiler/signals/signals.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/transform/signalFIRCompiler.hh`
- `/Users/letz/Developpements/RUST/faust/compiler/generator/instructions_compiler.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/generator/compile.cpp`

## 1. Goal

Define the canonical Rust architecture for grouped UI so that:

- grouped UI becomes a first-class compilation artifact,
- `buildUserInterface` no longer depends on backend-local reconstruction from
  flat widgets,
- the active Rust compile path converges behaviorally toward the C++ model,
- UI ownership and phase boundaries are frozen before implementation work.

This document is the Phase 0 gate for grouped UI parity work.

## 2. Frozen decisions

The following decisions are frozen by this architecture note.

### 2.1 UI is a first-class IR, not a backend heuristic

Rust will introduce a dedicated UI IR as the canonical source of truth for UI
layout and UI declarations.

Backend-local heuristics such as:

- "if widgets exist but no groups exist, synthesize a root group here"
- "recover group structure from widget leaves alone"

are not allowed in the target architecture.

### 2.2 UI IR is introduced at the `propagate` boundary

The new canonical ownership point is:

- `eval` still produces evaluated box syntax with explicit groups/widgets,
- `propagate` consumes that syntax,
- `propagate` produces both:
  - DSP signal outputs,
  - UI IR and control metadata.

This is the closest Rust ownership point to the C++ design, where UI path
context is carried during propagation.

### 2.3 `signals` is not the owner of UI layout

Rust `signals` will no longer be the owner of UI group structure.

Target rule:

- `signals` carries DSP/control semantics only,
- UI layout, grouping, labels, metadata, and ordering belong to UI IR,
- `signals` may reference controls by stable IDs, but it does not own layout.

This is a deliberate architectural adaptation relative to C++ internals, but it
is the canonical Rust target.

### 2.4 Canonical grouped UI root is constructed once, early

Root-group synthesis, when required by Faust semantics, happens exactly once
while constructing canonical UI IR.

It does not happen in FIR lowering or backend code generation.

This means the current fallback in
`crates/transform/src/signal_fir/module.rs` is transitional and must be removed
or reduced to an adapter during migration.

### 2.5 UI labels and metadata are owned by UI IR

By the time UI IR is created:

- labels have already been evaluated/interpolated,
- metadata embedded in labels has already been extracted,
- UI IR stores:
  - simplified display labels,
  - metadata declarations attached to the owning group/widget node.

No later phase may reparse raw label strings to rediscover metadata.

### 2.6 FIR consumes explicit UI IR, not flat widget side-effects

`transform` / signal-to-FIR lowering will consume explicit UI IR and emit FIR UI
instructions from it.

No backend may infer group nesting from DSP/control signal structure.

### 2.7 Deterministic control identity is mandatory

Every UI control must receive a deterministic `ControlId`.

`ControlId` assignment:

- must be stable across deterministic source traversal,
- must not depend on hash map iteration order,
- must not depend on raw `TreeId` values from unrelated arenas,
- must be suitable as the join key between DSP control references and UI IR.

## 3. C++ behavior to preserve

The relevant C++ property is not a specific data layout, but a behavioral
contract:

- UI context survives propagation,
- widgets are associated with a group path,
- `buildUserInterface` is emitted from explicit grouped structure,
- root-group naming follows C++ rules rather than backend-local defaults.

Important C++ observations:

- `signals.hh` and `signals.cpp` document/store UI path in clock environments,
- `propagate.cpp` threads `path` during propagation,
- `signalFIRCompiler.hh` consumes widget signals together with path context,
- `instructions_compiler.cpp` / `compile.cpp` emit explicit open/close box
  instructions,
- at root level, empty labels are resolved with C++ root naming rules, not by a
  generic module-name fallback.

Rust does not need to mirror the exact C++ internal representation, but it must
mirror these observable semantics.

## 4. Canonical Rust architecture

### 4.1 New dedicated crate ownership

Introduce a dedicated crate:

- `crates/ui`

Canonical API pattern:

- `UiBuilder`
- `UiMatch` + `match_ui`
- `UiId = TreeId`
- `UiProgram`

This follows the same builder/matcher discipline as:

- `boxes`
- `signals`
- `fir`

and keeps the UI layer aligned with the TreeArena-based architecture already
used across the Rust port.

### 4.2 Canonical products of `propagate`

`propagate` no longer conceptually returns "just signals".

Target output shape:

```rust
pub struct PropagateOutput {
    pub signals: Vec<SigId>,
    pub ui: UiProgram,
}
```

Equivalent naming is acceptable, but the ownership split is frozen:

- DSP signals
- UI IR

must both be explicit products of the propagation boundary.

### 4.3 `UiProgram` source-of-truth model

`UiProgram` is the canonical grouped UI artifact.

It contains:

- one canonical root UI group,
- one arena-backed tree of groups/widgets,
- one stable control registry indexed by `ControlId`,
- optional program-level metadata needed for root naming and validation.

Target top-level shape:

```rust
pub struct UiProgram {
    pub arena: TreeArena,
    pub root: UiId,
    pub controls: Vec<ControlSpec>,
}
```

Exact field names may vary, but the semantics are fixed.

### 4.4 UI node families

The canonical UI IR must represent at least:

- `Group`
  - kind: `Vertical | Horizontal | Tab`
  - simplified label
  - metadata declarations
  - ordered children
- `InputControl`
  - `Button`
  - `Checkbox`
  - `HSlider`
  - `VSlider`
  - `NumEntry`
- `OutputControl`
  - `HBargraph`
  - `VBargraph`
- `Soundfile`

Recommended node shape:

```rust
enum UiNodeKind {
    Group {
        kind: UiGroupKind,
        label: String,
        metadata: Vec<(String, String)>,
        children: Vec<UiId>,
    },
    InputControl {
        control: ControlId,
    },
    OutputControl {
        control: ControlId,
    },
    Soundfile {
        control: ControlId,
    },
}
```

This shape keeps layout in the tree and control parameters in the registry.

### 4.5 `ControlSpec` ownership

`ControlSpec` is the source of truth for one control's semantics.

It owns:

- stable `ControlId`,
- control kind,
- simplified label,
- control metadata,
- numeric ranges / init / step where applicable,
- declared UI direction (`input`, `output`, `soundfile`),
- any FIR/backend-relevant control configuration.

Recommended shape:

```rust
pub struct ControlSpec {
    pub id: ControlId,
    pub kind: ControlKind,
    pub label: String,
    pub metadata: Vec<(String, String)>,
    pub range: Option<ControlRange>,
}
```

The exact split between `UiNode` and `ControlSpec` may vary, but:

- layout belongs to the UI tree,
- control semantics belong to the control registry.

## 5. Phase boundary contract

Pipeline contract becomes:

`parse -> boxes -> eval -> propagate -> normalize -> type/interval -> transform -> fir -> backend`

with grouped UI carried explicitly in parallel after `propagate`.

### 5.1 Phase-by-phase ownership table

| Phase | Consumes | Produces | UI ownership status |
|---|---|---|---|
| `parser` | source | box AST | no canonical UI ownership yet |
| `eval` | box AST | evaluated boxes | explicit groups/widgets still in box syntax |
| `propagate` | evaluated boxes | `PropagateOutput { signals, ui }` | **canonical UI owner begins here** |
| `normalize` | `PropagateOutput` | normalized signals + unchanged `ui` | UI passthrough |
| `interval` / signal typing | normalized signals + unchanged `ui` | typed signals + unchanged `ui` | UI passthrough |
| `transform` | typed/prepared signals + `ui` | FIR module | lowers DSP and UI from parallel artifacts |
| `fir` | FIR module | FIR UI instructions already explicit | no UI reconstruction |
| `backend` | FIR module | text/binary output | emits UI only from FIR |

### 5.2 Hard boundary rule

After `propagate`:

- no phase may need to inspect group/widget box syntax to understand UI,
- no phase may need to recover UI grouping from raw labels or widget leaves.

`UiProgram` is the source of truth from that point onward.

## 6. Signals contract after the rewrite

### 6.1 Signals becomes DSP/control-semantic only

The current signal widget nodes carrying labels/ranges are transitional.

Target signal contract:

- no signal node owns group hierarchy,
- no signal node owns widget display labels,
- signal nodes reference controls by `ControlId`.

Recommended target node families:

- `SigMatch::ControlRef(ControlId)` for input controls,
- `SigMatch::ControlSink(ControlId, value)` for output controls/bargraphs,
- optional dedicated soundfile reference node if needed for DSP semantics.

Equivalent naming is acceptable, but the ownership split is frozen.

### 6.2 Migration policy for existing widget signal nodes

Current widget-like signal variants such as:

- `Checkbox`
- `Button`
- `HSlider`
- `VSlider`
- `NumEntry`
- `HBargraph`
- `VBargraph`

must be treated as transitional compatibility surface.

Target policy:

- they may coexist during migration,
- but backends must not depend on them for grouped UI structure,
- long-term target is `ControlId`-based signal references.

## 7. Root-group policy

### 7.1 Canonical rule

`UiProgram` always has exactly one canonical root group.

This root is:

- the explicit top-level group when the Faust program already provides one,
- otherwise a synthesized root group created during UI IR construction using
  C++ root naming rules.

### 7.2 Root synthesis rules

When synthesis is required:

- orientation follows the C++ default root-group rule,
- the root label follows C++ behavior:
  - use declared `name` metadata when appropriate,
  - otherwise fall back to filename-derived module name,
  - never use a backend-local arbitrary default.

### 7.3 Consequence for Rust implementation

The current helper:

- `maybe_wrap_ui_in_root_group(...)`

is not part of the target architecture.

It is allowed only as a migration adapter until `UiProgram` root synthesis is
implemented. The end state is:

- root-group synthesis belongs to UI IR construction,
- FIR lowering and backends do not synthesize root groups.

## 8. Label and metadata policy

### 8.1 Ownership point

Label interpolation stays in `eval`.

Metadata extraction for UI labels is completed before or during UI IR
construction in `propagate`.

### 8.2 Canonical storage rule

UI IR stores:

- simplified display label,
- extracted metadata declarations,
- no raw unevaluated label template.

### 8.3 Forbidden downstream behavior

Forbidden after UI IR construction:

- reparsing `%...` substitutions,
- reparsing `[key:value]`-style metadata from labels,
- inferring group path from slash-split labels alone.

## 9. Transform and FIR contract

### 9.1 Transform input

`transform` consumes:

- prepared/typed DSP signals,
- `UiProgram`.

It does not synthesize grouped UI from DSP signals.

### 9.2 FIR output

FIR remains the only backend-facing IR.

UI in FIR is emitted from `UiProgram` into explicit FIR nodes:

- `OpenBox`
- `CloseBox`
- `AddButton`
- `AddSlider`
- `AddBargraph`
- `AddSoundfile`
- `AddMetaDeclare`

### 9.3 Backend rule

Backends consume FIR UI instructions only.

No backend may inspect `signals` or `UiProgram` directly to recover UI
structure in production code paths.

## 10. Public/internal API classification

The following mapping statuses are frozen.

| Surface | Status | Rationale |
|---|---|---|
| `eval` explicit group/widget preservation | `1:1` behaviorally | already matches C++ intent |
| `propagate` returning UI as explicit parallel artifact | `adapted` | Rust uses explicit IR rather than C++ clockenv representation |
| `signals` owning only control references | `adapted` | cleaner Rust ownership split, same external behavior target |
| FIR explicit UI instructions | `1:1` behaviorally | same backend-visible contract |
| backend UI emission from FIR only | `1:1` behaviorally | same output contract as C++ |

External compatibility note:

- `UiProgram` is initially a workspace-internal architectural API,
- not yet a stable user-facing external API.

## 11. Validation contract

Required validation for the rewrite:

- differential C++ comparison on grouped UI corpus cases,
- explicit event-order checks for `buildUserInterface`,
- nested-group parity checks,
- empty-label root-group parity checks,
- metadata-on-group/widget parity checks,
- deterministic `ControlId` and emission-order tests.

Minimum corpus cases:

- `tests/corpus/rep_58_higher_order_named_direct_apply.dsp`
- `tests/corpus/rep_59_higher_order_named_argument_apply.dsp`
- `tests/corpus/rep_17_ui_groups.dsp`
- `tests/corpus/rep_28_nested_ui_groups.dsp`

## 12. Recommended implementation slices

Implementation should follow this order:

1. add failing grouped-UI parity tests
2. add `crates/ui` with canonical builder/matcher API
3. add `PropagateOutput { signals, ui }`
4. add canonical root-group synthesis in UI IR construction
5. add `ControlId`-based signal references and migration adapters
6. lower `UiProgram` to FIR explicit UI statements
7. remove backend/root-group fallback heuristics
8. widen C++ differential coverage

## 13. Non-goals

This architecture change does not aim to:

- expose a stable end-user UI API immediately,
- redesign audio-rate DSP semantics,
- change backend-visible FIR UI node families,
- copy the exact C++ internal storage layout for clock environments.

The target is behavioral parity with clearer Rust ownership.
