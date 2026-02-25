//! Factory-level `extern "C"` functions for `cranelift_dsp` (scaffold ABI).
//!
//! This module exports a first executable C ABI surface matching the Cranelift
//! FFI Phase-0 decisions:
//! - backend-prefixed naming (`createCCranelift...`)
//! - source creation keeps `opt_level`, omits LLVM `target`
//! - several LLVM-only families are intentionally deferred in V1
//!
//! The current implementation is a placeholder runtime layer: symbols exist and
//! support null-safe lifecycle smoke tests, but do not yet invoke the Cranelift
//! backend or real factory caching.

use std::ffi::{CStr, CString, c_char, c_void};
use std::os::raw::c_int;
use std::path::Path;

use crate::cache::{
    cache_all_sha_keys, cache_drain, cache_insert, cache_lookup, cache_remove_by_ptr, start_mt,
    stop_mt,
};
use crate::types::{
    CraneliftDspFactory, alloc_c_string, alloc_factory, free_c_string, free_factory,
};

/// Fixed error buffer size used by the Faust C APIs.
const ERROR_MSG_CAPACITY: usize = 4096;

/// Stable placeholder version string returned by [`getCLibFaustVersion`].
const CRANELIFT_FFI_SCAFFOLD_VERSION: &str =
    concat!("faust-rs-cranelift-ffi/", env!("CARGO_PKG_VERSION"));

/// Returns the Faust library version string.
///
/// This is a process-lifetime static C string in the scaffold implementation.
#[unsafe(no_mangle)]
pub extern "C" fn getCLibFaustVersion() -> *const c_char {
    use std::sync::OnceLock;
    static VERSION_C: OnceLock<CString> = OnceLock::new();
    VERSION_C
        .get_or_init(|| {
            CString::new(CRANELIFT_FFI_SCAFFOLD_VERSION).expect("version contains no NUL")
        })
        .as_ptr()
}

/// Create a Cranelift DSP factory from a Faust source file (scaffold).
///
/// # Safety
/// - `filename` must be a valid null-terminated C string.
/// - `argv` must point to `argc` valid null-terminated C strings (or be null if `argc == 0`).
/// - `error_msg` may be null; otherwise it must reference at least 4096 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createCCraneliftDSPFactoryFromFile(
    filename: *const c_char,
    argc: c_int,
    argv: *const *const c_char,
    error_msg: *mut c_char,
    opt_level: c_int,
) -> *mut CraneliftDspFactory {
    unsafe {
        if filename.is_null() {
            write_error(error_msg, "null filename pointer");
            return std::ptr::null_mut();
        }
        let filename = match CStr::from_ptr(filename).to_str() {
            Ok(s) => s,
            Err(e) => {
                write_error(error_msg, &format!("invalid UTF-8 in filename: {e}"));
                return std::ptr::null_mut();
            }
        };
        let args = match decode_c_argv(argc, argv) {
            Ok(args) => args,
            Err(e) => {
                write_error(error_msg, &e);
                return std::ptr::null_mut();
            }
        };
        let ptr = alloc_factory(build_scaffold_factory_from_file(filename, &args, opt_level));
        cache_insert(&(*ptr).sha_key, ptr);
        ptr
    }
}

/// Create a Cranelift DSP factory from a Faust source string (scaffold).
///
/// # Safety
/// - `dsp_content` must be a valid null-terminated C string.
/// - `name_app` may be null; if non-null it must be a valid C string.
/// - `argv` must point to `argc` valid C strings (or be null if `argc == 0`).
/// - `error_msg` may be null; otherwise it must reference at least 4096 bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createCCraneliftDSPFactoryFromString(
    name_app: *const c_char,
    dsp_content: *const c_char,
    argc: c_int,
    argv: *const *const c_char,
    error_msg: *mut c_char,
    opt_level: c_int,
) -> *mut CraneliftDspFactory {
    unsafe {
        if dsp_content.is_null() {
            write_error(error_msg, "null dsp_content pointer");
            return std::ptr::null_mut();
        }
        let name_app = if name_app.is_null() {
            "FaustDSP"
        } else {
            match CStr::from_ptr(name_app).to_str() {
                Ok(s) if !s.is_empty() => s,
                Ok(_) => "FaustDSP",
                Err(e) => {
                    write_error(error_msg, &format!("invalid UTF-8 in name_app: {e}"));
                    return std::ptr::null_mut();
                }
            }
        };
        let dsp_content = match CStr::from_ptr(dsp_content).to_str() {
            Ok(s) => s,
            Err(e) => {
                write_error(error_msg, &format!("invalid UTF-8 in dsp_content: {e}"));
                return std::ptr::null_mut();
            }
        };
        let args = match decode_c_argv(argc, argv) {
            Ok(args) => args,
            Err(e) => {
                write_error(error_msg, &e);
                return std::ptr::null_mut();
            }
        };
        let ptr = alloc_factory(build_scaffold_factory_from_source(
            name_app,
            dsp_content,
            &args,
            opt_level,
        ));
        cache_insert(&(*ptr).sha_key, ptr);
        ptr
    }
}

