# Local patches

Fork-local changes to upstream (`grame-cncm/faust-rs`) crates, parked here as
patch files rather than carried in the tree. Keeping them out of the tree means
`git merge upstream/main` touches only files upstream owns, so these changes
cannot produce merge conflicts.

Apply from the repository root:

```sh
git apply patches/<name>.patch
```

Reverse with `git apply -R patches/<name>.patch`. The patches are independent
and apply in either order.

Both are generated against upstream, not against the fork's merge base, so they
apply directly to a pristine upstream checkout — which also makes each one a
ready-to-send upstream proposal.

## eval-depth-guard.patch

Proposed upstream as
[grame-cncm/faust-rs#16](https://github.com/grame-cncm/faust-rs/issues/16) (open).

Bounds *syntactic* recursion depth in the evaluator. `LoopDetector` gains an
`eval_depth` counter, incremented at every `eval_value` entry, alongside the
existing `structural_depth`.

The `call_stack` cycle guard only records frames at definition-resolution and
tree-reentry sites. A deeply nested but *acyclic* expression — a long
`1 + 1 + ... + 1` chain — therefore recurses through `eval_value` without ever
pushing a frame, and overflows the OS stack before the `max_depth` budget can
trip. The counter converts that abort into a clean `RecursionDepthExceeded`,
matching the wording the semantic-recursion path already produces
(`"stack overflow in eval"`).

Nothing depends on this patch; it is a robustness fix, not an enabler. It
matters most to `crates/py-faust-rs`, where the failure mode is worst: a SIGABRT
inside a Python extension kills the host interpreter rather than raising
`ValueError`.

Covers three files — the counter and its unit tests in `crates/eval`, the
`eval_value` call site, and an integration test in `crates/compiler` driving a
4000-deep expression on a 64 MiB thread.

This is the most straightforwardly upstreamable of the patches here: a
self-contained crash fix with tests, touching no public API.

## owned-fbc-dsp-instance.patch

Proposed upstream as
[grame-cncm/faust-rs#17](https://github.com/grame-cncm/faust-rs/issues/17) (open).

Generalizes `FbcDspInstance` in `crates/codegen/src/backends/interp/instance.rs`
over `Borrow<FbcDspFactory<R>>`, splitting it into a shared base
(`FbcDspInstanceImpl`) and two aliases:

- `FbcDspInstance<'a, R>` borrows a factory owned elsewhere. This is the
  upstream form, unchanged in behaviour.
- `OwnedFbcDspInstance<R>` owns its factory and therefore carries no lifetime,
  so it can be stored, moved, and returned freely.

**Required by `crates/py-faust-rs`**, which will not compile without it. That
crate stores an `OwnedFbcDspInstance` inside a `#[pyclass]`, which must be a
self-contained movable value; a factory plus an instance borrowing it is
self-referential and cannot be stored otherwise.

The workaround-free alternatives were all worse. Upstream's only constructor is
`FbcDspInstance::new(factory: &'a mut FbcDspFactory<R>)`, and that `&mut` rules
out the usual safe self-referential crates (`self_cell`, `ouroboros`), which
hand the dependent a shared `&owner`. That leaves leaking the factory via
`Box::leak`, which leaks the bytecode of every compiled DSP, or a hand-rolled
unsafe self-reference, which would break the binding's no-hand-written-`unsafe`
invariant. Upstream's own `crates/interp-ffi` hit the same wall and took the
raw-pointer route (`types.rs`, `factory: *const InterpreterDspFactory`, plus an
`unsafe impl Send`), pushing the keep-the-factory-alive obligation onto the C
caller.

So this is a genuine gap in the interpreter API rather than a shim for the
binding, and it is a reasonable upstream proposal if the fork ever wants to shed
the patch. It is additive — a generic base plus two type aliases — so it carries
low conflict risk while it stays here.
