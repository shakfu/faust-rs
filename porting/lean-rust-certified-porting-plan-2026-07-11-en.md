# Lean/Rust Certified Porting Plan and Canonical Certificate Boundary

Status: normative porting plan; implementation not started.

Date: 2026-07-11.

Related documents:

- [`vector-mode-signal-level-analysis-cpp-port-plan-2026-07-10-en.md`](./vector-mode-signal-level-analysis-cpp-port-plan-2026-07-10-en.md)
- [`vector-mode-scheduling-formal-spec.lean`](./vector-mode-scheduling-formal-spec.lean)
- [`schemas/vector-verification-certificate-v1.schema.json`](./schemas/vector-verification-certificate-v1.schema.json)

## 1. Objective

Connect the progressive Rust port to the Lean specification so that each
critical compiler phase produces a finite result that can be checked
independently before the next phase consumes it.

The target is a high-assurance, fail-closed compiler pipeline:

```text
untrusted/complex producer
    -> result + canonical witness
    -> small independent verifier
    -> certified result or typed internal error
```

"Untrusted" here is an architectural term. It does not imply low-quality Rust;
it means correctness does not depend solely on the producer implementation.

Absolute security is not a realistic claim. The remaining trusted computing
base includes the specification, Lean kernel and runtime, serialization layer,
Rust toolchain, backend/runtime code, foreign functions, operating system, and
hardware. The achievable goal is to make structural compiler errors detectable
at narrow boundaries and to reduce semantic trust through refinement proofs and
differential execution.

## 2. Assurance Model

Use four explicit assurance levels.

| Level | Meaning | Required evidence |
|---|---|---|
| L1 - tested | Conventional implementation confidence | Rust unit, property, corpus, and C++ differential tests |
| L2 - runtime certified | A phase result is rejected unless a Rust verifier accepts it | Canonical artifact plus independent Rust checker |
| L3 - Lean checked | The Rust artifact is also accepted by the executable Lean reference checker | Cross-language artifact parity in CI |
| L4 - refinement proved | A selected Rust implementation is connected deductively to the Lean definition | Machine-checked refinement theorem for the supported Rust subset |

Required rollout policy:

- every new scheduler and vector-planning phase reaches L2 before activation;
- every versioned acceptance corpus reaches L3 before the corresponding phase
  gate is considered complete;
- L4 starts with small pure verifiers and analysis functions, not backends;
- a lower level must never be described as a proof of a higher level.

## 3. Trust Boundaries

### 3.1 Producer/checker separation

The scheduling algorithm, vector-plan builder, and FIR router may use complex
data structures and optimizations. Their checkers must instead use direct,
obviously terminating traversals over canonical finite snapshots.

The checker must not call the producer algorithm. In particular:

- `verify_schedule` checks permutation coverage and every edge order; it does
  not rerun DFS/BFS/Special/Reverse-BFS;
- `verify_vector_plan` reconstructs induced epoch graphs and checks ownership,
  endpoints, transports, barriers, and vector-safety evidence;
- `verify_routed_fir` reconstructs region/type/store/load/effect facts from the
  routed FIR snapshot rather than trusting planner booleans.

### 3.2 Lean as normative semantics

[`vector-mode-scheduling-formal-spec.lean`](./vector-mode-scheduling-formal-spec.lean)
defines the normative mathematical meaning of the finite checks. Executable
Lean functions are the reference oracle. Proposition-valued interfaces state
the remaining proof obligations.

The canonical JSON schema is not the semantics. It validates syntax, shape,
closed enums, integer ranges, and required fields. Semantic verification still
has to check uniqueness, hashes, graph order, ownership, completeness, effects,
and simulation premises.

### 3.3 Fail-closed behavior

A failed check must produce a typed compiler-internal diagnostic and stop before
the next trust boundary. No backend may receive FIR derived from a rejected
vector plan. Release builds may disable expensive Lean subprocess checks, but
must retain the L2 Rust checkers for all certificate-gated phases.

## 4. Canonical Artifact Schema

The normative machine-readable schema is:

```text
porting/schemas/vector-verification-certificate-v1.schema.json
```

It defines four top-level artifact kinds:

1. `schedule_certificate`;
2. `vector_plan_certificate`;
3. `routed_fir_certificate`;
4. `verification_result`.

The first three contain claims to be checked. They are not valid merely because
they satisfy JSON Schema. `verification_result` records a named verifier's
acceptance or rejection of the relevant canonical subject hash: `graph_hash`,
`plan_hash`, or `routed_fir_hash`.

### 4.1 Versioning

Every artifact contains:

```json
{
  "schema_version": 1,
  "artifact_kind": "schedule_certificate",
  "producer": {
    "name": "faust-rs",
    "version": "0.1.0",
    "git_commit": "0123456789abcdef0123456789abcdef01234567"
  },
  "program": {
    "case_id": "tests/corpus/example.dsp",
    "source_sha256": "...64 lowercase hexadecimal digits..."
  }
}
```

Rules:

- `schema_version` changes only for an incompatible representation or semantic
  interpretation;
- additive fields require a new schema version because all v1 objects reject
  unknown properties;
- readers reject unknown versions and artifact kinds;
- converters between versions are explicit tools, never implicit parser
  fallbacks;
- repository-relative `/`-separated `case_id` values are required for portable
  snapshots; absolute paths are forbidden.

### 4.2 Canonical JSON encoding

JSON Schema does not define byte identity. The following rules are therefore
normative for hashing and committed snapshots:

1. encode UTF-8 without a byte-order mark;
2. use RFC 8785 JSON Canonicalization Scheme object-key ordering and scalar
   rendering;
3. emit no insignificant whitespace for hash input;
4. terminate committed pretty-printed files with one LF;
5. use non-negative integers no greater than `2^53 - 1`;
6. do not encode integers as floating-point values or strings;
7. reject duplicate object keys before canonicalization;
8. reject unknown fields before hashing;
9. use lowercase hexadecimal SHA-256 strings;
10. normalize no user string implicitly: strings are hashed as supplied.

The Rust implementation should use a dedicated canonical serializer rather than
assuming `serde_json::to_string` is canonical. The Lean importer must parse the
same JSON value and independently recompute every declared hash.

### 4.3 Array-order rules

Arrays representing mathematical sets have one required order:

| Array | Canonical order |
|---|---|
| graph `nodes` | ascending node id |
| graph `edges` | `(consumer, dependency, kind)` ascending |
| plan `signals` | ascending signal id |
| plan `loops` | ascending loop id |
| plan `epochs` | `(rank, id)` ascending |
| epoch `loops` | ascending loop id |
| plan `transports` | ascending transport id |
| plan data/effect edges | `(consumer, dependency, kind)` ascending |
| effects on one signal | semantic source order, not sorted |
| loop roots | deterministic materialization/source order |
| schedule `ordered_nodes` | execution order, never sorted |
| routed FIR statements | emitted execution order, never sorted |

The verifier rejects noncanonical set ordering even when the represented set is
equivalent. This makes byte snapshots stable and removes accidental `HashMap`
iteration from the compatibility surface.

### 4.4 Edge convention

Every edge is encoded as:

```json
{
  "consumer": 7,
  "dependency": 3,
  "kind": "data"
}
```

It means `7 -> 3`: node 7 consumes node 3, so node 3 must execute first. No
field named `from` or `to` is permitted because those names have repeatedly
caused direction ambiguity.

The schedule condition is:

```text
position(edge.dependency) < position(edge.consumer)
```

### 4.5 Hash projections

Hashes bind a certificate to the exact object it checks.

```text
graph_hash = SHA256(canonical_json(graph))

plan_hash = SHA256(canonical_json(plan))

routed_fir_hash = SHA256(canonical_json({
    "plan_hash": plan_hash,
    "routed_fir": routed_fir
}))
```

The hash field itself, producer metadata, program metadata, schedules, and
verification results are excluded from these projections. `VectorPlan` has no
scheduling strategy or selected loop order, so changing `-ss` must leave
`plan_hash` byte-identical.

### 4.6 Schedule scope and strategy normalization

Each `ScheduleCertificate` identifies exactly one scheduling scope:

```text
scalar_control
scalar_region(region_id)
vector_epoch(epoch_id)
```

Vector epoch schedules are separate artifacts. They are never embedded in
`VectorPlanCertificate`, because changing `-ss` must leave both the plan hash and
the complete canonical plan-certificate bytes unchanged.

The schema stores the semantic enum, not the raw CLI integer:

```text
0   -> depth_first
1   -> breadth_first
2   -> special
3+  -> reverse_breadth_first
```

The original CLI token may appear in compiler diagnostics but not in canonical
cache identity or certificates. Thus `-ss 3` and `-ss 42` have the same
canonical strategy.

### 4.7 Type and effect normalization

Signal types use closed tagged objects:

```json
{ "kind": "int" }
{ "kind": "real" }
{ "kind": "tuple", "components": [{ "kind": "real" }] }
```

This v1 vocabulary intentionally matches the current Lean abstraction. FIR
widths and `FaustFloat` specialization belong in `RoutedFirCertificate`, where
the `fir_type` enum distinguishes `int32`, `int64`, `float32`, `float64`, and
`faust_float`.

