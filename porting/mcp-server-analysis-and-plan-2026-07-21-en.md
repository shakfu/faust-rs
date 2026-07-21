# Exposing faust-rs as an MCP Server: Analysis and Plan

Date: 2026-07-21

Written in English to match the `porting/` convention; the originating
discussion was in French.

## Scope

The question is whether `faust-rs` should be exposed as an MCP (Model Context
Protocol) server, and if so, which API to expose to users.

This document does not assume the answer. Part 1 inventories what already
exists — both the MCP servers already connected in this environment and the
capabilities `faust-rs` actually holds — and measures the hard constraints
that any wrapper would face. Part 2 derives an API from that inventory rather
than from the CLI flag list. Part 3 compares three implementation strategies.
Part 4 gives a phased plan with acceptance gates and rejecting mutations, per
the project's phase methodology. Part 5 records risks and open questions.

The short version of the conclusion, stated up front so the rest can be read
critically: **yes, but only for a narrow slice** — structured diagnostics,
compilation metadata, IR explanation, option comparison, and automatic
differentiation. Everything audio-runtime-shaped is already covered by other
servers and must not be duplicated.

---

## Part 1 — Analysis

### 1.1 The MCP landscape this server would join

Five Faust-related MCP servers are already connected in this environment.
Ignoring them would produce a redundant tool that competes for the model's
attention without adding capability.

| Server | Covers | Representative tools |
|---|---|---|
| `faustcode` | Session-based authoring loop: edit, submit, compile, run audio, inspect spectrum, MIDI, polyphony, library lookup | `submit`, `get_errors`, `run_audio`, `get_spectrum`, `search_faust_lib`, `get_faust_symbol`, `explain_faust_symbol_for_goal` |
| `faustnode` | Node/wasm runtime: compile to wasm, instantiate, drive params, metrics, MIDI | `compile`, `compile_and_start`, `get_dsp_json`, `set_param`, `get_audio_metrics` |
| `faustplugins` | Plugin graph composition (`par`/`seq` of plugins) | `create_par`, `create_seq`, `add_plugin` |
| `faustremote` | Remote compilation targets | `faustremote_compile`, `faustremote_targets_list` |
| `faust-mcp-app` | DSP generation, library search | `faust-generator`, `faust-libraries-search` |

Two observations follow.

**Observation A — the dynamic/audio side is saturated.** Compile-to-wasm,
instantiate, play, measure spectrum, drive parameters, MIDI: all present, in
two independent implementations. A `faust-rs` server must not re-enter this
space. Its complement is *static*: correctness, structure, explanation.

**Observation B — there is genuine overlap on errors, and it must be faced
honestly.** `faustcode.get_errors` already returns compilation errors. The
differentiator is not *that* `faust-rs` reports errors but *what shape* they
have. `faustcode` is backed by the reference compiler, whose diagnostics are
single-line strings. `faust-rs` emits a structured payload with stable codes,
multi-span labels with semantic roles, machine-readable computed facts, and
repair templates. That difference is the single strongest argument for this
server, and Section 1.2.1 quantifies it.

### 1.2 What faust-rs uniquely holds

#### 1.2.1 Structured diagnostics (the primary asset)

`--error-format json` already produces, for a single error, a payload
containing:

- a **stable code** — 34 by textual extraction, namespaced by pipeline stage:
  `FRS-LEX-*` (1), `FRS-PARSE-*` (3), `FRS-SRC-*` (3), `FRS-EVAL-*` (8),
  `FRS-PROP-*` (5), `FRS-COMP-*` (4), `FRS-FIR-*` (2), `FRS-SFIR-*` (8);
  of which 27 are actually constructible — see the correction in §1.4.5;
- **labels with semantic roles** — not just line/col but `definition_site` vs
  `call_site`, primary vs secondary style, with end positions;
- **computed facts** — e.g. `provided=3, expected_max=2, overflow=1`;
- **the rule that was violated**, stated as a predicate;
- **`binding_trace_path`** — the chain of definitions from `process` down to
  the failing one, e.g. `["process", "tst"]`;
- **`help` with a repair template** — e.g. `template: f(a, b); // keep
  provided args <= function input arity`.

A worked example from the session that motivated this document: a DSP
containing

```faust
gc = max(-gclip, min(gclip, fad(frame : specloss(prev), prev) : !, _));
```

