# utils

Shared FFI utilities for Rust-side Faust backend crates.

Provides common C ABI types (`UIGlue`, `MetaGlue`), heap allocation helpers,
error buffer writing, and CLI argument parsing used by `interp-ffi`,
`cranelift-ffi`, and `box-ffi`.

## Public API

### C ABI types

| Item | Description |
|---|---|
| `UIGlue` | C-ABI UI callback table (mirrors Faust `UIGlue`) |
| `MetaGlue` | C-ABI metadata callback table (mirrors Faust `MetaGlue`) |
| `FfiFaustFloat` | `FAUSTFLOAT` type alias (`f32`) used by FFI exports |

### Allocation helpers

| Function | Description |
|---|---|
| `alloc_c_string(s)` | Allocate a heap C string (NUL bytes escaped as `\\0`) |
| `alloc_opaque(value)` | Box a value and return an owning raw pointer |
| `free_c_string(ptr)` | Free a pointer returned by `alloc_c_string` |
| `free_opaque<T>(ptr)` | Free a pointer returned by `alloc_opaque` |
| `free_c_memory_c_string_only(ptr)` | Common `freeCMemory` behavior for C-string pointers |
| `null_c_string_array()` | Static null-terminated empty `char**` array pointer |

### FFI utilities

| Function | Description |
|---|---|
| `write_error_4096(buf, msg)` | Write error message into a 4096-byte Faust error buffer |
| `decode_c_argv(argc, argv)` | Decode a C `argv` array into a `Vec<String>` |
| `required_c_str_arg(ptr, label)` | Extract a required C string argument |
| `optional_c_str_arg(ptr, label)` | Extract an optional C string argument |

### Compile arguments

| Item | Description |
|---|---|
| `FfiCompileArgs` | Parsed CLI-like options: `-I`, `-cn`, `-double` |
| `parse_ffi_compile_args(argv)` | Parse a string slice into `FfiCompileArgs` |

### Factory caching

| Item | Description |
|---|---|
| `FactoryCache<T>` | Thread-safe SHA-keyed factory cache |

### Utilities

| Item | Description |
|---|---|
| `CRATE_NAME` | Crate identity string constant |
| `crate_id()` | Returns `CRATE_NAME` |
