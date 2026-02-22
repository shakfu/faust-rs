//! Factory-level `extern "C"` functions.
//!
//! Implements the C API from `interpreter-dsp-c.h` for factory lifecycle,
//! bitcode serialization, and the global factory cache.
//!
//! # Scope
//! - `readCInterpreterDSPFactoryFromBitcode[File]` — fully implemented via
//!   `codegen::backends::interp::serial::read_fbc`.
//! - `writeCInterpreterDSPFactoryToBitcode[File]` — fully implemented via
//!   `codegen::backends::interp::serial::write_fbc`.
//! - `createCInterpreterDSPFactoryFromFile/String/Signals/Boxes` — return
//!   `null` (full compiler pipeline not yet available in this crate).
//! - Cache management functions — fully implemented.

use std::ffi::{CStr, CString, c_char, c_void};
use std::io::BufReader;

use codegen::backends::interp::{FAUST_VERSION, read_fbc, write_fbc};

use crate::cache::{
    cache_all_sha_keys, cache_drain, cache_insert, cache_lookup,
    cache_remove_by_ptr, start_mt, stop_mt,
};
use crate::types::{alloc_c_string, alloc_factory, free_c_string, free_factory,
                   InterpreterDspFactory};

// ── Version ──────────────────────────────────────────────────────────────────

/// Returns the Faust library version string.
///
/// The returned pointer is valid for the lifetime of the process (static data).
#[unsafe(no_mangle)]
pub extern "C" fn getCLibFaustVersion() -> *const c_char {
    use std::sync::OnceLock;
    static VERSION_C: OnceLock<CString> = OnceLock::new();
    VERSION_C
        .get_or_init(|| CString::new(FAUST_VERSION).unwrap())
        .as_ptr()
}

// ── Bitcode serialization ─────────────────────────────────────────────────────

/// Create a DSP factory from a bitcode string in memory.
///
/// # Safety
/// - `bitcode` must be a valid null-terminated C string.
/// - `error_msg` must point to a buffer of at least 4096 bytes (may be null).
///
/// Returns a factory pointer on success, or null on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn readCInterpreterDSPFactoryFromBitcode(
    bitcode: *const c_char,
    error_msg: *mut c_char,
) -> *mut InterpreterDspFactory {
    unsafe {
        if bitcode.is_null() {
            write_error(error_msg, "null bitcode pointer");
            return std::ptr::null_mut();
        }
        let s = match CStr::from_ptr(bitcode).to_str() {
            Ok(s) => s,
            Err(e) => {
                write_error(error_msg, &format!("invalid UTF-8 in bitcode: {e}"));
                return std::ptr::null_mut();
            }
        };
        let mut reader = BufReader::new(s.as_bytes());
        match read_fbc::<f32>(&mut reader) {
            Ok(factory) => {
                let sha = factory.sha_key.clone();
                let ptr = alloc_factory(factory);
                cache_insert(&sha, ptr);
                ptr
            }
            Err(e) => {
                write_error(error_msg, &format!("{e}"));
                std::ptr::null_mut()
            }
        }
    }
}

/// Write a DSP factory to a bitcode string.
///
/// # Safety
/// `factory` must be a valid non-null factory pointer.
///
/// Returns a heap-allocated C string.  The caller must free it with
/// `freeCMemory`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn writeCInterpreterDSPFactoryToBitcode(
    factory: *mut InterpreterDspFactory,
) -> *mut c_char {
    unsafe {
        if factory.is_null() {
            return std::ptr::null_mut();
        }
        let mut buf: Vec<u8> = Vec::new();
        match write_fbc(&(*factory).inner, &mut buf, false) {
            Ok(()) => {
                let s = String::from_utf8_lossy(&buf);
                alloc_c_string(&s)
            }
            Err(_) => std::ptr::null_mut(),
        }
    }
}

/// Create a DSP factory from a bitcode file on disk.
///
/// # Safety
/// - `bit_code_path` must be a valid null-terminated C string.
/// - `error_msg` must point to a buffer of at least 4096 bytes (may be null).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn readCInterpreterDSPFactoryFromBitcodeFile(
    bit_code_path: *const c_char,
    error_msg: *mut c_char,
) -> *mut InterpreterDspFactory {
    unsafe {
        if bit_code_path.is_null() {
            write_error(error_msg, "null path pointer");
            return std::ptr::null_mut();
        }
        let path = match CStr::from_ptr(bit_code_path).to_str() {
            Ok(s) => s,
            Err(e) => {
                write_error(error_msg, &format!("invalid UTF-8 in path: {e}"));
                return std::ptr::null_mut();
            }
        };
        let file = match std::fs::File::open(path) {
            Ok(f) => f,
            Err(e) => {
                write_error(error_msg, &format!("cannot open file '{path}': {e}"));
                return std::ptr::null_mut();
            }
        };
        let mut reader = BufReader::new(file);
        match read_fbc::<f32>(&mut reader) {
            Ok(factory) => {
                let sha = factory.sha_key.clone();
                let ptr = alloc_factory(factory);
                cache_insert(&sha, ptr);
                ptr
            }
            Err(e) => {
                write_error(error_msg, &format!("{e}"));
                std::ptr::null_mut()
            }
        }
    }
}