fails because `,` binds looser than `:` inside an argument list, so `min`
receives three arguments. The JSON payload names the callee (`expr=min`),
gives `provided=3, expected_max=2`, points at both the definition site and the
call site, and supplies the repair template. That is enough for an agent to
fix the code without guessing. The human-readable string alone ("too many
arguments") is not — and indeed the file's own header comment misdiagnosed the
failure as a `fad`/`fft` interaction.

This is the asset. Everything else in this section is secondary to it.

#### 1.2.2 IR at every stage

`--dump-box`, `--dump-sig`, `--dump-fir`, `--dump-interp`, `--dump-cranelift`
expose each pipeline stage:
`parse -> eval -> propagate -> normalize/type/interval -> transform -> fir ->
backend`. No other server exposes any intermediate representation. This is
what makes "why is this expression expensive / why does it introduce delay /
why is it not vectorized" answerable rather than speculative.

It is also, as Section 1.4.1 shows, the capability most dangerous to expose
naively.

#### 1.2.3 Automatic differentiation (FAD/RAD)

`fad`/`rad` exist nowhere else in the Faust ecosystem. They are also the
feature where users most need help, because the output arity rule
(`body_outputs * (1 + n_differentiable_controls)`) is non-obvious and produces
downstream arity errors far from their cause. An agent that can ask "what is
the expected output arity of this `fad` expression" before writing the
consumer wiring avoids an entire error class.

#### 1.2.4 A verification pass

`--dump-fir-verify` runs the FIR verifier and reports without generating code;
`--fir-verify-strict` promotes warnings to errors; `--no-fir-verify` disables
it. A semantic check decoupled from codegen is directly useful as a "is this
DSP structurally sound" tool.

#### 1.2.5 A compilation option matrix

`--vec/--vs/--lv`, `--ss` (scheduling strategy 0..3+), `--mcd`/`--dlt` (delay
strategy thresholds), `--double`, plus nine backends (`asc`, `c`, `cpp`,
`cranelift`, `fir`, `interp`, `julia`, `rust`, `wasm`, `wast`). The
interesting capability is not compiling once but **compiling the same source
several ways and diffing the results** — a question no existing server can
answer.

#### 1.2.6 A validation harness

`crates/xtask` holds `golden`, `backend_align`, `lockstep_simd`,
`emission_determinism`, `structure_check`, `vector_coverage`, `p7_matrix`;
`crates/impulse-runner` holds the impulse-test oracle. Plus eight built-in FIR
fixtures (`sine_phasor`, `heavy_bench`, `passthrough`, `gain_bias_ui_meta`,
`table_state_delay`, `control_flow`, `math_intrinsics`, `ir_coverage`) and a
218-file DSP corpus.

This is real capability — but see Section 1.3, it belongs to a different
audience.

#### 1.2.7 Environment introspection

`--libdir`, `--includedir`, `--archdir`, `--dspdir`, `--pathslist` report
search paths. Minor, but it resolves the single most common class of "works on
my machine" failure (`stdfaust.lib` not found), which we hit in this very
session: `auto_wah.dsp` produced empty output for every dump mode purely
because the library path was unresolved.

### 1.3 Two audiences, and why only one should be served

The inventory above splits cleanly by audience.

**Audience A — DSP authors (and agents writing Faust for them).** Need:
correct code, understandable errors, structural explanation, working `fad`.
Served by 1.2.1, 1.2.2 (summarized), 1.2.3, 1.2.4, 1.2.5, 1.2.7.

**Audience B — faust-rs maintainers and porting agents.** Need: differential
testing against the C++ reference, corpus runs, raw IR, option matrices,
determinism gates. Served by 1.2.6 and raw 1.2.2.

Audience B is *already well served by the shell*. A Claude Code agent working
in this repository has `Bash`, the corpus, and `cargo xtask`. Wrapping those in
MCP tools adds a serialization layer, a token budget problem, and a
maintenance burden, in exchange for nothing that agent could not already do.

**Decision: the server targets Audience A only.** Audience B keeps the CLI and
`xtask`. This decision is what keeps the tool count small enough for the model
to use the server well; it should be revisited only if `faust-rs` development
moves to environments without repository shell access.

**This decision was challenged and re-examined; see Part 6**, which asks
directly whether such tooling helps improve the compiler itself. The
conclusion there refines this one rather than reversing it: the *tools* should
not be exposed to Audience B, but two of the *capabilities* they require are
compiler-improvement wins in their own right and should be built regardless of
whether the server ships.

### 1.4 Hard constraints, measured

#### 1.4.1 Token budget — the dominant constraint

Measured on this repository at `6f56dfbe`, release build:

| Source | Lines | `--dump-box` | `--dump-sig` | `--dump-fir` | `--dump-cpp` |
|---|---|---|---|---|---|
| trivial (`+~*(0.5) : *(hslider)`) | 1 | 302 B | 260 B | 5.3 KB | 2.5 KB |
| `fad_recursive_multi_control.dsp` | 1 | 598 B | 1.5 KB | 9.5 KB | 3.3 KB |
| `auto_chorus_stereo_fad_host.dsp` | 62 | 11 KB | **1.2 MB** | **5.0 MB** | **1.45 MB** |

A 62-line DSP — a perfectly ordinary one — produces a 5 MB FIR dump. That is
on the order of 1.2 M tokens: it does not fit in any context window, and even
the 1.2 MB signal dump would consume the entire budget.

Three consequences, all non-negotiable:

1. **No tool may ever return a raw IR dump unbounded.** The default response
   must be a *structured summary* (node counts by kind, depth, delay lines,
   state size, loop count), with full text available only under an explicit
   narrowing (`focus` on a sub-definition) that provably reduces size.
2. **Every tool needs `max_chars` with a signalled truncation contract** —
   the response must say it was truncated, by how much, and how to narrow.
3. **Designing the summarization is the actual engineering work** of this
   server. The MCP plumbing is a day; the IR summarizer is the project.

#### 1.4.2 The machine channel is not clean today

Measured behavior of `--error-format json`:

- the JSON payload is written to **stderr**, not stdout;
- it is **preceded by a plain-text line** (`C++ pipeline failed: ...`), so
  stderr is not parseable as JSON;
- stdout is empty on failure;
- exit code is 1 on failure, 0 on success.

A wrapper can strip the prefix, but that is a fragile contract to build a
public API on. **P0 of the plan must fix this in the CLI**, not in the
wrapper: a clean JSON-only stream, ideally on stdout, with the human prefix
suppressed under `--error-format json`.

#### 1.4.3 `timeout(1)` is unavailable; the slow-compile claim was wrong

The CLI has `--timeout` (default 120 s), but the macOS environment has **no
`timeout` binary** — a shell-based wrapper cannot rely on coreutils for the
outer bound and must kill the child process itself. That part stands.

**Correction (2026-07-21, during P0).** This section originally claimed that
`tests/corpus/ondemand_fft_roundtrip_id_016.dsp` exceeded 120 s of wall clock.
That measurement was wrong: the file compiles in ~0.2 s. The original timing
was taken while that corpus file carried an uncommitted working-tree
modification predating this analysis, and it conflated the compile with a
shell loop that also invoked a non-existent `timeout` binary. The full 218-file
corpus sweep run during P0 completed quickly with no outlier.

No slow-compile outlier has therefore been demonstrated. The wall-clock
ceiling of §2.0/§P1 is still worth having as a defensive measure — an
agent-facing tool must not block indefinitely — but it must not be justified by
this (retracted) measurement, and R4 below is downgraded accordingly.

The MCP server must impose its own wall-clock ceiling, well below the CLI
default. 15–20 s is the right order: an agent-facing tool that blocks for two
minutes is a broken tool.

#### 1.4.4 Source input model

MCP servers frequently run outside the user's filesystem (remote transport,
containerized). A path-only API breaks there. The server must accept **inline
source** plus an optional map of inline imports, materialize them into a
temporary directory, and compile that — with an *optional* `path` parameter
for the local case. This also makes the tools trivially testable.

