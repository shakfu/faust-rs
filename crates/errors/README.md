# errors

Structured diagnostics model shared by every stage of the `faust-rs` compiler pipeline.

## C++ provenance

| C++ path | Role |
|---|---|
| `compiler/errors/*` | Error classes and reporting helpers |
| Parser/eval/propagate pass-specific code | Per-stage diagnostic conventions |

## Public API

| Item | Description |
|---|---|
| `Diagnostic` | Single diagnostic with severity, stage, message, notes, labels |
| `DiagnosticBundle` | Aggregated set of diagnostics with error count |
| `DiagnosticCode` | Stable string identifier (`FRS-EVAL-0001`, …) |
| `Severity` | `Error` / `Warning` / `Remark` |
| `Stage` | Pipeline stage attribution (`Parser`, `Eval`, `Propagate`, …) |
| `SourceSpan` / `Label` | Source location and annotation |
| `codes::*` | All stable diagnostic codes as constants |

## Design invariants

- **Codes are stable**: wording can evolve without breaking CI or tool consumers.
- **Stage attribution is explicit**: failures can be bucketed per pipeline step.
- **Rendering is caller-owned**: this crate models data, not UI.

## Position in the pipeline

Compiler stages that emit structured diagnostics depend on `errors`; leaf IR,
runtime, FFI, and tooling crates may use their own typed errors instead. The
`errors` crate only models diagnostics: final rendering belongs to the
`faust-rs` binary or another consumer.