Effects remain in semantic source order and use tagged resource objects. Unknown
or impure foreign calls are explicit; absence of an effect entry means a purity
claim that the verifier must justify from signal analysis.

## 5. Rust/Lean Mapping Contract

| Canonical definition | Lean definition | Planned Rust owner |
|---|---|---|
| `GraphSnapshot` | `DependencyGraph` | `crates/transform` generic scheduler module |
| `ScheduleCertificate` | `ScheduleCertificate` | `crates/transform` |
| `Strategy` | `SchedulingStrategy` | `crates/transform`, threaded by `crates/compiler` |
| `SignalRecord` | `Decoration` plus placement facts | `signal_fir::vector_analysis` |
| `VectorPlan` | `VectorPlan` | `signal_fir::vector_analysis` |
| `VectorPlanCertificate` | `VectorPlanCertificate` | `signal_fir::vector_verify` |
| `Transport` | `Transport` | signal FIR vector routing |
| `RoutedFirCertificate` | `LoweringWitness` refinements | signal FIR router/verifier |
| execution equality | `VSimulation` | interpreter and backend differential gates |

The Rust types may be idiomatic and need not mirror Lean memory layouts. The
canonical DTO layer is the compatibility boundary. Conversion into a DTO must
be pure, deterministic, and tested independently from JSON rendering.

## 6. Progressive Integration Plan

### R0 - Freeze schema and examples

Deliverables:

- commit the v1 JSON Schema and this plan;
- add one valid and one invalid artifact example for each certificate kind and
  each schedule scope;
- validate examples structurally with a JSON Schema validator;
- add canonicalization and hash test vectors shared by Rust and Lean;
- pin the Lean version used in CI or document the elan toolchain requirement.

Pass criteria:

- Rust and Lean parse the same valid examples;
- both reject unknown fields, unknown enums, duplicate ids, and malformed hashes;
- canonical byte and hash test vectors are identical on Linux, macOS, and
  Windows.

### R1 - Schedule certificate at L2

Implement the generic Rust `GraphSnapshot`, `ScheduleCertificate`, and
`verify_schedule` before activating generalized `-ss`.

Required checks:

- canonical node and edge ordering;
- unique nodes and edges;
- every edge endpoint belongs to the node set;
- `node_count` agrees with both graph and order;
- `ordered_nodes` is a duplicate-free permutation;
- every dependency precedes its consumer;
- graph hash recomputation succeeds;
- all four strategies return typed cycle/malformed-graph errors.

The scheduler may be optimized later without changing the checker.

### R2 - Lean schedule importer and L3 CI

Add a Lean executable that:

1. reads a scoped `schedule_certificate` JSON artifact;
2. maps it to the existing Lean graph and strategy types;
3. recomputes the graph hash;
4. runs `verifySchedule`;
5. emits a `verification_result` artifact;
6. exits nonzero on malformed or rejected input.

CI must compare Rust and Lean acceptance on:

- the chain, diamond, asymmetric fork/join, disconnected, and path-heavy DAGs;
- exhaustive upper-triangular DAGs through six nodes;
- relabelled graphs and randomized insertion orders;
- deliberately corrupted node counts, orders, edges, and hashes.

### R3 - Vector plan certificate at L2/L3

Implement the strategy-independent `VectorPlan` DTO and verifier.

Required checks:

- unique signal, loop, epoch, transport, and stable-name identities;
- exact epoch coverage and unique epoch ranks;
- ownership/root equivalence and inline duplicability;
- complete edge endpoints and no loop self-edge after normalization;
- acyclic induced graph for every epoch;
- complete typed transports for every cross-loop current-sample read;
- effect ordering or proven commutation for incomparable loops;
- monotone cross-epoch barriers;
- recursion groups and clock islands remain serial;
- every vectorizable loop has a recognized `VecSafe` witness kind;
- changing `-ss` leaves canonical plan bytes and `plan_hash` unchanged.

The Lean checker should initially mirror these finite checks. Deeper semantic
witnesses can replace enumerated witness tags as the execution model matures.

### R4 - Routed FIR certificate

After signal-level routing, emit and verify:

- one region for every materialized loop and fixed control domain;
- signal-to-FIR value types;
- producer stores and consumer loads for each transport;
- identical chunk-local index expressions on both sides;
- exactly-once emission of nonduplicable effects;
- fixed epoch-body order and selected intra-epoch schedules;
- no undeclared cross-region value reference;
- no strategy-dependent storage name or allocation.

Backend generation is forbidden unless this certificate is accepted.

### R5 - Semantic reference execution

