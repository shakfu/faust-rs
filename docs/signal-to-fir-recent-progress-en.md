# Recent Signal-to-FIR Progress

This note summarizes the recent work done on the Rust signal-to-FIR fast lane.
It is meant as a compact companion to the more focused notes already present in
`docs/`, especially:

- [`recursion-debruijn-lowering-en.md`](./recursion-debruijn-lowering-en.md)
- [`flatnode-rec-to-signals-en.md`](./flatnode-rec-to-signals-en.md)
- [`developer-workflows-en.md`](./developer-workflows-en.md)

The scope here is narrower: what was recently implemented in the Signal -> FIR
step itself, and how the runtime-oriented lowering structure now works.

## 1. Overall Direction

The fast lane has moved from a minimal executable slice to a more structured
lowering pipeline with explicit staging, explicit execution-tier placement,
bucket-local CSE, multiple delay strategies, and a clearer separation between
general lowering orchestration and recursion-specific machinery.

In practical terms, the current flow is now:

```text
propagate
  -> signal_prepare
  -> signal_fir planner / contract checks
  -> FIR module assembly
       - instanceConstants
       - compute preamble
       - compute sample loop
```

This is still an incremental parity track, not yet the full C++ compiler
behavior, but the internal structure is now much closer to the intended final
model.

## 2. Preparation Before FIR Lowering

Before any FIR node is emitted, the output signal forest is now prepared in a
dedicated staging phase.

That preparation currently does the following:

- clones the output forest into a private arena,
- converts de Bruijn recursion to symbolic `SYMREC` / `SYMREF`,
- applies unary-recursion canonicalization,
- runs reduced typing and promotion,
- runs simplification,
- canonicalizes `Delay(x, 1)` back to `Delay1(x)`,
- verifies the prepared forest boundary before FIR lowering.

This gives the FIR lowerer a more stable and typed input contract than the
earlier direct lowering path.

## 3. Compilation Into Init / Block / Sample Zones

One of the main recent changes is that FIR emission is no longer treated as one
flat stream of statements.

The lowering pipeline now distinguishes three execution tiers driven by signal
variability:

- `Konst` values are emitted into `instanceConstants`,
- `Block` values are emitted into the `compute` preamble,
- `Samp` values stay in the sample loop.

This is implemented as a dedicated pre-lowering analysis pass that computes:

- signal reference counts,
- variability boundaries between parents and children.

That information is then used to hoist non-trivial shared or boundary-crossing
expressions into the appropriate bucket:

- init-time constants become `fConst*`,
- block-rate controls become `fSlow*`,
- sample-rate values remain inline unless later CSE materializes them.

The result is that FIR emission now matches the intended runtime structure more
closely: values are computed in the slowest valid zone instead of being
re-evaluated every sample.

## 4. FIR-Side CSE Materialization

After variability-driven placement, a second optimization pass performs
bucket-local CSE directly on FIR.

This pass works independently inside each execution bucket:

- `constants_statements`,
- `control_statements`,
- sample-loop statements.

For each bucket it:

1. counts FIR value uses,
2. identifies non-trivial expressions used more than once,
3. materializes them as temporary variables,
4. rewrites repeated uses to `LoadVar`.

This is important because it happens after placement, on FIR itself, so all FIR
backends benefit from the same deduplicated structure instead of each backend
having to rediscover sharing on its own.

## 5. Delay Handling: Multiple Runtime Strategies

Delay lowering has also become much more explicit and modular.

Instead of one generic delay path, the fast lane now supports several concrete
runtime strategies selected from delay size and thresholds:

- `ShiftModel`: small copy/shift delays,
- `CircularPow2Model`: power-of-two ring buffers using the shared `fIOTA`
  cursor,
- `IfWrappingModel`: exact-size ring buffers with a per-line wrapping counter.

This mirrors Faust's `-mcd` / `-dlt` strategy split.

Recent work in this area includes:

- a dedicated `DelayManager`,
- pre-scan resource planning for delay-line ownership,
- explicit delay-line metadata,
- shared write scheduling,
- support for bounded variable delays,
- alignment of `Delay1(x)` and `Delay(x, N)` so they can share one canonical
  line when appropriate.

The delay code is now separated into:

- analysis and allocation,
- runtime geometry models,
- strategy-specific FIR emission helpers.

That separation makes the fast lane easier to extend toward C++ parity without
reopening `module.rs` every time a new delay case is added.

## 6. Delay Analysis and Recursion-Delay Merging

Another recent step is the introduction of an accumulated delay-analysis layer.

This analysis records, for reachable signals and recursion outputs:

- maximum accumulated delay,
- delayed-access counts.

That metadata is then used to size recursion carriers and to support merged
delay behavior for recursion-rooted delay chains such as:

```text
Delay1^k(Proj(i, group))
```

This does not yet reproduce every C++ compacting behavior, but it establishes
the needed planning layer so recursion storage and delay storage are no longer
completely local, syntax-only decisions.

## 7. Extraction of Recursion Handling

Recursion handling has been pulled out of the main FIR lowering file into its
own recursion-specific layer.

The extracted recursion module now owns:

- recursion carrier storage strategy,
- carrier metadata,
- canonical carrier references,
- delayed recursion reference resolution,
- recursive-group projection decoding and validation,
- carrier allocation helpers,
- recursion-specific FIR helper emission.

`module.rs` still decides when recursive groups are materialized and how their
evaluation is integrated into sample phases, but it no longer owns all of the
data structures and helper logic itself.

This is a meaningful cleanup because recursion is no longer spread across
generic lowering code, delay code, and ad hoc helper state. The responsibilities
are clearer:

- `signal_prepare` stabilizes the symbolic recursion form,
- `recursion.rs` owns recursion runtime representation,
- `module.rs` orchestrates execution order,
- `delay.rs` owns non-recursive delay resources and shared delay mechanics.

## 8. Module Assembly Is Now Runtime-Shaped

The FIR module emitted by the fast lane is now assembled into the same
lifecycle-oriented sections expected by downstream backends:

- `metadata`
- `instanceConstants`
- `instanceResetUserInterface`
- `instanceClear`
- `buildUserInterface`
- `compute`

Within `compute`, the sample loop is itself structured into ordered phases:

- immediate per-sample work,
- post-output updates,
- end-of-sample maintenance.

This matters for correctness in delay and recursion handling, where write/read
ordering and post-output finalization have to remain stable.

## 9. What This Means

The recent Signal -> FIR work is not just “more node coverage”. It has changed
the shape of the implementation in four important ways:

1. lowering now starts from a verified prepared signal forest,
2. execution-tier placement is explicit and variability-driven,
3. FIR-level sharing is preserved through a real CSE pass,
4. delays and recursion now have dedicated subsystems instead of being embedded
   as local special cases in the main lowering loop.

That gives the project a much better base for the next parity steps:

- broader signal-family coverage,
- closer C++ delay compaction,
- deeper recursion parity,
- backend reuse of the same structured FIR output.
