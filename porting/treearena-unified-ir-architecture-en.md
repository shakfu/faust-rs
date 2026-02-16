# Unified TreeArena Architecture for All Faust IR Layers

> **Scope**: Architectural rationale for using `tlib::TreeArena` hash-consing across all
> three compiler IR layers: Boxes, Signals, and FIR.
>
> **Status**: Design note (discussion synthesis)

---

## 1. Context

The Rust port of the Faust compiler uses `tlib::TreeArena` as a hash-consing arena
to represent tree-structured intermediate representations. Each structurally identical
node is interned once and identified by a `TreeId` (a `Copy`-able `u32` index).

Boxes and Signals already follow a consistent **Builder + Matcher** pattern on top of
`TreeArena`:

| Layer | Write-side | Read-side | Id type |
|-------|-----------|-----------|---------|
| Boxes | `BoxBuilder` | `match_box() -> BoxMatch` | `BoxId = TreeId` |
| Signals | `SigBuilder` | `match_sig() -> SigMatch` | `SigId = TreeId` |

The question is whether FIR (Faust Imperative Representation) — the final IR before
code emission — should follow the same pattern instead of using classical Rust enums
with `Box`/`Vec` ownership.

## 2. Core mechanism: how TreeArena works

### 2.1 Hash-consing invariant

`TreeArena` guarantees **structural uniqueness**: for a given arena instance, two nodes
with the same `NodeKind` and the same ordered children always receive the same `TreeId`.

```rust
let a = arena.intern(NodeKind::Tag("ADD".into()), &[]);
let b = arena.intern(NodeKind::Tag("ADD".into()), &[]);
assert_eq!(a, b);  // same TreeId — guaranteed

let s1 = arena.intern(NodeKind::Tag("SEQ".into()), &[a, a]);
let s2 = arena.intern(NodeKind::Tag("SEQ".into()), &[b, b]);
assert_eq!(s1, s2);  // structural sharing propagates recursively
```

### 2.2 Arity-specialized interning

The arena uses dedicated hash maps by children count, avoiding heap allocation for the
common 0/1/2 arity cases:

```rust
pub struct TreeArena {
    nodes: Vec<TreeNode>,                              // contiguous storage
    interner0: AHashMap<NodeKind, TreeId>,              // 0 children
    interner1: AHashMap<(NodeKind, TreeId), TreeId>,    // 1 child
    interner2: AHashMap<(NodeKind, TreeId, TreeId), TreeId>, // 2 children
    interner_n: AHashMap<NodeKey, TreeId>,              // 3+ children
    nil: TreeId,
}
```

`ChildList` mirrors this with an inline-storage enum (`Empty | One | Two | Many`).

### 2.3 Builder + Matcher pattern

**Construction** (write-side) — a builder wraps the arena and provides typed,
infallible factory methods:

```rust
let mut b = BoxBuilder::new(&mut arena);
let expr = b.seq(b.wire(), b.add());
```

Each method calls `arena.intern(NodeKind::Tag(TAG), &children)` internally.

**Deconstruction** (read-side) — a free function maps an opaque `TreeId` back to a
typed Rust enum:

```rust
match match_box(&arena, expr) {
    BoxMatch::Seq(left, right) => { /* ... */ }
    BoxMatch::Wire => { /* ... */ }
    _ => { /* ... */ }
}
```

This gives exhaustive pattern matching verified at compile time and lifetime-bound
references (`BoxMatch<'a>`) tied to the arena borrow.

**Cost model of `match_box`**: the dispatch is *not* zero-cost. Internally, `match_box`
performs: (1) an indexed lookup into `Vec<TreeNode>` to read the `NodeKind`, (2) a
string comparison on the `Tag` value against ~60 constant branches, (3) a second
indexed lookup to read the children slice, and (4) copy of the child `TreeId` values
(trivial — `u32` copies). The string-based tag dispatch is the main overhead compared
to a native Rust enum discriminant (which would be a single integer comparison). This
is the cost of architectural uniformity: all IR layers share the same `TreeArena` and
`NodeKind::Tag` mechanism instead of having per-layer Rust enum types. The cost is
small (a few nanoseconds per dispatch) but not zero.