Extend the Lean model with a small executable signal/FIR semantics in increments:

1. constants, inputs, and pure arithmetic;
2. casts and tuple projections;
3. bounded delays;
4. recursion groups;
5. tables, UI, and effect observations;
6. clock-domain islands;
7. forward and reverse AD epochs.

For each increment, generate bounded programs and inputs, then compare:

```text
Lean scalar reference
Rust scalar interpreter
Rust vector interpreter, lv=0 and lv=1, ss=0/1/2/3
C++ reference where the behavior is implemented there
```

Use bit equality at current impulse-test boundaries. Any intentionally relaxed
numeric relation must be stated per operation and must not be introduced by
scheduling.

### R6 - Selected L4 refinement

Attempt deductive connection only after DTOs and executable checkers stabilize.
Prioritize pure, ownership-free functions:

- strategy decoding;
- schedule verification;
- loop-separation precedence;
- canonical sorting and projection construction;
- transport index bounds;
- epoch-barrier validation.

Possible implementation-proof approaches include translating a restricted Rust
subset into Lean or proving an equivalent functional model and validating the
compiled Rust through generated conformance tests. Tool choice is deferred until
a prototype demonstrates support for the repository's Rust edition, enums,
collections, error model, and CI platforms. No tool is accepted merely because
it can parse a toy function.

### R7 - Backend refinement gates

Treat backend correctness as refinement of certified FIR, not as a repetition of
the signal analysis proof.

For representative FIR programs require:

```text
Execute(Cranelift(FIR))      = Interpret(FIR)
Execute(Wasm(FIR))           = Interpret(FIR)
Execute(AssemblyScript(FIR)) = Interpret(FIR)
```

Initially this is differential testing at L1/L3. Formal backend proofs are a
separate long-term project because they include ABI, runtime, and target-machine
semantics.

## 7. CI and Developer Workflow

### Fast gate on every change

```text
cargo test for touched checker crates
canonical schema/example validation
Rust certificate negative tests
Lean compilation of the formal specification
small Rust/Lean cross-check corpus
```

### Workspace gate before merge

```text
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo run -p xtask -- golden-check
complete versioned certificate corpus through Lean
```

### Scheduled exhaustive gate

- exhaustive small DAG and small plan enumeration;
- randomized larger graphs with deterministic seeds;
- scalar/vector/backend differential execution;
- C++ topology and runtime comparison;
- canonical snapshot reproducibility on all CI operating systems.

Artifacts emitted by CI must use repository-relative case ids and record the
Rust and Lean verifier versions. A verification result never overrides a hash
mismatch or schema failure.

## 8. Negative Testing Policy

Every semantic check needs a mutation that proves rejection. Minimum mutations:

- remove, duplicate, or reorder one graph node;
- reverse one dependency edge;
- place a consumer before its dependency;
- alter `graph_hash` or `plan_hash`;
- duplicate a loop across epochs;
- change an epoch rank;
- assign an owned root to the wrong loop;
- mark a nonduplicable signal inline;
- remove one required transport;
- alter a transport type or length;
- remove an effect edge between conflicting loops;
- move a loop between forward/reverse AD epochs;
- change one routed FIR store/load index;
- duplicate or omit an effectful FIR statement.

A checker without a demonstrated rejecting mutation is not complete enough to
serve as a trust boundary.

## 9. Schema Evolution and Compatibility

- v1 is internal and experimental until R2 exits;
- once used by committed CI artifacts, incompatible changes require v2;
- the Lean and Rust readers declare the exact version set they accept;
- a verifier must never silently drop unknown fields;
- old artifacts remain verifiable with their pinned verifier or an audited
  explicit converter;
- semantic changes require a plan/journal entry even when JSON shape is
  unchanged;
- hash projection changes always require a schema version change.

## 10. Completion Criteria

The certified porting architecture is operational when:

1. the canonical schema has shared Rust/Lean parser and hash vectors;
2. scheduler results are L2 checked in every successful compiler path;
3. vector plans and routed FIR are rejected before emission on any failed check;
4. CI rechecks all versioned artifacts with Lean;
5. `-ss` changes only certified schedules, never `VectorPlan` identity;
6. `-vec -lv 0` and `-vec -lv 1` match scalar execution across all supported
   backends and scheduling strategies;
7. every unsupported semantic case fails with a typed diagnostic rather than
   bypassing certification;
8. the remaining trusted computing base and unproved obligations are listed in
   the current handoff and release documentation.

This architecture does not turn the complete compiler into one monolithic Lean
proof. It creates a sequence of small, auditable proof and verification
boundaries so that the Rust port can progress without postponing assurance until
the implementation is finished.
