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

## 6. Multi-Output Recursion and Genuine Mutual Recursion

### 6.1 The Pattern

Faust can produce recursive groups with multiple outputs without those outputs
being mutually recursive in the strict cross-coupled sense. Example:

```faust
process = si.bus(2) ~ (*(0.5), *(0.25));
```

Here each channel feeds back into its own slot through the parallel feedback
path. The `Rec` node has:
- `left` = `si.bus(2)` (2→2: identity on 2 channels)
- `right` = `(*(0.5), *(0.25))` (2→2: independent gain on each channel)

### 6.2 Signal Shape

The recursive group has **2 bodies** (one per output channel):

```
DEBRUIJNREC(
    body = list(
        delay1(proj(0, DEBRUIJNREF(1))) * 0.5,   ← body₀
        delay1(proj(1, DEBRUIJNREF(1))) * 0.25   ← body₁
    )
)
```

Both bodies reference the same `DEBRUIJNREF(1)` but select their own
projections (`proj(0, ...)` and `proj(1, ...)`). This is a multi-output
recursive group, but not yet a genuinely mutually recursive one.

### 6.3 Genuine Mutual Recursion by Crossing the Feedback Lanes

A genuinely mutually recursive variant crosses the two feedback lanes:

```faust
import("stdfaust.lib");

process = si.bus(2) ~ ((*(0.5), *(0.25)) : ro.cross(2));
```

The corresponding recursive group is now:

```
DEBRUIJNREC(
    body = list(
        delay1(proj(1, DEBRUIJNREF(1))) * 0.25,   ← body₀ depends on output 1
        delay1(proj(0, DEBRUIJNREF(1))) * 0.5     ← body₁ depends on output 0
    )
)
```

This is genuine mutual recursion:

- output 0 depends on output 1;
- output 1 depends on output 0.

### 6.4 General N-Output Case

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

## 7. Aperture: Measuring Recursive Openness

### 7.1 Concept

The **aperture** of a signal subtree is the maximum depth of free (unbound)
De Bruijn references it contains. It answers a fundamental question: *does this
expression depend on an enclosing recursive group, and if so, how many nesting
levels out?*

| Aperture | Meaning |
|----------|---------|
| `0` | The expression is **closed** — it has no free recursive references. It can be evaluated independently of any enclosing `DEBRUIJNREC`. |
| `1` | The expression references its **immediately enclosing** recursive group (`DEBRUIJNREF(1)`). |
| `2` | The expression references a group **two nesting levels out** (`DEBRUIJNREF(2)`). |
| `n` | The expression reaches `n` binder levels outward. |

This is directly analogous to the concept of **free variables** in lambda
calculus: a term with no free variables is closed (a combinator); a term with
free variables is open and must be evaluated in a context that binds them.
Aperture is the De Bruijn equivalent — instead of tracking variable *names*,
it tracks the *depth* of the deepest unbound reference.

### 7.2 Computation Algorithm

The aperture is computed recursively over the tree structure with three rules:

```
aperture(DEBRUIJNREF(level))  =  level
aperture(DEBRUIJNREC(body))   =  aperture(body) - 1
aperture(other_node)          =  max(aperture(child) for child in children)
aperture(leaf)                =  0
```

**Rule 1 — Reference nodes**: a `DEBRUIJNREF(level)` contributes exactly its
level. This is the base case that introduces openness.

**Rule 2 — Binder nodes**: a `DEBRUIJNREC(body)` *captures* one level of
reference. If the body has aperture 1 (referencing its own binder), the
resulting aperture is 0 — the binder has closed it. If the body has aperture 2
(reaching an outer binder), the result is 1 — one free level remains.

**Rule 3 — Other nodes**: for any composite node (arithmetic, delay, proj, ...),
the aperture is the maximum of its children's apertures. A single open child
is enough to make the parent open.

### 7.3 Worked Example

Consider this expression from a nested recursion:

```
add(
    delay1(proj(0, DEBRUIJNREF(1))),    ← aperture = 1
    mul(
        input(0),                        ← aperture = 0
        delay1(proj(0, DEBRUIJNREF(2)))  ← aperture = 2
    )                                    ← aperture = max(0, 2) = 2
)                                        ← aperture = max(1, 2) = 2
```

If this expression is wrapped in one `DEBRUIJNREC`:
```
DEBRUIJNREC(above)  →  aperture = 2 - 1 = 1   (still open — one free level)
```

