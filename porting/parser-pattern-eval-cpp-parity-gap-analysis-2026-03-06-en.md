# Parser / Pattern Matching / Evaluation Parity Gap Analysis (Rust vs C++)

> **Date**: 2026-03-06
> **Scope**: `crates/parser`, `crates/eval`, `crates/boxes`
> **Reference C++ baseline**: `master-dev-ocpp-od-fir-2-FIR19` (`8eebea429`)
> **Reference C++ source roots**:
> - `/Users/letz/Developpements/RUST/faust/compiler/parser`
> - `/Users/letz/Developpements/RUST/faust/compiler/evaluate`
> **Status**: historical gap analysis, updated on 2026-03-07 with current implementation status

This document was written as the initial parity-gap analysis on 2026-03-06.
Several items identified here have since been implemented.

Current status summary on 2026-03-07:

- parser grouped/patterned definitions: implemented
- evaluated `case` patterns + barrier semantics: implemented
- closure-valued evaluation model in `eval`: implemented
- `a2sb` lowering through evaluator values: implemented
- `slot` / `symbolic` / `modifLocalDef` support: implemented
- `prepare_pattern()` opacity parity against C++ `preparePattern()`: implemented
- definition-scoped metadata / `declare` parity (`boxMetadata`-level semantics): implemented
- no remaining parser/pattern/eval gap from the original list is still open in
  this scope

The closure follow-up described here is now completed and documented in
`porting/eval-true-closure-model-port-plan-2026-03-06-en.md`. The remaining
non-closure work is split out into
`porting/parser-pattern-eval-remaining-gap-plan-2026-03-07-en.md`.

---

## 1. What Was Checked

Local checks executed in this workspace:

1. `cargo test -p parser --all-targets`
2. `cargo test -p eval --all-targets`
3. `cargo test -p compiler --test signal_pipeline -- --nocapture`

Source inspection covered:

- `crates/parser/src/lib.rs`
- `crates/parser/src/grammar/faustparser.y`
- `crates/parser/src/source_reader.rs`
- `crates/eval/src/lib.rs`
- `crates/eval/src/pattern_matcher.rs`
- `crates/boxes/src/lib.rs`
- `crates/parser/tests/structural_cpp_differential.rs`
- `crates/compiler/tests/diagnostic_errors.rs`
- `porting/phases/phase-3-parser-parity-status-2026-02-28-en.md`
- `porting/phases/phase-3-parser-full-parity-plan-en.md`
- `porting/phases/phase-4-corpus-status-diff-report-en.md`
- `porting/phases/phase-6-backend-full-corpus-diff-report-en.md`
- `porting/pattern-matcher-performance-analysis-en.md`
- `/Users/letz/Developpements/RUST/faust/compiler/parser/faustparser.y`
- `/Users/letz/Developpements/RUST/faust/compiler/parser/sourcereader.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/evaluate/eval.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/evaluate/environment.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/patternmatcher/patternmatcher.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/boxes/boxes.cpp`

The original targeted Rust test suites were green on 2026-03-06, but they did
not yet cover all semantic cases needed for C++ parity in parser definitions,
`case`, and the evaluation-to-box lowering path.

---

## 2. Originally Confirmed Semantic Gaps On 2026-03-06

## 2.1 Patterned and multi-clause definitions were not parser/eval equivalent

Status on 2026-03-07: implemented.

Rust currently parses function definitions with identifier-only parameter lists,
while C++ accepts full argument patterns and later normalizes grouped
definitions.

Rust evidence:

- `crates/parser/src/grammar/faustparser.y` parses `Definition` through an
  identifier-oriented `ParamList`.
- `crates/parser/src/lib.rs` keeps `format_definitions()` as a no-op.
- `crates/eval/src/lib.rs` binds definitions directly and rejects repeated
  symbol names as redefinitions.

C++ reference behavior:

- `/Users/letz/Developpements/RUST/faust/compiler/parser/faustparser.y`
  parses definitions through `arglist`.
- `/Users/letz/Developpements/RUST/faust/compiler/parser/sourcereader.cpp`
  uses `standardArgList`, `makeDefinition`, and `formatDefinitions` to group
  and normalize same-name clauses.

