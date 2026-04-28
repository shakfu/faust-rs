# Reverse-Mode AD in `faust-rs`

Synthesis note describing the reverse-mode automatic differentiation
(RAD) pass implemented in `crates/propagate/src/reverse_ad.rs`, with
emphasis on the algorithm, the rule table, the temporal boundary, and
the relationship to the forward-mode pass.

## 1. Surface

`rad(expr, seeds)` is a two-child node mirroring `fad(expr, seeds)`.

```faust
x = hslider("x", 1, 0, 10, 0.01);
y = hslider("y", 2, 0, 10, 0.01);
loss = sin(x * y);
process = rad(loss, (x, y));
```

Output bundle layout (per-primal section):

```text
rad(expr, (s_0, …, s_{N-1})) =
    [ expr_0, expr_1, …, expr_{M-1},
      ∂ sum(expr_i) / ∂ s_0,
      ∂ sum(expr_i) / ∂ s_1,
      …,
      ∂ sum(expr_i) / ∂ s_{N-1} ]
```

The implicit cotangent on every primal output is `1.0` (sum cotangent).
Custom output cotangents are reserved for a future `vjp(expr, cotangent,
seeds)` primitive.

Arity contract:

- `body.outputs ≥ 1` (`PropagateError::RadBodyArity`),
- `seeds.outputs ≥ 1` (`PropagateError::RadSeedArity`),
- `inputs = max(body.inputs, seeds.inputs)`,
- `outputs = body.outputs + seeds.outputs`.

## 2. Algorithm

`reverse_ad.rs` performs three explicit passes on a single
`ReverseADTransform` instance.

### 2.1 Active subgraph collection

Postorder DFS from each primal output through the differentiable
children of every visited node. Descent stops at any `SigId` that
appears in the seed list, and DAG sharing is preserved by a `visited`
set so each node is visited at most once.

```text
collect_dfs(root):
  if seen(root):                          return
  if root ∈ seeds:                        record root, return
  for child in active_children(root):
    collect_dfs(child)
  postorder.push(root)
```

`active_children(sig)` reuses the same `match_sig` decoding as the
adjoint emission code, so a node that gets descended in pass (1) is
guaranteed to have a matching adjoint rule in pass (2).

### 2.2 Adjoint accumulation

Initialize each primal's adjoint to `1.0` (the sum cotangent), then
walk the postorder in reverse and emit local transpose contributions:

```text
for primal in primals:
  adjoints[primal] += 1.0

for y in reverse(postorder):
  if y ∈ seeds:                           continue           // leaf
  y_bar = adjoints[y]
  for (child, factor) in transpose_rule(y):
    adjoints[child] += y_bar · factor
```

`add_adjoint(target, contribution)` accumulates into the map: if
`target` already has an adjoint, it builds `old + contribution`;
otherwise it stores `contribution` directly. This is the structural
counterpart of the explicit-zero-fold step that FAD does at
construction time.

### 2.3 Seed extraction

```text
result = primals
for s in seeds:
  result.push(adjoints.get(s).unwrap_or(zero))
```

Repeated seed lanes preserve the same adjoint identity, so
`rad(a*b, (a, a))` yields two gradient lanes that alias the same
computed signal. Absent seeds (those never reached from any primal
output) yield `0.0`.

## 3. Rule table

The transpose rules below mirror the forward rules in `forward_ad.rs`
for every family that admits a causal reverse pass. Notation:
`y` = visited node, `y_bar` = its accumulated adjoint, `child_bar +=
…` = `add_adjoint(child, …)`.

### 3.1 Leaves and discrete operators

| Node | Reverse behaviour |
|------|-------------------|
| `int(c)`, `real(c)` | no children |
| `sigInput(_)` | no children |
| `hslider`, `vslider`, `numentry` (not seed) | no children |
| `button`, `checkbox` | no children (discrete) |
| seed `s` | descent stops; final `adjoints[s]` is the gradient lane |
| comparisons / shifts / bitwise `BinOp` | no contribution |

### 3.2 Arithmetic `BinOp`

| `y = …` | Adjoint contributions |
|---------|-----------------------|
| `x + z` | `x_bar += y_bar`; `z_bar += y_bar` |
| `x - z` | `x_bar += y_bar`; `z_bar += -y_bar` |
| `x * z` | `x_bar += y_bar · z`; `z_bar += y_bar · x` |
| `x / z` | `x_bar += y_bar / z`; `z_bar += y_bar · (-x / z²)` |
| `x % z` | `x_bar += y_bar`; `z_bar += y_bar · -⌊x/z⌋` |

### 3.3 Unary trig/transcendental