If wrapped in two nested `DEBRUIJNREC`:
```
DEBRUIJNREC(DEBRUIJNREC(above))  →  aperture = (2-1) - 1 = 0   (closed)
```

### 7.4 Implementation: C++ vs Rust

**C++ (`compiler/tlib/recursive-tree.cpp`)**

In the C++ compiler, aperture is a **synthesized field** stored on every tree
node (`CTree::fAperture`). It is computed once at construction time by
`calcTreeAperture()` and cached permanently — zero cost on subsequent reads:

```cpp
int CTree::calcTreeAperture(const Node& n, const tvec& br) {
    if (n == DEBRUIJNREF)   return int_value(br[0]);
    if (n == DEBRUIJN)      return br[0]->fAperture - 1;
    // else: max of children
    int rc = 0;
    for (auto& b : br) rc = max(rc, b->aperture());
    return rc;
}
```

Every node carries its aperture as `tree->aperture()`, so the test at
propagation time is a single field read.

**Rust (`crates/tlib/src/recursion.rs`)**

In the Rust compiler, `TreeArena` nodes do not carry a pre-computed aperture
field. Instead, aperture is computed on demand and memoized in an
`AHashMap<TreeId, i64>`. The single implementation lives in `tlib`:

```rust
fn aperture(arena: &TreeArena, root: TreeId, memo: &mut AHashMap<TreeId, i64>) -> i64 {
    if let Some(value) = memo.get(&root) { return *value; }
    let value = if let Some(level) = match_de_bruijn_ref(arena, root) {
        level
    } else if let Some(body) = match_de_bruijn_rec(arena, root) {
        aperture(arena, body, memo) - 1
    } else {
        arena.children(root).map_or(0, |ch|
            ch.iter().map(|&c| aperture(arena, *c, memo)).max().unwrap_or(0))
    };
    memo.insert(root, value);
    value
}
```

Two public entry points share this worker:
- `de_bruijn_aperture(arena, root)` — creates a fresh local cache, suitable
  for one-off queries.
- `de_bruijn_aperture_with_memo(arena, root, memo)` — accepts an external
  cache, used by `propagate` to amortize aperture costs across the full
  propagation traversal (the cache is shared with `liftn` inside
  `PropagateMemo`).

### 7.5 Role in the Pipeline

Aperture appears at several points in the compilation pipeline:

1. **During `Rec` lowering (step 7)**: determines which bodies of a recursive
   group are truly recursive (`aperture > 0`) versus degenerate
   (`aperture == 0`). Only recursive bodies are wrapped in `proj(i, group)`.

2. **During `liftn`**: the lifting operation uses a threshold to decide which
   references to increment. A reference with `level < threshold` is already
   bound inside the current scope and should not be lifted; a reference with
   `level >= threshold` is free and must be incremented. This is closely
   related to aperture — `liftn` effectively operates on the same structural
   information.

3. **During `de_bruijn_to_sym`**: the conversion from positional to named form
   uses aperture-like reasoning to determine which `DEBRUIJNREF` nodes are
   captured by a given `DEBRUIJNREC` binder (level 1) versus which reach an
   outer binder (level > 1).

### 7.6 Aperture Diagram

```
    DEBRUIJNREC                              aperture: max(1,2)-1 = 1
        │
        body = add(...)                      aperture: max(1,2) = 2
        ┌────────┴────────────┐
        │                     │
  delay1(proj(0,             mul(...)         aperture: max(0,2) = 2
    DEBRUIJNREF(1)))         ┌───┴───┐
        │                    │       │
    aperture: 1          input(0)  delay1(proj(0,
                         ap: 0       DEBRUIJNREF(2)))
                                         │
                                     aperture: 2
```

Aperture propagates **upward** from leaves (references) to the root, and each
`DEBRUIJNREC` binder **decrements** it by 1. When it reaches 0, the subtree
is closed.

---

## 8. Degenerate Recursion Cases

### 8.1 What Makes a Recursion "Degenerate"?

A recursion is degenerate when some output channels of the recursive group
**do not actually use the recursive feedback**. This means their `aperture`
is 0 — they contain no `DEBRUIJNREF` references.

### 8.2 The Aperture Test

