# Eval Label Port Plan (2026-03-07)

Status: proposed execution plan

Scope: port the C++ `evalLabel(...)` semantics used by `eval` for UI labels and
modulation targets into Rust `crates/eval`, with explicit staging and parity
gates.

## 1. Problem statement

The C++ evaluator supports dynamic label substitution through
`evalLabel(const char* src, Tree visited, Tree localValEnv)` in
`compiler/evaluate/eval.cpp`.

This logic is used when evaluating:

- widget labels (`button`, `checkbox`, `vslider`, `hslider`, `nentry`)
- group labels (`vgroup`, `hgroup`, `tgroup`)
- bargraph labels
- soundfile labels
- modulation target labels

The Rust evaluator currently implements only the literal-label subset needed by
the active corpus. This is explicitly documented in
`crates/eval/src/lib.rs` in `eval_modulation_label(...)`.

As a result, Rust does not yet provide full C++ parity for labels containing
dynamic `%...` substitutions.

## 2. C++ reference behavior

Primary reference files:

- `compiler/evaluate/eval.cpp`
- `compiler/evaluate/eval.hh`
- `compiler/labels.hh`

Relevant C++ entry points:

- `evalLabel(...)`
- `writeIdentValue(...)`
- UI/widget branches in `eval(...)`
- modulation branch in `eval(...)`

Observed semantic role:

- parse label text as a small substitution language
- copy ordinary characters unchanged
- interpret `%`-introduced substitutions
- support optional numeric formatting width before the identifier
- resolve identifiers from the current evaluation environment
- splice resolved values into the final label string

Supported C++ forms to preserve:

- `%i`
- `%2i`
- `%{name}`
- `%{identifier_with_alnum_chars}`

Exact accepted identifier grammar and formatting behavior must be locked from
the C++ source and differential tests before implementation is considered
complete.

## 3. Current Rust state

Current Rust behavior is intentionally narrower:

- `eval_modulation_label(...)` evaluates the label node as a Faust expression
- the result must already be a plain text label node
- metadata is stripped from the final text
- path matching is then based on `/`-separated literal segments

Current limitations relative to C++:

- no `%...` substitution parser
- no width/format handling
- no environment-driven label interpolation
- no shared helper reused across all UI label-bearing nodes
- no explicit parity fixtures for dynamic labels in `tests/corpus`

This is therefore an `adapted` status today, not `1:1`.

## 4. Porting objective

Target objective:

- provide one Rust helper with C++-equivalent label-substitution semantics for
  all evaluator-owned label evaluation paths
- keep literal-label behavior unchanged
- preserve path/group matching semantics already validated in Rust
- add corpus and unit coverage that differentially checks Rust against the C++
  reference compiler

Non-goals for the initial port:

- redesign the UI path model
- broaden label semantics beyond the C++ behavior
- introduce parser-level preprocessing of labels

## 5. Representation and API plan

Recommended Rust API shape:

- add an internal helper in `crates/eval/src/lib.rs`:
  - `fn eval_label(arena: &mut TreeArena, label_text: &str, env: &Environment, loop_detector: &mut LoopDetector) -> Result<String, EvalError>`
- keep it evaluator-internal first; do not expose it as a public crate API
- route all eval-owned label-bearing branches through this helper

Reasoning:

- this keeps parity-sensitive string evaluation in one place
- this avoids scattering partial `%` handling across widgets and modulation
- this mirrors the C++ design where label interpolation is a dedicated helper

## 6. Implementation stages

### Stage 0: pin exact C++ behavior

Before changing Rust code:

- inspect `writeIdentValue(...)` and any helper in `labels.hh`
- document:
  - accepted placeholder grammar
  - accepted identifier characters
  - width/format semantics
  - behavior on malformed placeholders
  - behavior when an identifier is undefined
  - integer vs real formatting rules
- record the findings in this document or a short companion note if needed

Pass criteria:

- no unresolved ambiguity remains about placeholder syntax or failure policy

### Stage 1: add focused Rust unit tests first

Add unit tests covering the intended helper behavior before implementation:

- literal label remains unchanged
- `%i` resolves from environment
- `%2i` applies width formatting with C++-matching output
- `%{name}` resolves the same way as bare `%name`
- malformed `%` sequences follow C++ behavior
- undefined identifiers follow C++ behavior
- multiple substitutions in one label are handled left-to-right

Pass criteria:

- tests express exact expected strings from C++ observations

### Stage 2: implement `eval_label`

Implement a small state machine equivalent to the C++ parser:

- state 0: copy ordinary characters, enter substitution mode on `%`
- state 1: gather optional width digits or identifier start
- state 2: gather bare identifier
- state 3: gather braced identifier until `}`

Important constraints:

- preserve the same malformed-input fallbacks as C++
- do not silently invent new placeholder forms
- keep formatting/parsing ASCII-oriented unless C++ behavior proves otherwise

Pass criteria:

- Stage 1 tests pass

### Stage 3: integrate all eval-owned label sites

Replace literal-only handling in Rust eval for:

- modulation target label evaluation
- widget labels
- group labels
- bargraph labels
- soundfile labels

Integration rule:

- evaluate dynamic substitutions once at eval time
- continue to strip metadata and build target paths exactly as today after
  interpolation

Pass criteria:

- no label-bearing eval path bypasses the helper

### Stage 4: corpus coverage

Add DSP fixtures under `tests/corpus` for:

- interpolated modulation target label
- interpolated widget label
- interpolated group path segment
- width-formatted substitution
- malformed placeholder fallback case

If some cases are too parity-sensitive to freeze immediately, place them first
under `tests/cpp_parity_known_gaps/` with a documented promotion path.

Pass criteria:

- Rust goldens exist for all new fixtures
- at least one successful end-to-end corpus case exercises group-path matching
  after interpolation

### Stage 5: C++ differential validation

Add or extend differential tests to compare Rust vs C++ on the new fixtures.

Required checks:

- Rust/C++ both accept valid dynamic-label cases
- the resulting signal / normalized compile status matches
- modulation target matching remains behaviorally aligned

Pass criteria:

- differential test suite passes against the pinned C++ reference branch

## 7. Error policy

This aspect is parity-sensitive and must not be guessed.

Before landing implementation, clarify from C++ source/tests:

- whether undefined identifiers in labels are hard errors, warnings, or textual
  passthrough
- whether malformed placeholders terminate parsing, pass through verbatim, or
  partially evaluate
- whether non-numeric substituted values are accepted and how they are rendered

Rust should mirror the C++ behavior exactly. If the C++ behavior is inconsistent
or undocumented, the Rust port must record the chosen parity rule and the
evidence used.

## 8. Validation checklist

Local validation required for the implementation PR:

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test --workspace --all-targets`
- `cargo run -p xtask -- golden-check`

Additional recommended validation:

- `cargo run -p xtask -- golden-check-cpp`
- targeted C++ differential tests for the new label fixtures

## 9. Mapping status

Current status:

- public API status: `deferred` for full `evalLabel` parity
- internal evaluator status: `adapted` literal-only subset

Target status after this plan:

- internal evaluator status: `1:1` for the C++ `evalLabel` behavior used by
  `eval`

Compatibility impact:

- fixes currently unsupported dynamic-label DSP programs
- reduces parity risk in modulation/UI naming paths
- should be backward-compatible for existing literal-label corpus cases

## 10. Risks and mitigations

Risk: placeholder grammar is underspecified.
Mitigation: pin exact behavior from C++ before coding.

Risk: label interpolation changes modulation target matching unexpectedly.
Mitigation: keep existing path splitting logic after interpolation and add
targeted corpus fixtures.

Risk: undefined identifiers in labels leak into silent Rust-only behavior.
Mitigation: differential tests must lock failure or fallback semantics.

Risk: implementation duplicates environment-to-string conversion logic
incorrectly.
Mitigation: centralize conversion in one helper and keep tests for integer/real
cases.

## 11. Completion criteria

This plan is complete when all of the following are true:

- Rust has one documented `eval_label` helper implementing the C++ placeholder
  semantics
- modulation and UI/group label paths use it consistently
- new corpus fixtures cover valid and malformed dynamic-label cases
- Rust/C++ differential tests pass on those fixtures
- the journaling and parity notes mark this area as no longer deferred
