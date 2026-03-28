# Recursion Lowering: From `Rec` Boxes to Signals via De Bruijn Encoding

> Internal design document for `faust-rs`.
> Source code: `crates/propagate/src/lib.rs`, `crates/tlib/src/recursion.rs`,
> `crates/transform/src/signal_prepare.rs`.

---

## 1. Context: The `~` Operator in Faust

In Faust, the tilde operator (`~`) creates feedback loops. For example:

```faust
process = + ~ *(0.5);
```

This describes a one-sample-delay feedback: the output is fed back, multiplied
by 0.5, and added to the input. At the box algebra level this becomes a
`Rec(left, right)` node where:

- **left** is the forward body (here `+`, arity 2→1),
- **right** is the feedback path (here `*(0.5)`, arity 1→1).

The recursion arity rule is:

```
left  : Li → Lo
right : Ri → Ro        with Ri ≤ Lo and Ro ≤ Li

Rec(left, right) : (Li - Ro) → Lo
```

The feedback path "steals" `Ro` inputs from the left body and replaces them
with delayed recursive signals, while the remaining `Li - Ro` inputs are
exposed as external inputs of the composition.

---

## 2. De Bruijn Notation — Principles

### 2.1 The Problem: Naming Recursive Variables

When lowering `Rec` boxes to signals, we need to express "this signal refers to
the output of the recursive group it belongs to." Named variables work for
simple cases, but Faust supports **nested** and **mutually** recursive groups.
Named variables require careful scoping rules (alpha-renaming, capture
avoidance). De Bruijn indices solve this structurally.

### 2.2 De Bruijn Indices in Lambda Calculus (Background)

In standard lambda calculus, De Bruijn indices replace variable names with
**positional numbers** counting how many binders separate the reference from
its binding site:

```
λx. λy. x        becomes    λ. λ. 2
λx. λy. y        becomes    λ. λ. 1
λx. x            becomes    λ. 1
```

The key insight: **level 1** always means "bound by the immediately enclosing
binder", **level 2** means "bound by the binder one step further out", etc.
This makes structural equality trivial — no alpha-equivalence needed.

### 2.3 De Bruijn in Faust Signal Trees

Faust adapts this to recursive signal groups with two node types:

| Node | Notation | Meaning |
|------|----------|---------|
| `DEBRUIJNREC(body)` | The **binder** | Wraps a recursive group body. Analogous to `λ` or `μ`. |
| `DEBRUIJNREF(level)` | The **reference** | Points to an enclosing binder. Level 1 = innermost. |

A simple feedback `+ ~ *(0.5)` produces (schematically):

```
DEBRUIJNREC(
    body = [add(delay1(proj(0, DEBRUIJNREF(1))), input(0))]
)
                         ↑
                         └── "I refer to the output of my
                              immediately enclosing DEBRUIJNREC"
```

### 2.4 Why Not Just Named Variables From the Start?

1. **Structural sharing**: the `TreeArena` interns nodes by structural
   identity. De Bruijn nodes produce deterministic tree shapes independent of
   naming context, maximizing sharing.
2. **Correct scoping by construction**: nested `~` operators produce nested
   `DEBRUIJNREC` binders; inner references automatically point to the right
   scope through their level number — no alpha-renaming pass needed.
3. **Standard technique**: the C++ Faust compiler uses the same encoding
   (`rec`/`ref` with De Bruijn levels), so the Rust port preserves structural
   parity.

---

## 3. The `Rec` Lowering Algorithm Step by Step

The following happens inside `propagate_in_slot_env` when it encounters
`FlatNodeKind::Rec(left, right)`:

### Step 1 — Arity Check

```
left  : Li → Lo
right : Ri → Ro
require: Ri ≤ Lo  AND  Ro ≤ Li
```

### Step 2 — Seed the Feedback Inputs (`make_mem_sig_proj_list`)

For each of the `Ri` feedback channels, create an initial "recursive
placeholder" signal:

```
l0[i] = delay1(proj(i, DEBRUIJNREF(1)))       for i in 0..Ri
```

This says: "the i-th feedback input is the previous sample (`delay1`) of the
i-th projection (`proj`) of the recursive group we are currently defining
(`DEBRUIJNREF(1)`)."

### Step 3 — Propagate the Feedback Path

```
l1 = propagate(right, l0)
```

The feedback path `right` receives the recursive placeholders and produces
`Ro` output signals.

