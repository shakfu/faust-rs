# py-faust-rs demos

Runnable scripts, one per aspect of the `faust_rs` Python API. Each prints its
results; `10` and `11` also write WAV files to `demos/out/`.

| Script | Shows |
|-|-|
| `01_hello.py` | `compile`, `compute`, channel layout, `version()` |
| `02_state.py` | state persisting across blocks, `cycle`, `reset()` |
| `03_params.py` | `params()`, `get_param`/`set_param` key lookup, bargraphs, `ReadOnlyParamError` |
| `04_factory.py` | `Factory`: several instances per compile, `get_json()`, `from_file`, `compile_file` |
| `05_precision.py` | `double=True` versus the `f32` default |
| `06_backends.py` | interpreter versus Cranelift: timing and sample-exact parity; `args=["-vec", ...]` |
| `07_compute_into.py` | `compute_into` with NumPy, `array.array`/`memoryview`, block streaming |
| `08_lifecycle.py` | `init`, `instance_init`, `instance_constants`, `instance_reset_user_interface`, `instance_clear` |
| `09_errors.py` | the `FaustError` exception hierarchy |
| `10_render_wav.py` | parameter automation between blocks, rendered to `out/melody.wav` |
| `11_stdlib.py` | `import("stdfaust.lib")` via `search_paths`; needs `FAUST_LIB_PATH` |

`_shared.py` defines `osc`/`phasor` in plain Faust, so only `11` needs the
standard library.

## Run

```bash
make develop                      # build the extension into .venv
make demos                        # run all of them
uv run --no-sync python demos/03_params.py
FAUST_LIB_PATH=/path/to/faust/libraries uv run --no-sync python demos/11_stdlib.py
```
