# C++ Parity Known Gaps Corpus

This folder contains small `.dsp` fixtures that are intentionally kept outside
the default `tests/corpus/*.dsp` scans.

Purpose:

- preserve minimal reproducers for known Rust vs C++ front-end parity gaps,
- keep those reproducers visible without freezing current Rust behavior into the
  default golden snapshots,
- provide focused inputs for manual differential runs and future parity gates.

These files are currently expected to be accepted by the C++ reference compiler
while the Rust port still diverges in parse or eval semantics.

## Current entries

- `gap_01_pattern_def_constant_clause.dsp`
  - purpose: patterned definition with a constant clause followed by a variable
    clause
  - current gap:
    - C++: accepted
    - Rust: parser/eval now accept it, but the signal pipeline still stops in
      `propagate` because the normalized definition lowers to a `case` node and
      `a2sb()` is still missing

- `gap_02_pattern_def_clause_grouping.dsp`
  - purpose: repeated same-name definition clauses that should be grouped into a
    single pattern-based definition family
  - current gap:
    - C++: accepted
    - Rust: parser/eval now group the clauses correctly, but the signal
      pipeline still stops in `propagate` because the grouped definition lowers
      to a `case` node and `a2sb()` is still missing

- `gap_03_case_pattern_constant_folding.dsp`
  - purpose: `case` pattern requiring compile-time pattern evaluation
  - current gap:
    - C++: accepted
    - Rust: evaluation reports `no case rule matches arguments` because
      `case` matching is built from raw rules and misses the folded match

- `gap_04_case_pattern_scope_barrier.dsp`
  - purpose: rule-local pattern variable that must not capture an outer binding
  - current gap:
    - C++: accepted
    - Rust: evaluation reports `no case rule matches arguments` because
      pattern-variable lookup crosses outer scopes and prevents the match

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