The chain rule `child_bar += y_bar · f'(child)` is applied with the
same closed-form derivatives as FAD:

| `y = f(x)` | `f'(x)` |
|-----------|---------|
| `sin(x)` | `cos(x)` |
| `cos(x)` | `-sin(x)` |
| `tan(x)` | `1 / cos²(x)` |
| `exp(x)` | `exp(x)` |
| `log(x)` | `1 / x` |
| `log10(x)` | `1 / (x · ln 10)` |
| `sqrt(x)` | `1 / (2 · √x)` |
| `abs(x)` | `x / |x|` |
| `acos(x)` | `-1 / √(1 - x²)` |
| `asin(x)` | `1 / √(1 - x²)` |
| `atan(x)` | `1 / (1 + x²)` |

### 3.4 Binary math

| `y = f(x, z)` | Contribution to `x_bar` | Contribution to `z_bar` |
|---------------|-------------------------|-------------------------|
| `pow(x, z)` | `y_bar · pow(x,z) · z / x` | `y_bar · pow(x,z) · log(x)` |
| `atan2(y_n, x_n)` | `y_bar · -y_n / (x_n² + y_n²)` to `x_n` | `y_bar · x_n / (x_n² + y_n²)` to `y_n` |
| `min(x, z)` | `y_bar` if `x < z`, else `0` | `y_bar` if `x ≥ z`, else `0` |
| `max(x, z)` | `y_bar` if `x > z`, else `0` | `y_bar` if `x ≤ z`, else `0` |
| `fmod(x, z)` | `y_bar` | `y_bar · -⌊x/z⌋` |
| `remainder(x, z)` | `y_bar` | `y_bar · -round(x/z)` |

The branch routing for `min`/`max` is materialized via `select2`; the
condition itself receives no adjoint since it is a discrete branch
selector.

### 3.5 Control flow and casts

| Node | Reverse behaviour |
|------|-------------------|
| `select2(cond, x, z)` | `x_bar += select2(cond, y_bar, 0)`; `z_bar += select2(cond, 0, y_bar)`; `cond` receives nothing |
| `float_cast(x)` | `x_bar += float_cast(y_bar)` |
| `int_cast(x)` | no contribution (discontinuous truncation) |

### 3.6 Read-only tables

For `y = rdtbl(T, idx)` where `T` is read-only (a `Waveform` or a
write-once `WrTbl(_, _, nil, nil)`), the table contents are treated as
constant data. RAD differentiates only through the read address using
the same symmetric finite-difference slope as FAD:

```text
y       = rdtbl(T, idx)
slope   = (rdtbl(T, idx + 1) - rdtbl(T, idx - 1)) / 2
idx_bar += y_bar · slope
```

Mutable tables (`WrTbl` with non-nil write ports) refuse adjoint and
raise `RadUnsupportedNode { kind: "writable-table" }`.

### 3.7 Foreign functions

Recognised unary FFun families (precision-agnostic match on the
descriptor name):

| Name | Adjoint |
|------|---------|
| `tanh` | `y_bar · (1 - tanh²(x))` (reuses primal) |
| `sinh` | `y_bar · cosh(x)` rebuilt as `y_bar · √(1 + sinh²(x))` |
| `cosh` | `y_bar · sinh(x)` rebuilt as `y_bar · (e^x - e^{-x}) / 2` |
| `atanh` | `y_bar / (1 - x²)` |
| `asinh` | `y_bar / √(1 + x²)` |
| `acosh` | `y_bar / √(x² - 1)` |

Non-unary or unrecognised FFun calls raise
`RadUnsupportedNode { kind: "ffun" }`.

### 3.8 Pass-through wrappers

`Attach`, `Enable`, `Control`, and `Output` are transparent to
differentiation: the adjoint is forwarded to the signal-carrying
operand only. Bargraphs (`vbargraph` / `hbargraph`) are metering
sinks — they are walked so seed-reachability is correctly classified
but propagate no adjoint.

## 4. Temporal boundary

Forward-mode AD applies a causal rule for delays:

```text
∂ delay1(x) / ∂p = delay1(x')         // tangent at frame n depends on frame n-1
```

Reverse-mode AD requires the transpose, which is anti-causal:

```text
adj_x[n] += adj_y[n + 1]              // adjoint at frame n depends on a future frame
```

A correct reverse pass would therefore need either

- a finite block tape that buffers primal intermediates and a backward
  scan over that block (BPTT — out of scope for phase 1), or
- a causal approximation that is explicitly not exact reverse mode.

Phase 1 RAD takes the strict route. Any signal family whose transpose
would be non-causal (delay, prefix, recursion, projection over a
recursion) raises `PropagateError::RadUnsupportedNode` with a tailored
diagnostic. The plan reserves `rad(expr, seeds, horizon)` and
`-rad-horizon N` for a future BPTT mode; phase 1 must never silently
emit a misleading gradient.