#### 1.4.5 Stable codes become a public contract

The moment `FRS-EVAL-0003` is returned over MCP, it is an API. The code table
should be frozen and documented before exposure; adding codes is fine,
renumbering is not.

**Correction (2026-07-21, during P0).** The "34 codes" figure used throughout
this document is the output of `grep -rhoE 'FRS-[A-Z]+-[0-9]+'`, which
overcounts. Documenting each code in `docs/diagnostics-codes-en.md` established
that:

- `FRS-SRC-0001..0003` and `FRS-COMP-0001..0003` (6 codes) are declared as
  constants in `crates/errors/src/codes.rs` but **never constructed anywhere** —
  dead declarations;
- `FRS-EVAL-0100` is not an emitted code at all: it is a literal inside a unit
  test in `crates/errors/src/lib.rs:310` (`bundle_counts_error_severity_only`),
  captured only because the grep is textual;
- `FRS-LEX-0001` has a live call site but is unreachable from the CLI: the
  lexer's catch-all rule in `faustlexer.l` matches every byte, so lexical
  failures surface as `FRS-PARSE-0001` or via the no-bundle fallback instead.

So the real surface is **27 constructible codes, of which 26 are reachable
through the CLI** — not 34. The frozen table currently pins all 34 (matching
the extraction grep, so the test is self-consistent), but pinning dead and
test-only codes into a *public* contract is a mistake. Before any MCP exposure,
decide per code: delete the dead ones, or implement their raise sites.
Recorded as open question O4.

#### 1.4.6 Determinism

`emission_determinism` exists as an xtask gate, so determinism is already a
project value. It matters doubly here: an agent that gets different output for
identical input cannot converge. Any tool returning generated code must be
byte-stable for a fixed (source, options) pair.

### 1.5 What must not be exposed

Explicitly out of scope, with reasons:

- **Audio execution, spectrum, MIDI, parameter driving** — `faustnode` and
  `faustcode` own this; two implementations already exist.
- **`--dump-cpp-from-fbc`, `--fir-fixture`, `--list-fir-fixtures`** — backend
  bring-up tools, Audience B.
- **`xtask` harnesses, corpus runs, golden generation** — Audience B.
- **`--dump-cranelift`** — experimental backend; exposing it invites bug
  reports on a moving target.
- **Arbitrary flag passthrough** — a `flags: string[]` escape hatch would make
  the tool schema meaningless and the server unversionnable. Options must be a
  closed, typed set.
- **Writing to the user's filesystem** — `-o/--output` must not be exposed;
  the server returns content, the client decides where it goes.

---

## Part 2 — Proposed API

Six tools. The temptation is to expose thirty (one per flag); that reliably
degrades model performance by flooding tool selection. Every tool below earns
its place by answering a question no other connected server can answer.

### 2.0 Shared conventions

**Source input** (all tools):

```jsonc
{
  "source": "process = ...;",        // inline source, preferred
  "path": "/abs/path.dsp",           // optional alternative
  "imports": { "mylib.lib": "..." }, // optional inline import map
  "import_dirs": ["/abs/libs"]       // optional, maps to -I
}
```

Exactly one of `source` / `path` is required.

**Options object** (closed set, shared by tools 2 and 4):

```jsonc
{
  "double": false,
  "vec": false, "vs": 32, "lv": 0,
  "scheduling_strategy": 0,
  "mcd": 16, "dlt": null,
  "class_name": "mydsp",
  "process_name": "process",
  "fir_verify": "on" | "off" | "strict"
}
```

**Response envelope** (all tools):

```jsonc
{
  "ok": true,
  "diagnostics": [ /* always present, may be empty; warnings included */ ],
  "truncated": { "field": "code", "returned": 20000, "total": 145372,
                 "hint": "narrow with focus=<definition>" },
  "timing_ms": 412
}
```

`diagnostics` is present on *success* too — warnings are the point.
`truncated` is absent when nothing was cut.

### 2.1 `faust_check` — the workhorse

```jsonc
{ "source": "...", "options": { ... }, "max_diagnostics": 20 }
```

Runs parse → eval → propagate → type → FIR verify. **No codegen.** Returns
the diagnostics array, each entry carrying: `code`, `severity`, `message`,
`labels` (with the *source excerpt inlined*, not just line/col — an agent
should not need a second round-trip to see the offending line), `notes`
(including the computed facts and the violated rule), `help`, and
`binding_trace_path`.

