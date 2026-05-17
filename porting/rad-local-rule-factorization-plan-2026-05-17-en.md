# RAD Local Rule Factorization Plan

Date: 2026-05-17

## Goal

Reduce semantic drift between the two reverse-mode AD implementations that
share the same local math rules:

- symbolic feed-forward RAD in `crates/propagate/src/reverse_ad.rs`;
- block-reverse-AD FIR lowering in `crates/transform/src/signal_fir/module.rs`.

The first slice deliberately factors only the rule identity and policy table.
It does not merge the two passes, because they operate at different IR levels.

## Current Split

`propagate::reverse_ad` works in Signal IR. It emits ordinary `SigId`
expressions for feed-forward `rad(...)`, before FIR lowering, backend real-type
selection, tape placement, and sample-loop scheduling.

`transform::signal_fir::module` lowers `SigBlockReverseAD` carriers into FIR. It
must choose tape loads versus recomputation, emit FIR math calls, register
helper prototypes, maintain adjoint carry state for delays, and decide whether
the sweep is placed in a forward or reverse loop.

Those responsibilities should remain separated.

## Factorization Boundary

Add a small shared rule module in `crates/signals`, because both `propagate` and
`transform` already depend on `signals` and `signals` does not depend on either
higher-level crate.

The shared module should expose backend-neutral rule identifiers:

- unary RAD math rules (`sin`, `cos`, `tan`, `exp`, `log`, `log10`, `sqrt`,
  `abs`, `acos`, `asin`, `atan`);
- binary RAD math rules (`pow`, `atan2`, `min`, `max`, `fmod`, `remainder`);
- binary operator rule classification for arithmetic versus discrete
  zero-gradient operators.

It should not expose `SigBuilder`, `FirBuilder`, `FirMathOp`, tape storage, loop
state, or backend type choices.

## Implementation Steps

1. Add `signals::ad_rules` with compact enums and mapping helpers.
2. Use the shared rule helpers in `propagate::reverse_ad` for dispatch of
   unary math, binary math, and `BinOp` zero-gradient classification.
3. Use the same helpers in `transform::signal_fir::module` for the
   `BlockReverseAD` adjoint dispatch.
4. Keep formulas emitted locally in each crate for this slice, but align the
   fragile `pow` formula so symbolic RAD and BRA/FIR use the same stable
   `y_bar * y * pow(x, y - 1)` derivative for the base.
5. Add focused tests or strengthen existing tests so the shared mapping is
   exercised directly.

## Non-Goals

- Do not move `BlockReverseAD` tape allocation or sweep scheduling out of
  `transform`.
- Do not make `propagate` depend on `fir`.
- Do not make the shared layer allocate signal or FIR nodes.
- Do not change the public `rad(expr, seeds)` output layout.

## Pass Criteria

- `cargo fmt --all`
- `cargo test -p signals ad_rules`
- `cargo test -p propagate reverse_ad --test core_api`
- `cargo test -p transform`

If the full workspace quality gate is run, it remains:

- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