/// Create a Cranelift DSP factory from signals (symbol present, not implemented).
///
/// # Safety
/// `error_msg` follows the same contract as other factory creation functions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createCCraneliftDSPFactoryFromSignals(
    _name_app: *const c_char,
    _signals: *mut c_void,
    _argc: c_int,
    _argv: *const *const c_char,
    error_msg: *mut c_char,
    _opt_level: c_int,
) -> *mut CraneliftDspFactory {
    unsafe {
        write_error(
            error_msg,
            "createCCraneliftDSPFactoryFromSignals is not implemented yet",
        );
    }
    std::ptr::null_mut()
}

/// Create a Cranelift DSP factory from boxes (symbol present, not implemented).
///
/// # Safety
/// `error_msg` follows the same contract as other factory creation functions.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createCCraneliftDSPFactoryFromBoxes(
    _name_app: *const c_char,
    _box_expr: *mut c_void,
    _argc: c_int,
    _argv: *const *const c_char,
    error_msg: *mut c_char,
    _opt_level: c_int,
) -> *mut CraneliftDspFactory {
    unsafe {
        write_error(
            error_msg,
            "createCCraneliftDSPFactoryFromBoxes is not implemented yet",
        );
    }
    std::ptr::null_mut()
}

/// Returns a factory from the cache by SHA key.
///
/// # Safety
/// `sha_key` may be null; invalid UTF-8 returns null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getCCraneliftDSPFactoryFromSHAKey(
    sha_key: *const c_char,
) -> *mut CraneliftDspFactory {
    unsafe {
        if sha_key.is_null() {
            return std::ptr::null_mut();
        }
        let sha_key = match CStr::from_ptr(sha_key).to_str() {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        cache_lookup(sha_key)
    }
}

/// Delete a Cranelift DSP factory.
///
/// Returns `true` when a non-null factory pointer was freed.
///
/// # Safety
/// `factory` must be a valid pointer previously returned by a Cranelift factory
/// creation function, and must not be used after this call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn deleteCCraneliftDSPFactory(factory: *mut CraneliftDspFactory) -> bool {
    unsafe {
        if factory.is_null() {
            return false;
        }
        cache_remove_by_ptr(factory);
        free_factory(factory);
        true
    }
}

/// Delete all cached Cranelift factories.
#[unsafe(no_mangle)]
pub extern "C" fn deleteAllCCraneliftDSPFactories() {
    for ptr in cache_drain() {
        unsafe {
            if !ptr.is_null() {
                free_factory(ptr);
            }
        }
    }
}

/// Return all cached Cranelift factory SHA keys as a null-terminated array.
///
/// The returned strings must be freed individually with `freeCMemory`. As in
/// the current `interp-ffi` implementation, the outer array deallocation path is
/// not yet modeled separately in the scaffold.
#[unsafe(no_mangle)]
pub extern "C" fn getAllCCraneliftDSPFactories() -> *mut *mut c_char {
    let keys = cache_all_sha_keys();
    if keys.is_empty() {
        return std::ptr::null_mut();
    }
    let mut ptrs: Vec<*mut c_char> = keys.into_iter().map(|k| alloc_c_string(&k)).collect();
    ptrs.push(std::ptr::null_mut());
    let boxed: Box<[*mut c_char]> = ptrs.into_boxed_slice();
    Box::into_raw(boxed).cast::<*mut c_char>()
}

/// Return a factory JSON description string.
///
/// The returned string must be freed by the caller with [`freeCMemory`].
///
/// # Safety
/// `factory` must be a valid factory pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getCCraneliftDSPFactoryJSON(
    factory: *mut CraneliftDspFactory,
) -> *mut c_char {
    unsafe {
        if factory.is_null() {
            return std::ptr::null_mut();
        }
        alloc_c_string(&(*factory).json)
    }
}

