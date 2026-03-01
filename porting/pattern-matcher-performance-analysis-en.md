# Pattern Matcher — Rust vs C++ Performance Analysis

> **Date**: 2026-03-01
> **Scope**: `eval::pattern_matcher` (Rust) vs `compiler/patternmatcher/patternmatcher.cpp` (C++)
> **Status**: Structural analysis — no benchmark data yet

---

## 1. Context

The Faust pattern matcher compiles `case` rules into a deterministic tree automaton
(incremental Graef algorithm, RTA 1991), then applies that automaton to evaluate
`case` expressions at compile time. It is invoked once per `case` node application
during the box-evaluation phase (`eval_box` / `evalCase`).

Both the C++ and Rust implementations follow the same algorithm. The differences
are entirely **structural**: how data is laid out in memory and how it is traversed.

---

## 2. Data structure comparison

### 2.1 Automaton state storage

| | C++ | Rust |
|---|---|---|
| State container | `vector<State*>` — vector of heap pointers | `Vec<State>` — flat contiguous storage |
| State access | 2 indirections: vector index → pointer → State | 1 direct index into `Vec` |
| Cache behaviour | Each `State` is a separate heap allocation; accessing state[i] causes a pointer dereference | All `State` structs are adjacent in memory; sequential access is prefetch-friendly |

The C++ `Automaton` holds `vector<State*>` where each `State*` was allocated separately
(inheriting from `Garbageable`). Accessing `state[i]` first reads the pointer from the
vector, then dereferences it — two memory accesses, with the second likely causing an
L1 cache miss when the automaton is large or cold.

The Rust `Automaton` holds `Vec<State>`. State `i` is at a known byte offset inside
a single contiguous allocation. One load, no pointer chasing.

### 2.2 Transition list storage

This is **the hottest inner loop**: `apply_pattern_matcher_internal` iterates all
transitions of a state for each node visited in the argument tree.

| | C++ | Rust |
|---|---|---|
| Transition container | `list<Trans>` — doubly-linked list | `Vec<Trans>` — contiguous array |
| Per-element access | Pointer chase: each `Trans` holds `prev/next` pointers | Direct slice indexing |
| Cache lines for 4 transitions | 4 separate allocations, 4 cache misses | ~80 bytes total, fits in 2 cache lines |
| Prefetch | Not possible (next address unknown until dereference) | CPU prefetcher can pipeline |

For typical Faust `case` expressions with 3–8 transitions per state, the linked-list
access pattern in C++ guarantees one L1 cache miss per transition. The Rust `Vec<Trans>`
fits the entire transition list for a state into 1–2 cache lines.

**Estimated hot-path speedup on transition traversal: 3–8×**, depending on cache
temperature and number of transitions per state.

### 2.3 Tree node representation

| | C++ | Rust |
|---|---|---|
| Node identity | `Tree*` — 8-byte pointer | `TreeId` — 4-byte `u32` |
| Comparison | `x == cst` — pointer equality, O(1) | `x == cst` — integer equality, O(1) |
| Density in `TransKind::Constant` | 8 bytes per stored constant | 4 bytes per stored constant |
| Density in `Vec<Trans>` | Larger `Trans` structs | Smaller `Trans` structs → more fit per cache line |

The 2× smaller node representation means that for the same number of transitions,
the Rust `Vec<Trans>` occupies half the cache footprint of the equivalent C++ structure.

---

## 3. Memory management overhead

### 3.1 C++: `Garbageable` allocation cost

All C++ pattern-matcher objects (`State`, `Trans`, `Rule`, `Automaton`) inherit from
`Garbageable`, which participates in the Faust garbage collector or reference-counting
scheme. Each `new State(...)` call:
1. Calls the GC-aware allocator.
2. Registers the object for GC tracking.
3. At destruction: triggers GC bookkeeping.

For an automaton with N states, this is N separate GC-tracked heap allocations.

