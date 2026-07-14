# Session Handoff

Date: 2026-07-14

## Repo State

- Branch: `ondemand-vec-fad-synthesis`
- HEAD: the commit containing this handoff.
- Current task: C++ scheduling inside symbolic recursion, completed in this
  commit.
- The checked path now covers pure graphs, fixed and bounded-variable delays,
  symbolic recursion, stateful clock islands, held outputs, and expanded FAD.

## Working Tree

- This commit removes the former context-bound `SYMREC` exception and makes
  scheduled shared sample materialization observable in generated code.
- Many unrelated pre-existing untracked scratch files remain untouched.
- `tests/impulse-tests` P7 generated directories were cleaned before the APF
  validation; only the 24 scalar APF combinations were regenerated. Run the
  full matrix again before attempting `p7-report` from local artifacts.

## What Changed

- Activated selected scalar schedules in control, top, and wrapper regions.
- Registered symbolic recursion binders globally and scheduled ordinary
  recurrence-body nodes through the same Hgraph as C++.
- Separated recursion-carrier reservation from simultaneous body commit so
  delayed scheduled reads can precede the owning projection.
- Materialized shared sample-rate signals at their Hsched positions; APF now
  produces four distinct `compute` bodies under `-ss 0/1/2/3`.
- Oriented direct conflicting signal effects before strategy selection.
- Kept held clock payloads under the correct guarded region.
- Preserved fixed forward/reverse epochs for reverse-time and BRA carriers.
- Replaced literal exponential special-schedule expansion with an
  order-equivalent memoized last-position summary and retained the literal
  algorithm as a test oracle.
- Added `p7-full`/`p7-report` orchestration and an xtask report checker that
  requires all 6,624 non-empty responses and binds each combination by exact
  byte count and SHA-256.
- Completed all 72 backend/mode/strategy combinations over the 92-DSP corpus;
  the versioned report records 6,624 accepted differential comparisons.
- Canonicalized vector copy-in, copy-out, and table-clear loop bodies as
  one-statement FIR blocks; strengthened the independent checker and added a
  C/C++ `noiseabs` regression for both loop variants.
- Made WASM and AssemblyScript compilation timeouts configurable through
  validated `FAUST_RS_TIMEOUT_SECONDS`, with a 600-second full-matrix default.
- Added scalar `-ss 0..3` and vector `-lv 0/1 x -ss 0..3` targets for all six
  executable backends, with aggregate `p7-matrix` and `p7-smoke` targets.
- Added validated scheduling-option handling to the interpreter, Cranelift,
  WASM, and AssemblyScript runners.
- Fixed the impulse `build` target so Cargo's Cranelift `--bin` filter cannot
  leave a stale `impulse-runner` release binary.
- Extended known-failure inheritance across vector and scheduling suffixes.
- Added canonical delay storage and loop phase DTOs derived only from verified
  decoration and vector-plan artifacts.
- Matched C++ copy geometry `R=4*ceil(D/4)`, temporary length `R+V`, and ring
  geometry `N=next_power_of_two(D+V)` with index/save transitions.
- Grouped recursive projection aliases by symbolic index and emitted one
  simultaneous `RecursionStep` per group and sample in its serial loop.
- Added independent coverage/geometry/phase checking and fail-closed clock/AD
  resource diagnostics.
- Refined P5.3 managed effects into loop-pre/sample/loop-post events, with phase
  barriers and recursion-step chains checked under all four `-ss` strategies.
- Added bounded copy/ring simulation models and exhaustive `DelaySim` checks
  against newest-first abstract history.
- Added one checked serial island per clock domain with exact wrapper, parent,
  guard, member-signal, nested-loop, and clock-state facts.
- Partitioned P5 transports into top-rate outer-chunk, domain-rate
  island-scalar, and persistent held-output routes so guarded code cannot reuse
  an outer chunk index or lose `PermVar` lifetime.
- Accepted propagated FAD as an ordinary signal graph and added explicit
  scalar `Forward < Reverse` fallbacks for reverse-time/BRA carriers.
- Added recursively checked FIR tuple constructors with deterministic types.
- Kept tuple transport fail-closed: C++-compatible scalar projections, not a
  new tuple array ABI, cross loop boundaries.
- Closed P5 routed-FIR verification for a real two-projection recursion under
  all four `-ss` strategies.
- Materialized P6.1 copy/ring delay words and simultaneous recursive
  projection captures into explicit loop `pre/exec/post` FIR.
- Rematerialized all P6.2 transport modes: outer chunk arrays, island-local
  stack scalars below guards, and cleared persistent held outputs.
- Nested single-fire P4 loop bodies under OD/US/DS guards and exact parent
  domains, with a checker for action, island, and lifetime coverage.
- Added output stores and both C++-shaped `-lv` chunk drivers around accepted
  P6.3b bodies.
- Assembled and independently checked all lifecycle sections before returning
  a production vector module.
- Activated the full checked P4/P5/P6 chain for the supported P6.5 subset under
  every `-ss`, while retaining named fallbacks outside that boundary.
