# Typed `FlatBoxId` Input Plan (2026-03-09)

Status: in progress

Scope: define a typed Rust input contract for `crates/propagate` that matches
the C++ post-`evalprocess -> a2sb -> propagate` boundary instead of accepting a
generic `BoxId`.

Reference C++ baseline: `master-dev-ocpp-od-fir-2-FIR19` (`8eebea429`)

Reference C++ source roots:

- `/Users/letz/Developpements/RUST/faust/compiler/evaluate/eval.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/propagate/propagate.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/propagate/propagate.hh`

## 1. Problem statement

The current Rust `propagate(...)` API accepts a plain `BoxId`:

- `pub fn propagate(arena: &mut TreeArena, box_tree: BoxId, inputs: &[SigId], cache: &mut ArityCache) -> Result<Vec<SigId>, PropagateError>`

That shape is too weak for the actual pipeline contract.

In the C++ compiler, the effective production route is:

- `evalprocess(...)`
- `a2sb(...)`
- `boxPropagateSig(...)`

This means `propagate` does not conceptually accept arbitrary box syntax. It
accepts a stricter first-order box language after evaluation and symbolic
lowering.

Today, Rust encodes that distinction indirectly:

- `eval` documents that `a2sb(...)` lowers residual closures and case matchers
  before propagation,
- `propagate` still has many `UnsupportedBox` branches for forms that should
  never survive the eval boundary,
- a few box families accepted by C++ `propagate` are still unsupported by Rust.

This weakens three things:

- the API contract is implicit rather than typed,
- dead/unreachable branches are mixed with genuine propagation gaps,
- regressions across the `eval -> propagate` boundary are detected late.

## 2. Goal

Introduce a typed post-eval propagation input model so that:

- `crates/propagate` no longer takes an unrestricted `BoxId` in its primary API,
- only box families valid after C++ `a2sb(...)` can enter propagation,
- evaluator-only syntax families become impossible to pass by construction,
- remaining missing C++ propagation families are tracked as explicit
  propagation implementation gaps instead of generic unsupported syntax,
- `TreeArena` hash-consing and structural sharing remain the source of truth,
- the name of the type itself signals that lambda-calculus and pattern-matching
  syntax have already disappeared at this boundary.

Non-goal:

- redesign the signal lowering algorithm itself in this step,
- replace every internal use of `TreeId` in one pass,
- collapse the entire `boxes` crate into a new owned IR,
- duplicate the post-eval box DAG into a second independently-owned tree.

## 3. C++ contract to preserve

The relevant C++ invariant is:

- `evalprocess(...)` computes `a2sb(eval(process, ...))` before propagation,
- `real_a2sb(...)` lowers residual evaluator forms (`abstr`, `case`, closure
  carriers) into first-order symbolic box syntax,
- `realPropagate(...)` then matches only against first-order propagation
  families.

Therefore the intended input language of C++ propagation excludes unresolved:

- identifier lookup syntax,
- application syntax,
- access syntax,
- iterator syntax that has not yet been expanded,
- local-definition syntax,
- residual abstractions / cases / pattern vars,
- residual modulation syntax.

But it still includes runtime-relevant first-order nodes such as:

- `symbolic` and `slot`,
- box composition (`seq`, `par`, `split`, `merge`, `rec`),
- widgets and groups,
- `route`,
- `ondemand`, `upsampling`, `downsampling`,
- `soundfile`,
- foreign functions and foreign state nodes.

## 4. Proposed Rust representation

Recommended first implementation shape:

- add a typed post-eval handle in `crates/propagate`, tentatively:
  - `pub struct FlatBoxId(TreeId);`
- make construction explicit:
  - `try_build_flat_box(...) -> Result<FlatBoxId, FlatBoxBuildError>`
- decode the admissible node family through a restricted internal view rather
  than by exposing the full `BoxMatch` universe to propagation internals.

Rationale:

- this keeps the boundary explicit and local to `propagate`,
- it preserves `TreeArena` hash-consing and structural sharing,
- it avoids duplicating potentially large shared subgraphs into a second owned
  IR,
- it still allows exact whitelisting of allowed node families,
- it cleanly separates:
  - invalid post-eval input,
  - valid-but-not-yet-implemented propagation families.

Recommended top-level shape:

```rust
pub struct FlatBoxId(TreeId);

enum FlatNodeKind {
    Int(i32),
    Real(f64),

    Wire,
    Cut,

    Slot(TreeId),
    Symbolic {
        slot: TreeId,
        body: FlatBoxId,
    },

    Metadata {
        body: FlatBoxId,
        metadata: TreeId,
    },

    Prim0(PropagatePrim0),
    Prim1(PropagatePrim1),
    Prim2(PropagatePrim2),
    Prim3(PropagatePrim3),
    Prim4(PropagatePrim4),
    Prim5(PropagatePrim5),

    ForeignConst {
        ty: TreeId,
        name: TreeId,
        file: TreeId,
    },
    ForeignVar {
        ty: TreeId,
        name: TreeId,
        file: TreeId,
    },
    ForeignFunction(PropagateForeignFunction),

    Button(TreeId),
    Checkbox(TreeId),
    VSlider {
        label: TreeId,
        cur: TreeId,
        min: TreeId,
        max: TreeId,
        step: TreeId,
    },
    HSlider {
        label: TreeId,
        cur: TreeId,
        min: TreeId,
        max: TreeId,
        step: TreeId,
    },
    NumEntry {
        label: TreeId,
        cur: TreeId,
        min: TreeId,
        max: TreeId,
        step: TreeId,
    },
    VBargraph {
        label: TreeId,
        min: TreeId,
        max: TreeId,
    },
    HBargraph {
        label: TreeId,
        min: TreeId,
        max: TreeId,
    },

    Soundfile {
        label: TreeId,
        chan: TreeId,
    },
    Waveform(TreeId),

    VGroup {
        label: TreeId,
        body: FlatBoxId,
    },
    HGroup {
        label: TreeId,
        body: FlatBoxId,
    },
    TGroup {
        label: TreeId,
        body: FlatBoxId,
    },

    Seq(FlatBoxId, FlatBoxId),
    Par(FlatBoxId, FlatBoxId),
    Split(FlatBoxId, FlatBoxId),
    Merge(FlatBoxId, FlatBoxId),
    Rec(FlatBoxId, FlatBoxId),

    Environment,

    Route {
        inputs: TreeId,
        outputs: TreeId,
        route_spec: TreeId,
    },

    Inputs(TreeId),
    Outputs(TreeId),

    Ondemand(FlatBoxId),
    Upsampling(FlatBoxId),
    Downsampling(FlatBoxId),
}
```

Notes:

- `FlatBoxId` is the typed boundary object; it preserves node identity and
  sharing because the source-of-truth remains the interned `TreeId`.
- `FlatNodeKind` is an internal restricted view, not a second owned IR.
- `flat` is intentional terminology: this boundary is reached after
  `eval/a2sb`, so residual lambda-calculus and pattern-matching syntax must no
  longer exist.
- The first version may keep several payloads as `TreeId` to avoid front-loading
  unrelated type migrations.
- Primitive payloads should eventually become dedicated enums instead of
  reusing broad `BoxMatch` reconstruction.
- `Route` should keep the raw route spec first, then optionally gain a decoded
  typed payload once route parity work starts.
- The decode step may be memoized in a small local cache if repeated
  `match_box(...)` calls become measurable.

## 5. Admissible node families

The `FlatBoxId` whitelist should be derived from the families handled by
C++ `realPropagate(...)`, not from the Rust crate's current supported subset.

### 5.1 Allowed after `eval/a2sb`

These families belong in the `FlatBoxId` contract:

- numeric literals: `Int`, `Real`
- structural atoms: `Wire`, `Cut`
- symbolic lowering carriers: `Slot`, `Symbolic`
- metadata wrapper: `Metadata`
- primitive boxes: arity 0 through 5
- foreign nodes used in propagation:
  - `FConst`
  - `FVar`
  - `FFun`
- UI widgets:
  - `Button`
  - `Checkbox`
  - `VSlider`
  - `HSlider`
  - `NumEntry`
  - `VBargraph`
  - `HBargraph`
- `Soundfile`
- `Waveform`
- UI groups:
  - `VGroup`
  - `HGroup`
  - `TGroup`
- composition algebra:
  - `Seq`
  - `Par`
  - `Split`
  - `Merge`
  - `Rec`
- `Environment`
- `Route`
- bus-count helpers:
  - `Inputs`
  - `Outputs`
- clocked wrappers:
  - `Ondemand`
  - `Upsampling`
  - `Downsampling`

### 5.2 Forbidden after `eval/a2sb`

These families should be rejected by the conversion into `FlatBoxId`:

- `Unknown`
- `Ident`
- `Appl`
- `Access`
- `IPar`
- `ISeq`
- `ISum`
- `IProd`
- `WithLocalDef`
- `ModifLocalDef`
- `WithRecDef`
- `Component`
- `Library`
- `Case`
- `PatternVar`
- `Abstr`
- `Modulation`

Interpretation:

- If one of these appears while building a `FlatBoxId`, that is not a
  normal propagation failure.
- It is a pipeline-boundary violation: `eval`/`a2sb` failed to produce valid
  post-eval first-order box IR.

## 6. Error model

The current `PropagateError::UnsupportedBox` conflates two different cases:

1. invalid input syntax for post-eval propagation,
2. valid post-eval syntax whose propagation semantics are not yet implemented.

The typed boundary should split them.

Recommended new error split:

- conversion-time error:
  - `FlatBoxBuildError::UnexpectedPostEvalBox { node, kind }`
- propagation-time error:
  - `PropagateError::UnimplementedFamily { node, family }`
  - or narrower semantic errors for supported families

Effect:

- evaluator regressions are reported at the boundary,
- propagation implementation gaps remain local to `crates/propagate`.

## 7. API plan

Recommended staged API evolution:

### Stage A: additive API

Add:

- `pub fn try_build_flat_box(arena: &TreeArena, root: BoxId) -> Result<FlatBoxId, FlatBoxBuildError>`
- `pub fn propagate_typed(arena: &mut TreeArena, root: FlatBoxId, inputs: &[SigId], cache: &mut ArityCache) -> Result<Vec<SigId>, PropagateError>`

Keep existing:

- `propagate(arena, box_tree, inputs, cache)`

Implementation:

- legacy `propagate(...)` becomes a thin adapter:
  - build `FlatBoxId`
  - call `propagate_typed(...)`

### Stage B: internal migration

Move `box_arity(...)` and recursive propagation internals to operate on
`FlatBoxId` plus the restricted decode view rather than on unrestricted
`BoxMatch`.

### Stage C: primary API switch

Once call sites are migrated, make the typed API the primary documented entry
point and leave the raw `BoxId` path as a transitional compatibility helper, or
remove it if no longer needed internally.

## 8. Conversion contract

`try_build_flat_box(...)` must be:

- structural,
- deterministic,
- non-evaluating,
- non-simplifying,
- parity-driven.

It is not another lowering pass.

It should:

- recursively validate the post-eval box tree into `FlatBoxId`,
- ensure every reachable child also belongs to the typed post-eval subset,
- preserve exact node order and payload shape,
- reject evaluator-only families,
- avoid inventing new canonicalizations.

It should not:

- run `eval`,
- run `a2sb`,
- infer missing values,
- normalize route specs,
- fold constants.

Those transformations belong upstream.

## 9. Expected impact on current Rust code

This plan implies three concrete classifications for the current
`crates/propagate` match arms.

### 9.1 Dead-by-contract arms

These should move out of propagation and into the `FlatBoxId` builder as
boundary errors:

- `Ident`
- `Appl`
- `Access`
- `IPar`
- `ISeq`
- `ISum`
- `IProd`
- `WithLocalDef`
- `ModifLocalDef`
- `WithRecDef`
- `Component`
- `Library`
- `Case`
- `PatternVar`
- `Abstr`
- `Modulation`

### 9.2 Valid but currently unsupported propagation families

These belong in the `FlatBoxId` contract even if Rust propagation does not
yet handle them fully:

- `Route`
- `Ondemand`
- `Upsampling`
- `Downsampling`
- `Soundfile`
- any remaining `FFun` details if current Rust modeling is narrower than C++

### 9.3 Already in the proper contract

These are valid typed post-eval material and should remain in the lowering
core:

- literals
- slots / symbolic
- composition operators
- widgets / groups
- waveform
- primitives
- `Environment`
- `Inputs` / `Outputs`

## 10. Testing plan

### 10.1 Structural builder tests

Add unit tests in `crates/propagate` for `try_build_flat_box(...)`:

- accepts a post-`a2sb` symbolic abstraction body,
- accepts `route`, `ondemand`, `upsampling`, `downsampling`, `soundfile`,
- rejects raw `case`,
- rejects raw `abstr`,
- rejects `modulation`,
- rejects `with` / `withrec` / `modif_local_def`,
- preserves composition tree structure exactly.

### 10.2 Boundary regression tests