This is the tool an agent calls ten times per session. It must be fast (no
backend work), cheap (bounded output), and idempotent. If only one tool ships,
it is this one.

*Budget:* small by construction. Cap at `max_diagnostics` (default 20) and
truncate each `notes` array.

### 2.2 `faust_compile` — code plus metadata

```jsonc
{ "source": "...", "lang": "cpp", "options": { ... }, "max_chars": 40000 }
```

Returns generated code **and, more importantly, metadata**:

```jsonc
{
  "code": "...",
  "meta": {
    "inputs": 2, "outputs": 2,
    "ui": { /* the --json payload */ },
    "state_bytes": 4096,
    "max_delay_samples": 512,
    "sample_rate_dependent": true
  }
}
```

The metadata is frequently worth more to an agent than the code: "how many
inputs does this have", "what controls did I actually declare", "how much
delay did I introduce" are the questions that arise while *writing* Faust.
Consider allowing `lang: "none"` to return metadata only — cheap and often
sufficient.

`lang` restricted to `c | cpp | rust | wasm | julia | interp | fir` (excluding
`cranelift`, `asc`, `wast` per §1.5).

*Budget:* `code` truncated to `max_chars`; `meta` never truncated.

### 2.3 `faust_explain` — structured IR, summarized

```jsonc
{ "source": "...", "stage": "box" | "signal" | "fir",
  "focus": "myfilter", "detail": "summary" | "full", "max_chars": 20000 }
```

`detail: "summary"` (the **default**, and the only mode allowed when the full
dump exceeds budget) returns:

```jsonc
{
  "stage": "fir",
  "summary": {
    "node_count": 18432,
    "by_kind": { "BinOp": 5100, "Delay": 42, "Load": 3200, ... },
    "max_depth": 87,
    "delay_lines": [ { "name": "...", "max_delay": 512, "strategy": "ring" } ],
    "state": { "int_slots": 12, "float_slots": 2048, "tables": 2 },
    "loops": 4,
    "estimated_full_chars": 4966941
  }
}
```

`detail: "full"` is permitted only when `focus` narrows the dump below
`max_chars`; otherwise the tool returns the summary plus an explicit
`truncated` block naming the definitions worth focusing on.

This is the tool that makes structural questions answerable, and per §1.4.1 it
is where the engineering effort concentrates. Ship it *after* 2.1/2.2, once
real usage shows which summary fields matter.

### 2.4 `faust_compare_options` — variant diffing

```jsonc
{ "source": "...", "lang": "cpp",
  "variants": [ { "label": "scalar", "options": {} },
                { "label": "vec32", "options": { "vec": true, "vs": 32 } } ] }
```

Compiles each variant and returns a **structured comparison**, never the raw
sources to diff by hand:

```jsonc
{ "rows": [ { "label": "scalar", "code_chars": 3317, "loops": 1,
              "state_bytes": 4096, "vectorized": false, "compile_ms": 210 },
            { "label": "vec32", "code_chars": 5901, "loops": 3,
              "state_bytes": 4096, "vectorized": true, "compile_ms": 480 } ],
  "structural_diff": [ "vec32 adds 2 loops", "identical state layout" ] }
```

Answers "does `-vec` help here", "does `-ss 1` change the schedule", "does
`--double` change the structure". Cap `variants` at 4 — the wall-clock ceiling
of §1.4.3 applies to the *sum*.

### 2.5 `faust_autodiff` — FAD/RAD with arity guidance

```jsonc
{ "source": "...", "expr": "specloss(prev)", "seed": "prev",
  "mode": "forward" | "reverse" }
```

Wraps `fad`/`rad` and returns, before anything else, **the expected output
arity** and the wiring the caller must supply:

```jsonc
{
  "differentiable_controls": ["fb", "vol"],
  "output_arity": { "body_outputs": 1, "controls": 2, "total": 3,
                    "formula": "body_outputs * (1 + n_controls)" },
  "suggested_wiring": "(fad(expr, seed) : !, _, _)",
  "generated_source": "..."
}
```

The `suggested_wiring` field is the direct antidote to the failure class that
motivated this document. Note the parenthesization in the suggestion: it must
be emitted parenthesized, because unparenthesized `: !, _` inside an argument
list re-associates as extra arguments (§1.2.1).

### 2.6 `faust_diagram` — block diagram as SVG

```jsonc
{ "source": "...", "focus": "myfilter", "fold": 25, "scaled": true }
```

Wraps `--svg` (+ `-f/-fc/-mns/-sc`). Returns the SVG inline. A multimodal
model can *read* a block diagram, and for routing/topology questions it is
often the fastest path to understanding — one image instead of 11 KB of box
IR. Cheap to implement since the renderer exists.

*Budget:* SVG is compact for folded diagrams; enforce `fold` to keep it so.

### 2.7 Tools deliberately not proposed

- `faust_paths` (wrapping `--pathslist` etc.) — fold this into the
  `diagnostics` of `faust_check` instead: when an import fails to resolve,
  the diagnostic should already carry the searched paths. A separate tool
  would be a symptom of a weak error message.
- `faust_verify` — subsumed by `faust_check` with `fir_verify: "strict"`.
- `faust_corpus` / `faust_fixture` — Audience B (§1.3).

---

## Part 3 — Implementation strategy

Three options, with the trade-off that matters for each.

### 3.1 Shell out to the `faust-rs` binary (TypeScript or Rust server)