### 2.4 Property storage

`PropertyStore<T>` attaches metadata to nodes by `(PropertyKey, TreeId)` with O(1)
indexed access — no hashing at lookup time:

```rust
let type_key = props.key("type");
props.set_with_key(node_id, type_key, some_annotation);
// internally: self.values[key_idx][node_id.as_u32()] = Some(value)
```

## 3. Why FIR should use the same architecture

### 3.1 FIR transformations are functional

The phase-6 design document defines FIR→FIR transformations as **input→output
functions**, not in-place mutations:

```rust
pub trait FirTransform {
    fn transform_value(&mut self, v: FirValue) -> FirValue { v }
    fn transform_stmt(&mut self, s: FirStmt) -> FirStmt { s }
    fn transform_block(&mut self, b: FirBlock) -> FirBlock {
        b.into_iter().map(|s| self.transform_stmt(s)).collect()
    }
}
```

Each pass takes a tree and produces a new one. Existing sub-trees are never mutated.
This is exactly the same model as Box and Signal transformations, and exactly the model
that hash-consing is designed to optimize: unchanged sub-trees keep their identity for
free.

### 3.2 FIR describes imperative code but is not itself mutable

The FIR *represents* imperative constructs (`ForLoop`, `StoreVar`, `If`), but the
representation itself is an immutable tree transformed functionally through passes.
These are two distinct levels:

| Level | Nature |
|-------|--------|
| What FIR **describes** | Imperative code (for, store, if) |
| What FIR **is** | An immutable tree rebuilt by each pass |

There is no reason to treat the data structure differently from Boxes or Signals.

### 3.3 The phase-6 document itself recommends arena IDs

Section 2.6, item 2 of `phase-6-fir-backends-en.md`:

> *"Replace raw-pointer instruction ownership with **arena IDs** and contiguous Rust
> containers (`Vec`/`SmallVec`) for stable traversal and simpler cloning."*

Using `TreeArena` with `FirId = TreeId` is the natural realization of this
recommendation, and extends it with hash-consing benefits.

### 3.4 Architectural coherence

A single representation model for all IR layers means one mental model, one set of
traversal patterns, and one shared infrastructure:

```
TreeArena (tlib)
    |
    +-- Box : BoxBuilder / match_box -> BoxMatch
    +-- Sig : SigBuilder / match_sig -> SigMatch
    +-- FIR : FirBuilder / match_fir -> FirMatch   <-- same pattern
```

## 4. Performance gains

### 4.1 Measured baseline: Rust TreeArena vs C++ CTree

The Phase 0 benchmark report (`phase-0-treearena-benchmark-report-en.md`) established
comparative performance at `n = 1,000,000` nodes (median of interleaved runs):

| Operation | Rust (ms) | C++ (ms) | Rust/C++ ratio |
|-----------|-----------|----------|----------------|
| create (intern new nodes) | 226.9 | 864.9 | **0.26x** |
| lookup (interner hits) | 210.2 | 719.5 | **0.29x** |
| traversal (cons/tl chain) | 99.8 | 984.2 | **0.10x** |
| property set | 5.8 | 468.5 | **0.01x** |
| property get | 2.1 | 7.6 | **0.28x** |

The Rust arena is **3-10x faster** than the C++ original on all measured operations.
This performance headroom means that applying `TreeArena` to FIR (an additional IR
layer) is well within budget.

### 4.2 Memory: structural deduplication

FIR programs exhibit massive sub-expression repetition:

- `LoadVar("fSampleRate", Struct)` — appears dozens/hundreds of times in a real DSP
- `Int32(0)`, `Int32(1)`, `Float(0.0)` — ubiquitous constants
- `BinOp(Mul, ...)` — recurring arithmetic patterns
- Loop bodies across channels share identical sub-structures

**Without hash-consing** (`Box<FirValue>`): each occurrence is a separate heap
allocation carrying its own `String`, `Box`, and payload. A 100-channel DSP allocates
100 identical `LoadVar("fSampleRate")` nodes.