### Step 4 — Build the Full Input Vector for the Body

```
rec_inputs = l1 ++ lift(external_inputs)
```

The body `left` receives:
- First `Ro` signals from the feedback path,
- Then `Li - Ro` external inputs, **lifted** by one De Bruijn level.

Lifting is critical: external signals that already contain `DEBRUIJNREF` nodes
from an *outer* recursion must have their levels incremented so they still point
to the correct outer binder after we introduce a new inner binder.

### Step 5 — Lift the Slot Environment

```
slot_env' = { k → liftn(v, 1) | (k, v) ∈ slot_env }
```

Same reason: any slot-environment entries containing `DEBRUIJNREF` nodes must
be lifted to avoid capture by the new inner binder.

### Step 6 — Propagate the Body

```
l2 = propagate(left, rec_inputs)    // using slot_env'
```

This produces `Lo` output signals that may reference `DEBRUIJNREF(1)`.

### Step 7 — Wrap in `DEBRUIJNREC` and Project

```
group = DEBRUIJNREC(list(l2[0], l2[1], ..., l2[Lo-1]))
```

Then for each output signal `l2[i]`:

```
if aperture(l2[i]) > 0:
    output[i] = proj(i, group)      // truly recursive — must go through the group
else:
    output[i] = l2[i]               // not recursive — emit directly (degenerate case)
```

---

## 4. Diagram: Signal Flow for `+ ~ *(0.5)`

```
                        ┌─────────────────────────────────────────────┐
                        │            DEBRUIJNREC (binder)             │
                        │                                             │
                        │   ┌─────────────────────────────────┐       │
    input(0) ──────────►│──►│              add                │──►────│──► output
                        │   │                                 │       │
                        │   └──────────▲──────────────────────┘       │
                        │              │                              │
                        │       delay1(proj(0, DEBRUIJNREF(1)))       │
                        │              │              ▲                │
                        │              │              │                │
                        │              │         ┌────┘                │
                        │              │         │  "my output        │
                        │              └──── *(0.5)   at index 0"     │
                        │                    (feedback path)          │
                        └─────────────────────────────────────────────┘
```

The `DEBRUIJNREF(1)` is the self-reference: "the group I'm inside."
The `proj(0, ...)` selects output slot 0 of that group.
The `delay1(...)` provides the one-sample delay that makes the feedback causal.

---

## 5. Nested Recursion and De Bruijn Levels

Consider nested feedback:

```faust
process = (+ ~ *(0.5)) ~ *(0.25);
```

This produces two nested `DEBRUIJNREC` binders:

```
DEBRUIJNREC₂(                          ← outer binder (level 2 from inside)
    body₂ = DEBRUIJNREC₁(              ← inner binder (level 1 from inside)
        body₁ = add(
            delay1(proj(0, DEBRUIJNREF(1))),  ← refers to inner group
            ...
        )
    ),
    ...delay1(proj(0, DEBRUIJNREF(1)))...  ← at this position, refers to outer
)
```

Inside `body₁`:
- `DEBRUIJNREF(1)` → inner group (DEBRUIJNREC₁)
- `DEBRUIJNREF(2)` → outer group (DEBRUIJNREC₂)

The **lifting** operation (`liftn`) ensures that when signals flow from the
outer scope into the inner scope, their reference levels are incremented so
they keep pointing to the outer binder.

```
liftn(DEBRUIJNREF(n), threshold=1) =
    if n < 1: DEBRUIJNREF(n)    // bound in this scope → unchanged
    if n ≥ 1: DEBRUIJNREF(n+1)  // free → lift past new binder
```

---

## 6. Mutually Recursive Forms (Multi-Output Recursion)

### 6.1 The Pattern

Mutual recursion in Faust arises when the `~` operator connects multi-channel
signals. Example:

```faust
process = si.bus(2) ~ (*(0.5), *(0.25));
```

Here both channels feed back into each other through the parallel feedback
path. The `Rec` node has:
- `left` = `si.bus(2)` (2→2: identity on 2 channels)
- `right` = `(*(0.5), *(0.25))` (2→2: independent gain on each channel)

### 6.2 Signal Shape

The recursive group has **2 bodies** (one per output channel):

```
DEBRUIJNREC(
    body = list(
        delay1(proj(0, DEBRUIJNREF(1))) * 0.5 + input(0),   ← body₀
        delay1(proj(1, DEBRUIJNREF(1))) * 0.25 + input(1)   ← body₁
    )
)
```

