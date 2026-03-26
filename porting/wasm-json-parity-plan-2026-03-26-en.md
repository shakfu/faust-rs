# WASM JSON Parity Plan

**Date:** 2026-03-26
**Status:** In progress
**Target crates:** `codegen`, `compiler`
**Primary backend module:** `codegen::backends::wasm`
**C++ provenance:** `architecture/faust/gui/JSONUI.h`, `compiler/generator/json_instructions.hh`, `compiler/generator/code_container.hh`, `compiler/generator/wasm/wasm_code_container.cpp`, `architecture/faust/gui/JSONUIDecoder.h`

---

## 1. Purpose

This document defines the parity plan for the JSON companion emitted by
`faust-rs -lang wasm`.

The current Rust WASM backend emits a minimal scaffold JSON. That is
insufficient for the standard Faust WASM wrappers and runtimes, which expect a
full JSON description matching the C++ compiler contract.

This plan isolates the JSON/runtime compatibility work from the broader WASM
backend bring-up so it can be implemented and validated as a dedicated track.

Implementation note:

- the extracted generic JSON builder is now also used by the strict CLI
  `-json` / `--json` path
- this keeps the C++-style global JSON contract separate from the WASM-enriched
  companion JSON that carries runtime offsets

---

## 2. C++ Chain Summary

The C++ compiler builds the JSON through the following chain:

1. `CodeContainer::generateJSON(...)` in `compiler/generator/code_container.hh`
   prepares global metadata:
   - compiler version
   - compile options
   - library list
   - include pathnames
   - DSP struct size
   - memory layout
   - compute complexity

2. `generateUserInterface(visitor)` and `generateMetaData(visitor)` replay FIR
   UI and metadata instructions into `JSONInstVisitor`.

3. `JSONInstVisitor` in `compiler/generator/json_instructions.hh` translates
   FIR UI/meta instructions into `JSONUIReal` calls and builds the
   `varname -> full path` table used to attach memory offsets.

4. `JSONUIReal::JSON()` in `architecture/faust/gui/JSONUI.h` serializes the
   final JSON object with:
   - DSP identity fields
   - compilation metadata
   - DSP size and optional memory layout
   - `meta`
   - hierarchical `ui`
   - per-widget `address`, `shortname`, `varname`, and `index`

5. The C++ WASM backend in `compiler/generator/wasm/wasm_code_container.cpp`
   emits that JSON beside the `.wasm` file and also embeds it in a data
   segment.

6. Runtimes decode the JSON using `JSONUIDecoder` in
   `architecture/faust/gui/JSONUIDecoder.h`.

---

## 3. Runtime-Critical Fields

The standard Faust WASM wrappers and decoders rely on the following fields as
operational inputs, not just metadata:

- `size`
  DSP struct size in bytes. JS wrappers use it as the start of the audio heap.

- `inputs`
- `outputs`

- `sr_index`
  Optional sample-rate field offset used by native decoders.

- `meta`
  Top-level metadata entries.

- `ui`
  Hierarchical UI tree.

- `ui[*].index`
  Byte offset in DSP memory for each control/bargraph/soundfile zone.

- `ui[*].address`
  Canonical path used by JS wrappers and host-side parameter lookup.

- `ui[*].init`, `min`, `max`, `step`
  Used to reset controls and build host-side UI.

The following fields are not always required to boot a wrapper, but are part of
the C++ contract and should be emitted for parity:

- `name`
- `filename`
- `version`
- `compile_options`
- `library_list`
- `include_pathnames`
- `memory_layout`
- `compute_cost`
- `sha_key`
- `code`

---

## 4. Current Rust Gap

The current Rust backend emits a scaffold JSON roughly of the form:

```json
{
  "name": "...",
  "backend": "wasm",
  "scaffold": true,
  "double_precision": ...,
  "internal_memory": ...,
  "inputs": ...,
  "outputs": ...
}
```

This diverges from C++ in several critical ways:

- no `ui`
- no `meta`
- no `size`
- no `index` offsets for controls
- no `filename`
- no `compile_options`
- no `library_list`
- no `include_pathnames`
- no `sr_index`