**With hash-consing** (`TreeArena`): all identical occurrences map to a single
`TreeId`. Typical deduplication ratios in hash-consing compilers are **30-70%** of
total node count.

### 4.3 CPU cache: spatial locality

`TreeArena` stores all nodes in a single `Vec<TreeNode>` — contiguous memory. Every
traversal pass benefits from hardware prefetching.

With `Box`-based trees, each pointer dereference is a potential cache miss to a
different heap location. This matters because FIR undergoes ~20 transformation passes
(`MoveVariablesInFront`, `FunctionInliner`, `CastRemover`, `ControlExpander`,
`ArrayToPointer`, ...), each traversing the entire tree. The cache locality gain
compounds across all passes.

### 4.4 Equality: O(1) vs O(n)

| | `Box<FirValue>` | `TreeArena` |
|---|---|---|
| Structural equality | O(tree size), recursive | O(1), compare two `u32` |
| Use as HashMap key | Recursive hash + eq | Direct `u32` key |

This directly impacts:
- **CSE (Common Subexpression Elimination)**: free with hash-consing — same structure
  already has same `TreeId`. Without it, every candidate pair requires recursive
  comparison.
- **FirTypeChecker**: verification caches indexed by `TreeId` use direct `Vec` indexing
  instead of `HashMap` with recursive hashing.
- **Optimization passes**: any pass that checks "have I seen this sub-expression
  before?" benefits from O(1) identity.

### 4.5 Copies: Copy vs Clone

| | `Box<FirValue>` | `TreeArena` |
|---|---|---|
| Copy a sub-tree reference | `clone()` — deep-copy, O(n) allocations | Copy a `u32` — one CPU instruction |

When `FunctionInliner` substitutes a call with a function body, with `TreeArena` it
copies a single `FirId`. When `MoveVariablesInFront` rearranges blocks, unchanged
sub-trees are just `u32` values moved around.

### 4.6 Allocation: amortized vs per-node

| | `Box<FirValue>` | `TreeArena` |
|---|---|---|
| Per-node cost | 1 `malloc` call (~20-50ns) | `Vec::push` amortized O(1), or 0 if already interned |

Over thousands of FIR nodes, the difference in allocator pressure is substantial.

### 4.7 Deallocation: O(1) vs O(n) recursive drop

| | `Box<FirValue>` | `TreeArena` |
|---|---|---|
| Drop entire IR | Recursive drop through every `Box`, `String`, `Vec` — O(n) `free` calls | Drop one `Vec<TreeNode>` + interner tables — O(1) allocator calls |

### 4.8 Property lookups

| | `Box<FirValue>` | `TreeArena` |
|---|---|---|
| Attach annotation to a node | `HashMap<*const FirValue, T>` — hash + compare | `props.values[key_idx][tree_id]` — direct Vec index, O(1) |

For FIR, this means type annotations, inlining flags, memory zone info (`iZone`/`fZone`),
and scheduling metadata can all be attached via `PropertyStore` with the same
high-performance pattern already proven on Boxes and Signals.

## 5. Dispatch cost: Rust `match_box` vs C++ `isBoxX`

### 5.1 How C++ dispatch works

In the C++ Faust compiler, symbols are **interned pointers** (`Sym`). Each box/signal
tag (e.g. `BOXSEQ`, `SIGBINOP`) is a unique pointer allocated once at startup in the
global table:

```cpp
// global.hh — allocated once during initialization
Sym BOXSEQ;    // unique pointer
Sym BOXPAR;    // another unique pointer
Sym SIGINPUT;  // ...
```

Node comparison uses `Node::operator==`, which compares two integers:

```cpp
bool operator==(const Node& n) const {
    return fType == n.fType && fData.v == n.fData.v;  // type tag + pointer-as-int64
}
```

When `fType == kSymNode`, `fData.s` holds the `Sym` pointer. Comparing two symbol nodes
is a **single pointer equality** — one CPU instruction.

A box pattern match like `isBoxSeq` then does:

```cpp
bool isBoxSeq(Tree t, Tree& x, Tree& y) {
    return isTree(t, gGlobal->BOXSEQ, x, y);
}

bool isTree(const Tree& t, const Node& n, Tree& a, Tree& b) {
    if ((t->node() == n) && (t->arity() == 2)) {   // pointer compare + int compare
        a = t->branch(0);                           // read from std::vector<Tree>
        b = t->branch(1);
        return true;
    }
    return false;
}
```

Cost of one `isBoxSeq` call: **~10–15 cycles** (pointer compare + arity check + 2
branch reads from `std::vector`).

### 5.2 How C++ dispatches across node types

The typical C++ pattern (e.g. `propagate.cpp`, `simplify.cpp`) is a **linear if/else
chain**:

```cpp
if (isBoxInt(box, &i))          { ... }
else if (isBoxReal(box, &r))    { ... }
else if (isBoxWire(box))        { ... }
else if (isBoxCut(box))         { ... }
else if (isBoxSeq(box, t1, t2)) { ... }
else if (isBoxPar(box, t1, t2)) { ... }
// ... 50+ branches
```

Each **failed** test pays the full cost: dereference the `Tree*` pointer (potential
cache miss — trees are individually heap-allocated via `new CTree`), read the node,
compare the symbol pointer, check arity. If the matching branch is the 30th in the
chain, 29 tests have been executed and failed — each accessing the same `Tree*` but
also touching `gGlobal->BOXFOO` symbol pointers scattered across memory.

Typical dispatch cost: **~100–300 cycles** (10–20 failed tests × 10–15 cycles each).

### 5.3 How Rust `match_box` dispatches

```rust
pub fn match_box<'a>(arena: &'a TreeArena, id: BoxId) -> BoxMatch<'a> {
    match arena.kind(id) {                          // (1) one Vec index → NodeKind
        Some(NodeKind::Tag(tag)) => match tag.as_ref() {
            "BOXSEQ" => match match_binary(...) {   // (2) string match over ~60 branches
                Some((a, b)) => BoxMatch::Seq(a, b),
                ...
            },
            ...
        },
        ...
    }
}
```

The Rust path reads from the arena **once** (step 1), then dispatches via a
`match` on `&str` against ~60 constant string branches (step 2). The compiler can
optimize this multi-way string match (length pre-check, prefix discrimination, etc.),
but it remains fundamentally costlier than a single pointer comparison.

However, the node is accessed **only once** — not N times as in the C++ if/else chain.

### 5.4 Per-step cost comparison

| Step | C++ (`isBoxSeq`) | Rust (`match_box`) |
|------|-------------------|-------------------|
| **Identify node type** | `node() == n` → pointer comparison ≈ 2–4 cycles | `match tag.as_ref()` → string comparison over ~60 branches ≈ 20–80 cycles |
| **Check arity** | `arity() == 2` → int compare ≈ 1 cycle | `children.len() != 2` → int compare ≈ 1 cycle |
| **Extract children** | `branch(0), branch(1)` → 2 reads from `std::vector<Tree>` (64-bit pointers) ≈ 2–4 cycles | `children.get(0), get(1)` → 2 reads from `ChildList` (32-bit `u32`) ≈ 2–4 cycles |
| **Memory layout** | `Tree*` individually heap-allocated (`new CTree`) → scattered | `Vec<TreeNode>` contiguous → prefetch-friendly |
| **Children storage** | `std::vector<Tree>` — separate heap allocation, 64-bit pointers | `ChildList` enum — inline for arity 0–2, no heap, 32-bit ids |

### 5.5 Full dispatch cost comparison

| Scenario | C++ (linear if/else chain) | Rust (`match_box`) |
|----------|----------------------------|-------------------|
| **Best case** (first branch matches) | ~10–15 cycles | ~30–40 cycles |
| **Typical case** (match on 10th–20th branch) | ~100–300 cycles | ~30–100 cycles |
| **Worst case** (match on 50th branch) | ~500–750 cycles | ~50–100 cycles |

**Key insight**: the C++ approach has a **lower per-comparison cost** (pointer vs
string), but the Rust approach makes **fewer memory accesses per dispatch** (one arena
read vs N `Tree*` dereferences). For typical programs, Rust `match_box` is **~2–3x
faster** on the dispatch path alone because the C++ linear chain accumulates failed
tests.