Add evaluator-to-propagate tests asserting:

- `eval_process(...)` output always converts to `FlatBoxId` for current
  supported corpus,
- failures at this boundary identify the leaking node family explicitly.

### 10.3 Differential tests vs C++

Add parity fixtures covering representative post-eval forms:

- symbolic closure lowering,
- route,
- soundfile,
- ondemand / upsampling / downsampling,
- widget/group paths,
- recursive composition with symbolic slots.

Goal:

- validate the `FlatBoxId` whitelist against actual C++ accepted input,
  not only against current Rust behavior.

## 11. Migration stages

### Stage 0: spec freeze

Before implementation:

- freeze the exact allowed/disallowed family list from C++,
- record any ambiguity in this document,
- confirm whether `Environment` should remain an explicit valid family in Rust
  even if it usually propagates to zero outputs.

Pass criterion:

- no unresolved family is left in a “maybe allowed” state.

### Stage 1: additive typed boundary

- add `FlatBoxId`
- add `FlatBoxBuildError`
- add `try_build_flat_box(...)`
- add unit tests for accepted/rejected families

Pass criterion:

- every current `UnsupportedBox` arm is classified as either:
  - boundary-invalid
  - valid-but-unimplemented

### Stage 2: typed arity path

- port `box_arity(...)` onto `FlatBoxId`
- keep raw `BoxId` wrapper as adapter only

Pass criterion:

- arity inference no longer inspects evaluator-only node families directly.

### Stage 3: typed propagation core

- port recursive lowering to `FlatBoxId` plus decode view
- keep behavior identical on already-supported families

Pass criterion:

- current `propagate` tests pass through the typed path.

### Stage 4: close known propagation-family gaps

- implement typed `Route`
- implement typed `FFun`
- implement typed `Ondemand`
- implement typed `Upsampling`
- implement typed `Downsampling`
- implement typed `Soundfile`

Pass criterion:

- these families are no longer represented as builder-valid but lowering-invalid.

Current status on 2026-03-09:

- completed:
  - typed `Route`
  - typed `FFun`
- completed after the `signals` extension:
  - `Soundfile`
  - `Ondemand`
  - `Upsampling`
  - `Downsampling`

Interpretation:

- stage 4 is now closed at the typed flat-boundary level,
- the remaining notable adaptation is internal:
  - Rust currently represents propagated clock environments with the same list
    field ordering as C++, but still leaves the `slotenv` / `path` payloads
    empty in this first pass,
  - this is sufficient for the newly-ported `soundfile` / `ondemand` /
    `upsampling` / `downsampling` lowering paths, but it is still an adapted
    internal representation rather than a complete 1:1 port of all C++
    clock-environment helpers.

### Stage 5: tighten public contract

- update docs to state that propagation consumes post-eval first-order box IR
- optionally deprecate raw `BoxId` propagation entry points

Pass criterion:

- pipeline boundary is explicit in API docs and tests.

## 12. Mapping status

Recommended status classification for this plan:

- C++ propagation input semantics -> Rust `FlatBoxId` + restricted decode
  view: `adapted representation`, target `1:1` semantic contract
- `propagate(...)` public raw `BoxId` API: `adapted`, transitional
- post-eval boundary enforcement: currently implicit, target explicit and typed

## 13. Recommended first implementation order

The highest-signal first slice is:

1. add `FlatBoxId` and builder,
2. reject evaluator-only syntax at the boundary,
3. keep existing propagation semantics by translating `FlatBoxId` into the
   current lowering path if needed,
4. only then migrate internals to typed recursion,
5. finally implement the C++ families that are valid but still unsupported.

This ordering reduces risk because it separates:

- contract hardening,
- internal refactor,
- parity expansion.

## 14. Open questions to resolve during implementation

- Should `Slot` carry a dedicated `SlotId` wrapper instead of raw `TreeId` from
  the first iteration?
- Should `Route` store decoded `(src, dst)` pairs in the restricted decode view
  immediately, or preserve raw `TreeId` first for lower migration cost?
- Should widget labels remain raw `TreeId` payloads in the restricted decode
  view, or should a later pass introduce a typed post-eval label form?
- Should the raw `BoxId` API remain public for box-ffi convenience, or should
  conversion to `FlatBoxId` happen before any public propagation entry
  point?

These are implementation tradeoffs, not blockers for the core spec: the family
whitelist and boundary contract should be fixed first.