As a consequence, the standard Node/WebAudio wrappers fail as soon as they try
to parse `json.ui` or use control offsets.

---

## 5. Rust Design Direction

The Rust implementation should not hand-assemble ad hoc JSON strings in the CLI
layer. It should follow the same semantic split as C++:

1. derive JSON-relevant information from canonical FIR and compile context
2. reconstruct the hierarchical UI tree from FIR UI instructions
3. resolve each widget `varname` to a concrete WASM memory offset
4. serialize one stable JSON object from a typed Rust representation

This keeps the JSON contract owned by the WASM backend, where the final memory
layout is known.

---

## 6. Proposed Rust Components

### 6.1 `WasmJsonDescription`

Add one typed backend-local description struct in
`crates/codegen/src/backends/wasm/` holding:

- identity:
  - `name`
  - `filename`
  - `version`
  - `compile_options`
  - `library_list`
  - `include_pathnames`
  - `sha_key`
  - `code`

- runtime fields:
  - `size`
  - `inputs`
  - `outputs`
  - `sr_index`

- optional extended fields:
  - `memory_layout`
  - `compute_cost`

- payload fields:
  - `meta`
  - `ui`

Serialization can initially stay manual for output stability, but the
intermediate representation should be typed.

### 6.2 FIR JSON builder

Add one FIR walker that reads:

- `metadata(...)` function body
- `buildUserInterface(...)` function body
- `instanceResetUserInterface(...)` function body if needed for validation

and reconstructs:

- top-level metadata entries
- UI group nesting
- widget kind, label, varname, ranges
- per-widget attached metadata declarations

This is the Rust analogue of `JSONInstVisitor + JSONUIReal`.

### 6.3 Widget offset resolver

Map FIR UI `varname` strings to `WasmMemoryLayout.field_offsets` entries.

This resolver is the critical parity point for:

- `ui[*].index`
- `getParamValue`
- `setParamValue`
- `instanceResetUserInterface`
- host-side control proxying

Unsupported cases must fail explicitly. Silent omission is not acceptable for
runtime compatibility.

### 6.4 Compiler-side context carrier

The backend currently only receives `FirStore`, `FirId`, and `WasmOptions`.

To emit C++-parity JSON fields such as `filename`, `compile_options`,
`library_list`, and `include_pathnames`, extend the compiler/backend boundary
with explicit compile metadata, for example:

- source filename
- compilation options string
- import search paths
- aggregated library list if available

This should be an explicit adapted API change, documented as such.

---

## 7. Implementation Steps

### Step 1. Introduce typed WASM JSON model

Status: completed on 2026-03-26

Deliverables:

- backend-local Rust structs for JSON payload
- serializer producing stable field ordering
- unit tests for scalar serialization

Pass criteria:

- existing scaffold tests updated to the new JSON model
- no CLI behavior change yet beyond internal refactor

### Step 2. Build JSON from FIR metadata and UI instructions

Status: completed on 2026-03-26

Deliverables:

- FIR walker for `metadata`
- FIR walker for `buildUserInterface`
- group nesting reconstruction
- support for:
  - `OpenBox`
  - `CloseBox`
  - `AddButton`
  - `AddSlider`
  - `AddBargraph`
  - `AddMetaDeclare`
  - `AddSoundfile`

Pass criteria:

- targeted tests using existing FIR fixtures with UI/meta
- emitted `ui` tree matches expected structure, labels, ranges, and metadata
- root `name` / `filename` honor top-level metadata declarations when present

### Step 3. Resolve widget `index` from WASM memory layout

Status: mostly completed on 2026-03-26

Deliverables:

- `varname -> field offset` resolution
- correct `ui[*].index`
- `size = memory_layout.struct_size`
- `sr_index` from `fSampleRate` field when present

Pass criteria:

- tests asserting `index` offsets for slider/bargraph fixtures
- `getParamValue` and `setParamValue` remain consistent with emitted JSON
- `size` and `sr_index` are derived from the actual WASM memory layout