/// Write a DSP factory to a bitcode file on disk.
///
/// # Safety
/// - `factory` must be a valid non-null factory pointer.
/// - `bit_code_path` must be a valid null-terminated C string.
///
/// Returns `true` on success, `false` on failure.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn writeCInterpreterDSPFactoryToBitcodeFile(
    factory: *mut InterpreterDspFactory,
    bit_code_path: *const c_char,
) -> bool {
    unsafe {
        if factory.is_null() || bit_code_path.is_null() {
            return false;
        }
        let path = match CStr::from_ptr(bit_code_path).to_str() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let file = match std::fs::File::create(path) {
            Ok(f) => f,
            Err(_) => return false,
        };
        let mut writer = std::io::BufWriter::new(file);
        write_fbc(&(*factory).inner, &mut writer, false).is_ok()
    }
}

// ── Unimplemented factory constructors ───────────────────────────────────────
// These require the full Faust compiler pipeline which is not yet available.

/// Not implemented — returns null.
///
/// # Safety
/// All pointer arguments are accepted but the function always returns null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createCInterpreterDSPFactoryFromFile(
    _filename: *const c_char,
    _argc: i32,
    _argv: *const *const c_char,
    error_msg: *mut c_char,
) -> *mut InterpreterDspFactory {
    unsafe {
        write_error(error_msg, "createCInterpreterDSPFactoryFromFile: not implemented (full compiler pipeline not available)");
        std::ptr::null_mut()
    }
}

/// Not implemented — returns null.
///
/// # Safety
/// All pointer arguments are accepted but the function always returns null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createCInterpreterDSPFactoryFromString(
    _name_app: *const c_char,
    _dsp_content: *const c_char,
    _argc: i32,
    _argv: *const *const c_char,
    error_msg: *mut c_char,
) -> *mut InterpreterDspFactory {
    unsafe {
        write_error(error_msg, "createCInterpreterDSPFactoryFromString: not implemented (full compiler pipeline not available)");
        std::ptr::null_mut()
    }
}

// ── Cache management ──────────────────────────────────────────────────────────

/// Look up a factory in the cache by SHA key.
///
/// Increments the conceptual reference count (currently just returns the cached
/// pointer — reference counting is deferred to a future implementation).
///
/// # Safety
/// `sha_key` must be a valid null-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getCInterpreterDSPFactoryFromSHAKey(
    sha_key: *const c_char,
) -> *mut InterpreterDspFactory {
    unsafe {
        if sha_key.is_null() {
            return std::ptr::null_mut();
        }
        let sha = match CStr::from_ptr(sha_key).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        cache_lookup(sha)
    }
}

/// Delete a DSP factory (frees the Rust allocation).
///
/// Removes the factory from the global cache and drops it.
/// Returns `true` if the memory was actually freed.
///
/// # Safety
/// `factory` must be a valid non-null factory pointer or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn deleteCInterpreterDSPFactory(
    factory: *mut InterpreterDspFactory,
) -> bool {
    unsafe {
        if factory.is_null() {
            return false;
        }
        cache_remove_by_ptr(factory);
        free_factory(factory);
        true
    }
}

/// Delete all factories held in the global cache.
#[unsafe(no_mangle)]
pub extern "C" fn deleteAllCInterpreterDSPFactories() {
    for ptr in cache_drain() {
        unsafe {
            free_factory(ptr);
        }
    }
}

/// Return all factory SHA keys as a null-terminated array of C strings.
///
/// Each string in the returned array must be freed with `freeCMemory`.
/// The outer array pointer itself must also be freed with `freeCMemory`.
///
/// Returns null if the cache is empty.
#[unsafe(no_mangle)]
pub extern "C" fn getAllCInterpreterDSPFactories() -> *mut *mut c_char {
    let keys = cache_all_sha_keys();
    if keys.is_empty() {
        return std::ptr::null_mut();
    }
    // Allocate an array of (keys.len() + 1) pointers (null-terminated).
    let mut ptrs: Vec<*mut c_char> = keys
        .into_iter()
        .map(|k| alloc_c_string(&k))
        .collect();
    ptrs.push(std::ptr::null_mut()); // null terminator

    let boxed: Box<[*mut c_char]> = ptrs.into_boxed_slice();
    // SAFETY: box raw ptr to the first element of the slice.
    let raw = Box::into_raw(boxed);
    raw.cast::<*mut c_char>()
}

