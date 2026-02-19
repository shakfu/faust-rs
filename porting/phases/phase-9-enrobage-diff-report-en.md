# Phase 9 Enrobage Differential Report

Date: 2026-02-19

## 1. Scope

Validate Rust enrobage behavior against C++ reference behavior on the
architecture-wrapper envelope (include injection, marker slicing, wrapper lines)
using the same DSP input and wrapper fixture.

Compared artifacts:
- Rust wrapped output: `/tmp/enrobage_rust_wrap.cpp`
- C++ wrapped output: `/tmp/enrobage_cpp_wrap.cpp`

Input/fixtures:
- DSP: `tests/corpus/rep_01_passthrough.dsp`
- Wrapper architecture: `crates/compiler/tests/fixtures/enrobage/arch/wrapper.cpp`
- Wrapper include dir: `crates/compiler/tests/fixtures/enrobage/arch`

Reference revisions:
- Rust branch HEAD before this report: `db919f0`
- C++ reference tree: `8eebea429`
- C++ compiler binary: `faust 2.84.3`

## 2. Commands

Rust:

```bash
cargo run -p compiler -- -lang cpp \
  tests/corpus/rep_01_passthrough.dsp \
  -a crates/compiler/tests/fixtures/enrobage/arch/wrapper.cpp \
  -A crates/compiler/tests/fixtures/enrobage/arch \
  -i \
  -o /tmp/enrobage_rust_wrap.cpp
```

C++:

```bash
/usr/local/bin/faust \
  tests/corpus/rep_01_passthrough.dsp \
  -lang cpp \
  -a crates/compiler/tests/fixtures/enrobage/arch/wrapper.cpp \
  -A crates/compiler/tests/fixtures/enrobage/arch \
  -i \
  -o /tmp/enrobage_cpp_wrap.cpp
```

## 3. Differential checks

| Check | C++ | Rust | Status |
|---|---:|---:|---|
| `// injected_one` occurrences | 1 | 1 | PASS |
| `#define ENROBAGE_ONE 1` occurrences | 1 | 1 | PASS |
| `// injected_two` occurrences | 1 | 1 | PASS |
| `#define ENROBAGE_TWO 2` occurrences | 1 | 1 | PASS |
| `static mydsp* g_dsp = new mydsp();` occurrences | 1 | 1 | PASS |
| `static dsp* g_base = nullptr;` occurrences | 1 | 1 | PASS |
| `static int dsp_token = 0;` occurrences | 1 | 1 | PASS |
| `static int mydsp_token = 1;` occurrences | 1 | 1 | PASS |
| residual `<<includeIntrinsic>>` markers | 0 | 0 | PASS |
| residual `<<includeclass>>` markers | 0 | 0 | PASS |
| wrapper placement order (`g_dsp` before class, `build_instance` after class) | yes | yes | PASS |

Anchor lines:
- C++: `g_dsp@19`, `class@48`, `build_instance@124`
- Rust: `g_dsp@10`, `class@30`, `build_instance@83`

## 4. Full-file diff triage

A full-file diff remains non-empty (`126` lines C++ vs `83` lines Rust). The
mismatch is triaged and not attributed to enrobage stream semantics:

- C++ output contains additional backend-generated prologue/guards/comments and
  different class-body details from the C++ compiler backend.
- Rust output currently wraps Rust-generated C++ module text (existing backend
  emission contract), so non-enrobage code sections differ.

This is tracked as a backend parity item outside strict enrobage API parity.

## 5. Conclusion

For the enrobage-specific contract validated here (wrapper slicing, inline
injection, marker removal, wrapper line placement), Rust behavior matches C++
reference behavior on the selected fixture with no untriaged enrobage mismatch.