/// Return the factory name as a heap C string.
///
/// # Safety
/// `factory` must be a valid factory pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getCCraneliftDSPFactoryName(
    factory: *mut CraneliftDspFactory,
) -> *mut c_char {
    unsafe {
        if factory.is_null() {
            return std::ptr::null_mut();
        }
        alloc_c_string(&(*factory).name)
    }
}

/// Return the factory SHA key as a heap C string.
///
/// # Safety
/// `factory` must be a valid factory pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getCCraneliftDSPFactorySHAKey(
    factory: *mut CraneliftDspFactory,
) -> *mut c_char {
    unsafe {
        if factory.is_null() {
            return std::ptr::null_mut();
        }
        alloc_c_string(&(*factory).sha_key)
    }
}

/// Return the expanded DSP code as a heap C string (scaffold placeholder).
///
/// # Safety
/// `factory` must be a valid factory pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getCCraneliftDSPFactoryDSPCode(
    factory: *mut CraneliftDspFactory,
) -> *mut c_char {
    unsafe {
        if factory.is_null() {
            return std::ptr::null_mut();
        }
        alloc_c_string(&(*factory).dsp_code)
    }
}

/// Return the compile options string as a heap C string.
///
/// # Safety
/// `factory` must be a valid factory pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getCCraneliftDSPFactoryCompileOptions(
    factory: *mut CraneliftDspFactory,
) -> *mut c_char {
    unsafe {
        if factory.is_null() {
            return std::ptr::null_mut();
        }
        alloc_c_string(&(*factory).compile_options)
    }
}

/// Return the factory library dependency list (scaffold: empty static array).
///
/// # Safety
/// `factory` may be null; it is ignored by the scaffold.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getCCraneliftDSPFactoryLibraryList(
    _factory: *mut CraneliftDspFactory,
) -> *const *const c_char {
    null_c_string_array()
}

/// Return include pathnames used by the factory (scaffold: empty static array).
///
/// # Safety
/// `factory` may be null; it is ignored by the scaffold.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getCCraneliftDSPFactoryIncludePathnames(
    _factory: *mut CraneliftDspFactory,
) -> *const *const c_char {
    null_c_string_array()
}

/// Return warning messages produced during compilation (scaffold: empty static array).
///
/// # Safety
/// `factory` may be null; it is ignored by the scaffold.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getCCraneliftDSPFactoryWarningMessages(
    _factory: *mut CraneliftDspFactory,
) -> *const *const c_char {
    null_c_string_array()
}

/// Read a Cranelift factory from bitcode in memory (symbol present, not implemented).
///
/// # Safety
/// `error_msg` follows the standard Faust C API error-buffer contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn readCCraneliftDSPFactoryFromBitcode(
    _bit_code: *const c_char,
    error_msg: *mut c_char,
) -> *mut CraneliftDspFactory {
    unsafe {
        write_error(
            error_msg,
            "readCCraneliftDSPFactoryFromBitcode is not implemented yet",
        );
    }
    std::ptr::null_mut()
}

/// Write a Cranelift factory to a backend bitcode string (symbol present, not implemented).
///
/// # Safety
/// `factory` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn writeCCraneliftDSPFactoryToBitcode(
    _factory: *mut CraneliftDspFactory,
) -> *mut c_char {
    std::ptr::null_mut()
}

/// Read a Cranelift factory from a bitcode file (symbol present, not implemented).
///
/// # Safety
/// `error_msg` follows the standard Faust C API error-buffer contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn readCCraneliftDSPFactoryFromBitcodeFile(
    _bit_code_path: *const c_char,
    error_msg: *mut c_char,
) -> *mut CraneliftDspFactory {
    unsafe {
        write_error(
            error_msg,
            "readCCraneliftDSPFactoryFromBitcodeFile is not implemented yet",
        );
    }
    std::ptr::null_mut()
}

/// Write a Cranelift factory to a bitcode file (symbol present, not implemented).
///
/// # Safety
/// `factory` and `bit_code_path` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn writeCCraneliftDSPFactoryToBitcodeFile(
    _factory: *mut CraneliftDspFactory,
    _bit_code_path: *const c_char,
) -> bool {
    false
}

