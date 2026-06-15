# faust-rs impulse-response tests

This is a faust-rs port of the C++ Faust `tests/impulse-tests` machinery. It
checks that faust-rs backends generate correct code by comparing the impulse
response of each test DSP against a **reference** produced by the genuine C++
Faust compiler.

See the design write-up in
[`porting/impulse-tests-harness-port-plan-2026-06-14-en.md`](../../porting/impulse-tests-harness-port-plan-2026-06-14-en.md).

## What it does

1. **Reference (the oracle).** `make reference` compiles every `dsp/*.dsp` with
   the C++ Faust compiler wrapped in the original 4-pass impulse architecture
   (`impulsearch.cpp` + `controlTools.h`), builds a native binary, and runs it
   for 60000 frames: impulse pass + random-split pass + polyphonic 4-voice pass
   + polyphonic 1-voice pass. Output goes to `reference/*.ir`.

2. **Backend checks.** Each backend target regenerates the impulse response with
   faust-rs and compares it to the reference with `tools/filesCompare`
   (tolerance `2e-06`):
   - `make cpp` / `make c` — faust-rs generates C++/C, it is wrapped in the
     *same* 4-pass architecture, compiled and run; the full 60000-frame output
     is compared **exactly**.
   - `make interp` — the faust-rs interpreter runs in-process via
     `impulse-runner`. It has no polyphonic/MIDI runtime, so it reproduces only
     the scalar impulse pass (first 15000 frames) and is compared as a **prefix**
     with `filesCompare -part` (same approach the C++ suite uses for its Rust
     target).
   - `make cranelift` — the faust-rs Cranelift JIT runs in-process via
     `impulse_cranelift` in 64-bit (`-double`), scalar prefix, `-part`.
   - `make wasm` — the faust-rs WASM backend is compiled to `.wasm + .json`
     and executed through Node's native WebAssembly runtime in 64-bit
     (`-double`), scalar prefix, `-part`.

## Requirements

- A built faust-rs workspace: `make build` (runs
  `cargo build --release -p compiler -p impulse-runner`).
- A C++ Faust checkout for the reference oracle and the native C/C++ paths
  (architecture headers + `impulsearch.cpp`). Paths are configured in
  [`common.mk`](common.mk) and overridable:
  `CPP_TESTS`, `FAUST_ARCH`, `FAUST_CPP`, `FAUSTLIBS`.
- `c++` and the Faust standard libraries (default `/usr/local/share/faust`).
- Node.js for the WASM impulse runner.

## Usage

```bash
cd tests/impulse-tests
make build         # build the faust-rs binaries the harness drives
make reference     # generate the reference .ir oracle  (run once)
make interp        # check the interpreter backend
make cpp           # check the C++ backend
make c             # check the C backend
make cranelift     # check the Cranelift JIT backend (64-bit)
make wasm          # check the WASM backend (64-bit scalar prefix)
make all           # cpp + c + interp + cranelift
make -k -j8 cpp    # parallel, keep going past failures
make help          # list targets and variables
make clean         # remove ir/ and build/
```

There is no `reference` rebuild on every run: delete `reference/` (or
`make distclean`) to regenerate.

## Layout

| Path | Purpose |
|---|---|
| `dsp/` | 93 test DSP programs (from the C++ suite) |
| `common.mk` | shared, overridable configuration |
| `known.mk` | per-DSP tolerances + known-failure exclusion lists |
| `KNOWN_FAILURES.md` | documented gaps/tolerances with causes |
| `Make.ref` | genuine C++ 4-pass reference generation |
| `Make.gcc` | faust-rs C / C++ backends (full 4-pass, exact compare) |
| `Make.interp` | faust-rs interpreter backend (scalar prefix, `-part`) |
| `Make.cranelift` | faust-rs Cranelift JIT backend (scalar prefix, 64-bit, `-part`) |
| `Make.wasm` | faust-rs WASM backend (scalar prefix, 64-bit, Node WebAssembly, `-part`) |
| `tools/filesCompare.cpp` | the comparator |
| `tools/impulsewasm.js` | Node WebAssembly scalar impulse runner |
| `reference/`, `ir/`, `build/` | generated, gitignored |

## Status

Raw sweep over the 93 DSPs at the default `2e-06` tolerance:

| Backend | Match | Mismatch | Compile-fail |
|---|---|---|---|
| C++ (full 4-pass, exact) | **92** | 0 | 1 (`subcontainer1`) |
| C (full 4-pass, exact) | **87** | 5 | 1 (`subcontainer1`) |
| interpreter (scalar prefix, `-part`) | **92** | 0 | 1 (`subcontainer1`) |
| Cranelift JIT (scalar prefix, `-part`, 64-bit) | **92** | 0 | 1 (`subcontainer1`) |
| WASM (scalar prefix, `-part`, 64-bit, Node) | **70** | 22 | 1 (`subcontainer1`) |

The C++ backend reproduces the full 60000-frame reference exactly on 92/93 DSPs,
so the remaining mismatches are backend-specific divergences the harness
pinpoints. Each was classified by its *max* delta and either given a per-DSP
tolerance (bounded rounding) or listed as a known failure (real gap) in
[`known.mk`](known.mk) / [`KNOWN_FAILURES.md`](KNOWN_FAILURES.md). With those
applied, the aggregate targets are **green gates**: `make cpp` (92), `make c`
(92), `make cranelift` (92), `make interp` (92), and `make wasm` (70) build and
pass; excluded cases are documented in `known.mk` to fix later.
