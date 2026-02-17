# Faust-rs Diagnostics and Error-Reporting Model

> Scope: define a structured, phase-wide diagnostics model for the Rust port, with semantic parity against C++ behavior and better user-facing feedback quality.
> Status: planning baseline (to be implemented incrementally across `errors`, `parser`, `eval`, `propagate`, and `compiler`).

---

## 1. Why this document exists

The C++ compiler is functionally mature, but its error reporting model is historically string/exception based and globally coupled (`gErrorCount`, `gErrorMessage`, direct throws from parser/eval paths).

The Rust port can keep behavioral parity while improving:
- source localization precision,
- diagnostic consistency across phases,
- actionable hints/suggestions,
- machine-readable outputs for CI/IDE tooling.

User-facing motivation is explicit in Faust docs:
- <https://faustdoc.grame.fr/manual/errors/> states current error reporting can be improved.

---

## 2. Source-of-truth references

### 2.1 C++ baseline (current compiler)

- `/Users/letz/Developpements/RUST/faust/compiler/errors/errormsg.hh`
- `/Users/letz/Developpements/RUST/faust/compiler/errors/errormsg.cpp`
- `/Users/letz/Developpements/RUST/faust/compiler/parser/faustparser.y` (`yyerror`)
- `/Users/letz/Developpements/RUST/faust/compiler/evaluate/eval.cpp` (`evalerror`, manual throw paths)

Key observation:
- C++ mostly formats string messages and throws exceptions immediately.
- Definition/use line properties exist and are valuable parity anchors.

### 2.2 Current Rust state (observed)

- `crates/parser-proto/src/context.rs`
  - `ParserDiagnostic` exists, but location is currently file + line (no explicit range labels).
- `crates/parser-proto/src/lib.rs`
  - `parse_program` still exposes `errors: Vec<String>` in `ParseOutput`.
- `crates/compiler/src/lib.rs`
  - parse failures are collapsed to counters (`parse_errors`, `recoveries`) in `CompilerError::Parse`.
- `crates/compiler/src/main.rs`
  - CLI output remains mostly generic (`Parse failed: ...`, `Signal pipeline failed: ...`).
- `crates/eval/src/lib.rs`, `crates/propagate/src/lib.rs`
  - typed error enums exist, but mostly without rich source labels/help payload.

---

## 3. Design principles

1. Behavioral parity first, diagnostics quality better by default.
2. No hidden mutable global diagnostics state.
3. Deterministic output across platforms for test snapshots.
4. One diagnostic vocabulary shared by all phases.
5. Human and machine formats are first-class.
6. No temporary diagnostics stubs: each integrated phase must emit structured diagnostics directly.

---

## 4. Target architecture

### 4.1 Core types in `crates/errors`

The `errors` crate should own a canonical model similar to:

```rust
pub enum Severity {
    Error,
    Warning,
    Remark,
}

pub enum Stage {
    SourceReader,
    Lexer,
    Parser,
    Eval,
    Propagate,
    Normalize,
    Transform,
    Fir,
    Codegen,
    Compiler,
}

pub struct SourceSpan {
    pub file: std::path::PathBuf,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

pub enum LabelStyle {
    Primary,
    Secondary,
}

pub struct Label {
    pub style: LabelStyle,
    pub span: SourceSpan,
    pub message: String,
}

pub struct DiagnosticCode(pub &'static str); // ex: "FRS-PARSE-0003"

pub struct Diagnostic {
    pub severity: Severity,
    pub stage: Stage,
    pub code: DiagnosticCode,
    pub message: String,
    pub labels: Vec<Label>,
    pub notes: Vec<String>,
    pub help: Vec<String>,
}

pub struct DiagnosticBundle {
    pub diagnostics: Vec<Diagnostic>,
}
```

### 4.2 Code taxonomy (stable identifiers)

Use stable code families:

- `FRS-SRC-*` (imports/files/source reader)
- `FRS-LEX-*`
- `FRS-PARSE-*`
- `FRS-EVAL-*`
- `FRS-PROP-*`
- `FRS-NORM-*`
- `FRS-TR-*`
- `FRS-FIR-*`
- `FRS-CG-*`
- `FRS-COMP-*`

Codes are stable contracts for tests, CI triage, and tooling integration.

### 4.3 Conversion boundary

Each phase error type must implement conversion to diagnostics:

```rust
pub trait IntoDiagnostic {
    fn into_diagnostic(self) -> Diagnostic;
}
```