/// Enable multi-thread-safe factory mode.
///
/// The scaffold toggles an internal compatibility flag and returns `true`.
#[unsafe(no_mangle)]
pub extern "C" fn startMTDSPFactories() -> bool {
    start_mt()
}

/// Disable multi-thread-safe factory mode (compatibility flag only).
#[unsafe(no_mangle)]
pub extern "C" fn stopMTDSPFactories() {
    stop_mt();
}

/// Free memory allocated by this library for C strings.
///
/// # Safety
/// `ptr` must be null or a pointer previously returned by a Cranelift FFI
/// function that documents `freeCMemory` ownership.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn freeCMemory(ptr: *mut c_void) {
    unsafe {
        if !ptr.is_null() {
            free_c_string(ptr.cast::<c_char>());
        }
    }
}

/// Scaffold-only factory status string kept for unit tests.
#[must_use]
pub fn factory_status() -> &'static str {
    "cranelift-ffi factory scaffold"
}

/// Build a placeholder factory from a source file path.
fn build_scaffold_factory_from_file(
    filename: &str,
    argv: &[String],
    opt_level: c_int,
) -> CraneliftDspFactory {
    let source_name = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("FaustDSP");
    let dsp_code = format!("// scaffold source from file: {filename}");
    build_scaffold_factory_common(source_name, &dsp_code, argv, opt_level)
}

/// Build a placeholder factory from inline DSP source.
fn build_scaffold_factory_from_source(
    name_app: &str,
    dsp_content: &str,
    argv: &[String],
    opt_level: c_int,
) -> CraneliftDspFactory {
    build_scaffold_factory_common(name_app, dsp_content, argv, opt_level)
}

/// Shared placeholder factory builder.
fn build_scaffold_factory_common(
    name: &str,
    dsp_code: &str,
    argv: &[String],
    opt_level: c_int,
) -> CraneliftDspFactory {
    let compile_options = if argv.is_empty() {
        format!("opt_level={opt_level}")
    } else {
        format!("opt_level={opt_level}; argv={}", argv.join(" "))
    };
    let sha_key = format!(
        "cranelift-scaffold:{}:{}:{}",
        name,
        opt_level,
        argv.join("\x1f")
    );
    let json = format!(
        "{{\"name\":\"{}\",\"backend\":\"cranelift\",\"status\":\"scaffold\"}}",
        json_escape(name)
    );
    CraneliftDspFactory {
        name: name.to_owned(),
        sha_key,
        dsp_code: dsp_code.to_owned(),
        compile_options,
        json,
        num_inputs: 1,
        num_outputs: 1,
    }
}

/// Decode a conventional `argc`/`argv` C array into owned Rust strings.
fn decode_c_argv(argc: c_int, argv: *const *const c_char) -> Result<Vec<String>, String> {
    if argc < 0 {
        return Err("negative argc".to_owned());
    }
    let argc = usize::try_from(argc).map_err(|_| "argc out of range".to_owned())?;
    if argc == 0 {
        return Ok(Vec::new());
    }
    if argv.is_null() {
        return Err("argv is null while argc > 0".to_owned());
    }
    let mut out = Vec::with_capacity(argc);
    for idx in 0..argc {
        let ptr = unsafe { *argv.add(idx) };
        if ptr.is_null() {
            return Err(format!("argv[{idx}] is null"));
        }
        let s = unsafe { CStr::from_ptr(ptr) }
            .to_str()
            .map_err(|e| format!("argv[{idx}] invalid UTF-8: {e}"))?;
        out.push(s.to_owned());
    }
    Ok(out)
}

/// Return a static null-terminated empty `char**` array.
fn null_c_string_array() -> *const *const c_char {
    struct SyncNullArray([*const c_char; 1]);
    // SAFETY: Immutable static null pointer array.
    unsafe impl Sync for SyncNullArray {}
    static NULL_ARRAY: SyncNullArray = SyncNullArray([std::ptr::null()]);
    NULL_ARRAY.0.as_ptr()
}

/// Write an error message to a standard 4096-byte Faust error buffer.
///
/// # Safety
/// `buf` must point to at least 4096 bytes or be null.
unsafe fn write_error(buf: *mut c_char, msg: &str) {
    if buf.is_null() {
        return;
    }
    let bytes = msg.as_bytes();
    let len = bytes.len().min(ERROR_MSG_CAPACITY - 1);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr().cast::<c_char>(), buf, len);
        *buf.add(len) = 0;
    }
}

