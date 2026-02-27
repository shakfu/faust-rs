# box-ffi

Faust-style C/C++ export for box manipulation, backed by Rust `boxes::BoxBuilder`.

## Scope

- Exposes a broad `Cbox*` constructor family (`libfaust-box-c.h` style).
- Exposes a thin C++ convenience wrapper (`libfaust-box.h`).
- Uses a process-global context (`createLibContext` / `destroyLibContext`) like
  libfaust APIs.

## Notes

- `Box` handles are opaque and stable within one active context.
- `Ctree2str` and `CprintBox` return heap C strings allocated by Rust;
  call `freeCMemory`.
- This is an incremental parity layer; advanced `CisBox*` matchers are not yet
  exported in this first version.