Observed consequences:

1. `foo(0) = _; foo(x) = x; process = foo;`
   - C++: accepted
   - Rust: parse failure
2. `foo(x) = x; foo(y) = y; process = foo;`
   - C++: accepted
   - Rust: rejected later as `RedefinedSymbol`

This is a hard parity blocker because Faust pattern-based function definitions
are part of the language surface, not an edge case.

## 2.2 `case` rules were compiled from raw patterns instead of evaluated patterns

Status on 2026-03-07: implemented.

Rust currently builds the automaton directly from the stored rule list. C++
first evaluates and simplifies pattern expressions before automaton
construction.

Rust evidence:

- `crates/eval/src/lib.rs` feeds `rules_rev` directly into the pattern matcher.

C++ reference behavior:

- `/Users/letz/Developpements/RUST/faust/compiler/evaluate/eval.cpp` runs
  `evalRuleList`, `evalPattern`, and pattern simplification before
  `make_pattern_matcher`.

Observed consequence:

`foo = case { (1+1) => _; }; process = foo(2);`

- C++: accepted
- Rust: `no case rule matches arguments`

Any rule whose left-hand side depends on compile-time simplification can drift
from C++ until this stage is ported.

## 2.3 Pattern-variable scope barriers were incorrect

Status on 2026-03-07: implemented.

Rust rule matching currently checks existing bindings through full environment
lookup. C++ inserts an environment barrier so that a pattern variable only
sees bindings introduced by the current rule match.

Rust evidence:

- `crates/eval/src/lib.rs` creates rule environments with `push_scope()`.
- `crates/eval/src/pattern_matcher.rs` uses `env.lookup(...)` for repeated
  variable handling.

C++ reference behavior:

- `/Users/letz/Developpements/RUST/faust/compiler/evaluate/environment.cpp`
  uses `pushEnvBarrier`.
- `searchIdDef` stops lookup at the barrier.

Observed consequence:

`x = 1; foo = case { (x) => x; }; process = foo(2);`

- C++: the rule matches and returns `2`
- Rust: `no case rule matches arguments`

This is a semantic bug in variable capture and non-linearity handling, not just
an implementation detail.

## 2.4 The Rust pipeline still lacked the C++ `a2sb()` stage

Status on 2026-03-07: implemented.

C++ does not expose raw abstractions, pattern matchers, or modulation nodes to
the rest of the pipeline. `evalprocess` always runs the result through `a2sb()`
to lower closures into symbolic boxes and slots. Rust still returns the raw
evaluated box tree.

Rust evidence:

- `crates/eval/src/lib.rs` returns `eval_box(...)` directly from
  `eval_process_with_stats()`.

C++ reference behavior:

- `/Users/letz/Developpements/RUST/faust/compiler/evaluate/eval.cpp`
  routes `evalprocess` through `a2sb(eval(...))`.

Observed consequences:

- `case`, `abstr`, and `modulation` forms can survive until `propagate`.
- Compiler tests already record this as a known failure mode for some fixtures.

This is the main structural reason parser/eval parity is still incomplete even
when local parser/eval unit tests pass.

## 2.5 Required box families for parity were still missing

Status on 2026-03-07: implemented to the level required by the parser/pattern/eval
parity scope. `slot`, `symbolic`, `modifLocalDef`, and definition-scoped
`Metadata` support are now present.

Porting `a2sb()` requires box families that exist in C++ but are not yet
represented in the Rust `boxes` crate.

C++ reference boxes include:

- `boxSlot`
- `boxSymbolic`
- `boxPatternMatcher`
- `boxMetadata`
- `boxModifLocalDef`

Rust evidence:

- `crates/boxes/src/lib.rs` does not expose equivalent node families in
  `BoxMatch`.

This gap is now closed in the parser/pattern/eval scope covered by this report.

## 2.6 `prepare_pattern()` is broader than the C++ implementation

Status on 2026-03-07: implemented.

Rust recursively rewrites patterns across the generic tree shape. C++ keeps a
whitelist/blacklist boundary and leaves several box families opaque during
pattern preparation.

Rust evidence:

- `crates/parser/src/lib.rs` recursively descends through generic tagged nodes.

C++ reference behavior:

- `/Users/letz/Developpements/RUST/faust/compiler/boxes/boxes.cpp`
  explicitly preserves `abstr`, `access`, `component`, `environment`, `slot`,
  `symbolic`, `case`, and related forms as opaque in `preparePattern()`.

Historical impact on 2026-03-06:

- this was a likely source of future mismatches on complex patterns,
- current tests did not meaningfully constrain this area.

## 2.7 Metadata and local-definition modifiers are not yet end-to-end equivalent

Status on 2026-03-07:

- `boxModifLocalDef` / `expr [ defs ]`: implemented
- definition-scoped metadata / `declare` reinjection parity: implemented
- top-level `declare key value;` remains an explicitly documented `adapted`
  parser-context representation rather than a runtime-global metadata store

On 2026-03-06 Rust recorded declaration metadata in parser-side context but did
not yet reinject definition-scoped metadata through equivalent box nodes and
evaluation semantics.

Rust evidence:

- `crates/parser/src/lib.rs` stored metadata in `ParserCtx`.
- no equivalent of C++ `boxMetadata` / `boxModifLocalDef` existed in
  `crates/boxes`.

C++ reference behavior:

- `/Users/letz/Developpements/RUST/faust/compiler/parser/sourcereader.cpp`
  reinjects definition metadata through `boxMetadata`.
- `/Users/letz/Developpements/RUST/faust/compiler/parser/faustparser.y` and
  `/Users/letz/Developpements/RUST/faust/compiler/evaluate/eval.cpp` support
  `expr { defs }` via `boxModifLocalDef`.

Historical impact on 2026-03-06:

- metadata semantics remained partial,
- local-definition modifier forms were not yet fully aligned with C++.

## 2.8 Differential coverage was too narrow to guard the missing semantics

Status on 2026-03-07: improved substantially and now covers the previously
open `prepare_pattern()` and definition-metadata parity gaps with dedicated
regressions.

On 2026-03-06 the parser differential harness did not cover the cases above,
and the compiler corpus still tolerated known `case`/closure failures.

Rust evidence:

- `crates/parser/tests/structural_cpp_differential.rs` only covers a small set
  of structural fixtures.
- `crates/compiler/tests/diagnostic_errors.rs` still encodes expected failures
  for some `case` fixtures.

Impact:

- local green test runs do not imply semantic parity,
- these gaps can regress silently.

---

## 3. Historical Correction Plan Used

The correction order below is designed to minimize rework and to recover C++
parity in the same order the language surface is actually consumed.

## 3.1 Restore definition semantics first

Deliverable:

- Rust parser accepts patterned definitions and grouped same-name clauses with
  the same normalization model as C++.

Actions:

1. Replace the current definition grammar path with an `ArgList`-based model.
2. Port `standardArgList`.
3. Port `makeDefinition`.
4. Port `formatDefinitions`.
5. Apply the same normalization path to:
   - top-level definitions,
   - `with` definitions,
   - recursive/local definition groups,
   - `expr { defs }` forms.

Pass criteria:

- `foo(0) = _; foo(x) = x; process = foo;` parses in Rust.
- `foo(x) = x; foo(y) = y; process = foo;` no longer fails as a plain symbol
  redefinition.
- parser structural tests compare grouped definitions against C++ output.

## 3.2 Restore `case` rule evaluation semantics

Deliverable:

- Rust compiles `case` automata from evaluated/simplified pattern rules, not
  raw parser trees.

Actions:

1. Port the C++ `evalRuleList` flow.
2. Port `evalPattern`.
3. Port pattern simplification before automaton construction.
4. Revisit automaton caching so the cache key reflects evaluated rules rather
   than only the raw rule list shape.

Pass criteria:

- `foo = case { (1+1) => _; }; process = foo(2);` behaves like C++.
- cache reuse is correct for environment-dependent patterns.

## 3.3 Fix pattern-variable barrier semantics

Deliverable:

- Pattern variables only see bindings introduced during the current rule match.

Actions:

1. Introduce a real barrier notion in the Rust evaluation environment, or an
   equivalent rule-local lookup path.