This keeps phase-local enums (`EvalError`, `PropagateError`, parser/source reader errors) while unifying emission.

### 4.4 Rendering

Support both:

1. Human-oriented rendering (`file:line:col`, snippet, caret labels, notes/help).
2. Machine-oriented JSON rendering (stable schema + stable code fields).

CLI policy:
- `--error-format human` (default)
- `--error-format json` (CI/IDE)

---

## 5. Phase-specific integration

### 5.1 Parser and lexer

Required outcomes:

1. Preserve parser recovery classes and recovery counts.
2. Upgrade location precision to `file + line + column + range`.
3. Keep parser context diagnostics structured end-to-end.
4. Avoid flattening to `Vec<String>` in production interfaces.

### 5.2 SourceReader/import diagnostics

Required outcomes:

1. Report unresolved imports with source location and import origin.
2. Report cycle paths with explicit chain context.
3. Keep deterministic ordering of reported import issues.

### 5.3 Eval diagnostics

Required outcomes:

1. Map key evaluator failures to stable codes.
2. Attach relevant definition/use labels when source metadata exists.
3. Provide contextual help for common mistakes (undefined symbol, arity mismatch, invalid iteration count).
4. When full source labels are not available, include explicit node context notes:
   - `node_id`,
   - canonical box expression preview for the offending node.
5. For undefined-symbol failures, provide nearest-symbol suggestions from visible scope.

### 5.4 Propagate diagnostics

Required outcomes:

1. Stable codes for arity and unsupported-node classes.
2. Include box-node context and, when available, source labels propagated from parser metadata.
3. Emit concise primary messages and explicit expected/got notes.
4. Add rule-specific computed explanations and corrective hints for composition errors
   (for example split/merge divisibility and computed remainder).

### 5.5 Compiler orchestration diagnostics

Required outcomes:

1. Preserve stage information from lower crates.
2. Never reduce failures to counters only when diagnostics are available.
3. Export consolidated bundle for CLI and API surfaces.
4. Keep JSON output schema stable (`severity`, `stage`, `code`, `message`, `labels`, `notes`, `help`).
5. In human format, render snippets/carets whenever labels with source spans are available.

---

## 6. Migration plan (deliverables + pass criteria)

### Deliverable A — `errors` crate core model

- Add canonical types (`Diagnostic`, `DiagnosticCode`, `Stage`, `SourceSpan`, bundle).
- Add unit tests for deterministic formatting and JSON schema stability.

Pass criterion:
- `errors` crate provides stable public diagnostics API used by at least one consumer crate.

### Deliverable B — parser diagnostics parity baseline

- Replace parse `Vec<String>` as primary error carrier with structured diagnostics.
- Add line/column/range population path.

Pass criterion:
- malformed parser corpus validates class + location + code expectations.

### Deliverable C — source reader diagnostic enrichment

- Convert `SourceReaderError` to structured diagnostics (with import chain details).

Pass criterion:
- unresolved import and cycle fixtures produce deterministic multi-label diagnostics.

### Deliverable D — eval/propagate integration

- Map top-priority `EvalError` / `PropagateError` variants to stable code families.
- Attach source labels where metadata exists.

Pass criterion:
- differential negative tests show equivalent failure classes vs C++ and richer Rust diagnostics.

### Deliverable E — compiler + CLI output model

- Aggregate per-stage diagnostics in `compiler`.
- Add `--error-format human|json`.

Pass criterion:
- CLI snapshot tests for both formats pass on Linux/macOS/Windows.

### Deliverable F — quality lock and documentation closure

- Cross-phase docs updated (`phase-1`, `phase-3`, `phase-4`, integration docs).
- Rustdoc provenance for error-related APIs.

Pass criterion:
- no touched phase documents rely on ad hoc string-only error channels.

### Deliverable G — diagnostics UX explainability tranche (post step-9)

- Enrich `EvalError`/`PropagateError` conversion with explicit node context notes:
  - `node_id=<id>`,
  - compact box-expression preview.
- Add rule-specific guidance for arity/composition failures:
  - failed algebraic condition,
  - computed values,
  - actionable correction hints.
- Propagate parser-origin spans to Phase 4 errors when available on offending nodes.
- Upgrade human renderer with snippet/caret output while keeping JSON schema stable.
- Expand negative corpus + snapshots for both formats.

Pass criterion:
- For representative eval/propagate failures, CLI human output identifies:
  - source location (or explicit fallback when unavailable),
  - failing expression context,
  - correction hint.
