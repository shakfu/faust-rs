---
title: "Note: the ondemand / upsampling / downsampling primitives in faust-rs"
author: "Claude Opus 4.8"
date: "2026-07-21"
---

# Clock domains in `faust-rs`: `ondemand`, `upsampling`, `downsampling`

French version: [ondemand-note-fr.md](ondemand-note-fr.md) (same content; keep
both in sync on amendment).

This note presents the three clock-domain primitives from the Faust
programmer's point of view. It assumes no knowledge of the compiler internals.

In ordinary Faust, every signal advances at one rate: one sample per tick, for
the whole program. The clock-domain primitives break that assumption. They let a
sub-expression run **at its own rate** — less often, more often, or only when
something happens — while the rest of the program keeps running at the audio
rate.

```faust
ondemand(C)
upsampling(C)
downsampling(C)
```

## 1. The clock is an extra input

All three take one expression and return a *wrapped* expression with **one more
input than the body**:

> if `C : u → v` then `ondemand(C) : u+1 → v`

That extra **first** input is the clock `H`. It is an ordinary Faust signal —
you can compute it, gate it, derive it from a UI element:

```faust
// The body runs only while the button is held.
process = (button("gate"), _) : ondemand(*(2));
```

The same arity rule applies to `upsampling` and `downsampling`.

## 2. What each primitive does with the clock

| Primitive | Clock reading | Effect per outer tick |
|---|---|---|
| `ondemand`, clock range ⊆ [0,1] | run if `H ≠ 0` | body executes 0 or 1 time |
| `ondemand`, clock range wider than [0,1] | run `H` times | body executes `H` times |
| `upsampling` | `H` = factor | body executes `H` times |
| `downsampling` | `H` = period | body executes on every `H`-th tick |

The two `ondemand` rows are not two primitives, and the choice is not something
you declare: the compiler infers it from the clock signal's **value range**.

- If the inferred range is contained in **[0,1]**, the clock is read as a
  condition and the body runs at most once, whenever `H ≠ 0`.
- If the range is wider, the clock is read as a **count** and the body runs `H`
  times.

Note what the first case does *not* say: it is the range that must lie in
[0,1], not the individual values. A clock that can take the value `0.5` still
has range ⊆ [0,1], so it is a condition — and since `0.5 ≠ 0`, the body runs
**once**, not half a time. There is no fractional execution; if you want a
count, give the clock a range that exceeds 1.

Constant clocks are simplified away early:

- `H == 0` — the body never runs and the outputs are replaced by `0`;
- a constant non-zero clock collapses to the corresponding fixed structure.

So `ondemand` with a literal clock costs nothing at run time; the primitives are
only "dynamic" when the clock is.

## 2 bis. `ma.SR` inside a domain

`upsampling` and `downsampling` change the *rate* of the body, so they also
change what the body means by "the sample rate". `ma.SR` is adapted
automatically:

| Context | Value of `ma.SR` inside the body |
|---|---|
| `upsampling(C)` with clock `H` | `SR * H` |
| `downsampling(C)` with clock `H` | `SR / H` |
| nested US/DS | the factors compose |
| `ondemand(C)` | **unchanged** — still the outer `SR` |

Verified on emitted C++: under `upsampling` with clock 2 the constant becomes
`fSampleRate * 2`; under `downsampling` with clock 4 it becomes
`fSampleRate * 0.25`; and `upsampling(2)` wrapping `downsampling(4)` yields
`fSampleRate * 0.5`, i.e. the whole stack of factors is unrolled.

This is what you want most of the time: a filter whose coefficients are computed
from `ma.SR` inside an `upsampling` body is *automatically* tuned for the
oversampled rate, with nothing to pass in by hand.

**The `ondemand` row is the trap.** `ondemand` does not adapt `ma.SR`, and it
cannot: its firing rate depends on the clock signal at run time, so there is no
constant ratio to fold into `SR`. A body that computes coefficients from
`ma.SR` inside an `ondemand` will therefore be tuned for the *outer* rate, not
for its own firing rate. If your body needs its effective rate, compute it
outside and pass it in as an ordinary input.

## 3. The subtlety that matters: time inside a domain is local

This is the part that surprises people, and it is the whole point of the
construct.

Inside a clock domain, **time advances at the domain's rate, not the audio
rate**. A one-sample delay inside an `ondemand` body is one *firing* late, not
one audio sample late. The same holds for every stateful construct: delay lines,
recursion (`~`), tables, and accumulators all count in *fire time*.

