# Forward AD Gradient Descent — Gain Control Optimization

**Date:** 2026-04-14
**Prerequisite:** Forward AD (`fad`) implemented and working in faust-rs, including FAD inside recursions (`suppress_fad` mechanism).

---

## 1. Problem Statement

Given an audio input signal `x(n)`, find the gain `g` that minimizes the mean squared error between:
- **Estimate:** `y(n) = x(n) * g`
- **Reference:** `r(n) = x(n) * g_target`

where `g_target` is a known target gain (e.g., 0.7).

The error function is:
```
E(g) = (y(n) - r(n))^2 = x(n)^2 * (g - g_target)^2
```

The gradient:
```
dE/dg = 2 * x(n)^2 * (g - g_target)
```

FAD computes this gradient automatically by differentiating E w.r.t. the UI control `g`.

---

## 2. Architecture Constraints

### 2.1 FAD Output Model

`fad(expr)` with `N` outputs and `K` differentiable controls produces `N * (1 + K)` signal outputs:
```
[out_1, d_out_1/d_ctrl_1, ..., d_out_1/d_ctrl_K, out_2, ...]
```

These tangent signals are **extra outputs** of the DSP program. They are not routed back internally — they are available to the host or to downstream signal processing.

### 2.2 Parameter Update Challenge

In standard Faust, UI controls (`hslider`, `vslider`, `numentry`) are **set by the host**, not by the DSP. A pure Faust program cannot modify its own slider values. This means gradient descent requires either:

- **(A) Host-driven:** The host reads the gradient output, updates the slider, runs the next buffer.
- **(B) Perturbation trick:** The gain is maintained as recursive state (not a slider). A dummy `eps` slider (always 0) serves as the differentiation variable. The chain rule gives `dE/deps|_{eps=0} = dE/dg`.

---

## 3. Approach A — Host-Driven Gradient Descent (Recommended)

### 3.1 DSP Program

```faust
// fad_gradient_host.dsp
// 
// Outputs: [error, d_error/d_g]
// Host reads gradient (output 1) and updates "g" slider each buffer.

import("stdfaust.lib");

target_gain = 0.7;
g = hslider("g", 0.1, 0, 1, 0.001);

// Error: (input * g - input * target_gain)^2
error = _ <: (*(g) - *(target_gain)) : ^(2);

process = fad(error);
```

**Signal analysis:**
- `error` has 1 output, 1 differentiable control (`g`)
- `fad(error)` produces 2 outputs: `[E, dE/dg]`
- `process_arity: inputs=1, outputs=1` (box-level, FAD transparent)
- `signals.len() = 2` (after propagation, FAD expanded)

### 3.2 Host-Side Algorithm (Pseudo-code)

```python
# Initialize
dsp = compile("fad_gradient_host.dsp")
g = 0.1          # initial guess
lr = 0.001       # learning rate
buffer_size = 64

for iteration in range(1000):
    dsp.set_param("g", g)
    
    # Generate test input (e.g., sine wave)
    input_buffer = generate_sine(440, buffer_size)
    
    # Run DSP
    outputs = dsp.compute(input_buffer)
    error = outputs[0]       # E(g)
    gradient = outputs[1]    # dE/dg
    
    # Gradient descent update
    avg_gradient = mean(gradient)
    g = g - lr * avg_gradient
    g = clamp(g, 0.0, 1.0)
    
    if abs(g - 0.7) < 1e-6:
        print(f"Converged at iteration {iteration}, g = {g}")
        break
```

### 3.3 Convergence Analysis

For a constant-amplitude input `x`:
```
dE/dg = 2 * x^2 * (g - g_target)
```

With learning rate `lr`:
```
g(n+1) = g(n) - lr * 2 * x^2 * (g(n) - g_target)
       = g(n) * (1 - 2*lr*x^2) + 2*lr*x^2*g_target
```

Convergence requires `|1 - 2*lr*x^2| < 1`, i.e., `lr < 1/x^2`.

For unit-amplitude input (`x=1`), any `lr < 1` converges. For `lr = 0.01`:
- After ~100 iterations: `g ~ 0.69`
- After ~500 iterations: `g ~ 0.6999`

---

## 4. Approach B — Self-Contained Faust (Perturbation Trick)

### 4.1 Concept

Maintain `g` as recursive state. Introduce a perturbation `eps` (always 0) as the FAD differentiation variable:

```
g_perturbed = g + eps
E(eps) = (x * g_perturbed - x * g_target)^2
dE/deps|_{eps=0} = dE/dg  (by chain rule, since d(g+eps)/deps = 1)
```

### 4.2 DSP Program

```faust
// fad_gradient_selfcontained.dsp
//
// Self-contained gradient descent using the perturbation trick.
// eps must remain at 0. The gradient w.r.t. eps equals the gradient w.r.t. g.

import("stdfaust.lib");

target = 0.7;
lr = hslider("lr[autodiff:false]", 0.001, 0, 0.1, 0.0001);
eps = hslider("eps", 0, -1, 1, 0.001);

// Recursive gain optimization
// State: g (current gain estimate)
// Each sample: g_new = g_old - lr * gradient
//
// The trick: fad() differentiates w.r.t. eps.
// Since g_perturbed = g + eps, dE/deps = dE/dg.

gain_opt(g_prev, x) = g_new, out
with {
    g_pert = g_prev + eps;
    out = x * g_pert;
    ref = x * target;
    err = (out - ref) ^ 2;
    grad = 2 * x * (out - ref);  // analytical for now
    g_new = g_prev - lr * grad;
};

// Recursive: g feeds back, audio passes through
process = 0.5, _ : gain_opt ~ (!,_) : (!, _);
```

**Challenge:** Routing the FAD tangent output (gradient) back into the recursion is complex. The `fad()` wrapper would need to encompass the error computation, and its tangent output must drive the gain update — but this creates a circular dependency between the FAD expansion and the recursive state.

### 4.3 Feasibility Assessment

The perturbation approach is **theoretically sound** but **practically complex** in Faust's signal routing model:

1. `fad()` must wrap only the error computation, not the gain update
2. The tangent output must be extracted and routed to the gain update
3. With `fad()` inside `~` (which works via `suppress_fad`), the tangent becomes an extra Rec output
4. But the Rec itself needs to use the tangent for its own state update — this is a circular dependency

**Verdict:** Approach A (host-driven) is the practical choice. Approach B is included for completeness and may be feasible with careful signal routing, but is not the primary recommendation.

---

## 5. Extensions

### 5.1 Multiple Parameters

```faust
// Host-driven: optimize both gain and bias
g = hslider("g", 0.1, 0, 1, 0.001);
b = hslider("b", 0, -1, 1, 0.001);
target_gain = 0.7;
target_bias = 0.1;

error = _ <: (*(g) + b - (*(target_gain) + target_bias)) : ^(2);
process = fad(error);
// Outputs: [E, dE/dg, dE/db] — 3 signals
// Host updates both g and b each iteration
```

### 5.2 Nonlinear Processing

```faust
// Optimize gain before a nonlinear function
g = hslider("g", 1, 0, 10, 0.01);
target = 3.14;

error = _ <: (*(g) : sin - *(target) : sin) : ^(2);
process = fad(error);
// dE/dg involves cos(g*x) * x via chain rule — FAD handles automatically
```

### 5.3 Temporal (with Delay)

```faust
// Optimize a feedback gain in a recursive filter
g = hslider("g", 0.1, 0, 0.99, 0.001);
target_g = 0.5;

filt = + ~ *(g);
filt_ref = + ~ *(target_g);
error = _ <: (filt - filt_ref) : ^(2);
process = fad(error);
// Gradient accounts for the recursive structure
```

---

## 6. Test Plan

### 6.1 Corpus Files

| File | Description | Inputs | Outputs (box) | Signals | Controls |
|------|-------------|--------|---------------|---------|----------|
| `fad_gradient_host.dsp` | Host-driven gradient | 1 | 1 | 2 | 1 (g) |

### 6.2 Signal Pipeline Test

```rust
#[test]
fn corpus_fad_gradient_host_compiles_through_full_signal_pipeline() {
    let (process_arity, signals, ui) = compile_dsp("fad_gradient_host.dsp");
    assert_eq!(process_arity.inputs, 1);
    assert_eq!(process_arity.outputs, 1);
    assert_eq!(signals.len(), 2);  // error + gradient
    assert_eq!(ui.controls.len(), 1);  // just "g"
}
```

### 6.3 Verification

```
cargo test -p compiler --test signal_pipeline corpus_fad_gradient
cargo test -p compiler --test signal_pipeline  # all tests pass
```