Both bodies reference the same `DEBRUIJNREF(1)` but select different
projections (`proj(0, ...)` and `proj(1, ...)`).

### 6.3 General N-Output Case

For an N-channel feedback:

```
make_mem_sig_proj_list(N) produces:
    [ delay1(proj(0, DEBRUIJNREF(1))),
      delay1(proj(1, DEBRUIJNREF(1))),
      ...
      delay1(proj(N-1, DEBRUIJNREF(1))) ]
```

Each projection index corresponds to one slot in the recursive group body
list. The final wrapped form is:

```
group = DEBRUIJNREC(list(body₀, body₁, ..., body_{N-1}))
output[i] = proj(i, group)     if aperture(body_i) > 0
output[i] = body_i             otherwise
```

---

## 7. Degenerate Recursion Cases

### 7.1 What Makes a Recursion "Degenerate"?

A recursion is degenerate when some output channels of the recursive group
**do not actually use the recursive feedback**. This means their `aperture`
is 0 — they contain no `DEBRUIJNREF` references.

### 7.2 The Aperture Test

The `aperture(expr)` function computes the **maximum free De Bruijn level**
in a signal expression:

| Node | Aperture |
|------|----------|
| `DEBRUIJNREF(level)` | `level` |
| `DEBRUIJNREC(body)` | `aperture(body) - 1` (the binder captures one level) |
| Any other node | `max(aperture(children))` |
| Leaf (no refs) | `0` |

When `aperture(body_i) > 0`, the body genuinely references the recursive
group → it must be emitted as `proj(i, group)`.

When `aperture(body_i) == 0`, the body has no recursive dependency → it can
be emitted directly, bypassing the recursive wrapper entirely.

### 7.3 Why This Matters: The `proj(7, W)` Problem

Consider an 8-channel feedback where only channel 7 actually feeds back:

```faust
process = si.bus(8) ~ (!, !, !, !, !, !, !, *(gain));
```

Channels 0–6 discard their feedback input (`!`), so they are not genuinely
recursive. Only channel 7 multiplies its feedback by `gain`.

**During propagation** (in `propagate_in_slot_env`), the aperture test catches
this at step 7:

```
for i in 0..8:
    if aperture(l2[i]) > 0:
        output[i] = proj(i, group)      // only channel 7
    else:
        output[i] = l2[i]               // channels 0–6: direct
```

So channels 0–6 are emitted directly (no `proj`), and only channel 7 goes
through `proj(7, group)`.

### 7.4 The C++ Degeneracy Elimination

The C++ compiler goes further with `inlineDegenerateRecursions()`: it detects
that 7 of 8 bodies are non-recursive, **removes them from the group**, and
reduces it to a single-body group. But the projection index is preserved:

```
Before: SYMREC([b₀, b₁, ..., b₇])  with proj(7, W)
After:  SYMREC([b₇])                with proj(7, W)  ← index 7, arity 1!
```

This creates an **out-of-bounds projection**: `proj(7, W)` on a group of
arity 1.

### 7.5 The Rust Fix: `canonicalize_unary_rec_projections`

In `signal_prepare.rs`, after `de_bruijn_to_sym` converts De Bruijn form to
symbolic form, a canonicalization pass rewrites:

```
proj(k, group)  →  proj(0, group)     when group has exactly 1 body
```

This is a narrower fix than the full C++ degeneracy elimination. It does not
rebuild the recursive dependency graph or rewrite projection definitions. It
simply normalizes the index once the physical arity is known to be 1.

**Real-world trigger**: `re.zita_rev1_stereo(...)` (from `Birds.dsp`), an
8-delay-line algorithmic reverb whose feedback matrix produces exactly this
pattern after evaluation.

### 7.6 Was this strictly necessary?

Not in the absolute semantic sense.

Older C++ revisions existed before `inlineDegenerateRecursions()` was added.
They still handled degenerate recursive cases correctly, but without rewriting
the signal tree into a canonical unary form.

The older strategy was roughly:

1. `propagate` already emitted closed branches directly when `aperture == 0`,
   so only genuinely recursive outputs remained as projections.
2. the remaining projections could keep their original logical index;
3. generator-side code tolerated that shape instead of requiring a globally
   normalized recursive IR.

In particular:

- scalar recursion generation directly compiled the requested projection
  definition;