*Pros:* zero coupling to internal crate APIs; the binary is already the
tested surface; trivial to keep in sync with CLI changes.
*Cons:* one process spawn per call (tens of ms, acceptable); must parse
stdout/stderr, which is fragile until §1.4.2 is fixed; **cannot produce the IR
summaries of §2.3 without re-parsing megabytes of dump text** — that is the
blocker.

### 3.2 In-process Rust server linking `crates/compiler`

`crates/compiler` already exposes a `Compiler` facade with box/signal/FIR dump
helpers, and the workspace has an established FFI layer (`faust-ffi`,
`box-ffi`, `signal-ffi`, `interp-ffi`, `wasm-ffi`).

*Pros:* the summarizer of §2.3 can walk the **IR structures directly** instead
of their text rendering — this is the difference between a 5 MB string
reduction and a cheap traversal; no serialization round-trip; wall-clock
control via threads.
*Cons:* couples the server to internal APIs that are still moving; a compiler
panic takes the server down (must run compilation on a supervised thread or
child process).

### 3.3 Hybrid — recommended

Shell out for tools 2.1, 2.2, 2.5, 2.6 (all of which consume bounded,
already-structured output: JSON diagnostics, generated code, the `--json`
description, SVG). Link `crates/compiler` in-process for 2.3 and 2.4, where
structural traversal is the whole value.

This lets M1 ship on the binary alone while the in-process path is developed
behind the same tool schema — the client never observes the difference.

**Language:** Rust, in-workspace, as `crates/mcp-server`. Reasons: the
hybrid's second half requires linking the workspace anyway; the option/flag
model can be shared with the CLI instead of re-declared; and it keeps the
schema and the compiler versioned together. A TypeScript server would be
faster to write for M1 and strictly worse from M2 onward.

---

## Part 4 — Phased plan

Following the project's phase methodology: each phase pairs a **producer**
with an **independent checker**, and lands **rejecting mutations** proving the
checker actually rejects. No phase is qualified without them.

### P0 — Make the machine channel clean (prerequisite, CLI-side)

Not MCP work; the server cannot be built on the current stderr contract.

- Under `--error-format json`, emit the payload on **stdout**, alone, with no
  human-readable prefix; keep stderr for genuine out-of-band failures.
- Add `--check` (or `--emit check`): run the full front-end + FIR verify, no
  codegen, exit 0/1, always emit a diagnostics payload (empty array on
  success) so success and failure share one schema.
- Freeze and document the `FRS-*` table (§1.4.5) in
  `docs/diagnostics-codes-en.md`.

*Checker:* a test asserting stdout parses as JSON with no leading bytes, for
one success and one failure case per stage namespace (LEX/PARSE/SRC/EVAL/
PROP/COMP/FIR/SFIR).
*Rejecting mutations:* (a) reintroduce the `C++ pipeline failed:` prefix →
checker must fail; (b) route the payload back to stderr → must fail;
(c) renumber one `FRS-*` code → the frozen-table test must fail.

*Gate:* stdout is byte-exactly a JSON document for every corpus file, success
and failure alike.

**Status: DONE, 2026-07-21.** Implemented in `crates/compiler/src/cli/`
(`args.rs`, `diagnostics.rs`, `runner.rs`, `tests.rs`), with
`docs/diagnostics-codes-en.md` and `crates/compiler/tests/
cli_diagnostics_channel.rs` (11 subprocess tests). Gate met: 218/218 corpus
files emit a parseable JSON document on stdout under
`--check --error-format json`. All three rejecting mutations observed to fail
the checker (7/11 tests fall for mutation (b), independently re-verified).
Two design points settled during implementation:

- *stdout conflict rule* — on a successful dump-mode compile, generated output
  stays on stdout and no diagnostics payload is added; the CLI never emitted
  one on success outside `--check`, so there is nothing to interleave.
- *no-bundle fallback* — `CompilerError` variants that carry no
  `DiagnosticBundle` (backend codegen, import failures) now emit a
  `code: null` envelope of the same shape rather than nothing, so the schema
  is uniform across every failure path.

Two corrections to this document came out of the work: §1.4.3 (retracted
slow-compile measurement) and §1.4.5 (27 constructible codes, not 34).

### P1 — Minimum viable server: `check`, `compile`, `autodiff`

Create `crates/mcp-server`, shelling out to the binary (§3.1). Implement 2.1,
2.2, 2.5, plus the shared envelope and source-materialization of §2.0/§1.4.4.
Enforce the wall-clock ceiling in-process (§1.4.3) — no reliance on
`timeout(1)`.

*Checker:* schema-conformance tests over the 218-file corpus — every response
validates against the declared JSON schema, `truncated` is present exactly
when output was cut, `timing_ms` is always populated.
*Rejecting mutations:* (a) remove the `max_chars` cap → budget test must fail;
(b) return a path outside the temp dir → sandbox test must fail; (c) emit
`ok: true` alongside a non-empty error-severity diagnostic → consistency test
must fail; (d) drop the parentheses from `suggested_wiring` → the
round-trip test (feed the suggestion back through `faust_check`) must fail.

*Gate:* the `repro_fad_error.dsp` scenario is fixable by an agent using only
`faust_check` output, with no access to the file's comments.

### P2 — The IR summarizer

Link `crates/compiler` in-process (§3.2) and implement 2.3. This is the
largest phase; budget accordingly.

- Define the summary schema per stage (box / signal / FIR).
- Implement traversal-based summarization — **never** by post-processing the
  dump text.
- Implement `focus` narrowing with a *pre-computed* size estimate, so
  `detail: "full"` is refused before the dump is materialized rather than
  after.