2. Stop using full parent-chain lookup for repeated pattern variable checks.
3. Keep rule-local non-linearity behavior aligned with the C++ matcher.

Pass criteria:

- `x = 1; foo = case { (x) => x; }; process = foo(2);` returns `2` in Rust.
- repeated variable patterns still enforce equality inside the same rule.

## 3.4 Port `a2sb()` and the missing box families

Deliverable:

- the Rust evaluator lowers abstractions and pattern matchers into the same box
  families expected by the downstream pipeline.

Actions:

1. Add Rust equivalents for:
   - `Slot`
   - `Symbolic`
   - `PatternMatcher`
   - `Metadata`
   - `ModifLocalDef`
2. Port `a2sb()` semantics from C++.
3. Route `eval_process_with_stats()` through the new lowering stage.
4. Recheck `propagate` assumptions once these node kinds appear.

Pass criteria:

- `case` and `lambda` fixtures no longer fail because raw `Case`/`Abstr` nodes
  leak into `propagate`.
- end-to-end compiler fixtures can move from expected-failure status to parity
  assertions.

## 3.5 Align `prepare_pattern()` with the C++ opacity boundaries

Deliverable:

- Rust pattern preparation preserves the same opaque node families as C++.

Actions:

1. Compare the current recursive Rust walk against the C++ `preparePattern()`
   whitelist/blacklist behavior.
2. Make opacity decisions explicit in Rust rather than relying on generic tree
   recursion.
3. Add structural tests for mixed patterns containing `case`, symbolic nodes,
   abstraction, and environment-like forms.

Pass criteria:

- prepared pattern trees for targeted complex cases match C++ shape decisions.

## 3.6 Port metadata and local-definition modifier semantics

Deliverable:

- `declare` metadata and `expr { defs }` style modifier semantics behave like
  the C++ compiler.

Actions:

1. Port parser-side reinjection through `Metadata` and `ModifLocalDef`.
2. Port evaluator handling for those nodes.
3. Add differential tests that assert both success/failure and structural shape.

Pass criteria:

- metadata-bearing definitions survive parser-to-eval transitions correctly.
- local-definition modifier forms produce the same box structure as C++.

## 3.7 Expand parity guardrails immediately

Deliverable:

- the missing semantics are covered by Rust/C++ differential tests and no
  longer rely on manual audit only.

Actions:

1. Add parser/eval/compiler differential tests for:
   - `foo(0) = _; foo(x) = x; process = foo;`
   - `foo(x) = x; foo(y) = y; process = foo;`
   - `foo = case { (1+1) => _; }; process = foo(2);`
   - `x = 1; foo = case { (x) => x; }; process = foo(2);`
2. Promote currently known failing fixtures into normal parity gates as each
   semantic block is closed.
3. Reclassify:
   - `rep_13_case_expression`
   - `rep_16_lambda_abstraction`
   - `rep_24_case_three_rules`
   - `rep_27_lambda_two_args`
   - `rep_32_modulation_single`
   - `rep_33_modulation_chain`

Pass criteria:

- these cases are permanently exercised in CI,
- green local tests become a meaningful indicator of C++ semantic parity.

---

## 4. Historical Closure Order

1. Definition grammar and normalization parity.
2. `case` rule evaluation parity.
3. Pattern-variable barrier fix.
4. `a2sb()` plus missing box families.
5. `prepare_pattern()` parity hardening.
6. Metadata / `ModifLocalDef` parity.
7. Differential and corpus guardrail expansion.

This ordering is important. Without step 1, the parser still rejects valid
Faust function-definition forms. Without steps 2 to 4, the evaluator still
cannot produce C++-equivalent box trees for `case`, abstraction, and modulation
flows.

---

## 5. Historical Conclusion And Current Outcome

On 2026-03-06 the Rust parser/eval stack was operational but not yet
semantically equivalent to the C++ compiler for patterned definitions, `case`
matching, rule-local variable binding, and closure-to-symbolic lowering.

That historical conclusion has now been retired for this scope. The identified
parser/pattern/eval items have since been implemented, guarded by dedicated
tests, and split from any remaining out-of-scope adapted representations.