The diagnostic kinds are:

| `kind` | Family |
|--------|--------|
| `delay-or-prefix` | `Delay1`, `Delay`, `Prefix` |
| `recursive-projection` | `Proj`, `Rec` |
| `writable-table` / `writable-table-or-waveform-direct` | mutable tables |
| `ffun` | non-unary or unrecognised foreign function |
| `soundfile` | `Soundfile`, `SoundfileLength`, `SoundfileRate`, `SoundfileBuffer` |
| `other` | catch-all (representation casts, generators, opaque) |

Each kind emits a structured `Diagnostic` with kind-specific notes
and help text, so users get an actionable explanation pointing either
at FAD or at the rewrite they need to do.

## 5. Relationship to FAD

Both passes share the same differentiable subset for feed-forward
expressions; only the temporal extension differs.

| Property | FAD | RAD (phase 1) |
|----------|-----|---------------|
| Direction | tangent ↑ (per seed) | adjoint ↓ (per primal) |
| Seed model | explicit `(s_0, …, s_{N-1})` | explicit `(s_0, …, s_{N-1})` |
| Output layout | `[p, t_0, …, t_{N-1}]` interleaved per primal | `[primals…, gradient(s_0), …]` flat |
| Cost per added seed | one extra tangent lane per primal node | one extra accumulation in the seed map |
| Cost per added primal | one extra rebuild | one extra adjoint init + one extra postorder sweep |
| Recursive primals | yes (via `DEBRUIJNREC` interleaving) | refused with `recursive-projection` |
| Delays | yes (causal forward rule) | refused with `delay-or-prefix` |
| Multi-output cotangent | implicit per-tangent | implicit all-ones (sum cotangent) |

For feed-forward expressions, RAD gradients agree with the
corresponding FAD tangent lanes lane-by-lane (scalar primal) or with
the **sum** of FAD tangent lanes across primals (multi-output, due to
the implicit all-ones cotangent). This identity is pinned by the
parity tests in `crates/compiler/tests/rad_runtime.rs`.

## 6. Test surface

- **Structural** ([crates/propagate/tests/core_api.rs](crates/propagate/tests/core_api.rs))
  — arity contract, feed-forward success, temporal/recursive rejection,
  diagnostic content checks.
- **Runtime parity** ([crates/compiler/tests/rad_runtime.rs](crates/compiler/tests/rad_runtime.rs))
  — RAD vs FAD parity, RAD vs central finite differences, repeated /
  absent seeds, multi-output sum cotangent, read-only table index,
  unary FFun (tanh).
- **Corpus** ([tests/corpus/rad_*.dsp](tests/corpus)) — eight fixtures
  pin the source-level shape of each contract: arithmetic, trig
  composition, multi-seed, multi-output, repeated/absent seeds,
  read-only table indexing, plus the three error fixtures
  (`err_rad_zero_body`, `err_rad_zero_seed`,
  `err_rad_delay_temporal_unsupported`).

## 7. Out-of-scope and future work

Per plan §3, the following remain explicitly out of scope for phase 1:

- reverse-through-time / BPTT for IIR state (delay, prefix, recursion),
- adjoints over mutable tables,
- adjoints over soundfile content,
- custom vector-output cotangent API (`vjp(...)`),
- backend-level Enzyme/LLVM integration,
- automatic discovery of differentiable UI controls.

Plan phases E and F sketch the next steps:

- **Phase E** — a scoped `RecRadMode` for local acyclic recursive
  subsets where the transpose remains causal by construction.
- **Phase F** — a finite-horizon BPTT mode (`rad(expr, seeds, horizon)`
  or `-rad-horizon N`) requiring a runtime tape and a backend backward
  sweep.

These phases are gated separately and require explicit documentation
of latency and memory footprints before merge.

## 8. Source locations

- `generate_rad_signals` and `ReverseADTransform`:
  [crates/propagate/src/reverse_ad.rs](crates/propagate/src/reverse_ad.rs)
- Propagation arm and arity contract:
  [crates/propagate/src/lib.rs](crates/propagate/src/lib.rs) — search
  for `FlatNodeKind::ReverseAD`.
- `RadBodyArity` / `RadSeedArity` / `RadUnsupportedNode` diagnostics:
  same file, `IntoDiagnostic` impl on `PropagateError`.
- Implementation plan:
  [porting/reverse-ad-rad-implementation-plan-2026-04-27-en.md](../porting/reverse-ad-rad-implementation-plan-2026-04-27-en.md).