*Checker:* an independent size oracle — for every corpus file and every stage,
assert the returned payload is under budget **and** that
`summary.estimated_full_chars` matches the actual dump length within 5 %.
*Rejecting mutations:* (a) make the summarizer render text then measure →
memory/latency test must fail on `auto_chorus_stereo_fad_host.dsp`;
(b) skew `estimated_full_chars` by 20 % → oracle must fail; (c) let `full`
through when focus does not narrow → budget test must fail.

*Gate:* `auto_chorus_stereo_fad_host.dsp` (5 MB FIR) returns a useful summary
in under 20 KB and under the wall-clock ceiling.

### P3 — Option comparison

Implement 2.4 on top of P2's structural traversal. Requires a stable
structural fingerprint (loops, state layout, vectorization status) — reuse
`xtask::structure_check` primitives rather than inventing a second notion of
structural identity.

*Checker:* known-answer tests — `-vec` on a DSP with a serial dependency must
report `vectorized: false`; `--double` must report identical loop structure
with changed state bytes; two identical variants must produce an empty
`structural_diff`.
*Rejecting mutations:* (a) make `structural_diff` always empty → must fail;
(b) compare rendered code size only → the identical-structure test must fail.

*Gate:* the comparison distinguishes at least the four axes `vec`, `ss`,
`double`, `mcd` on corpus DSPs where the difference is known.

### P4 — Diagram

Implement 2.6. Small; deliberately last among the tools because its value is
real but narrower than P1–P3.

*Checker:* SVG well-formedness + size bound across the corpus.
*Rejecting mutation:* disable `fold` → size bound must fail on a large DSP.

### P5 — Qualification and publication

- Tool-description tuning: the descriptions are what drive selection, and
  overlap with `faustcode.get_errors` (§1.1, Observation B) must be resolved
  *in the description text* — `faust_check` should say explicitly that it
  returns structured, machine-actionable diagnostics for automated repair,
  and that audio execution belongs to the other servers.
- End-to-end agent evaluation: a fixed set of ~20 broken DSPs (drawn from the
  corpus plus real errors like `repro_fad_error.dsp`), measuring fix rate
  with and without the server. This is the only honest measure of whether the
  server was worth building.
- Documentation: one page per tool with a worked example.

*Gate:* measured fix-rate improvement on the evaluation set. If it is not
materially better than `faustcode` alone, the server should be cut back to
`faust_check` only rather than shipped wide.

### Sequencing note

P0 → P1 is the minimum useful increment and is small (P0 is a day of CLI work,
P1 a few days). P2 is where the effort concentrates and should not be started
until P1 has produced real usage traces showing which summary fields agents
actually ask for. Building P2's schema from imagination is the most likely way
to waste the effort.

---

## Part 5 — Risks and open questions

**R1 — Overlap with `faustcode` is not fully resolvable by design alone.**
Both servers answer "is my DSP broken". Model selection between them will be
driven by tool descriptions, which is a weak lever. Mitigation: the P5
evaluation should measure selection accuracy, not just fix rate. Open
question: is there appetite to have `faustcode` delegate its `get_errors` to
`faust-rs` instead, collapsing the overlap? That would be strictly better than
two competing tools, and is worth asking before P1.

**R2 — faust-rs is a compiler under active development.** Its diagnostics and
IR are moving targets, and an MCP server freezes parts of them into a public
contract. Mitigation: version the tool schema explicitly; keep IR summaries
descriptive (counts, kinds) rather than committing to exact node taxonomies.

**R3 — Divergence from the reference compiler.** If `faust-rs` accepts or
rejects something the reference compiler does not, an agent using this server
will produce code that fails elsewhere. Mitigation: `faust_check` diagnostics
should eventually carry a confidence/parity marker for constructs known to
diverge. Open question: is the current parity level good enough to expose to
non-expert users, or should the server be positioned as a
`faust-rs`-development aid until parity is certified?

**R4 — Wall-clock outliers (downgraded 2026-07-21).** This risk originally
rested on a corpus DSP said to exceed 120 s; that measurement was wrong and has
been retracted (§1.4.3). No outlier is currently demonstrated, so the risk is
speculative rather than observed. The mitigation is still cheap and worth
keeping: return a structured `timeout` diagnostic naming the stage reached, so
that if an outlier does appear the agent learns something rather than nothing.

**R5 — Panic safety.** In-process linking (P2+) means a compiler panic can
take down the server. Mitigation: run compilation on a supervised thread with
`catch_unwind`, or keep a child-process boundary for the in-process path too
and pay the IPC cost.

**Open question O1 — transport and distribution.** Local stdio server (simple,
requires a local build) versus a hosted one (requires sandboxing untrusted DSP
source, since `-I` and imports touch the filesystem). This document assumes
local stdio; a hosted deployment needs its own security analysis and would
change §1.4.4 from a convenience into a requirement.

**Open question O4 — what to do with the 7 non-emitted codes (§1.4.5).**
Delete `FRS-SRC-0001..3` / `FRS-COMP-0001..3` as dead declarations, or
implement their raise sites? And should `FRS-EVAL-0100` (a test literal) be
excluded from the frozen table, which would mean the table is no longer the
output of a simple grep? Must be settled before the code table is published,
not after.

**Open question O2 — should `faust_compile` expose `wasm`?** It overlaps
`faustnode.compile` directly. Arguably yes for parity checking, arguably no
per §1.5. Deferred to P1 review.

---

## Part 6 — Does this kind of tooling help improve the compiler itself?

Part 1 set Audience B (faust-rs maintainers and porting agents) aside on the
grounds that the shell already serves them. That is a convenient conclusion
for keeping the tool count low, which is exactly why it deserves adversarial
scrutiny. This part asks the question directly and answers it on evidence.