- Propagated `VectorPipelineStatus` through transform and compiler FIR outputs.
- Lowered fixed positive delays through the exact accepted P6.1 copy/ring
  storage equations rather than reconstructing storage from signals.
- Resolved symbolic recursion through reachable binders and flattened all
  next-value declarations into the enclosing sample scope before simultaneous
  state writes.
- Activated stateless OD/US/DS islands and emitted held output stores after the
  guard on every outer sample.
- Activated expanded FAD as an ordinary typed signal graph and retained exact
  `FRS-VEC-RAD-SCALAR` fallback reporting for reverse AD.
- Added scalar/vector bit-exact interpreter coverage for clocks and FAD and
  checked production selection for recursion, FAD, and clocks under all four
  scheduling strategies.
- Added `ClockRing` storage with one persistent cursor per domain and one cursor
  advance per guarded fire.
- Lowered bounded variable amounts from the accepted type interval and ordered
  delayed inter-loop producers before readers without an immediate transport.
- Projected each clock island through the accepted `-ss` schedule, fixing a
  one-fire recursion lag caused by canonical-id assembly order.
- Added parity coverage for variable delays and clock-local delay/recursion for
  both `-lv` variants and all four scheduling strategies.

## Decisions And Constraints

- Scalar `-ss` controls only legal same-tick reordering. Recursion binders,
  clock guards, lifecycle routing, state maintenance, output stores, and AD
  epochs remain semantic barriers outside the scheduling policy.
- Compact `-ss 2` is exactly equivalent while the C++ raw sequence length fits
  `u128`; beyond that non-materializable parity domain it uses verified DFS to
  preserve scheduler totality.
- P7.2 is complete translation-validation evidence for the executable impulse
  contracts, not the P7 exit gate and not a proof of `V-Simulation`.
- P7.3 must add FIR/WAST/Julia artifact gates and supported single-precision
  coverage before final-state/effect, coverage-baseline, and cost-model work.
- Only exact delay/recursion resources covered by the P6.1 artifact replace
  conservative P5.3 effects; all other state remains rejected or serial.
- Exhaustive bounded simulation is executable evidence, not an unbounded proof.
- `VectorRouteSession` accepts tuple-valued definitions but intentionally
  rejects tuple transports; P6.3b lowers scalar projections into state words.
- No C/C++ ABI changes; the Rust FIR output API gains an adapted diagnostic
  status describing certified selection versus fallback.
- `VerifiedPureVectorProgram` and `PureLowering` keep their historical public
  names for Rust source/diagnostic compatibility; their accepted scope is now
  broader than pure graphs. This is an adapted additive API, not a C/C++ ABI
  change.
- UI programs and RAD remain explicit fallbacks. P6.6 does not certify those
  transitional modules.
- Full R4/R5 still requires canonical serialization/hash binding, Lean
  acceptance, and stable C++ corpus retention.

## Validation

- `make -C tests/impulse-tests -j8 all-ss dspfiles=dsp/APF.dsp` passes all 24
  scalar APF comparisons across six executable backends and four strategies.
- `cargo test -p transform signal_fir --lib` passes all 220 selected tests.
- `cargo test -p compiler --test p3_shadow_mode` and
  `cargo test -p compiler --test vector_mode` pass.
- `make -C tests/impulse-tests all-ss` passes all 2,208 scalar comparisons:
  92 DSPs x 6 executable backends x 4 scheduling strategies.
- Authoritative-order, direct-effect, compact-special-schedule, ondemand,
  clocked differential, reverse-AD, and scalar bit-parity tests pass.
- `make -C tests/impulse-tests -j16 p7-matrix` passes all 6,624 differential
  comparisons across the complete 72-combination matrix.
- `make -C tests/impulse-tests p7-report` verifies 72 x 92 non-empty responses
  and emits the versioned 10,958,514,432-byte SHA-256 inventory.
- `cargo fmt --all -- --check` passes.
- `cargo clippy --workspace --all-targets -- -D warnings` passes.
- `cargo test --workspace --all-targets` passes.
- `cargo run -p xtask -- golden-check` passes all 190 snapshots unchanged.

## Next Steps

1. Keep P7.3 (FIR/WAST/Julia artifact and single-precision gates) deferred as
   requested unless it becomes necessary for a release gate.
2. Add optimized/unoptimized final-state/effect parity and the versioned
   vectorization-coverage baseline before cost-model work or fallback removal.

## Useful Commands

- `cargo test -p transform signal_fir::vector_events --lib`
- `cargo test -p transform signal_fir::vector_state --lib`
- `cargo test -p transform signal_fir::vector_clock_ad --lib`
- `cargo test -p transform signal_fir::vector_route --lib`
- `cargo test -p transform signal_fir::vector_assemble --lib`
- `cargo test -p transform signal_fir::vector_module --lib`
- `cargo test -p compiler --test vector_mode`
- `cargo test -p compiler --test vector_clock_ad`
- `make -C tests/impulse-tests -j8 p7-smoke`
- `make -C tests/impulse-tests -j16 p7-full`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo run -p xtask -- golden-check`