- Snapshot tests cover both `--error-format human` and `--error-format json`
  on Linux/macOS/Windows.

Remaining UX improvements (next tranche, prioritized):

1. Precise in-line pointing:
- attach parser-origin spans to non-identifier expression nodes so diagnostics can point to operator-level columns (`<:`, `:>`, `~`, etc.), not only definition starts.

2. Alias-resolution context:
- include explicit binding trace notes for propagated failures when relevant (`process -> bar -> foo`).

3. Readable expression context:
- keep machine-oriented internal preview (`box_expr=...`) but add a human-facing normalized expression form for diagnostics.

4. Paired-side mismatch context:
- for composition errors, include both sides with computed arities in dedicated notes (left expression/output vs right expression/input).

5. Snapshot expansion:
- extend negative snapshot corpus to alias chains, recursive mismatches, and UI-driven composition failures.

6. Operator-specific correction hints:
- tune `help` messages per composition class (`seq`, `split`, `merge`, `rec`) with concrete fix patterns.

Next micro-tranche (post-step-6, readability-focused):

1. C++-style paired rendering in human output:
- add explicit blocks:
  - `Here A = ...` (+ `has ... inputs/outputs`),
  - `while B = ...` (+ `has ... inputs/outputs`).

2. Readable primitive/UI expression pretty-print:
- replace internal forms (`1(str(...), cons(...))`, primitive tags) with user-facing Faust forms:
  - `hslider("gain", 0.5, 0.0, 1.0, 0.01)`,
  - `+`, `*`, etc.

3. Definition-owner clarity:
- add explicit owner note when available:
  - `error originates from definition 'foo'`,
  - keep `binding_trace=process -> ... -> foo` as structural context.

4. Numeric correction proposals:
- include one computed target when possible:
  - e.g. `right inputs should be 4 (nearest multiple of 2), got 3`.

5. Snapshot lock for readability rules:
- add human/json snapshot assertions specifically for:
  - C++-style `A/B` blocks,
  - UI pretty-print,
  - owner note + numeric proposal presence.

Eval-specific readability follow-up (after current micro-tranche):

1. Increase node-carrying coverage in `EvalError`:
- add offending-node context for currently node-less variants where possible:
  - `UndefinedSymbol`,
  - `MissingProcessDefinition` (attach process-definition/root context note),
  - application/matching arity failures.

2. Source-label enrichment for eval failures:
- when eval emits node context, reuse compiler source-label attachment path to provide
  line/column/snippet for eval diagnostics with the same quality as propagate.

3. Eval-friendly actionable hints:
- refine eval `help` payloads with scoped fix patterns:
  - unresolved symbols (scope/definition site),
  - missing process (top-level contract),
  - application arity mismatch (expected vs provided arguments).

4. Eval negative snapshot expansion:
- add dedicated eval human/json snapshots for:
  - undefined symbol chains,
  - missing `process`,
  - arity and case-pattern mismatch failures.

Pass criterion:
- representative eval failures include either source label or explicit fallback note,
- eval diagnostics contain stable `FRS-EVAL-*` codes with user-facing correction hints,
- human/json snapshots lock the output contract.

---

## 7. Test strategy

1. Unit tests in `errors`:
   - code and severity stability,
   - label ordering determinism,
   - renderer output shape.
2. Parser/source-reader negative corpus:
   - malformed syntax,
   - unresolved import,
   - cycle chain.
3. Eval/propagate negative corpus:
   - undefined symbols,
   - bad arity compositions,
   - unsupported box families in current scope.
4. Differential checks:
   - same failure class as C++ (behavior),
   - richer structure in Rust (quality).
5. Snapshot tests:
   - human output snapshots,
   - JSON schema snapshots.

---

## 8. Non-goals

1. Reproducing byte-for-byte C++ error strings.
2. Reintroducing global mutable error channels.
3. Blocking Phase 4/5 implementation on “perfect final wording”.

The target is stable structured diagnostics with incremental UX improvements.

---

## 9. Adoption links in the porting plan

This document is normative for diagnostics architecture and must be read with:

- `faust-rust-porting-plan-en.md`
- `phases/phase-1-fondations-en.md` (errors crate scope)
- `phases/phase-3-parser-en.md` (diagnostics/recovery parity)
- `phases/phase-4-signaux-en.md` (eval/propagate diagnostics integration)
- `phases/phase-0-gglobal-decomposition-map-en.md` (`gErrorCount`/`gErrorMessage` decomposition)