/// Minimal JSON string escaping for scaffold metadata text.
fn json_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;

    use super::{
        createCCraneliftDSPFactoryFromFile, createCCraneliftDSPFactoryFromString,
        deleteAllCCraneliftDSPFactories, deleteCCraneliftDSPFactory, factory_status, freeCMemory,
        getAllCCraneliftDSPFactories, getCCraneliftDSPFactoryCompileOptions,
        getCCraneliftDSPFactoryFromSHAKey, getCCraneliftDSPFactoryJSON,
        getCCraneliftDSPFactoryName, getCCraneliftDSPFactorySHAKey, getCLibFaustVersion,
    };

    #[test]
    fn factory_scaffold_status_is_stable() {
        assert_eq!(factory_status(), "cranelift-ffi factory scaffold");
    }

    #[test]
    fn version_symbol_returns_static_c_string() {
        let ptr = getCLibFaustVersion();
        assert!(!ptr.is_null());
        let s = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap();
        assert!(s.contains("cranelift-ffi"));
    }

    #[test]
    fn create_factory_from_string_scaffold_roundtrip_queries() {
        let name = c"mydsp";
        let src = c"process = _;";
        let args = [c"-vec"];
        let argv = [args[0].as_ptr()];
        let mut err = [0_i8; 4096];

        let factory = unsafe {
            createCCraneliftDSPFactoryFromString(
                name.as_ptr(),
                src.as_ptr(),
                1,
                argv.as_ptr(),
                err.as_mut_ptr(),
                2,
            )
        };
        assert!(!factory.is_null());

        let name_ptr = unsafe { getCCraneliftDSPFactoryName(factory) };
        let json_ptr = unsafe { getCCraneliftDSPFactoryJSON(factory) };
        let opts_ptr = unsafe { getCCraneliftDSPFactoryCompileOptions(factory) };
        assert!(!name_ptr.is_null());
        assert!(!json_ptr.is_null());
        assert!(!opts_ptr.is_null());

        let name_s = unsafe { CStr::from_ptr(name_ptr) }.to_str().unwrap();
        let json_s = unsafe { CStr::from_ptr(json_ptr) }.to_str().unwrap();
        let opts_s = unsafe { CStr::from_ptr(opts_ptr) }.to_str().unwrap();
        assert_eq!(name_s, "mydsp");
        assert!(json_s.contains("\"backend\":\"cranelift\""));
        assert!(opts_s.contains("opt_level=2"));

        unsafe {
            freeCMemory(name_ptr.cast());
            freeCMemory(json_ptr.cast());
            freeCMemory(opts_ptr.cast());
            assert!(deleteCCraneliftDSPFactory(factory));
        }
    }

    #[test]
    fn create_factory_from_file_rejects_null_filename() {
        let mut err = [0_i8; 4096];
        let factory = unsafe {
            createCCraneliftDSPFactoryFromFile(
                std::ptr::null(),
                0,
                std::ptr::null(),
                err.as_mut_ptr(),
                0,
            )
        };
        assert!(factory.is_null());
        let msg = unsafe { CStr::from_ptr(err.as_ptr()) }.to_str().unwrap();
        assert!(msg.contains("null filename"));
    }

    #[test]
    fn cache_lookup_and_list_are_wired_to_created_factories() {
        let name = c"cachetest";
        let src = c"process = _;";
        let mut err = [0_i8; 4096];

        let factory = unsafe {
            createCCraneliftDSPFactoryFromString(
                name.as_ptr(),
                src.as_ptr(),
                0,
                std::ptr::null(),
                err.as_mut_ptr(),
                3,
            )
        };
        assert!(!factory.is_null());

        let sha_ptr = unsafe { getCCraneliftDSPFactorySHAKey(factory) };
        assert!(!sha_ptr.is_null());
        let looked_up = unsafe { getCCraneliftDSPFactoryFromSHAKey(sha_ptr.cast_const()) };
        assert_eq!(looked_up, factory);

        let all_ptr = getAllCCraneliftDSPFactories();
        assert!(!all_ptr.is_null());
        let first = unsafe { *all_ptr };
        assert!(!first.is_null());

        unsafe {
            freeCMemory(sha_ptr.cast());
            // free returned strings (outer array is intentionally not freed in scaffold).
            freeCMemory(first.cast());
            deleteAllCCraneliftDSPFactories();
        }
    }
}