### 5.6 FIR-specific comparison: visitor pattern vs `match_fir`

In C++, FIR instructions use the **visitor pattern** with virtual dispatch:

```cpp
struct InstVisitor {
    virtual void visit(LoadVarInst* inst) {}
    virtual void visit(StoreVarInst* inst) {}
    virtual void visit(BinopInst* inst) {}
    // ~50 virtual methods
};

// dispatch: inst->accept(visitor) → vtable lookup + indirect call
```

Cost: 1 vtable load + 1 indirect jump ≈ **~10–15 cycles**. This is the most efficient
C++ dispatch mechanism of the three IR layers, because the CPU resolves a single
indirection regardless of the number of instruction types.

With `TreeArena` + `match_fir`, FIR dispatch would use the same string-based mechanism
as `match_box`: **~30–100 cycles**. This is slower than the C++ virtual dispatch for
FIR specifically. However, this per-dispatch cost is dominated by the cumulative gains
from hash-consing (structural sharing, O(1) equality, zero-cost copies, cache locality,
bulk deallocation, property lookups) which apply across all ~20 FIR transformation
passes.

### 5.7 Optimization path: numeric tags

The string-comparison overhead can be eliminated by replacing `NodeKind::Tag(Arc<str>)`
with a **numeric tag id**:

```rust
pub enum NodeKind {
    Nil,
    Cons,
    Symbol(Arc<str>),
    StringLiteral(Arc<str>),
    Int(i64),
    FloatBits(u64),
    Tag(u32),          // ← numeric index instead of Arc<str>
}
```

With a tag registry mapping `&str → u32` populated at initialization (mirroring the C++
`Sym` interning), the dispatch in `match_box` / `match_sig` / `match_fir` would become
a `match` on a `u32` — a jump table or binary search on integers, comparable to
the C++ pointer comparison cost (~2–4 cycles per comparison). This would bring the
Rust dispatch to:

| Scenario | Current (string tags) | With numeric tags | C++ (pointer `Sym`) |
|----------|-----------------------|-------------------|---------------------|
| Best case | ~30–40 cycles | ~5–10 cycles | ~10–15 cycles |
| Typical case | ~30–100 cycles | ~5–15 cycles | ~100–300 cycles |
| Worst case | ~50–100 cycles | ~10–20 cycles | ~500–750 cycles |

This optimization is backward-compatible: the builder API and matcher API remain
identical; only the internal `NodeKind` representation changes. It can be applied
after MVP parity is achieved, guided by profiling data.

### 5.8 Summary: dispatch is not the bottleneck

Even without the numeric tag optimization, the Rust dispatch model trades a higher
per-comparison cost for fewer memory accesses and better cache behavior. The net effect
is favorable for typical programs. Combined with all the other `TreeArena` benefits
(which apply at every other point in the compilation pipeline), the dispatch overhead
is not a performance concern.

## 6. Concrete FIR design sketch

