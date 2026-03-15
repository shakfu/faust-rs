# BoxPatternMatcher — First-class Arena Node Plan

**Date**: 2026-03-15
**Status**: Ready for implementation
**Prerequisite**: pattern-match-numeric-simplify (done — `match_num` + Real→Int coercion)
**Blocks**: 9 impulse-test DSP files (stack overflow in `force_value_to_box`)

## Problem

Partially-applied `case` expressions cannot be faithfully represented in the
tree arena. The current `EvalValue::PatternMatcher` lives on the Rust host side
only. When `force_value_to_box` is called (e.g. inside `eval_box → BoxMatch::Seq`),
a partially-applied PM must be converted to a `TreeId`. The current workaround
calls `lower_pattern_matcher_to_symbolic`, which re-enters the evaluator
(`apply_value_list → eval_value → force_value_to_box`) creating infinite
recursion → stack overflow.

The C++ compiler solves this with `boxPatternMatcher` — a first-class tree node
that carries the automaton state directly in the hash-consed tree. The evaluator
returns it as-is, `applyList` advances it, and `a2sb` lowers it to symbolic
form only at the very end.

## C++ Reference

### Node structure (`boxes.cpp:1164-1186`)

```cpp
Tree boxPatternMatcher(Automaton* a, int state, Tree env, Tree orig, Tree revParList) {
    return tree(BOXPATMATCHER, tree((void*)a), tree(state), env, orig, revParList);
}
```

5 branches:
1. `tree((void*)automaton_ptr)` — pointer to Automaton (GC-managed)
2. `tree(state)` — integer automaton state index
3. `env` — Faust-list of per-rule environment optionals
4. `orig` — original `case{...}` rules box (for error messages / `a2sb`)
5. `revParList` — reversed list of consumed argument boxes

### Key lifecycle points

| Site | Behaviour |
|------|-----------|
| `eval()` (line 638) | Returns `boxPatternMatcher` as-is (already in normal form) |
| `applyList()` (line 1253) | Detects `isBoxPatternMatcher`, calls `apply_pattern_matcher` with current state, builds NEW `boxPatternMatcher` with updated state/envs/params |
| `a2sb()` (line 223) | Creates symbolic slots for remaining args, applies PM to fill them, wraps result in nested `boxSymbolic` |

## Design

### Challenge: Automaton cannot be hash-consed

The `TreeArena` stores hash-consed nodes of type `NodeKind` (Int, Real, Symbol,
Tag, Nil). An `Automaton` is a complex struct (states, transitions, RHS trees,
variable sets) that cannot be meaningfully interned into the tree.

**Solution**: Side-table of `Automaton` values indexed by a dense integer key.
The tree node stores that key as an `Int` child.

### Side-table location

Add to `eval/src/lib.rs` (evaluator scope):

```rust
/// Side-table mapping dense keys to pattern-matcher Automatons.
/// Key 0 is reserved (unused). Keys are monotonically increasing.
struct AutomatonStore {
    entries: Vec<AutomatonEntry>,
}

struct AutomatonEntry {
    automaton: pattern_matcher::Automaton,
}

impl AutomatonStore {
    fn new() -> Self { Self { entries: vec![] } }

    fn insert(&mut self, automaton: pattern_matcher::Automaton) -> i64 {
        let key = self.entries.len() as i64;
        self.entries.push(AutomatonEntry { automaton });
        key
    }

    fn get(&self, key: i64) -> Option<&pattern_matcher::Automaton> {
        self.entries.get(key as usize).map(|e| &e.automaton)
    }
}
```

Thread it through the evaluator as `&mut AutomatonStore` alongside `&mut TreeArena`.

### Tree node encoding

Tag: `BOXPATMATCHER`

Children (4):
1. `boxInt(automaton_key)` — index into `AutomatonStore`
2. `boxInt(state)` — automaton state index
3. `env_list` — Faust-list encoding of per-rule environments (each element is
   either `nil` (failed rule) or an association-list of bindings)
4. `original_rules` — the original `boxCase(rules)` tree

Note: C++ has 5 branches (including `revParList`). We omit `revParList` because
Rust's evaluator does not need it — the consumed arguments are already folded
into the `env_list` bindings by `apply_pattern_matcher`. The original rules
tree is kept for `a2sb` arity computation and error messages.

### Alternative: keep `revParList` for strict C++ parity

If we later need `revParList` (e.g. for debugging or printing), add it as a 5th
child. For now the minimal 4-child encoding suffices.

## Implementation Steps

### Step 1: Add tag and builder in `crates/boxes/src/lib.rs`