/// Return the JSON description of a factory's UI and metadata.
///
/// The returned string must be freed with `freeCMemory`.
///
/// # Safety
/// `factory` must be a valid non-null factory pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getCInterpreterDSPFactoryJSON(
    factory: *mut InterpreterDspFactory,
) -> *mut c_char {
    unsafe {
        if factory.is_null() {
            return std::ptr::null_mut();
        }
        let inner = &(*factory).inner;
        let json = build_json(inner);
        alloc_c_string(&json)
    }
}

/// Return library dependencies of a factory (always empty for the interpreter).
///
/// The returned null-terminated array must be freed by the caller using
/// `freeCMemory` on each element and then on the array itself.
///
/// # Safety
/// `factory` must be a valid non-null factory pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getCInterpreterDSPFactoryLibraryList(
    _factory: *mut InterpreterDspFactory,
) -> *const *const c_char {
    // The interpreter backend has no external library dependencies.
    // Wrap in a Sync-safe newtype because raw pointers are not Sync by default.
    struct SyncNullArray([*const c_char; 1]);
    unsafe impl Sync for SyncNullArray {}
    static NULL_ARRAY: SyncNullArray = SyncNullArray([std::ptr::null()]);
    NULL_ARRAY.0.as_ptr()
}

// ── Multi-thread mode ─────────────────────────────────────────────────────────

/// Enable multi-thread safe access mode.
///
/// Returns `true` if multi-thread access was successfully started.
#[unsafe(no_mangle)]
pub extern "C" fn startMTDSPFactories() -> bool {
    start_mt()
}

/// Disable multi-thread safe access mode.
#[unsafe(no_mangle)]
pub extern "C" fn stopMTDSPFactories() {
    stop_mt();
}

// ── Memory management ─────────────────────────────────────────────────────────

/// Free a C string (or array of C strings) allocated by this library.
///
/// # Safety
/// `ptr` must be a valid pointer previously returned by one of the `write*`,
/// `getAll*`, or `get*JSON` functions — or null.
///
/// For null-terminated `char**` arrays returned by `getAllCInterpreterDSPFactories`:
/// first call `freeCMemory` on each individual string element, then on the
/// outer array pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn freeCMemory(ptr: *mut c_void) {
    unsafe {
        if !ptr.is_null() {
            free_c_string(ptr as *mut c_char);
        }
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Write an error message into the C error buffer (max 4095 chars + NUL).
///
/// # Safety
/// `buf` must point to at least 4096 bytes or be null.
unsafe fn write_error(buf: *mut c_char, msg: &str) {
    if buf.is_null() {
        return;
    }
    unsafe {
        let bytes = msg.as_bytes();
        let len = bytes.len().min(4095);
        std::ptr::copy_nonoverlapping(bytes.as_ptr().cast::<c_char>(), buf, len);
        *buf.add(len) = 0;
    }
}

/// Build a minimal JSON description of a factory's UI and metadata.
fn build_json(inner: &codegen::backends::interp::FbcDspFactory<f32>) -> String {
    use std::fmt::Write;

    let mut s = String::new();
    let _ = writeln!(s, "{{");
    let _ = writeln!(s, r#"  "name": "{}","#, json_escape(&inner.name));
    let _ = writeln!(s, r#"  "sha_key": "{}","#, json_escape(&inner.sha_key));
    let _ = writeln!(
        s,
        r#"  "compile_options": "{}","#,
        json_escape(&inner.compile_options)
    );
    let _ = writeln!(s, r#"  "version": "{}","#, FAUST_VERSION);
    let _ = writeln!(s, r#"  "inputs": {},"#, inner.num_inputs);
    let _ = writeln!(s, r#"  "outputs": {},"#, inner.num_outputs);

    // Meta block
    let _ = write!(s, r#"  "meta": ["#);
    for (i, m) in inner.meta_block.iter().enumerate() {
        if i > 0 {
            let _ = write!(s, ", ");
        }
        let _ = write!(
            s,
            r#"{{ "{}": "{}" }}"#,
            json_escape(&m.key),
            json_escape(&m.value)
        );
    }
    let _ = writeln!(s, r"],");

    // UI block (simplified — just list widgets)
    let _ = write!(s, r#"  "ui": ["#);
    for (i, u) in inner.ui_block.iter().enumerate() {
        if i > 0 {
            let _ = write!(s, ", ");
        }
        let _ = write!(
            s,
            r#"{{ "type": "{}", "label": "{}", "address": {} }}"#,
            json_escape(&format!("{:?}", u.opcode)),
            json_escape(&u.label),
            u.offset
        );
    }
    let _ = writeln!(s, "]");

    let _ = write!(s, "}}");
    s
}

/// Escape a string for JSON output.
fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