```rust
pub type FirId = TreeId;

/// Write-side: typed construction over TreeArena
pub struct FirBuilder<'a> {
    arena: &'a mut TreeArena,
}

impl<'a> FirBuilder<'a> {
    pub fn new(arena: &'a mut TreeArena) -> Self { Self { arena } }

    pub fn int32(&mut self, v: i32) -> FirId {
        self.arena.int(v as i64)
    }
    pub fn float(&mut self, v: f64) -> FirId {
        self.arena.float(v)
    }
    pub fn load_var(&mut self, name: &str, access: FirId) -> FirId {
        let sym = self.arena.symbol(name);
        intern_tag(self.arena, FIR_LOADVAR_TAG, &[sym, access])
    }
    pub fn bin_op(&mut self, op: FirId, lhs: FirId, rhs: FirId) -> FirId {
        intern_tag(self.arena, FIR_BINOP_TAG, &[op, lhs, rhs])
    }
    pub fn for_loop(&mut self, var: FirId, upper: FirId, body: FirId) -> FirId {
        intern_tag(self.arena, FIR_FORLOOP_TAG, &[var, upper, body])
    }
    pub fn store_var(&mut self, name: &str, access: FirId, value: FirId) -> FirId {
        let sym = self.arena.symbol(name);
        intern_tag(self.arena, FIR_STOREVAR_TAG, &[sym, access, value])
    }
    pub fn block(&mut self, stmts: &[FirId]) -> FirId {
        // Build as cons-list — hash-consed, identical blocks share identity
        let mut list = self.arena.nil();
        for &s in stmts.iter().rev() {
            list = self.arena.cons(s, list);
        }
        list
    }
    // ... all other FIR node types
}

/// Read-side: typed enum for pattern matching
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FirMatch<'a> {
    // Values
    Int32(i64),
    Float(f64),
    LoadVar { name: &'a str, access: FirId },
    BinOp { op: FirId, lhs: FirId, rhs: FirId },
    Neg(FirId),
    Cast { typ: FirId, value: FirId },
    Select2 { cond: FirId, then_: FirId, else_: FirId },
    FunCall { name: &'a str, args: FirId },
    ArrayAccess { array: FirId, index: FirId },

    // Statements
    DeclareVar { name: &'a str, typ: FirId, access: FirId, init: FirId },
    StoreVar { name: &'a str, access: FirId, value: FirId },
    ForLoop { var: FirId, upper: FirId, body: FirId },
    If { cond: FirId, then_: FirId, else_: FirId },
    Block(FirId),  // head of cons-list
    Return(FirId),

    // UI
    AddSlider { typ: FirId, label: &'a str, var: FirId, params: FirId },
    AddButton { typ: FirId, label: &'a str, var: FirId },
    // ... other FIR node types

    Unknown,
}

/// Canonical dispatcher
pub fn match_fir<'a>(arena: &'a TreeArena, id: FirId) -> FirMatch<'a> {
    // Same structure as match_box / match_sig
    match arena.kind(id) {
        Some(NodeKind::Int(v)) => FirMatch::Int32(*v),
        Some(NodeKind::FloatBits(bits)) => FirMatch::Float(f64::from_bits(*bits)),
        Some(NodeKind::Tag(tag)) => match tag.as_ref() {
            FIR_LOADVAR_TAG => { /* extract children */ }
            FIR_BINOP_TAG => { /* extract children */ }
            FIR_FORLOOP_TAG => { /* extract children */ }
            // ...
            _ => FirMatch::Unknown,
        },
        _ => FirMatch::Unknown,
    }
}

/// Functional FIR->FIR transformation (unchanged sub-trees keep their FirId)
pub trait FirTransform {
    fn transform(&mut self, arena: &mut TreeArena, id: FirId) -> FirId;
}
```

## 7. Summary: one architecture, three IR layers

| Property | Mechanism | Benefit |
|----------|-----------|---------|
| Structural sharing | Hash-consing in `TreeArena` | Memory proportional to unique nodes, not total |
| O(1) equality | Same structure = same `TreeId` | Trivial comparison, free HashMap keys |
| Zero-cost copy | `TreeId` is `Copy` (`u32`) | No deep-clone, no allocation |
| Cache-friendly traversal | Contiguous `Vec<TreeNode>` storage | Hardware prefetching across all ~20 FIR passes |
| Amortized allocation | `Vec::push` + interner dedup | No per-node `malloc` |
| Bulk deallocation | Drop one `Vec` | No recursive `free` walk |
| O(1) property lookup | `PropertyStore` indexed by `TreeId` | Direct `Vec` indexing, no hashing |
| Exhaustive matching | `FirMatch` enum + `match_fir()` | Compile-time coverage checking (dispatch has small string-comparison cost, see §2.3) |
| Architectural coherence | Same pattern for Box / Sig / FIR | One mental model for the entire compiler |
| Proven performance | Phase 0 benchmarks: 3-10x faster than C++ | Ample headroom for adding FIR to the arena |

The `TreeArena` + `Builder` + `Matcher` pattern is not specific to functional IR — it
applies to any immutable tree that is built once and transformed functionally. FIR meets
this criterion: it describes imperative code but is itself an immutable data structure
rebuilt by each transformation pass. Extending the pattern to FIR unifies the compiler
around a single, high-performance representation model.
