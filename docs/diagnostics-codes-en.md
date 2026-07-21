# Frozen `FRS-*` Diagnostic Code Table

This is the authoritative, frozen list of stable diagnostic codes (`FRS-*`)
emitted by the Rust compiler's structured diagnostics (`--error-format json`,
`--error-format human`, and the `--check` mode). It is part of the P0 phase of
`porting/mcp-server-analysis-and-plan-2026-07-21-en.md` (§1.4.5: "Stable codes
become a public contract"), and exists so that a consumer — CI, an IDE, or a
future MCP server — can treat the code set as a versioned API rather than
re-deriving it from source on every change.

**Freeze rule.** Adding a new code is fine. Renumbering or repurposing an
existing code is not — it silently breaks every consumer that matched on it.
`crates/compiler/src/cli/tests.rs::frozen_frs_code_table_matches_source`
enforces this by re-running the exact extraction command below and diffing
the result against the table in this document; both adding an undocumented
code and renumbering a documented one fail that test.

**Source of truth / how this table was generated.** The canonical way to
enumerate every code actually present in source is:

```bash
grep -rhoE 'FRS-[A-Z]+-[0-9]+' --include=*.rs crates/ | sort -u
```

This currently returns **34 codes** across **8 stage-family namespaces**:
`FRS-LEX-*` (1), `FRS-PARSE-*` (3), `FRS-SRC-*` (3), `FRS-EVAL-*` (8),
`FRS-PROP-*` (5), `FRS-COMP-*` (4), `FRS-FIR-*` (2), `FRS-SFIR-*` (8).

Note the family prefix (`LEX`, `PARSE`, ...) is a naming convention only; the
JSON payload's `"stage"` field comes from the independent `errors::Stage`
enum and does not always equal the family name (e.g. every `FRS-SFIR-*` code
reports `"stage": "transform"`, not `"stage": "sfir"` — there is no `Sfir`
`Stage` variant). Both are listed per code below.

## Important caveat: several codes are currently unreachable or unused

The extraction command above is a textual grep over `.rs` source, not a
reachability analysis. Building this table required tracing every code from
its `errors::codes::*` constant to an actual call site, and that surfaced
real gaps, recorded here rather than papered over:

- **`FRS-SRC-0001`, `FRS-SRC-0002`, `FRS-SRC-0003`** are defined in
  `crates/errors/src/codes.rs` and listed in `codes::all_codes()`, but no
  code anywhere in the workspace ever constructs a `Diagnostic` with them.
  The real-world failure they were presumably meant to cover — an
  unresolved `import(...)` — does happen (e.g. `import("missing.lib")`
  fails), but it surfaces as `CompilerError::Import(SourceReaderError)`,
  which carries **no** `DiagnosticBundle` at all (see the `format_fallback_diagnostics_json`
  note below). The `FRS-SRC-*` family is reserved but dormant.
- **`FRS-COMP-0001`, `FRS-COMP-0002`, `FRS-COMP-0003`** are similarly
  defined and registered but never raised. Only `FRS-COMP-0004` is wired up.
- **`FRS-LEX-0001`** is defined and its call site
  (`crates/parser/src/lib.rs:1926`) is live code, but it is not reachable
  from any DSP text found during this audit: `crates/parser/src/grammar/faustlexer.l`
  ends with a catch-all `. 'EXTRA'` rule, so every single byte the lexer
  sees matches *some* token (an `EXTRA` token in the worst case) and the
  failure surfaces one layer up as a `FRS-PARSE-0001` parse error instead of
  a `lrpar::LexParseError::LexError`. Genuinely invalid bytes (e.g. a
  non-UTF-8 byte sequence) are rejected even earlier, at file read time,
  before lexing starts, with no diagnostics bundle at all.
- **`FRS-FIR-0001`** (verifier *error*, as opposed to `FRS-FIR-0002`
  warnings) requires the FIR verifier to reject FIR text that a
  *successful* front-end run produced — i.e. a compiler bug, not a user
  DSP mistake. No corpus file triggers it; only `--fir-fixture` bring-up
  fixtures could, and the eight built-in fixtures
  (`--list-fir-fixtures`) are all valid by construction.
- **`FRS-EVAL-0100`** does not come from `errors::codes` at all. It is a
  literal string used once, in `crates/errors/src/lib.rs`'s own unit test
  `bundle_counts_error_severity_only`, purely to exercise
  `DiagnosticBundle::error_count()`'s severity filtering. It is not a code
  the compiler ever emits. It is included here — and in the frozen set —
  because the task that produced this table defined "frozen" as "whatever
  the grep above returns", and excluding it would make the checker test
  diverge from its own specification. If this test literal is ever changed,
  update this table in the same commit (that is the "renumbering fails"
  contract in practice, not just in principle).

Nothing here blocks freezing: a dormant or unreachable code is still a valid,
stable reservation. But a consumer should not assume every documented code is
observable in practice today.

## Code table

### `FRS-LEX-*` — Lexer (1 code)

| Code | Stage | Meaning | Raised at |
|---|---|---|---|
| `FRS-LEX-0001` | `lexer` (via `Stage::Parser` in practice — see caveat) | Lexer encountered an invalid token sequence. | `crates/parser/src/lib.rs:1926` (`parser_code_for_lex_parse_error`); currently unreachable, see caveat above. |

### `FRS-PARSE-*` — Parser (3 codes)

| Code | Stage | Meaning | Raised at |
|---|---|---|---|
| `FRS-PARSE-0001` | `parser` | Parser encountered an unexpected token. | `crates/parser/src/lib.rs:1917` (default case), `:1927` (`LexParseError::ParseError`) |
| `FRS-PARSE-0002` | `parser` | Parser recovered from an error and emitted recovery diagnostics (warning/remark severity). | `crates/parser/src/lib.rs:1913` |
| `FRS-PARSE-0003` | `parser` | Parser encountered an invalid literal form. | `crates/parser/src/lib.rs:1915` |

### `FRS-SRC-*` — Source reader (3 codes, currently unused)

| Code | Stage | Meaning | Raised at |
|---|---|---|---|
| `FRS-SRC-0001` | `source_reader` | Source reader I/O failure. | Defined `crates/errors/src/codes.rs:9`; never constructed — see caveat. |
| `FRS-SRC-0002` | `source_reader` | Imported file could not be resolved. | Defined `crates/errors/src/codes.rs:11`; never constructed — see caveat. Real unresolved imports raise `CompilerError::Import` with no diagnostics bundle instead. |
| `FRS-SRC-0003` | `source_reader` | Import graph contains a cycle. | Defined `crates/errors/src/codes.rs:13`; never constructed — see caveat. |

### `FRS-EVAL-*` — Box evaluation (8 codes)

| Code | Stage | Meaning | Raised at |
|---|---|---|---|
| `FRS-EVAL-0001` | `eval` | `process` definition is missing. | `crates/eval/src/error.rs:403` |
| `FRS-EVAL-0002` | `eval` | Symbol lookup failed during eval (undefined symbol). | `crates/eval/src/error.rs:433` |
| `FRS-EVAL-0003` | `eval` | Arity mismatch detected during eval (e.g. too many arguments). | `crates/eval/src/error.rs:471,488` |
| `FRS-EVAL-0004` | `eval` | Invalid iteration construct detected during eval. | `crates/eval/src/error.rs:658` |
| `FRS-EVAL-0005` | `eval` | Symbol redefined with a different value in the same lexical scope. | `crates/eval/src/error.rs:620` |
| `FRS-EVAL-0006` | `eval` | Slider/numentry init value is outside the `[min, max]` range. | `crates/eval/src/error.rs:692` |
| `FRS-EVAL-0099` | `eval` | Generic eval failure fallback code (covers eval-error variants without a dedicated code). | `crates/eval/src/error.rs` (multiple sites, e.g. `:508,517,530,539,554,584,592,603,646,669,704`) |
| `FRS-EVAL-0100` | `eval` | Not a real compiler code — synthetic literal used only in `errors` crate's own unit test to exercise severity-filtered counting. See caveat above. | `crates/errors/src/lib.rs:310` (test-only) |

### `FRS-PROP-*` — Box-to-signal propagation (5 codes)

| Code | Stage | Meaning | Raised at |
|---|---|---|---|
| `FRS-PROP-0001` | `propagate` | Unsupported box node encountered in propagate. | `crates/propagate/src/error.rs:227,436` |
| `FRS-PROP-0002` | `propagate` | Arity mismatch in propagate composition rules (`seq`/`split`/`merge`/UI wiring). | `crates/propagate/src/error.rs:235,247,268,301,398,406,414` |
| `FRS-PROP-0003` | `propagate` | Recursion/projection contract mismatch in propagate (`rec` arity/alias). | `crates/propagate/src/error.rs:339` |
| `FRS-PROP-0004` | `propagate` | Automatic differentiation (`fad`/`rad`) reached a clock-domain boundary it cannot cross. | `crates/propagate/src/error.rs:548` |
| `FRS-PROP-0099` | `propagate` | Generic propagate failure fallback code. | `crates/propagate/src/error.rs:372,380,390,422` |

### `FRS-COMP-*` — Top-level compiler pipeline (4 codes)

| Code | Stage | Meaning | Raised at |
|---|---|---|---|
| `FRS-COMP-0001` | `compiler` | Parse stage failed in top-level compiler pipeline. | Defined `crates/errors/src/codes.rs:81`; never constructed — see caveat. |
| `FRS-COMP-0002` | `compiler` | Eval stage failed in top-level compiler pipeline. | Defined `crates/errors/src/codes.rs:83`; never constructed — see caveat. |
| `FRS-COMP-0003` | `compiler` | Propagate stage failed in top-level compiler pipeline. | Defined `crates/errors/src/codes.rs:85`; never constructed — see caveat. |
| `FRS-COMP-0004` | `compiler` | Signal type validation failed in top-level compiler pipeline (`sigtype`/interval checks after propagation). | `crates/compiler/src/error_mapping.rs:142` (`type_error_to_compiler`); reachable, e.g. `tests/corpus/rep_74_soundfile_basic.dsp` (out-of-range soundfile part number). |

### `FRS-FIR-*` — FIR verifier (2 codes)

| Code | Stage | Meaning | Raised at |
|---|---|---|---|
| `FRS-FIR-0001` | `fir` | FIR verifier error diagnostic (fatal; details in notes as `fir_code=...`). | `crates/compiler/src/json_naming.rs:27`; currently unreachable from any known DSP input — see caveat. |
| `FRS-FIR-0002` | `fir` | FIR verifier warning diagnostic (details in notes as `fir_code=...`); promoted to fatal under `--fir-verify-strict`. | `crates/compiler/src/json_naming.rs:28`; reachable, e.g. a DSP whose generated FIR contains a constant-zero division warning (`fir_code=FIR-B04`) combined with `--fir-verify-strict`. |

### `FRS-SFIR-*` — Signal-to-FIR lowering (8 codes)

| Code | Stage | Meaning | Raised at |
|---|---|---|---|
| `FRS-SFIR-0001` | `transform` | Invalid options passed to signal→FIR lowering. | `crates/compiler/src/json_naming.rs:51` |
| `FRS-SFIR-0002` | `transform` | Empty signal list provided to signal→FIR lowering. | `crates/compiler/src/json_naming.rs:52` |
| `FRS-SFIR-0003` | `transform` | Signal outputs arity mismatch in signal→FIR lowering. | `crates/compiler/src/json_naming.rs:53` |
| `FRS-SFIR-0004` | `transform` | Unsupported signal node in signal→FIR lowering. | `crates/compiler/src/json_naming.rs:54`; reachable, e.g. `tests/corpus/err_fad_rad_temporal.dsp`. |
| `FRS-SFIR-0005` | `transform` | Unsupported binary operator in signal→FIR lowering. | `crates/compiler/src/json_naming.rs:55` |
| `FRS-SFIR-0006` | `transform` | Input index out of range in signal→FIR lowering. | `crates/compiler/src/json_naming.rs:56` |
| `FRS-SFIR-0007` | `transform` | Clocked node (`ondemand`/`upsampling`/`downsampling`) reached signal→FIR lowering before the clock-domain back half is ported. | `crates/compiler/src/json_naming.rs:57` |
| `FRS-SFIR-0008` | `transform` | Clock-environment inference / hierarchical-graph validation failed. | `crates/compiler/src/json_naming.rs:58` |

## The no-bundle fallback (`code: null`)

Some `CompilerError` variants carry no `DiagnosticBundle` at all — backend
codegen failures (`Codegen`, `CodegenC`, `CodegenJulia`, `CodegenInterp`,
`CodegenWasm`) and source/import failures (`Import`, `MissingRoot`). None of
the codes in this table apply to them. Under `--error-format json`,
`crates/compiler/src/cli/diagnostics.rs::format_fallback_diagnostics_json`
still emits a single-diagnostic envelope for these so stdout is always valid
JSON (D1), but with `"code": null` instead of a real `FRS-*` code — this is
intentional, not an omission from this table, and consumers should treat
`code == null` as "unstructured legacy error text" rather than look it up
here.

## Where this is enforced

- `crates/compiler/src/cli/tests.rs::frozen_frs_code_table_matches_source` —
  re-runs the extraction grep and diffs it against the set documented above;
  fails on an undocumented new code or a renumbered existing one.
- `crates/errors/src/codes.rs`'s own `all_codes_follow_stable_format` /
  `all_codes_are_unique` unit tests check the format/uniqueness invariants of
  the *registered* subset (`codes::all_codes()`), which is a strict subset of
  this table (it excludes the test-only `FRS-EVAL-0100` literal).