```rust
// Tag constant (near line 113, after BOX_CASE_TAG)
const BOX_PATTERN_MATCHER_TAG: &str = "BOXPATMATCHER";

// BoxMatch variant (near line 908, after Case)
/// Partially-applied pattern matcher: (automaton_key, state, envs, original_rules)
PatternMatcher(BoxId, BoxId, BoxId, BoxId),

// BoxBuilder method
pub fn pattern_matcher(
    &mut self,
    automaton_key: BoxId,
    state: BoxId,
    envs: BoxId,
    original_rules: BoxId,
) -> BoxId {
    let tag = self.arena.intern_tag(BOX_PATTERN_MATCHER_TAG);
    self.arena.tagged4(tag, automaton_key, state, envs, original_rules)
}

// match_box arm (in the arity-4 dispatch section)
BOX_PATTERN_MATCHER_TAG => BoxMatch::PatternMatcher(c[0], c[1], c[2], c[3]),
```

Verify: `cargo check -p boxes`

### Step 2: Add `AutomatonStore` in `crates/eval/src/lib.rs`

Define `AutomatonStore` as described above. Add it as a field to the evaluator
context (currently threaded as function parameters — add one more parameter,
or bundle arena + store into a context struct).

**Decision point**: Threading `&mut AutomatonStore` alongside `&mut TreeArena`
through every evaluator function is verbose. Two options:

- **A) Separate parameter** — minimal diff, explicit, but touches many
  signatures. Same pattern as `&mut LoopDetector`.
- **B) Context struct** — bundle `arena`, `automaton_store`, `loop_detector`
  into `EvalCtx`. Cleaner long-term but large mechanical diff.

**Recommendation**: Option A for now (separate parameter). Refactor to B later.

### Step 3: Encode PM environments as tree lists

The C++ `boxPatternMatcher` stores environments as a Faust tree-list (each
element is itself a list of `(sym, value)` pairs, or `nil` for failed rules).

We need two helper functions:

```rust
/// Encode Vec<Option<Environment>> as a Faust list of association-lists.
fn envs_to_tree(arena: &mut TreeArena, envs: &[Option<Environment>]) -> TreeId

/// Decode a Faust list of association-lists back to Vec<Option<Environment>>.
fn tree_to_envs(arena: &TreeArena, tree: TreeId) -> Vec<Option<Environment>>
```

Each `Environment` is serialised as a list of `(symbol, box)` pairs. Since
environments bind `EvalValue` (which can be Closure or PatternMatcher), we only
need to handle the common case where pattern-matcher rule environments bind
boxes (which they do — `apply_pattern_matcher` binds raw `TreeId` subterms).

**Simplification**: Rule environments from `apply_pattern_matcher` only contain
`TreeId` bindings (variable → matched subtree). We can encode each as a
flat assoc-list of `(sym_node, value_node)` pairs.

### Step 4: Create `boxPatternMatcher` in `eval_case_rules`

Currently `eval_case_rules` builds an `EvalValue::PatternMatcher`. Change it to:

1. Insert the `Automaton` into `AutomatonStore` → get `key`.
2. Build the initial env list (all rules active → all `Some(empty_env)`).
3. Create the tree node:
   ```rust
   let mut b = BoxBuilder::new(arena);
   let key_node = b.int(key as i32);
   let state_node = b.int(0);
   let envs_node = envs_to_tree(arena, &initial_envs);
   b.pattern_matcher(key_node, state_node, envs_node, original_rules)
   ```
4. Return `EvalValue::Box(pm_node)`.

### Step 5: Handle `boxPatternMatcher` in `eval_box`

In `eval_box`, add a `BoxMatch::PatternMatcher` arm that returns the node
as-is (matching C++ `eval()` line 638):

```rust
BoxMatch::PatternMatcher(_, _, _, _) => Ok(expr),
```

Since it's already a `TreeId`, `force_value_to_box` is never called — this
eliminates the stack overflow.

### Step 6: Handle `boxPatternMatcher` in `apply_value_list_value`

In the `EvalValue::Box(fun)` arm of `apply_value_list_value`, detect
`BoxMatch::PatternMatcher` and handle it:

```rust
BoxMatch::PatternMatcher(key_node, state_node, envs_node, orig_node) => {
    let key = match_box_int(arena, key_node)?;
    let state = match_box_int(arena, state_node)? as usize;
    let envs = tree_to_envs(arena, envs_node);
    let automaton = automaton_store.get(key as i64).unwrap();

    // Consume one argument
    let arg = arena.hd(larg).unwrap();
    let tl = arena.tl(larg).unwrap();

    let arg_val = eval_box(arena, arg, env, loop_detector)?;
    let (new_state, new_envs) = apply_pattern_matcher(
        arena, automaton, state, arg_val, &envs
    );

    if new_state < 0 {
        return Err(EvalError::PatternMatchFailed { node: orig_node });
    }

    // Check for final state
    if automaton.final_state(new_state as usize) && arena.is_nil(tl) {
        // Pick first matching rule and evaluate RHS
        for rule in &automaton.states[new_state as usize].rules {
            if let Some(rule_env) = &new_envs[rule.r] {
                return eval_value(arena, automaton.rhs[rule.r], rule_env, loop_detector);
            }
        }
        return Err(EvalError::PatternMatchFailed { node: orig_node });
    }

    // Build updated boxPatternMatcher
    let mut b = BoxBuilder::new(arena);
    let new_key = b.int(key);
    let new_st = b.int(new_state);
    let new_envs_node = envs_to_tree(arena, &new_envs);
    let pm_node = b.pattern_matcher(new_key, new_st, new_envs_node, orig_node);

    if arena.is_nil(tl) {
        Ok(EvalValue::Box(pm_node))
    } else {
        apply_value_list_value(arena, EvalValue::Box(pm_node), tl, env, loop_detector, call_site)
    }
}
```