### Step 4. Extend compiler/backend API with compile context

Status: in progress

Deliverables:

- explicit Rust carrier for source filename and compile metadata
- wire it from `compiler` into WASM codegen
- emit:
  - `filename`
  - `version`
  - `compile_options`
  - `include_pathnames`
  - `library_list`

Pass criteria:

- compiler tests assert JSON contains provenance fields
- no regression in `-lang wasm` binary emission

Current implementation note:

- a dedicated Rust JSON-context carrier now exists on the WASM backend boundary
- file-backed compilation now feeds:
  - `filename`
  - `version`
  - `include_pathnames`
  - `library_list`
- `compile_options` remains deferred until Rust has one explicit compiler-side
  source of truth matching the C++ `printCompilationOptions1()` contract
- a strict C++-style CLI `-json` / `--json` path is now wired through the
  compiler facade and reuses the generic FIR JSON builder without WASM widget
  `index` offsets

### Step 5. Remove runtime-incompatible scaffold assumptions

Status: in progress

Deliverables:

- remove `scaffold`-style JSON contract
- ensure `instanceResetUserInterface` lowering is present when FIR body exists
- align JSON `size` and control offsets with actual DSP memory layout

Pass criteria:

- generated JSON is consumable by the standard Faust Node/WebAudio wrapper on a
  representative UI DSP

### Step 6. Differential validation against C++

Status: not started

Deliverables:

- compare Rust vs C++ JSON on a focused corpus:
  - no-UI DSP
  - slider/button DSP
  - bargraph DSP
  - soundfile DSP
  - nested groups and metadata

Pass criteria:

- structural parity for required runtime fields
- documented accepted differences for non-critical fields, if any

---

## 8. Validation Matrix

At minimum, each implementation step should validate:

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p codegen wasm_`
- `cargo test -p compiler wasm`

Additional parity/runnable checks:

- generate `.wasm` + `.json` with `faust-rs -lang wasm`
- instantiate with the standard Faust Node wrapper
- validate:
  - `parse_ui`
  - `getParams`
  - `setParamValue/getParamValue`
  - `instanceResetUserInterface`
  - `init`

Suggested DSP cases:

- `tests/corpus/rep_09_ui_slider.dsp`
- `tests/corpus/rep_56_noise_smoo_slider.dsp`
- `tests/impulse-tests/dsp/APF.dsp`

---

## 9. Risks and Decision Points

### 9.1 Embedded JSON data segment vs DSP-at-offset-0 ABI

The historical C++ backend writes JSON in a data segment at offset `0`, while
the runtime ABI also treats DSP memory as based at `0`. The surrounding Faust
tooling appears to rely on converting the JSON before using the DSP instance.

For Rust, this must be re-checked carefully before mirroring or changing the
behavior:

- if we keep the embedded JSON, document the exact ABI assumption
- if we drop it, document the deviation and keep file-emitted JSON parity

This is a parity-sensitive decision and should not be made silently.

### 9.2 Library/include provenance availability

The Rust compiler currently does not expose the exact same metadata bundle to
the backend that the C++ compiler uses. If some provenance fields cannot be
recovered yet, the gap must be documented explicitly and closed through an
API-carried compile context, not guessed locally in the backend.

### 9.3 UI metadata attachment order

`AddMetaDeclare` applies either to the current group or the current widget,
depending on visitation context. The Rust builder must preserve this ordering
rule exactly or wrappers and host tooling will observe metadata drift.

---

## 10. Definition of Done

The Rust WASM JSON work is considered operational for v1 when:

- `faust-rs -lang wasm` emits `.wasm` and `.json`
- the JSON is accepted by the standard Faust Node/WebAudio wrapper
- `parse_ui` succeeds on representative UI DSPs
- `getParamValue`, `setParamValue`, and `instanceResetUserInterface` behave
  consistently with the emitted JSON offsets
- required runtime fields match C++ semantics
- remaining non-critical JSON differences, if any, are documented in
  `porting/` and covered by tests
