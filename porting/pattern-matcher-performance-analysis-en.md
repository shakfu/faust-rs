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
| Automaton rebuild per call | `make_pattern_matcher` called each `evalCase` | Same (before cache, see §8) |
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

## 7. Rust-only optimisation opportunities

The following improvements are applicable to the Rust implementation.
They are ranked by estimated impact; all are independent of each other.

### 7.1 Cache the automaton per `Case` node *(impact: extreme — implemented)*

**Status: implemented** (see §8 for design details).

Both C++ and Rust originally rebuild the automaton on every `apply_case_rules` call.
Since the `Case` node is immutable after construction (it is hash-consed in the arena),
the automaton can be computed once and reused for all subsequent applications of the same
`case` node.

Cost before: O(n_rules × pattern_depth) per call.
Cost after: O(n_rules × pattern_depth) for the first call, O(1) for all subsequent calls.

This is the most impactful single optimisation available — a Faust `case` used inside a
`par(i, N, …)` or `seq(i, N, …)` loop is applied N times, so the saving scales linearly
with N.

### 7.2 Remove dead `build_automaton_metadata` *(impact: medium, free)*

`build_automaton_metadata` computes the `match_num` flag for every state after construction
but `apply_pattern_matcher_internal` never reads it. The full DFS traversal of the automaton
graph is pure wasted work. Either the flag should be removed, or the numeric fast-path that
consults it should be implemented.

### 7.3 Defer `Environment` scope creation *(impact: medium)*

`apply_case_rules` currently creates one `Environment` scope per rule before matching begins:

```rust
let mut envs: Vec<Option<Environment>> = (0..n).map(|_| Some(env.push_scope())).collect();
```

If there are R rules and only 1 will win, R−1 scope copies are wasted. Refactoring to
record only `(variable → path)` pairs during matching and build a single scope for the
winning rule afterwards would save O(R) allocations per call.

### 7.4 `SmallVec` for `Trans` and `Rule` *(impact: medium)*

Typical states have 3–8 transitions and 1–3 rules. Using `SmallVec<[Trans; 8]>` and
`SmallVec<[Rule; 4]>` eliminates heap allocation for the common case. This reduces
allocator pressure during `make_pattern_matcher` and improves locality of the hot loop.

### 7.5 Reusable scratch buffer for `substs` *(impact: medium)*

`apply_pattern_matcher` allocates `vec![Vec::new(); n]` on every call. For a `case` with
5 rules applied to 3 arguments, that is 15 `Vec` allocations per case application.
Passing a pre-allocated `&mut Vec<Subst>` and calling `clear()` before each use reduces
this to zero on the hot path.

---

## 8. Implemented: automaton cache in `LoopDetector`

The automaton cache is stored as a private field of `LoopDetector`:

```rust
// pattern_matcher.rs
pub(crate) type AutomatonCache = AHashMap<TreeId, Automaton>;

// lib.rs — LoopDetector gains a private field
pub struct LoopDetector {
    call_stack: Vec<TreeId>,
    max_depth: usize,
    automaton_cache: AutomatonCache,   // ← new
}
```

`LoopDetector` is already threaded through every recursive `eval_box` call, making it
the natural carrier for evaluation-phase caches without changing any public API.

The lookup in `apply_case_rules` becomes:

```rust
if !loop_detector.automaton_cache.contains_key(&rules_rev) {
    let automaton = pattern_matcher::make_pattern_matcher(arena, rules_rev);
    loop_detector.automaton_cache.insert(rules_rev, automaton);
}
let automaton = loop_detector.automaton_cache.get(&rules_rev).unwrap();
```

Since `Automaton` is stored by value in the map (not behind a pointer), cache hits involve
a single hash lookup and a direct reference to the contiguous `Vec<State>` — no extra
indirection compared with the non-cached path.