### Step 7: Handle `boxPatternMatcher` in `a2sb`

`a2sb` must lower a `boxPatternMatcher` to symbolic form when it encounters
one during signal generation:

```rust
BoxMatch::PatternMatcher(key_node, state_node, envs_node, orig_node) => {
    let key = match_box_int(arena, key_node)?;
    let state = match_box_int(arena, state_node)? as usize;
    let envs = tree_to_envs(arena, envs_node);
    let automaton = automaton_store.get(key as i64).unwrap();

    // Compute how many more arguments are needed
    let total_arity = case_expected_arity(arena, orig_node)?;
    let consumed = /* derive from automaton depth or state */;
    let remaining = total_arity - consumed;

    // Create fresh symbolic slots
    let slots: Vec<_> = (0..remaining).map(|_| fresh_slot(arena, loop_detector)).collect();

    // Apply slots one by one to advance the PM to final state
    let mut current = expr; // the boxPatternMatcher node
    for slot in &slots {
        let slot_list = arena.cons(*slot, arena.nil());
        let applied = apply_value_list_value(
            arena, EvalValue::Box(current), slot_list, &Environment::empty(),
            loop_detector, None
        )?;
        current = force_value_to_box(arena, applied, loop_detector)?;
    }

    // Wrap in nested boxSymbolic
    let mut result = current;
    for slot in slots.into_iter().rev() {
        let mut b = BoxBuilder::new(arena);
        result = b.symbolic(slot, result);
    }
    Ok(result)
}
```

### Step 8: Remove `EvalValue::PatternMatcher` variant

Once all PM handling goes through `boxPatternMatcher` tree nodes:

1. Remove `PatternMatcherValue` struct.
2. Remove `EvalValue::PatternMatcher(PatternMatcherValue)` variant.
3. Remove `lower_pattern_matcher_to_symbolic` (no longer needed).
4. Remove the PM arm from `force_value_to_box`.
5. Simplify `apply_value_list_value` — remove the dedicated PM arm; the
   `EvalValue::Box` arm now handles it via `BoxMatch::PatternMatcher`.

### Step 9: Clean up debug code

- Remove `AtomicUsize` depth counter in `simplify_pattern`.
- Remove any temporary `eprintln!` / `println!` debug traces.

### Step 10: Verify

```bash
# Single file that was failing
./target/release/faust-rs --dump-sig /Users/letz/faust/tests/impulse-tests/dsp/carre_volterra.dsp

# All 9 affected files
for f in carre_volterra gate_compressor parametric_eq phaser_flanger \
         pitch_shifter spectral_tilt thru_zero_flanger vcf_wah_pedals \
         virtual_analog_oscillators; do
  ./target/release/faust-rs --dump-sig \
    "/Users/letz/faust/tests/impulse-tests/dsp/${f}.dsp" 2>&1 | tail -1
done

# Full impulse suite
for f in /Users/letz/faust/tests/impulse-tests/dsp/*.dsp; do
  ./target/release/faust-rs --dump-sig --timeout 120 "$f" >/dev/null 2>&1 \
    || echo "FAIL: $(basename $f)"
done

# Unit tests
cargo test -p eval
cargo test -p boxes
```

## Files to modify

| File | Change |
|------|--------|
| `crates/boxes/src/lib.rs` | Add `BOX_PATTERN_MATCHER_TAG`, `BoxMatch::PatternMatcher`, `BoxBuilder::pattern_matcher`, `match_box` arm |
| `crates/eval/src/lib.rs` | Add `AutomatonStore`, `envs_to_tree`/`tree_to_envs`, handle `BoxMatch::PatternMatcher` in `eval_box`, `apply_value_list_value`, `a2sb`, `force_value_to_box`; remove `EvalValue::PatternMatcher` |
| `crates/eval/src/pattern_matcher.rs` | No changes expected (already updated for `match_num`) |

## Risk assessment

| Risk | Mitigation |
|------|-----------|
| Environment serialisation loses closure bindings | Pattern-matcher rule envs only bind raw TreeId (verified by inspection of `apply_pattern_matcher`) |
| `AutomatonStore` keys leak (never freed) | Acceptable for batch compiler; same as C++ GC-managed pointers |
| Hash-consing of `boxPatternMatcher` nodes with same key but different envs | Different envs → different `envs_node` → different tree node. Correct. |
| `consumed` count hard to derive in `a2sb` | Use `automaton.states[0].trans` depth or `rev_param_list` length. Alternative: store consumed count as extra Int child. |

## Estimated effort

~200-300 lines of code changes across 2 files. Mechanical but straightforward —
follows the proven C++ architecture exactly.
