# box-ffi

Faust-style C/C++ export for box manipulation, backed by Rust `boxes::BoxBuilder`.

## Scope

- Exposes a broad `Cbox*` constructor family (`libfaust-box-c.h` style).
- Exposes a thin C++ convenience wrapper (`libfaust-box.h`).
- Uses a process-global context (`createLibContext` / `destroyLibContext`) like
  libfaust APIs.
- Uses `tree-ffi` for shared tree handles, context-owned C strings,
  null-terminated handle arrays, and common Box/Signal enum definitions.

## Notes

- `Box` handles are opaque and stable within one active context.
- `Ctree2str` and `CprintBox` return heap C strings allocated by Rust;
  call `freeCMemory`.
- This is an incremental parity layer; rows marked as exact candidates in the
  generated API matrix still need focused semantic parity tests.