```faust
// `prev` is the previous value produced *while the gate was open*,
// not the value one audio sample ago.
process = (button("gate"), _) : ondemand(+ ~ _);
```

If you want audio-rate history, keep the state outside the domain and pass it in.
If you want per-event history — a counter of events, the previous frame, a value
held between firings — put it inside. Choosing the wrong side is the most common
source of confusion, and it is silent: both versions compile.

## 4. Typical use cases

**Control-rate computation.** Anything that does not need to be recomputed 48000
times a second: envelope followers feeding a display, parameter smoothing logic,
expensive analysis. Wrap it in `downsampling` and pick a period.

```faust
process = (256, _) : downsampling(expensive_analysis);
```

**Event-triggered work.** A body that should run only when something happens —
a note-on, a threshold crossing, a button. `ondemand` with a 0/1 clock is
exactly this, and unlike a `select2` it does not *compute both branches*: the
body genuinely does not execute.

**Oversampling a nonlinearity.** Run a saturator or an oscillator at a multiple
of the audio rate to push aliasing up, using `upsampling`. Note that the
primitive controls *execution rate* — the anti-aliasing filters around it are
still yours to write.

**Frame-rate / spectral processing.** Combined with the `interleave.lib`
primitive `il.interleave(N, FX)`, `ondemand` lets a frame operator run once per
`N` samples, which makes per-frame FFT and STFT-style processing expressible in
plain Faust. `il.interleave(N, id)` is exactly `@(N-1)`, so the round-trip
latency of the construct is `N-1` samples. See
[ondemand-fft-spectral-comparison-en.md](ondemand-fft-spectral-comparison-en.md).

## 5. Links with FAD and RAD

Clock domains are the practical vehicle for **in-graph learning** — the
applicative motivation behind much of this machinery. See
[fad-note-en.md](fad-note-en.md) and [rad-usage-en.md](rad-usage-en.md) for the
differentiation primitives themselves.

**Learning at control rate.** A gradient step does not need to run per sample.
Wrapping an optimizer in a domain decouples adaptation rate from audio rate:

```faust
// One optimizer step every 64 samples instead of 48000 times a second.
process = (64, _) : downsampling(ad.fit_adam(...));
```

**Event-triggered adaptation.** `ondemand` with a 0/1 clock gives you
"adapt only while this gate is open", which is how you freeze a learned
parameter outside a training phase without adding branches to the audio path.

**Decimated gradients.** Compute a loss at audio rate but update at a lower
rate, keeping the expensive part of the backward pass in a slower domain.

**Frame-rate DDSP.** With `interleave`, a differentiable spectral loss becomes
expressible: FFT the frame, compare against a target spectrum, differentiate the
result.

**One rule to remember:** differentiation and clock domains compose *inside* a
domain, but a derivative does not flow **across** a domain boundary. `fad`
inside an `ondemand` body is supported, and its tangents are validated
numerically against finite differences. Differentiating a signal that crosses
into or out of a domain is a different matter: the compiler has a dedicated
diagnostic for automatic differentiation reaching a clock-domain boundary it
cannot cross (`FRS-PROP-0004`). If you are building a learning loop, keep the
seed, the loss, and the update in the **same** domain.

## 6. Practical notes and current limits

- The clock is a signal, so it can itself be computed inside another domain.
  Nesting works, but reason in fire time at each level — the rates multiply.
- `ondemand` with a clock ranging beyond 1 executes the body `H` times per tick. A clock
  derived from an unbounded computation can therefore make one audio tick
  arbitrarily expensive; bound it if it comes from user input.
- Clock domains compose with vector mode (`-vec`); stateful shapes inside a
  domain run in fire time in every backend.
- The C++ Faust compiler is the reference for the clocked machinery itself, but
  **there is no C++ reference for the combination of FAD/RAD with clock
  domains** — faust-rs defines those semantics, and the oracle is numerical
  agreement with finite differences.

## See also

- [fad-note-en.md](fad-note-en.md) — forward-mode differentiation
- [rad-usage-en.md](rad-usage-en.md) — reverse-mode differentiation
- [ondemand-fft-spectral-comparison-en.md](ondemand-fft-spectral-comparison-en.md) — spectral processing built on these primitives
- `porting/ondemand-clock-domains-analysis-port-plan-2026-06-10-en.md` — compiler-side semantics and port plan
- `porting/ondemand-fad-rad-cohabitation-2026-06-10-en.md` — FAD/RAD × domains, in detail
