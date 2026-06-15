# Impulse-tests harness — port plan

Date: 2026-06-14
Status: Phase 0–2 implemented and verified; Phases 3–6 planned.
Location of the machinery: `tests/impulse-tests/`.

## 1. Goal

Port the C++ Faust impulse-response test mechanism
(`/Users/letz/Developpements/RUST/faust/tests/impulse-tests`) into faust-rs:

- bring in the suite of test DSP programs (`dsp/*.dsp`);
- provide a Makefile-driven harness that **creates the reference `.ir` files**
  and **checks the faust-rs backends** (interp, c, cpp, cranelift, wasm/wast…)
  against them.

The harness must stay faithful to the C++ mechanism so results are directly
comparable, while accounting for capabilities faust-rs does not have yet
(polyphony, MIDI, soundfile at runtime).

## 2. How the C++ mechanism works (recap)

- Each reference `.ir` is the **4-pass protocol** implemented in
  `archs/controlTools.h`, driven by `archs/impulsearch.cpp::main`, run for
  `60000` frames total (`nbsamples`):
  1. `runDSP` — impulse on frame 0 of each input, all `button` widgets held at
     1.0 during the first block then 0.0, on a fresh + cloned instance
     (`nbsamples/4 = 15000` frames);
  2. `runDSP(..., random=true)` — same impulse, but each block's `compute` is
     split at a random offset (block-split invariance check); identical output
     for a correct DSP (15000 frames);
  3. `runPolyDSP(..., 4)` — DSP wrapped in `mydsp_poly`, 4 voices keyed on
     (15000 frames);
  4. `runPolyDSP(..., 1)` — 1 voice (15000 frames).
- Sample rate `44100`, block size `kFrames = 64`, `-double` internal precision.
- Output format, per frame: `printf("%6d : ", n)` then `printf(" %8.6f", x)`
  per output channel, after `normalize()` (|x| < 1e-6 → 0; NaN/Inf abort).
  Header: `number_of_inputs : %3d`, `number_of_outputs : %3d`,
  `number_of_frames : %6d`.
- `tools/filesCompare.cpp` compares a test file against a reference with a
  default tolerance of `2e-06`. It reads the **test** header's frame count
  (`count1`) and compares only that many frames. With `-part` it tolerates a
  frame-count mismatch, i.e. it compares the test file as a **prefix** of the
  reference. (This is exactly what the C++ `Make.rust` relies on: a scalar-only
  Rust architecture emits ~15000 frames and is compared against the 60000-frame
  reference prefix.)

## 3. Key design decisions

Confirmed with the maintainer on 2026-06-14:

1. **Reference = full 4-pass, 60000 frames** (not a reduced scalar-only
   reference).
2. **Oracle = genuine native C++**: the reference compiler (`faust -lang cpp`)
   wrapped in the original `impulsearch.cpp`/`controlTools.h`, compiled with
   `c++`, executed. Bit-for-bit reproduction of the existing C++ `reference/`.
3. **Wire all backends** (interp, c, cpp, cranelift, wasm/wast) over time.

Consequences:

- The reference oracle is intrinsically tied to a **C++ Faust checkout** (it
  pulls `poly-dsp.h`, `MidiUI.h`, `SoundUI.h`, `libfaust.h`… from the Faust
  architecture/compiler tree). The harness references that tree through
  overridable make variables rather than vendoring the un-standalone headers.
- faust-rs backends that **generate native C/C++** (`-lang c`, `-lang cpp`) are
  wrapped in the *same* 4-pass architecture. faust-rs's generated class
  implements the standard Faust `dsp` interface (`clone()`,
  `buildUserInterface()`, `compute()`…), so they run the **full** protocol
  including polyphony and are compared to the reference with **no `-part`**.
- faust-rs backends executed by an **in-process Rust runtime** (interp today,
  cranelift next) have no poly/MIDI/soundfile wrapper, so they reproduce only
  the **scalar pass** (frames 0–14999) and are compared with **`-part`** — the
  same compromise the C++ `Make.rust` already uses for the Rust target.

## 4. Directory layout (implemented)

```
tests/impulse-tests/
├── common.mk         # shared, overridable configuration
├── Makefile          # top-level driver (build/reference/interp/c/cpp/all/help)
├── Make.ref          # genuine C++ 4-pass reference generation (the oracle)
├── Make.gcc          # faust-rs C / C++ native backends (full 4-pass, exact)
├── Make.interp       # faust-rs interpreter backend (scalar prefix, -part)
├── archs/README.md   # why the impulse architecture is referenced in place
├── tools/
│   └── filesCompare.cpp   # the comparator (vendored, compiles standalone)
├── dsp/              # 93 test DSP programs (copied from the C++ suite)
├── reference/        # generated .ir oracle           (gitignored)
├── ir/<backend>/     # generated per-backend .ir       (gitignored)
└── build/<backend>/  # generated sources + binaries    (gitignored)
```