The aperture function (see [section 7](#7-aperture-measuring-recursive-openness)
for the full treatment) determines which bodies are genuinely recursive:

- `aperture(body_i) > 0` → the body references the recursive group → emit as
  `proj(i, group)`.
- `aperture(body_i) == 0` → no recursive dependency → emit directly, bypassing
  the recursive wrapper.

### 8.3 Why This Matters: The `proj(7, W)` Problem

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

### 8.4 The C++ Degeneracy Elimination

The C++ compiler goes further with `inlineDegenerateRecursions()`: it detects
that 7 of 8 bodies are non-recursive, **removes them from the group**, and
reduces it to a single-body group. But the projection index is preserved:

```
Before: SYMREC([b₀, b₁, ..., b₇])  with proj(7, W)
After:  SYMREC([b₇])                with proj(7, W)  ← index 7, arity 1!
```

This creates an **out-of-bounds projection**: `proj(7, W)` on a group of
arity 1.

### 8.5 The Rust Fix: `canonicalize_unary_rec_projections`

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

### 8.6 Was this strictly necessary?

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

## 9. Conversion: De Bruijn to Symbolic Form (`de_bruijn_to_sym`)

### 9.1 Why a Second Representation?

De Bruijn indices are ideal during construction (correct scoping by
construction, deterministic sharing), but they are opaque for later passes:
reading `DEBRUIJNREF(2)` requires counting enclosing binders manually. The
symbolic form replaces positional levels with **named variables**, making
recursive groups self-documenting and easier to process by the FIR backend.

| De Bruijn form | Symbolic form |
|----------------|---------------|
| `DEBRUIJNREC(body)` | `SYMREC(var, body)` |
| `DEBRUIJNREF(level)` | `SYMREF(var)` |

### 9.2 Algorithm Overview

The conversion is a two-phase recursive traversal for each `DEBRUIJNREC`
binder encountered:

```
function convert(node):
    if node is DEBRUIJNREC(body):
        var ← fresh_var()                     // allocate W0, W1, W2, ...
        body' ← substitute(body, level=1, replacement=SYMREF(var))
        body'' ← convert(body')               // recurse into converted body
        return SYMREC(var, body'')

    if node is DEBRUIJNREF(level):
        error("unbound reference")            // should not happen on closed trees

    if node is SYMREC or SYMREF:
        pass through unchanged

    otherwise:
        return rebuild(node, [convert(child) for child in children])
```

**Precondition**: the input tree must be **closed** (`aperture ≤ 0`). An open
tree would leave unresolved `DEBRUIJNREF` nodes after conversion. The function
checks this and returns an error otherwise.

### 9.3 The `substitute` Helper

The key operation is `substitute(tree, level, replacement)`, which replaces
every `DEBRUIJNREF(level)` at exactly the given level with the replacement
node:

```
function substitute(node, level, replacement):
    if aperture(node) < level:
        return node                           // optimization: no ref can match

    if node is DEBRUIJNREF(n):
        if n == level: return replacement     // this is the one we're binding
        else:          return node            // belongs to another binder

    if node is DEBRUIJNREC(body):
        return DEBRUIJNREC(substitute(body, level + 1, replacement))
                                              // ↑ inside a binder, target moves up

    otherwise:
        return rebuild(node, [substitute(child, level, replacement) for child in children])
```

The critical detail is the `level + 1` when descending into a nested
`DEBRUIJNREC`: the target reference level shifts because the inner binder
introduces a new scope. This ensures that only references belonging to the
*current* binder are substituted.

The **aperture shortcut** (`aperture(node) < level → return node`) avoids
traversing subtrees that cannot possibly contain a matching reference. This is
the main performance optimization and is shared between the C++ and Rust
implementations.

### 9.4 Worked Example: Simple Recursion

```
Input:   DEBRUIJNREC(add(delay1(proj(0, DEBRUIJNREF(1))), input(0)))

Step 1:  var = W0
Step 2:  substitute(body, 1, SYMREF(W0))
         → add(delay1(proj(0, SYMREF(W0))), input(0))
Step 3:  convert recursively (no more DEBRUIJNREC inside)
         → no change
Step 4:  SYMREC(W0, add(delay1(proj(0, SYMREF(W0))), input(0)))
```

### 9.5 Worked Example: Nested Recursion

```
Input:   DEBRUIJNREC(                              ← outer
             add(
                 DEBRUIJNREC(                      ← inner
                     mul(DEBRUIJNREF(1),           ← refers to inner
                         DEBRUIJNREF(2))           ← refers to outer
                 ),
                 DEBRUIJNREF(1)                    ← refers to outer
             )
         )

Outer conversion:
  var_outer = W0
  substitute(body, 1, SYMREF(W0)):
    - DEBRUIJNREF(1) in add → SYMREF(W0)
    - Inside inner DEBRUIJNREC: level becomes 2
      - DEBRUIJNREF(1) stays (level 1 ≠ 2) → still refers to inner
      - DEBRUIJNREF(2) matches level 2    → SYMREF(W0)

  After outer substitute:
    add(
        DEBRUIJNREC(mul(DEBRUIJNREF(1), SYMREF(W0))),
        SYMREF(W0)
    )

  Recurse convert into the inner DEBRUIJNREC:
    var_inner = W1
    substitute(mul(DEBRUIJNREF(1), SYMREF(W0)), 1, SYMREF(W1)):
      - DEBRUIJNREF(1) → SYMREF(W1)
      - SYMREF(W0) → unchanged (not a DEBRUIJNREF)

  Final result:
    SYMREC(W0, add(SYMREC(W1, mul(SYMREF(W1), SYMREF(W0))), SYMREF(W0)))
```

Each binder gets its own unique name. References are now explicit — `W0`
always means the outer group, `W1` always means the inner group, regardless
of nesting depth.

### 9.6 Fresh Variable Allocation

Both C++ and Rust allocate variable names from a deterministic sequence
`W0, W1, W2, ...`. The allocation must produce names that do not collide with
pre-existing symbols in the arena:

- **C++**: uses `unique("W")`, which generates a fresh name via a global
  counter.
- **Rust**: iterates the index counter, attempts to intern `W{n}`, and skips
  any name that was already present in the arena (detected by checking whether
  `arena.len()` increased after the intern call).

This collision avoidance is necessary because the arena may already contain
symbols named `W0`, `W1`, ... from evaluated Faust code or from earlier
conversion passes.

### 9.7 Memoization and Sharing

Both implementations preserve structural sharing through memoization:

- **C++**: uses tree properties (`setProperty`/`getProperty`) keyed by
  `DEBRUIJN2SYM` for conversion and a compound key
  `(SUBSTITUTE, level, replacement)` for substitution.
- **Rust**: uses three separate `AHashMap` caches inside the `Converter`
  struct: `convert_memo`, `substitute_memo`, and `aperture_memo`.

This is critical for performance: the signal tree has extensive sharing (the
`TreeArena` interns structurally identical subtrees), so without memoization
the same subtree would be traversed exponentially many times.

### 9.8 Implementation: C++ vs Rust

**C++ (`compiler/tlib/recursive-tree.cpp`)**

```cpp
static Tree calcDeBruijn2Sym(Tree t) {
    Tree body, var;
    if (isRec(t, body)) {
        var = tree(unique("W"));
        return rec(var, deBruijn2Sym(substitute(body, 1, ref(var))));
    } else if (isRef(t, var)) {
        return t;                       // already symbolic
    } else {
        // rebuild with converted children
        tvec br(t->arity());
        for (int i = 0; i < t->arity(); i++)
            br[i] = deBruijn2Sym(t->branch(i));
        return tree(t->node(), br);
    }
}
```

**Rust (`crates/tlib/src/recursion.rs`)**

```rust
fn convert(&mut self, id: TreeId) -> Result<TreeId, RecursionError> {
    if let Some(mapped) = self.convert_memo.get(&id) { return Ok(*mapped); }

    if let Some(body) = match_de_bruijn_rec(self.arena, id) {
        let var = self.fresh_var();
        let replacement = sym_ref(self.arena, var);
        let substituted = self.substitute(body, 1, replacement)?;
        let converted_body = self.convert(substituted)?;
        let out = sym_rec(self.arena, var, converted_body);
        self.convert_memo.insert(id, out);
        return Ok(out);
    }
    // ... SYMREF passthrough, DEBRUIJNREF error, generic rebuild
}
```

The Rust version differs in two ways:
1. Returns `Result` with typed errors instead of assertions (`faustassert`).
2. Keeps all memos in a single `Converter` struct (scoped lifetime) rather
   than tree properties (global lifetime).

### 9.9 Diagram: Conversion Flow

```
    DEBRUIJNREC ──────────────────────────────────────► SYMREC(W0, ...)
         │                                                  │
         │  1. fresh_var() → W0                             │
         │  2. substitute(body, 1, SYMREF(W0))              │
         │  3. convert(substituted_body)                    │
         │                                                  │
         ▼                                                  ▼
    DEBRUIJNREF(1) ─── substitute ──────────────────► SYMREF(W0)
    DEBRUIJNREF(2) ─── unchanged (level ≠ 1) ──────► DEBRUIJNREF(2)
                       (will be caught by outer convert)

    Other nodes ───── rebuild with converted children ───► same structure
```

---

## 10. Full Pipeline: De Bruijn to Symbolic to FIR

```
Box tree (Rec nodes)
         │
         ▼
    propagate_in_slot_env           ← De Bruijn encoding (sections 3–6)
         │
         ▼
Signal tree with DEBRUIJNREC / DEBRUIJNREF nodes
         │
         ▼
    de_bruijn_to_sym (tlib)         ← Symbolic conversion (section 9)
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

---

## 11. Design Discussion: Why Not Canonicalize in `propagate`?

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

## 12. Summary of Key Functions

| Function | File | Role |
|----------|------|------|
| `make_mem_sig_proj_list` | `propagate/lib.rs` | Seeds `Ri` feedback placeholders: `delay1(proj(i, DEBRUIJNREF(1)))` |
| `lift_signals` / `liftn` | `propagate/lib.rs` | Increments De Bruijn levels to avoid capture by new binders |
| `de_bruijn_aperture` / `de_bruijn_aperture_with_memo` | `tlib/recursion.rs` | Computes max free De Bruijn level (0 = closed, >0 = recursive). The `_with_memo` variant accepts an external cache for amortized use by `propagate`. See [section 7](#7-aperture-measuring-recursive-openness) |
| `debruijn_rec` / `debruijn_ref` | `propagate/lib.rs` | Constructors for `DEBRUIJNREC` / `DEBRUIJNREF` nodes |
| `de_bruijn_to_sym` | `tlib/recursion.rs` | Converts positional De Bruijn to named `SYMREC`/`SYMREF`. See [section 9](#9-conversion-de-bruijn-to-symbolic-form-de_bruijn_to_sym) |
| `Converter::substitute` | `tlib/recursion.rs` | Replaces `DEBRUIJNREF(level)` with a symbolic replacement, with aperture shortcut |
| `Converter::fresh_var` | `tlib/recursion.rs` | Allocates collision-free symbolic variable names (`W0`, `W1`, ...) |
| `canonicalize_unary_rec_projections` | `transform/signal_prepare.rs` | Normalizes single-body recursion groups to dense physical slot `0` for typing, promotion, and FIR lowering |

---

## 13. Glossary

- **Aperture**: The maximum free De Bruijn reference level in a subtree. If > 0, the subtree is open (has unbound recursive references); if 0, it is closed. Analogous to counting free variables in lambda calculus. Computed by three rules: `DEBRUIJNREF(n)` → `n`; `DEBRUIJNREC(body)` → `aperture(body) - 1`; other nodes → `max(children)`. See [section 7](#7-aperture-measuring-recursive-openness).
- **Binder** (`DEBRUIJNREC`): Introduces a recursive scope. Each binder captures references at level 1.
- **Degenerate recursion**: A recursive group where some outputs do not actually depend on the feedback. Their aperture is 0.
- **De Bruijn index/level**: A nameless reference scheme where the integer counts enclosing binders between reference and binding site.
- **Lifting** (`liftn`): Incrementing De Bruijn levels of free references to preserve correct binding when a new binder is introduced.
- **Projection** (`proj(i, group)`): Selects the i-th output from a multi-output recursive group.
- **Substitution** (`substitute`): Replaces all `DEBRUIJNREF` nodes at a given level with a replacement node. Descending into a nested `DEBRUIJNREC` increments the target level. Uses the aperture shortcut to skip closed subtrees.
- **SYMREC/SYMREF**: Named symbolic form of recursion, produced by `de_bruijn_to_sym` from the positional De Bruijn form. `SYMREC(var, body)` binds `var` in `body`; `SYMREF(var)` references it. See [section 9](#9-conversion-de-bruijn-to-symbolic-form-de_bruijn_to_sym).
