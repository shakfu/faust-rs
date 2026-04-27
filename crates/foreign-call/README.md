# foreign-call

Runtime bridge from symbolic Faust foreign-function bindings to raw host
function pointers.

## Scope

This crate provides the low-level dispatch helper used when a compiled DSP needs
to call a foreign C ABI function by address.

## Public API

| Item | Description |
|---|---|
| `ScalarType` | Supported scalar ABI types: `Int32`, `Float32`, `Float64`, `Bool`, `Void` |
| `Value` | Runtime value wrapper for supported scalar arguments and returns |
| `invoke(addr, ret, args)` | Dispatch an `extern "C"` function pointer with a supported signature |

## Safety boundary

`foreign-call` is the workspace exception to the global `unsafe_code = "forbid"`
lint. It must transmute raw addresses into function pointers, so the caller is
responsible for ensuring that:

- `addr` points to a valid `extern "C"` function,
- `ret` matches the real return type,
- `args` exactly match the real argument types and arity,
- the pointed function remains valid for the duration of the call.

Unsupported signatures return `None` instead of attempting a call.