The interpreter runner is a workspace binary: `crates/impulse-runner`
(`target/release/impulse-runner`), the faust-rs analogue of the C++
`tools/impulseinterp.cpp`.

## 5. Components

### 5.1 `crates/impulse-runner` (interp backend) — DONE

Compiles a DSP through the faust-rs library to interpreter bytecode
(`generate_interp_module::<f64>` when `-double`) and runs the scalar impulse
pass in-process via `FbcDspInstance`:

- SR 44100, block 64, impulse on frame 0, `button`/`checkbox` zones (discovered
  through `ui_instructions()`) driven 1.0 during block 0 then 0.0 via
  `set_real_zone`;
- emits the exact reference text format with the same `normalize()` zero-clamp;
- defaults to 15000 frames; `.ir` to stdout.

### 5.2 Makefiles — DONE

- `Make.ref`: `faust -lang cpp -double -i -a $(IMPULSE_ARCH)` → `c++` → run →
  `reference/%.ir`.
- `Make.gcc`: `faust-rs -lang {c,cpp} -double -i -a $(IMPULSE_ARCH)` → `c++` →
  run → `ir/<outdir>/%.ir` → `filesCompare` (no `-part`).
- `Make.interp`: `impulse-runner … -n 15000` → `ir/interp/%.ir` →
  `filesCompare -part`.
- `common.mk` centralizes overridable paths (`FAUST_CPP`, `FAUST_RS`, `RUNNER`,
  `CPP_TESTS`, `FAUST_ARCH`, `IMPULSE_ARCH`, `FAUSTLIBS`, `precision`,
  `NFRAMES`, `SCALARFRAMES`).

### 5.3 Comparator — DONE

`tools/filesCompare.cpp` vendored verbatim (self-contained, builds with `c++`).
A pure-Rust reimplementation as an `xtask` subcommand is a possible later
refinement to drop the C++ dependency for the *compare* step.

## 6. Verification status (2026-06-14)

Full-suite sweep over the 93 DSPs against the genuine C++ reference
(default `2e-06` tolerance). References regenerated by `Make.ref` are
byte-identical to the committed C++ `reference/` (verified on `bargraph`).

| Backend | Match | Mismatch | Compile-fail |
|---|---|---|---|
| **reference** (oracle) | 93 generated | — | 0 |
| **cpp** (full 4-pass, exact) | **92** | 0 | 1 (`subcontainer1`) |
| **c** (full 4-pass, exact) | **87** | 5 | 1 (`subcontainer1`) |
| **interp** (scalar prefix, `-part`) | **74** | 18 | 1 (`subcontainer1`) |

The 5 C-backend mismatches diverge on `c` but not `cpp`. Four are tiny rounding
in the polyphonic passes (`harpe`/`noise`/`noiseabs` ≈ 2–3e-6, `comb_bug_exp`
1.1e-4) absorbed by per-DSP tolerance; only `grain3` (2.6e-3, grain/table path)
is a genuine excluded divergence.

Key insight: the **C++ backend reproduces the full 60000-frame reference exactly
on 92/93 DSPs** (including polyphony), so the 18 interpreter mismatches are
**interp-backend-specific divergences**, not test-harness or DSP issues — the
suite turns them into actionable backend bugs. Their first-divergence deltas:

- *Within a looser tolerance* (≤ 1e-4, smoothing / init / rounding):
  `cubic_distortion` (2e-6), `mixer` (6e-6), `virtual_analog_oscillators`
  (3e-6), `carre_volterra` (2e-5), `tester` (6e-5), `parametric_eq` (4e-5),
  `gate_compressor` (2e-4), `phaser_flanger` / `spectral_tilt` /
  `vcf_wah_pedals` (1e-4), `reverb_tester` (2e-3) — candidates for per-DSP
  `precision` overrides.
- *Structural interp gaps* (delta ≈ 0.01–1): `comb_delay1` / `comb_delay2`
  (delay line emits silence where the reference has the comb echo),
  `reverb_designer`, `math_simp` (output 24), `norm3` (output 2), `UITester`
  (UI/button default), and `sound` (soundfile, unsupported) — real interpreter
  backend bugs to file.

`subcontainer1` is the single faust-rs compile failure shared by both backends
(sub-container codegen gap).

