# C++ Parity Known Gaps Corpus

This folder contains small `.dsp` fixtures that are intentionally kept outside
the default `tests/corpus/*.dsp` scans.

Purpose:

- preserve minimal reproducers for known Rust vs C++ front-end parity gaps,
- keep those reproducers visible without freezing current Rust behavior into the
  default golden snapshots,
- provide focused inputs for manual differential runs and future parity gates.

These files were added as focused C++ parity reproducers. The parser/eval
pipeline now accepts all current entries; they are kept here until they are
promoted into the regular parity guardrails.

## Current entries

- `gap_01_pattern_def_constant_clause.dsp`
  - purpose: patterned definition with a constant clause followed by a variable
    clause
  - current status:
    - C++: accepted
    - Rust: fixed through parser `prepare_pattern` + eval `a2sb`
  - next step:
    - promote this fixture into the regular parity guardrails

- `gap_02_pattern_def_clause_grouping.dsp`
  - purpose: repeated same-name definition clauses that should be grouped into a
    single pattern-based definition family
  - current status:
    - C++: accepted
    - Rust: fixed through parser `prepare_pattern` + eval `a2sb`
  - next step:
    - promote this fixture into the regular parity guardrails

- `gap_03_case_pattern_constant_folding.dsp`
  - purpose: `case` pattern requiring compile-time pattern evaluation
  - current status:
    - C++: accepted
    - Rust: fixed in the eval phase; the folded pattern now matches correctly
  - next step:
    - promote this fixture into the regular parity guardrails instead of keeping
      it only in the known-gap corpus

- `gap_04_case_pattern_scope_barrier.dsp`
  - purpose: rule-local pattern variable that must not capture an outer binding
  - current status:
    - C++: accepted
    - Rust: fixed in the eval phase; pattern-variable lookup now stops at the
      barrier for nonlinearity checks while RHS evaluation still sees outer scope
  - next step:
    - promote this fixture into the regular parity guardrails instead of keeping
      it only in the known-gap corpus

## Suggested manual differential commands

Reference C++:

```bash
/usr/local/bin/faust tests/cpp_parity_known_gaps/gap_01_pattern_def_constant_clause.dsp -norm
/usr/local/bin/faust tests/cpp_parity_known_gaps/gap_02_pattern_def_clause_grouping.dsp -norm
/usr/local/bin/faust tests/cpp_parity_known_gaps/gap_03_case_pattern_constant_folding.dsp -norm
/usr/local/bin/faust tests/cpp_parity_known_gaps/gap_04_case_pattern_scope_barrier.dsp -norm
```

Rust compiler:

```bash
cargo run -p compiler -- --dump-sig tests/cpp_parity_known_gaps/gap_01_pattern_def_constant_clause.dsp
cargo run -p compiler -- --dump-sig tests/cpp_parity_known_gaps/gap_02_pattern_def_clause_grouping.dsp
cargo run -p compiler -- --dump-sig tests/cpp_parity_known_gaps/gap_03_case_pattern_constant_folding.dsp
cargo run -p compiler -- --dump-sig tests/cpp_parity_known_gaps/gap_04_case_pattern_scope_barrier.dsp
```