### 3.2 Rust: arena-local allocation

Rust states are pushed into `Vec<State>` via `Vec::push`. No GC registration, no
per-object bookkeeping. The entire automaton lives in three contiguous heap allocations:
- `Vec<State>` (states + their inline rules/trans Vecs)
- The `Vec<TreeId>` for `rhs`
- Individual `Vec<Rule>` and `Vec<Trans>` per state (amortised O(1) push)

At drop: a single deallocation cascade, no GC cycle.

---

## 4. Algorithmic equivalences

These aspects are **identical** between C++ and Rust — neither has an advantage:

| Aspect | C++ | Rust |
|---|---|---|
| Algorithm | Incremental Graef (RTA 1991) | Same |
| Automaton rebuild per call | `make_pattern_matcher` called each `evalCase` | Same |
| Transition ordering invariant | Var first, then Constants sorted, then Ops sorted | Same |
| Nonlinearity check | Path-based subterm extraction + equality check | Same (`subtree` + `TreeId` equality) |
| Rule priority | First active rule in final state wins | Same |
| `match_num` fast path | Computed, **not used** | Computed, **not used** |

The `match_num` flag — intended to skip numeric-constant checks when no numeric
pattern exists — is present in both implementations but never consulted during
matching. It represents a shared future optimisation opportunity.

---

## 5. Quantitative summary

| Dimension | C++ | Rust | Estimated ratio |
|---|---|---|---|
| Transition traversal (hot loop) | `list<Trans>` — 1 cache miss/step | `Vec<Trans>` — O(1) slice index | **3–8× faster in Rust** |
| State access | 2 indirections | 1 direct index | **~2× faster in Rust** |
| `TreeId` size | 8 bytes (`Tree*`) | 4 bytes (`u32`) | **2× denser in Rust** |
| GC overhead per automaton | N `Garbageable` allocs | 0 | **Rust has no GC cost** |
| Node comparison | Pointer equality | Integer equality | Equivalent |
| Algorithm complexity | O(depth × n_trans) | O(depth × n_trans) | Equivalent |

---

## 6. Limitations of this analysis

1. **No benchmark data**: All figures above are structural estimates, not measurements.
   A `criterion` benchmark comparing both on real Faust programs (e.g. `karplus.dsp`,
   `freeverb.dsp`) is needed to validate the 3–8× claim.

2. **The rebuild cost dominates for small inputs**: For a case with 2 rules and 1
   pattern each, the automaton construction (`make_state`, `merge_state`) may cost
   more than the matching pass. The structural advantages are more pronounced for
   larger case blocks (10+ rules, deep structural patterns).

3. **C++ compiler optimisations**: GCC/clang may partially mitigate the linked-list
   overhead through prefetching or inlining of `list<Trans>` traversal. Rust's LLVM
   backend has similar capabilities for slice iteration.

4. **Cache context**: The Faust compiler processes programs sequentially; the automaton
   is warm in cache if the same case node is applied repeatedly (e.g. in iterative
   forms). In that scenario, the C++ linked-list disadvantage shrinks but does not
   disappear.

---

## 7. Future optimisation: cache the automaton per `Case` node

Currently, both C++ and Rust rebuild the automaton on every `apply_case_rules` call.
Since the `Case` node is immutable after construction (it is hash-consed in the arena),
the automaton could be **computed once and stored** — either in a side table keyed by
`TreeId`, or as a lazily-initialised field associated with the `Case` node.

This would reduce the construction cost from O(n_rules × pattern_depth) per call to
O(1) per call (after the first). It is the most impactful single optimisation available
to both implementations, independent of the structural advantages described above.

In Rust, a natural implementation would be:

```rust
// Side table: TreeId of Case node → compiled Automaton
type AutomatonCache = AHashMap<TreeId, Automaton>;
```

Passed alongside `arena` into `apply_case_rules`, allowing the automaton to be
reused across repeated applications of the same `case` node.