- instruction-style recursion generation only materialized projections that
  were actually used.

So Rust could also have chosen that looser contract.

However, the current Rust fast-lane deliberately chooses a denser invariant:
after symbolic conversion, a single-body recursion group should be addressed
through physical slot `0`.

This gives a simpler prepared IR to downstream passes.

---

## 8. Full Pipeline: De Bruijn to Symbolic to FIR

```
Box tree (Rec nodes)
         │
         ▼
    propagate_in_slot_env           ← De Bruijn encoding (this document)
         │
         ▼
Signal tree with DEBRUIJNREC / DEBRUIJNREF nodes
         │
         ▼
    de_bruijn_to_sym (tlib)         ← Convert to named form
         │
         ▼
Signal tree with SYMREC(var, body) / SYMREF(var) nodes
         │
         ▼
    canonicalize_unary_rec_projections (signal_prepare)
         │
         ▼
    signal_fir                      ← FIR code generation
```

### Conversion: `de_bruijn_to_sym`

For each `DEBRUIJNREC(body)` binder:
1. Allocate a fresh symbolic variable `W0`, `W1`, ...
2. Substitute `DEBRUIJNREF(1)` in the body with `SYMREF(W0)`.
3. Wrap as `SYMREC(W0, converted_body)`.

```
DEBRUIJNREC(add(delay1(proj(0, DEBRUIJNREF(1))), input(0)))
    ↓ de_bruijn_to_sym
SYMREC(W0, add(delay1(proj(0, SYMREF(W0))), input(0)))
```

This produces human-readable, named recursive groups suitable for the FIR
backend.

---

## 9. Design Discussion: Why Not Canonicalize in `propagate`?

A natural question arises: since `propagate` already knows which channels are
degenerate (via the aperture test at step 7), could we perform the
canonicalization there instead of deferring it to `signal_prepare`?

### What `propagate` Could Do

At step 7, `propagate` already distinguishes recursive bodies (`aperture > 0`)
from non-recursive ones (`aperture == 0`). It could go further:

1. Filter the group body list to keep only truly recursive bodies.
2. Build a smaller `DEBRUIJNREC` group with only those bodies.
3. Renumber projection indices to be dense (`0..N_recursive`).

This would eliminate the degenerate case at its source, before
`de_bruijn_to_sym` ever runs.

### Why the Current Design Keeps It in `signal_prepare`

**The problem does not originate in `propagate`.** The Rust `propagate` builds
a valid 8-body group with `proj(7, group)` — index 7 is in bounds (7 < 8).
The out-of-bounds index only appears *after* the C++
`inlineDegenerateRecursions()` pass removes 7 non-recursive bodies from the
group while preserving the original projection index. The canonicalization in
`signal_prepare` is therefore a **compatibility fix** for the shape produced
by the C++ pipeline.

The current placement is justified by several factors:

1. **Operates on `SYMREC`/`SYMREF`**: the canonicalization works on the
   symbolic form, which does not exist yet during propagation (still De Bruijn
   form). A propagation-level version would need to be written differently.

2. **Structural parity with C++**: `propagate` produces the same De Bruijn
   shape as the C++ compiler. Modifying group construction at this stage would
   diverge from the C++ structure and complicate parity verification.

3. **Separation of concerns**: `propagate` faithfully translates box semantics
   into signals. `signal_prepare` is the normalization stage before FIR — the
   natural place for canonicalizations.

4. **Complexity budget**: `propagate` is already dense. Adding body filtering
   and index renumbering increases the error surface in critical code.

### When Moving It Would Make Sense

If the project were to fully port `inlineDegenerateRecursions()` into the Rust
pipeline (recursive dependency graph construction, projection-definition
rewriting via `hasProjDefinition`/`setProjDefinition`, etc.), then it would
make sense to build a reduced group directly in `propagate` rather than
building a full group and reducing it afterwards. This would be more efficient
(single pass). However, this is a significantly larger effort than the current
targeted compatibility fix.

### Current Porting Status of `inlineDegenerateRecursions()`

`inlineDegenerateRecursions()` is a **C++ Faust compiler pass only** — it has
**not been ported to Rust**. The Rust pipeline does not:

- build the recursive dependency graph,
- analyze projections through `hasProjDefinition(...)` / `setProjDefinition(...)`,
- rewrite projection definitions under delays,
- or inline recursive projection definitions the way the C++ rewrite rules do.

