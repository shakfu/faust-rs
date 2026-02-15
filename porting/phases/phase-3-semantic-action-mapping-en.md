# Phase 3 Parser Semantic Action Mapping (C++ -> Rust)

## 1. Purpose

This document is the Gate B remaining step 4 artifact:
- maps touched C++ parser semantic actions to Rust `parser-proto` actions,
- records whether mapping is `1:1` or `adapted`,
- links each family to structural parity checks.

Source of truth (C++):
- `/Users/letz/Developpements/RUST/faust/compiler/parser/faustparser.y`

Rust implementation:
- `crates/parser-proto/src/grammar/faustparser.y`
- `crates/parser-proto/src/lib.rs`
- `crates/boxes/src/lib.rs`

## 2. Mapping Table (Touched Families)

| Family | C++ action reference | Rust action reference | Mapping status | Structural check |
|---|---|---|---|---|
| Program root + formatted definitions | `program: stmtlist { gGlobal->gResult = formatDefinitions($$); }` | `Program -> StmtList`, `ParseState::format_definitions`, `ctx.set_parse_result` | `adapted` (explicit context) | `crates/parser-proto/tests/parser_slice1.rs` |
| Definition / rec definition + def/use properties | `definition`, `recinition`, `setDefProp`, `setUseProp` | `Definition`, `RecDefinition`, `mark_def_at_cursor`, `ident_from_token(..., mark_use=true)` | `1:1` behavior | `crates/parser-proto/tests/parser_slice1.rs`, `crates/parser-proto/tests/parser_ctx.rs` |
| Statement side effects (`import`/`declare`) | `importFile`, `declareMetadata`, `declareDefinitionMetadata` | `import_statement`, `declare_metadata_from_token`, `declare_definition_metadata_from_tokens` | `adapted` (recorded in `ParserCtx`) | `crates/parser-proto/tests/parser_slice4.rs` |
| Doc/listing side effects | `declareDoc`, listing switches | `doc_statement`, `note_doc_*`, `set_lst_*` | `adapted` (tracked in `ParserCtx`) | `crates/parser-proto/tests/parser_slice5_doc.rs` |
| Expression composition | `boxPar/boxSeq/boxSplit/boxMerge/boxRec` | same constructors in grammar actions | `1:1` | `crates/parser-proto/tests/parser_slice2.rs` |
| Local/recursive scopes | `boxWithLocalDef`, `boxWithRecDef` | same constructors; `format_definitions` bridge | `1:1` | `crates/parser-proto/tests/parser_slice6_scope_modules.rs`, `crates/parser-proto/tests/parser_semantic_parity.rs` |
| Infix lowering | `boxSeq(boxPar(a,b), boxOp())` pattern | `ParseState::binary_prim` / `postfix_prim` | `1:1` formula | `crates/parser-proto/tests/parser_slice2.rs`, `crates/parser-proto/tests/parser_semantic_parity.rs` |
| Application + access | `buildBoxAppl`, `boxAccess` | `ParseState::apply_box`, `access_box` | `1:1` (reversed arglist preserved) | `crates/parser-proto/tests/parser_slice2.rs`, `crates/parser-proto/tests/parser_semantic_parity.rs` |
| Primitive zero-arg families | `boxPrefix`, `boxIntCast`, `boxFloatCast`, `boxReadOnlyTable`, `boxWriteReadTable`, `boxSelect2`, `boxSelect3`, `boxAssertBound`, `boxLowest`, `boxHighest`, `boxAttach`, `boxEnable`, `boxControl` | matching `boxes::box_*` constructors in `Primitive` | `1:1` | `crates/parser-proto/tests/parser_slice10_primitives.rs`, `crates/parser-proto/tests/parser_semantic_parity.rs` |
| Foreign forms/signatures | `ffunction`, `boxFConst`, `boxFVar`, signature list encoding | `box_foreign_function`, `box_fconst`, `box_fvar`, `foreign_name_slots`, `foreign_signature` | `1:1` encoding | `crates/parser-proto/tests/parser_slice7_foreign.rs`, `crates/parser-proto/tests/parser_semantic_parity.rs` |
| CASE + rule preparation | `boxCase(checkRulelist(...))`, `prepareRule(s)` | `box_case_checked`, `prepare_pattern` (arity check + pattern var rewrite) | `adapted` (explicit helper) | `crates/parser-proto/tests/parser_slice8_case.rs`, `crates/parser-proto/tests/parser_semantic_parity.rs` |
| Module/waveform/route | `boxComponent`, `boxLibrary`, `boxEnvironment`, `boxWaveform`, `boxRoute` + fake route default | matching constructors + `route_box_default_spec`, `waveform_box_from_ctx` | `1:1` behavior | `crates/parser-proto/tests/parser_slice6_scope_modules.rs`, `crates/parser-proto/tests/parser_semantic_parity.rs` |
| UI families | `boxButton`, `boxCheckbox`, `boxVSlider`, `boxHSlider`, `boxNumEntry`, `boxV/HBargraph`, `boxV/H/TGroup`, `boxSoundfile` | matching `boxes::box_*` constructors | `1:1` | `crates/parser-proto/tests/parser_slice3.rs`, `crates/parser-proto/tests/parser_slice9_lambda_groups.rs` |
| Iterative and wrappers | `boxIPar`, `boxISeq`, `boxISum`, `boxIProd`, `boxInputs`, `boxOutputs`, `boxOndemand`, `boxUpsampling`, `boxDownsampling` | matching `boxes::box_*` constructors | `1:1` | `crates/parser-proto/tests/parser_slice3.rs`, `crates/parser-proto/tests/parser_slice9_lambda_groups.rs`, `crates/parser-proto/tests/parser_semantic_parity.rs` |
| Lambda abstraction | `buildBoxAbstr(params, body)` | `ParseState::box_lambda` -> `boxes::build_box_abstr` | `1:1` | `crates/parser-proto/tests/parser_slice9_lambda_groups.rs`, `crates/parser-proto/tests/parser_semantic_parity.rs` |

## 3. Structural Differential Strategy

Current structural parity gate is implemented as:
1. C++ action formulas encoded as Rust structural expectations (`dump_box` and `is_box_*` predicates).
2. Consolidated semantic parity corpus:
   - `crates/parser-proto/tests/parser_semantic_parity.rs`.
3. C++ acceptance envelope for the stable semantic corpus:
   - same test file (`semantic_shape_corpus_is_accepted_by_cpp_reference`),
   - plus class-level differential harness in `crates/parser-proto/tests/cpp_differential.rs`.

Notes:
- Structural comparison is shape-based (Tree/Box form), never pointer-identity based.
- Cases known to trigger later compilation-stage rejections in C++ are kept in Rust structural tests but are excluded from strict C++ acceptance envelope checks.

## 4. Open Items (for full 100% semantic parity)

- Port remaining C++ action families not yet in the migrated grammar scope.
- Extend structural parity corpus to stdlib/import-heavy fixtures after full parser integration.
- When production parser replaces `parser-proto`, preserve this mapping and parity corpus (path updates only).
