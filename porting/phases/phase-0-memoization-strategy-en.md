# Compiler Memoization Strategy (C++ to Rust)

This document outlines how algorithmic memoization (caching) translates from the C++ Faust compiler to the Rust compiler architecture.

## 1. The C++ Baseline: Property-Based Memoization

In the C++ Faust compiler, memoization is heavily reliant on the `Tree` property system (`compiler/tlib/property.hh`). 
Every `Tree` node can secretly host a dynamically-typed dictionary of properties. Compiler passes use specific key pointers (`gGlobal->NORMALFORM`, `gGlobal->RECURSIVNESS`, `gGlobal->OCCURRENCESPROPERTY`, etc.) to cache the result of costly recursive traversals directly onto the AST/DAG nodes.

Common C++ memoization sites:
- `getBoxTypeProperty`: Caches Box input/output arities.
- `getPropagateProperty`: Caches signal lists generated from a Box diagram.
- `getProperty(..., gGlobal->NORMALFORM)`: Caches signal simplification results.
- `getProperty(..., gGlobal->RECURSIVNESS)`: Caches cycle-detection markers for `letrec`.
- `getProperty(..., gGlobal->OCCURRENCESPROPERTY)`: Caches the number of times a signal is read (used for code-gen variable scheduling).

## 2. Why the `eval` Crate Does Not Need Memoization

A common question is whether the `evaluate` pass (lambda-calculus reduction of the block diagram) needs memoization.

In both C++ and Rust, `eval` **does not** memoize its deep reduction paths. 
This is because:
1. **Context Dependence**: The result of `eval(Tree)` depends entirely on the current lexical `Environment`. Caching would require a key of `(Tree, Environment)`. Hashing or cloning a full lexical environment is often more expensive than the evaluation itself.
2. **Hash-Consing**: The `TreeArena` natively canonicalizes structures (hash-consing). When two different evaluation paths yield the same structural Box/Signal (e.g., two identical oscillators), the arena assigns them the exact same physical `TreeId` (or memory pointer in C++). This ensures strict memory compression and prevents subsequent passes from blowing up exponentially, making `eval` memoization unnecessary.

## 3. The Rust Pattern: Explicit Cache Threading

Unlike C++ where nodes are mutable property bags, Rust's `TreeArena` issues immutable `TreeId` handles. We cannot attach pass-specific caches to nodes after they are interned.

Instead, the Rust port implements memoization via **Explicit Cache Threading**:
- A pass defines a local cache type, typically `AHashMap<TreeId, ResultType>`.
- The entry-point of the pass instantiates the cache.
- The cache is passed as a mutable reference (`&mut AHashMap`) down the recursive call tree.

### Example: `crates/propagate`
Arity inference (`box_arity`) and signal generation (`propagate`) historically caused exponential traversals on complex diagrams if not cached. 
In Rust, this is solved by threading an `ArityCache`:
```rust
pub type ArityCache = AHashMap<BoxId, Result<BoxArity, PropagateError>>;

pub fn box_arity(arena: &TreeArena, box_tree: BoxId, cache: &mut ArityCache) -> Result<BoxArity, PropagateError>;
```

## 4. Pending Memoization Sites in Rust

As the Rust compiler scaffold expands into full implementations, we MUST introduce explicit caches in the following passes to maintain linear time complexity on massive DAGs:

| System / Crate | C++ Property Equivalent | Rust Future Implementation | Purpose |
|----------------|-------------------------|----------------------------|---------|
| `normalize` / `signals` | `gGlobal->NORMALFORM` | `AHashMap<SigId, SigId>` in simplification loops | Avoid re-simplifying the same sub-signals (e.g. `X * 0 -> 0`). |
| `transform` | `gGlobal->RECURSIVNESS` | `AHashMap<SigId, bool>` or `HashSet<SigId>` | Detect infinite loops or mark recursive paths in FIR/IIR analysis. |
| `codegen` | `gGlobal->OCCURRENCESPROPERTY` | `AHashMap<SigId, usize>` in DAG scheduling | Count how many times a signal is consumed to decide if it gets stored in a local C++ variable. |
| `codegen` | `gGlobal->COMPUTEDELAYPROPERTY` | `AHashMap<SigId, usize>` | Compute the recursive delay of a signal for memory allocation. |

**Rule of Thumb:** Any compiler pass that traverses a recursive graph and can visit the same `TreeId` / `SigId` more than once through different structural branches (e.g., split/mix/par matrices) must implement an explicit `AHashMap` cache parameter.