The answer has two halves, and conflating them is the main risk:

- **As an MCP tool surface: mostly no.** Section 6.2 gives the reasons, and
  they are decisive.
- **As a forcing function for capabilities the compiler lacks: yes,
  substantially.** Section 6.3 identifies three, two of which are on the
  critical path of Part 4 anyway.

### 6.1 How compiler work is actually done today, measured

The existing maintainer-facing surface, as it stands at `6f56dfbe`:

| Workflow | Command | Output form |
|---|---|---|
| C++/Rust parity over the corpus | `xtask corpus-status-report` | 25 KB Markdown written to `porting/phases/` |
| Backend divergence | `xtask cpp-backend-diff-report`, `backend-full-corpus-diff-report` | 3.6 KB / 48 KB Markdown to `porting/phases/` |
| Parser parity | `xtask parser-parity-report` | 4 KB Markdown to `porting/phases/` |
| Golden snapshots | `xtask golden-check`, `golden-check-cpp` | pass/fail to stdout; 198 Rust goldens, 1.0 MB tree |
| Runtime alignment | `xtask backend-align-smoke`, `interp-trace-*` | pass/fail + trace files |
| Structural FIR scan | `xtask fir-dump-scan` | stdout |
| Numeric oracle | `crates/impulse-runner` | pass/fail |

Three properties of this surface matter for the question.

**(a) It is batch-and-file-mediated, not query-shaped.** Every parity workflow
regenerates a whole-corpus report and writes it to a file. To answer "did my
change fix case `foo`", an agent runs a full-corpus pass and diffs a Markdown
document. There is no way to ask about five cases.

**(b) The reports go stale silently.** `phase-4-corpus-status-diff-report-en.md`
is dated 2026-06-10 and reports `Total cases: 190`. The corpus now holds 218
`.dsp` files. An agent reading that file cold gets a six-week-old picture of a
corpus that has grown 15 %, with nothing in the document signalling staleness.

**(c) Expected divergence is not distinguished from regression.** The same
report lists 78 `ERR/OK` cases (C++ errors, Rust succeeds). Inspection shows
the bulk are `undefined symbol : fad` / `rad` — i.e. the reference compiler
lacks a feature `faust-rs` deliberately adds. Those are *by design*, not
failures, but the report classifies them identically to a genuine parity
break. An agent — or a human — reading it cold cannot tell the two apart
without knowing the project's history.

Properties (b) and (c) are pre-existing weaknesses that have nothing to do
with MCP. They surfaced only because asking "would a tool interface help"
forced an audit of what the current interface actually returns. That is worth
noting as a result in itself.

### 6.2 Why an MCP surface for Audience B is nonetheless the wrong answer

Four reasons, in decreasing order of weight.

**6.2.1 MCP's value is crossing a host boundary; in-repo work has none.**
The protocol earns its overhead when the consumer cannot reach the producer
directly — different machine, different process, no shell. An agent working in
this repository has `Bash`, the corpus, `cargo xtask`, and the source. Wrapping
local commands in a protocol adds serialization, a schema, and a server to
keep alive, in exchange for access that already exists.

**6.2.2 The xtask surface must stay fluid, and a schema would freeze it.**
The command list is explicitly phase-driven — `phase-3-parser-parity`,
`phase-4-corpus-status`, `phase-6-*-diff`, `p7_matrix`, `vector_coverage` —
and it changes as porting phases open and close. `crates/xtask/src/main.rs`
states the design invariant that "the command surface stays intentionally
simple". A published tool schema over that surface creates version drag on
precisely the layer that must be free to churn. Audience A's schema, by
contrast, is over the *language*, which is stable.

**6.2.3 The bottleneck in compiler work is knowledge, not access.** What makes
an agent effective here is knowing that cached `.ir` files produce false
greens, that structural certification is not numeric proof, that the coverage
check is blind to gains, that typed FIR walkers silently skip unknown node
kinds. That knowledge lives in the porting docs (one plan is 131 KB), the
journal, and `CLAUDE.md`. No tool call transmits it. The right vehicle is
documentation and skills, and effort spent on an MCP surface is effort not
spent there.

**6.2.4 It would double the maintenance of the validation harness.** Every
gate would need both its xtask path (for CI) and its tool path (for agents),
which either drift or force an abstraction layer neither wants.

### 6.3 What genuinely does help — the capabilities, not the tools

The productive reframing: separate *what the MCP design requires* from *the
MCP packaging*. Three required capabilities improve the compiler on their own
merits, and would be worth building even if the server is cancelled outright.

**C1 — The structural IR summarizer (P2) helps maintainers more than users.**

This is the strongest finding of Part 6, and it partially inverts §1.3.
Compiler work *is* IR reading: diagnosing a lowering bug, a scheduling
divergence, or a vectorization miss means looking at signal or FIR structure.
The measurements of §1.4.1 — 1.2 MB of signal IR and 5.0 MB of FIR IR from a
62-line DSP — are therefore a *maintainer's* problem at least as much as a
user's. Today the only options are to read a truncated dump or to grep it,
both of which lose structure.

A traversal-based summarizer answering "how many delay lines, what depth, how
much state, how many loops, which kinds dominate" — with `focus` narrowing to
one definition — is directly useful for compiler debugging. Crucially it must
live in the **library**, not the MCP server, so that `xtask`, tests, and the
CLI can all call it. Exposing it over MCP then costs nothing extra.

