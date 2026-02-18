# Faust-rs Compiler CLI Guide (User)

This guide documents the current user-facing options of the `compiler` binary.

## 1. Quick start

```bash
# Show scaffold version (no input file)
cargo run -p compiler

# Default compile mode with input file: C++ output on stdout
cargo run -p compiler -- tests/corpus/rep_01_passthrough.dsp
```

Installed binary usage:

```bash
# Install the CLI binary
cargo install --path crates/compiler

# Then use the installed command directly
faust-rs -lang c foo.dsp
faust-rs -lang cpp foo.dsp
```

## 2. Main command form

```bash
cargo run -p compiler -- [MODE] <input.dsp> [OPTIONS]
```

Only one mode can be selected at a time.

## 3. Modes

### `--golden`

Generate golden snapshot text for one DSP file.

```bash
cargo run -p compiler -- --golden tests/corpus/rep_01_passthrough.dsp
```

Notes:

- `--import-dir` is not supported in this mode.

### `--parse`

Parse one DSP file and print parser status.

```bash
cargo run -p compiler -- --parse tests/corpus/rep_01_passthrough.dsp
```

### `--dump-box`

Parse and dump Box IR text.

```bash
cargo run -p compiler -- --dump-box tests/corpus/rep_01_passthrough.dsp
```

### `--dump-sig`

Run parse/eval/propagate and dump Signal IR text.

```bash
cargo run -p compiler -- --dump-sig tests/corpus/rep_01_passthrough.dsp
```

### `--dump-fir`

Run parse/eval/propagate + signal->FIR lowering and dump FIR IR text.

```bash
cargo run -p compiler -- --dump-fir tests/corpus/rep_01_passthrough.dsp
```

### `--dump-cpp`

Generate C++ backend output text.

```bash
cargo run -p compiler -- --dump-cpp tests/corpus/rep_01_passthrough.dsp
```

### `--dump-c`

Generate C backend output text.

```bash
cargo run -p compiler -- --dump-c tests/corpus/rep_01_passthrough.dsp
```

### `--lang c|cpp`

Faust-style backend language selector (equivalent to `--dump-c` or `--dump-cpp`).

```bash
cargo run -p compiler -- --lang c tests/corpus/rep_01_passthrough.dsp
cargo run -p compiler -- --lang cpp tests/corpus/rep_01_passthrough.dsp
```

Legacy compatibility:

- `-lang c` and `-lang cpp` are accepted.
- `-lang -c` maps to `--lang c`.
- `-lang -cpp` maps to `--lang cpp`.

Installed binary examples:

```bash
faust-rs -lang c foo.dsp
faust-rs -lang cpp foo.dsp
```

## 4. Common options

### `-o, --output <file>`

Write text output to a file instead of stdout.

```bash
cargo run -p compiler -- --dump-cpp tests/corpus/rep_01_passthrough.dsp -o /tmp/out.cpp
```

### `-I, --import-dir <dir>`

Add import search directories. Can be repeated.

```bash
cargo run -p compiler -- --dump-sig main.dsp -I ./lib -I ./third_party/faust
```

## 5. Diagnostics options

### `--error-format human|json`

- `human` (default): readable terminal diagnostics.
- `json`: structured diagnostics for tools/CI.

### `--error-verbosity standard|debug`

- `standard` (default): concise diagnostics.
- `debug`: includes low-level internal notes/fields.

### `--help-error-format`

Print a dedicated summary for diagnostics options and exit.

```bash
cargo run -p compiler -- --help-error-format
```

For interpretation details, see `docs/user-diagnostics-guide-en.md`.

## 6. Signal->FIR lane selection

### `--signal-fir-lane legacy|fast`

Select the lowering lane used before FIR-backed outputs.

- `legacy`: temporary legacy bridge.
- `fast`: transform fast-lane.

Default: `fast` when option is omitted.

Valid with:

- `--dump-cpp`
- `--dump-c`
- `--dump-fir`

Invalid with:

- `--parse`, `--dump-box`, `--dump-sig`, `--golden`

Examples:

```bash
cargo run -p compiler -- --dump-fir tests/corpus/rep_01_passthrough.dsp --signal-fir-lane legacy
cargo run -p compiler -- --dump-cpp tests/corpus/rep_01_passthrough.dsp --signal-fir-lane fast
```

## 7. Mode rules and defaults

- With an input file and no explicit mode, default mode is C++ generation.
- Without input file and without mode, the command prints scaffold version.
- More than one mode at once is rejected.

## 8. Exit behavior

- Success: exit code `0`.
- Pipeline or I/O error: non-zero exit with diagnostics on stderr.