## 7. Remaining phases

### Phase 3 — characterize and curate (DONE)
- Full `c`/`cpp`/`interp` sweeps recorded (table in §6); each mismatch's *max*
  |delta| measured to separate bounded rounding from real divergence.
- [`known.mk`](../tests/impulse-tests/known.mk) holds `PRECISION_<dsp>` tolerance
  overrides and `KNOWN_FAIL_<backend>` / `KNOWN_FAIL_all` exclusion lists;
  [`KNOWN_FAILURES.md`](../tests/impulse-tests/KNOWN_FAILURES.md) documents each
  with cause. The backend makefiles `filter-out` the excluded names and apply
  the per-DSP tolerance, so `make interp` / `c` / `cpp` are **green gates**
  (78 / 91 / 92 cases). `filesCompare` was patched to accept a tolerance
  together with `-part` (upstream ignored it).
- Remaining curated divergences are real backend bugs to fix — removing a
  `KNOWN_FAIL_*` entry re-covers it.
- Fixed via the harness: the interpreter ignored `is_reverse` in its general
  `ForLoop` compiler, so short fixed delays (`@(3..mcd)`, shift-array strategy)
  emitted silence. Honoring it (matching the Cranelift `lower_for_loop`
  contract) fixed `comb_delay1/2`, `math_simp`, `norm3` and moved the interp
  gate 78 -> 82/93.

### Phase 4 — cranelift backend (DONE, 64-bit)
- `crates/cranelift-ffi/src/bin/impulse_cranelift.rs` runs the in-process JIT
  (scalar pass + `-part`), wired via `Make.cranelift`.
- The Cranelift backend was f32-only; it was extended to **64-bit** so it can be
  compared directly against the `-double` reference. `FirType::FaustFloat` now
  resolves to `F64`/8-byte under a new `CraneliftOptions::double_precision`,
  threaded through the type map, struct layout, static-table data, op/const
  selection (`canon_real`), and the C-API factory (which sets
  `RealType::Float64` from `-double`). The runtime UI-zone writer
  (`apply_control_defaults`) now writes a `FaustFloat` zone at the field's actual
  width (the f32-into-8-byte bug that corrupted slider inits, e.g. freeverb).
- Result: **83/93** match the `-double` reference (was f32-only before; better
  than interp). A second fix made the runtime run the JIT-compiled
  `instanceClear` at init (api.rs now compiles it; `JitDspModule` carries
  `instance_clear_entry_addr`); some DSPs fill state buffers with non-zero init
  values there (e.g. `prefix` writes `fRec7 = 1.0`), so skipping it broke
  `prefix`/`phasor` — now fixed. Curated `KNOWN_FAIL_cranelift`: `table2`
  (rwtable), `bells`, `karplus`/`karplus32`, `UITester` (needs button driving),
  `reverb_designer`/`reverb_tester` (shared drift), `sound` (soundfile JIT
  crash), `grain3`.
- No f32 regression (cranelift-ffi 31 + codegen 251 tests still pass).

### Phase 5 — wasm / wast backends
- `Make.wasm`: `faust-rs -lang wasm` → a Node harness driving the scalar pass
  (impulse + button zones) → `.ir` → `filesCompare -part`. Reuse the JSON/JS
  glue from the existing `wasm` xtask plumbing.

### Phase 6 — integration & independence
- An `xtask impulse-check` entry point so CI can run the interp sweep without a
  C++ build toolchain, gated on a committed snapshot of references (or a
  `FAUST_CPP_BIN`-driven regeneration step).
- Optionally a faust-rs-native impulse architecture to remove the C++ Faust
  dependency from reference generation once a Rust poly/MIDI shim exists
  (cross-references [[project_ondemand_clock_domains]] for the FAD/poly work).
- Pure-Rust comparator in `xtask` to drop the vendored `filesCompare`.

## 8. Usage

```bash
cd tests/impulse-tests
make build                 # cargo build faust-rs compiler + impulse-runner
make reference             # generate the C++ oracle (needs a Faust checkout)
make interp                # check the interpreter backend (scalar prefix)
make cpp                   # check the C++ backend (full 4-pass)
make c                     # check the C backend (full 4-pass)
make -k -j8 all            # everything, keep going past failures
make help                  # variables and targets
```

Override the C++ tree location when it differs:

```bash
make reference CPP_TESTS=/path/to/faust/tests/impulse-tests \
               FAUST_ARCH=/path/to/faust/architecture \
               FAUST_CPP=/path/to/faust/build/bin/faust
```