Corollary: P2 should be re-scoped as a `crates/`-level capability with an MCP
adapter, rather than as MCP-server work. That also derisks it, since it can be
validated by the existing test harness rather than through a protocol.

**C2 — A clean machine channel (P0) is a CI and harness win.**

§1.4.2 found that `--error-format json` writes to stderr behind a plain-text
prefix, so stderr is not parseable as JSON. That is not merely an MCP
blocker — it means *any* automated consumer (CI, the xtask reports, an IDE,
the impulse runner) must string-scrape. Fixing it, plus adding a `--check`
mode with a uniform success/failure schema, improves the harness independently
of MCP. The frozen `FRS-*` code table (§1.4.5) likewise gives CI something
stable to assert on, which it currently lacks.

**C3 — Query-shaped differential access, replacing batch reports.**

Property (a) of §6.1 is a real inefficiency: there is no way to ask "parity
status of these five cases, now". The fix is not an MCP tool but an xtask
mode that takes a case list and emits machine-readable status — with, per
(b) and (c), a generation timestamp, a corpus-size check, and an explicit
`expected-divergence` classification so that the 78 `fad`/`rad` cases stop
masquerading as parity breaks.

This is the cheapest of the three and arguably the highest immediate value,
because it fixes a report that is currently misleading.

### 6.4 A second-order benefit: dogfooding diagnostics

There is one way the *server itself* improves the compiler, and it is not
about capability but about feedback.

Diagnostic quality is normally unmeasured: an error message is written once,
judged by its author, and never evaluated against the population of people who
hit it. Routing real usage through an agent makes that population observable —
every case where an agent receives a diagnostic and still fails to fix the code
is a diagnostic-quality bug, with a reproducible trace.

The session that motivated this document is a live example. The compiler's
message was *correct*: `too many arguments: expected at most 2, got 3`, with
`expr=min` and both spans. Yet the DSP's own header comment attributed the
failure to a `fad`/`fft` interaction at `N > 8` — a confident misdiagnosis by
the human author. A correct message that readers routinely misread is a
diagnostic defect, and it is invisible without consumption data. Here the
missing element is proximate: the message never says *why* three arguments
were parsed, i.e. that `,` re-associated inside the argument list. A note
naming the precedence rule would likely have prevented the misdiagnosis.

The P5 evaluation set (§P5) is what turns this into a measurement rather than
an anecdote, and its results should feed diagnostics work directly.

### 6.5 Verdict

**Does an MCP tool help improve the compiler? Not as a tool surface — but the
work it forces does, and two of those items are on the Part 4 critical path
anyway.**

Concretely, this changes the plan as follows:

- **P0 is re-justified independently of MCP** (C2). It should be done even if
  the server is cancelled; it improves CI, the harness, and IDE integration.
- **P2 is re-scoped** (C1): build the IR summarizer as a library capability in
  `crates/`, callable from `xtask`, tests, CLI, and the MCP adapter alike. Do
  not build it inside the MCP server.
- **A new item C3 is added, independent of the server**: a query-shaped,
  machine-readable, staleness-aware, expected-divergence-classifying parity
  mode for `xtask`. Highest value-to-effort ratio of the three; recommended
  first, before P1.
- **No MCP tools are added for Audience B.** §1.3's decision stands.
- **The P5 evaluation set gains a second purpose**: it is the diagnostic
  quality instrument (§6.4), not only a go/no-go gate for the server.

**Open question O3 — is C3 worth doing before P1?** It has no dependency on
the MCP work, it fixes a report that is currently stale and misleading, and it
is the smallest of the three items. My recommendation is yes: do C3 first, as
a standalone improvement, and let it validate the claim that query-shaped
access beats batch reports before any protocol work begins.

## Appendix A — Measured data

Environment: macOS (Darwin 21.6.0), `faust-rs` release build at `6f56dfbe`,
2026-07-21.

- Backends available: `asc, c, cpp, cranelift, fir, interp, julia, rust, wasm,
  wast` (10 values; `cranelift` experimental).
- Diagnostic codes: 34 by textual extraction, across 8 stage namespaces — but
  only **27 constructible / 26 CLI-reachable**; see the correction in §1.4.5.
- Built-in FIR fixtures: 8.
- Corpus size: 218 `.dsp` files under `tests/corpus/`.
- `--timeout` default: 120 s. `timeout(1)` unavailable on this platform.
- `--error-format json`: payload on stderr, preceded by a plain-text line;
  stdout empty; exit 1.
- Dump sizes: see §1.4.1.
- Golden snapshots: 198 Rust cases, 1.0 MB tree under `tests/golden/`.
- Generated parity reports (`porting/phases/`): corpus-status 25 KB,
  backend-full-corpus-diff 48 KB, cpp-backend-diff 3.6 KB, parser-parity 4 KB.
- `phase-4-corpus-status-diff-report-en.md`: dated 2026-06-10, reports 190
  cases against a corpus that now holds 218; 78 `ERR/OK` entries, predominantly
  `undefined symbol : fad|rad` (expected divergence, unclassified as such).

## Appendix B — Tool summary

| Tool | Question answered | Phase | Budget risk |
|---|---|---|---|
| `faust_check` | "Why is my DSP broken and how do I fix it?" | P1 | low |
| `faust_compile` | "What does this compile to, and what is its interface?" | P1 | medium |
| `faust_autodiff` | "How do I wire this `fad`/`rad` expression?" | P1 | low |
| `faust_explain` | "What is the structure/cost of this DSP?" | P2 | **high** |
| `faust_compare_options` | "Does this compilation option help?" | P3 | medium |
| `faust_diagram` | "What is the topology of this DSP?" | P4 | low |