The current Rust pipeline instead follows this simplified path:

1. **`propagate`** builds the full De Bruijn group (all N bodies, including
   non-recursive ones) — no elimination at this stage.
2. **`signal_prepare`** converts De Bruijn to symbolic form
   (`de_bruijn_to_sym`), then applies the narrow
   `canonicalize_unary_rec_projections` fix: when a symbolic group has exactly
   1 body, any projection index targeting it is rewritten to 0.

This is explicitly documented in `signal_prepare.rs` (lines 43–58) as a
**compatibility normalization**, not a full port.

This fix is also no longer the only guardrail in the Rust codebase. The FIR
lowerer now defensively remaps unary symbolic groups to slot `0` as well.
However, preparation-level canonicalization still has architectural value:

- type inference sees dense physical indices instead of `proj(7, unary_group)`;
- promotion can keep reasoning on a dense slot model;
- FIR lowering does not need to carry the older C++ logical/physical index
  distinction through every consumer.

More concretely: with the current Rust typer, an out-of-bounds projection on a
unary symbolic group would otherwise fall back to a maximal/imprecise type
instead of reusing the unique body type. So early canonicalization improves not
only lowering robustness, but also typing precision.

**Does the Rust pipeline need the full pass?** Currently, the narrow fix is
sufficient for all programs encountered (notably `Birds.dsp` /
`re.zita_rev1_stereo`). However, if future Faust programs produce degenerate
groups reduced to N > 1 bodies with non-dense projection indices, the full
port would become necessary.

### Where the C++ pass actually lives

The reference C++ compiler performs the full elimination in:

- `compiler/transform/sigDegenerateRecursionElimination.hh`
- `compiler/transform/sigDegenerateRecursionElimination.cpp`
- function: `inlineDegenerateRecursions(Tree siglist, bool trace)`

and invokes it later from generator-side code, notably in:

- `compiler/generator/compile_scal.cpp`
- `compiler/generator/instructions_compiler.cpp`

So even in C++, this is a later transform/generator concern, not a
`propagate` concern.

### Could Rust have followed the older C++ approach?

Yes, but it would have required a different IR contract.

Rust would need to accept that a projection may keep a logical index that is no
longer equal to the physical slot index of the reduced group. In practice, that
would mean teaching this special case to several downstream consumers:

- type inference,
- promotion/normalization,
- FIR lowering,
- and any later pass that reasons on recursion-group arity.

The current Rust design instead normalizes once in `signal_prepare` and lets the
rest of the pipeline reason on dense physical slots only.

---

## 10. Summary of key functions

| Function | File | Role |
|----------|------|------|
| `make_mem_sig_proj_list` | `propagate/lib.rs` | Seeds `Ri` feedback placeholders: `delay1(proj(i, DEBRUIJNREF(1)))` |
| `lift_signals` / `liftn` | `propagate/lib.rs` | Increments De Bruijn levels to avoid capture by new binders |
| `aperture` | `propagate/lib.rs` | Computes max free De Bruijn level (0 = not recursive) |
| `debruijn_rec` / `debruijn_ref` | `propagate/lib.rs` | Constructors for `DEBRUIJNREC` / `DEBRUIJNREF` nodes |
| `de_bruijn_to_sym` | `tlib/recursion.rs` | Converts positional De Bruijn to named `SYMREC`/`SYMREF` |
| `canonicalize_unary_rec_projections` | `transform/signal_prepare.rs` | Normalizes single-body recursion groups to dense physical slot `0` for typing, promotion, and FIR lowering |

---

## 11. Glossary

- **Aperture**: The maximum free De Bruijn reference level in a subtree. If > 0, the subtree has unbound recursive references.
- **Binder** (`DEBRUIJNREC`): Introduces a recursive scope. Each binder captures references at level 1.
- **Degenerate recursion**: A recursive group where some outputs do not actually depend on the feedback. Their aperture is 0.
- **De Bruijn index/level**: A nameless reference scheme where the integer counts enclosing binders between reference and binding site.
- **Lifting** (`liftn`): Incrementing De Bruijn levels of free references to preserve correct binding when a new binder is introduced.
- **Projection** (`proj(i, group)`): Selects the i-th output from a multi-output recursive group.
- **SYMREC/SYMREF**: Named symbolic form of recursion, produced by `de_bruijn_to_sym` from the positional De Bruijn form.
