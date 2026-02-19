# Phase 3 Parser Differential Triage (Rust vs C++)

## 1. Scope

Tracks parser-class mismatches from the differential harness:
- `crates/parser-proto/tests/cpp_differential.rs`
- Corpus inputs: `tests/corpus/*.dsp`
- Additional malformed fixtures embedded in the test.

Reference binary:
- `FAUST_CPP_BIN=/usr/local/bin/faust`

## 2. Latest Run

Command:

```bash
FAUST_CPP_BIN=/usr/local/bin/faust cargo test -p parser-proto --test cpp_differential -- --nocapture
```

Result:
- test status: pass
- parser-class mismatches: 0
- untriaged mismatches: 0

## 3. Current Triage Table

| Case family | Status | Notes |
|---|---|---|
| `rep_*.dsp` corpus parse envelope | closed | Rust parser class aligns with C++ parser class envelope in current harness. |
| `err_*` parser-only malformed cases | closed | Parser-invalid corpus entries are explicitly classified (`*_parse_*` naming rule). |
| Inline malformed fixtures (`missing_rpar`, `missing_enddef`, modulation malformed, etc.) | closed | Rust emits parser-error/recovery class consistent with C++ parse-error envelope. |
| Non-parser failures (eval/propagate stage failures in C++) | acknowledged | Counted as valid parser outcomes when C++ reports non-parse errors (`OtherError`). |

## 4. Update Rule

When a mismatch appears:
1. add a row with `open` status,
2. capture reproduction command + fixture name,
3. assign owner and target fix commit,
4. mark `closed` only after differential re-run is green.
