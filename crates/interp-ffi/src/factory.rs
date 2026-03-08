//! Factory-level `extern "C"` functions.
//!
//! Implements the C API from `interpreter-dsp-c.h` for factory lifecycle,
//! bitcode serialization, and the global factory cache.
//!
//! # Scope
//! - `readCInterpreterDSPFactoryFromBitcode[File]` — auto-detects `float`/`double`
//!   from the `.fbc` header and deserializes the matching variant.
//! - `writeCInterpreterDSPFactoryToBitcode[File]` — dispatches on the factory variant.
//! - `createCInterpreterDSPFactoryFromFile/String` — compiled through the
//!   top-level `compiler` crate; recognizes `-double` in `argv`.
//! - `createCInterpreterDSPFactoryFromSignals/Boxes` — return `null`.
//! - Cache management functions — fully implemented.

use std::ffi::{CStr, CString, c_char, c_void};
use std::io::{BufRead, BufReader};
use std::path::Path;

use codegen::backends::interp::{FAUST_VERSION, read_fbc};
use compiler::{Compiler as FaustCompiler, RealType, SignalFirLane, default_import_search_paths};
use utils::{
    FfiCompileArgs, decode_c_argv as decode_c_argv_shared, free_c_memory_c_string_only,
    null_c_string_array, optional_c_str_arg,
    parse_ffi_compile_args as parse_ffi_compile_args_shared, required_c_str_arg, write_error_4096,
};

use crate::cache::{
    cache_all_sha_keys, cache_drain, cache_insert, cache_lookup, cache_remove_by_ptr, start_mt,
    stop_mt,
};
use crate::types::{
    FbcDspFactoryAny, InterpreterDspFactory, alloc_c_string, alloc_factory, free_factory,
    write_fbc_any,
};

// ── Version ───────────────────────────────────────────────────────────────────

/// Returns the Faust library version string.
///
/// The returned pointer is valid for the lifetime of the process (static data).
///
/// # Safety
/// The returned pointer must not be freed or mutated by the caller.
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
/// The precision (`float` or `double`) is auto-detected from the `.fbc` header.
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
        match read_fbc_any(&mut reader) {
            Ok(factory) => {
                let sha = factory.sha_key().to_owned();
                let ptr = alloc_factory(factory);
                cache_insert(&sha, ptr);
                ptr
            }
            Err(e) => {
                write_error(error_msg, &e.to_string());
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
/// Returns a heap-allocated C string.  The caller must free it with `freeCMemory`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn writeCInterpreterDSPFactoryToBitcode(
    factory: *mut InterpreterDspFactory,
) -> *mut c_char {
    unsafe {
        if factory.is_null() {
            return std::ptr::null_mut();
        }
        let mut buf: Vec<u8> = Vec::new();
        match write_fbc_any(&(*factory).inner, &mut buf) {
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
/// The precision (`float` or `double`) is auto-detected from the `.fbc` header.
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
        let path = match required_c_str_arg(bit_code_path, "path") {
            Ok(s) => s,
            Err(e) => {
                write_error(error_msg, &e);
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
        match read_fbc_any(&mut reader) {
            Ok(factory) => {
                let sha = factory.sha_key().to_owned();
                let ptr = alloc_factory(factory);
                cache_insert(&sha, ptr);
                ptr
            }
            Err(e) => {
                write_error(error_msg, &e.to_string());
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
        let path = match required_c_str_arg(bit_code_path, "path") {
            Ok(s) => s,
            Err(_) => return false,
        };
        let file = match std::fs::File::create(path) {
            Ok(f) => f,
            Err(_) => return false,
        };
        let mut writer = std::io::BufWriter::new(file);
        write_fbc_any(&(*factory).inner, &mut writer).is_ok()
    }
}

// ── Factory constructors (compiler pipeline) ──────────────────────────────────

/// Create a DSP factory from a Faust source file using the compiler fast-lane.
///
/// Accepts `-double` in `argv` to produce a double-precision factory.
///
/// # Safety
/// Pointer arguments must follow the C API contract (null-terminated strings).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createCInterpreterDSPFactoryFromFile(
    filename: *const c_char,
    argc: i32,
    argv: *const *const c_char,
    error_msg: *mut c_char,
) -> *mut InterpreterDspFactory {
    unsafe {
        let filename = match required_c_str_arg(filename, "filename") {
            Ok(s) => s,
            Err(e) => {
                write_error(error_msg, &e);
                return std::ptr::null_mut();
            }
        };
        let argv = match decode_c_argv(argc, argv) {
            Ok(args) => args,
            Err(e) => {
                write_error(error_msg, &e);
                return std::ptr::null_mut();
            }
        };
        create_interp_factory_with_argv(&argv, error_msg, |argv| {
            compile_factory_from_file_fastlane(Path::new(filename), argv)
        })
    }
}

/// Create a DSP factory from a Faust source string using the compiler fast-lane.
///
/// Accepts `-double` in `argv` to produce a double-precision factory.
///
/// # Safety
/// Pointer arguments must follow the C API contract (null-terminated strings).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn createCInterpreterDSPFactoryFromString(
    name_app: *const c_char,
    dsp_content: *const c_char,
    argc: i32,
    argv: *const *const c_char,
    error_msg: *mut c_char,
) -> *mut InterpreterDspFactory {
    unsafe {
        if dsp_content.is_null() {
            write_error(error_msg, "null dsp_content pointer");
            return std::ptr::null_mut();
        }
        let source_name = match optional_c_str_arg(name_app, "name_app") {
            Ok(Some(s)) if !s.is_empty() => s,
            Ok(_) => "FaustDSP",
            Err(e) => {
                write_error(error_msg, &e);
                return std::ptr::null_mut();
            }
        };
        let dsp_content = match required_c_str_arg(dsp_content, "dsp_content") {
            Ok(s) => s,
            Err(e) => {
                write_error(error_msg, &e);
                return std::ptr::null_mut();
            }
        };
        let argv = match decode_c_argv(argc, argv) {
            Ok(args) => args,
            Err(e) => {
                write_error(error_msg, &e);
                return std::ptr::null_mut();
            }
        };
        create_interp_factory_with_argv(&argv, error_msg, |argv| {
            compile_factory_from_string_fastlane(source_name, dsp_content, argv)
        })
    }
}

// ── Cache management ──────────────────────────────────────────────────────────

/// Look up a factory in the cache by SHA key.
///
/// # Safety
/// `sha_key` must be a valid null-terminated C string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getCInterpreterDSPFactoryFromSHAKey(
    sha_key: *const c_char,
) -> *mut InterpreterDspFactory {
    unsafe {
        let sha = match required_c_str_arg(sha_key, "sha_key") {
            Ok(s) => s,
            Err(_) => return std::ptr::null_mut(),
        };
        cache_lookup(sha)
    }
}

/// Delete a DSP factory (frees the Rust allocation).
///
/// # Safety
/// `factory` must be a valid non-null factory pointer or null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn deleteCInterpreterDSPFactory(factory: *mut InterpreterDspFactory) -> bool {
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
///
/// # Safety
/// Callers must ensure no live instances still reference the deleted factories.
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
/// # Safety
/// The caller owns the returned allocation and must free each string element,
/// then free the outer array pointer using `freeCMemory`.
#[unsafe(no_mangle)]
pub extern "C" fn getAllCInterpreterDSPFactories() -> *mut *mut c_char {
    let keys = cache_all_sha_keys();
    if keys.is_empty() {
        return std::ptr::null_mut();
    }
    let mut ptrs: Vec<*mut c_char> = keys.into_iter().map(|k| alloc_c_string(&k)).collect();
    ptrs.push(std::ptr::null_mut());
    let boxed: Box<[*mut c_char]> = ptrs.into_boxed_slice();
    let raw = Box::into_raw(boxed);
    raw.cast::<*mut c_char>()
}

/// Return the JSON description of a factory's UI and metadata.
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
        let json = build_json(&(*factory).inner);
        alloc_c_string(&json)
    }
}

/// Return library dependencies of a factory (always empty for the interpreter).
///
/// # Safety
/// `factory` must be a valid non-null factory pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn getCInterpreterDSPFactoryLibraryList(
    _factory: *mut InterpreterDspFactory,
) -> *const *const c_char {
    null_c_string_array()
}

// ── Multi-thread mode ─────────────────────────────────────────────────────────

/// Enable multi-thread safe access mode.
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
#[unsafe(no_mangle)]
pub unsafe extern "C" fn freeCMemory(ptr: *mut c_void) {
    unsafe { free_c_memory_c_string_only(ptr) }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

/// Write an error message into the C error buffer (max 4095 chars + NUL).
///
/// # Safety
/// `buf` must point to at least 4096 bytes or be null.
unsafe fn write_error(buf: *mut c_char, msg: &str) {
    unsafe { write_error_4096(buf, msg) }
}

/// Auto-detect precision from the `.fbc` header and deserialize the factory.
///
/// The first line of a `.fbc` file is `interpreter_dsp_factory float` or
/// `interpreter_dsp_factory double`.  We peek at that line (without consuming
/// the rest of the reader) to select the correct `read_fbc` instantiation.
fn read_fbc_any(reader: &mut dyn BufRead) -> Result<FbcDspFactoryAny, String> {
    // Buffer the entire content first so we can re-read after peeking.
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .map_err(|e| format!("I/O error reading .fbc content: {e}"))?;

    let first_line = content.lines().next().unwrap_or("");
    let is_double = first_line.trim_end().ends_with("double");

    if is_double {
        let mut cursor = std::io::Cursor::new(content.as_bytes());
        read_fbc::<f64>(&mut cursor)
            .map(FbcDspFactoryAny::Float64)
            .map_err(|e| e.to_string())
    } else {
        let mut cursor = std::io::Cursor::new(content.as_bytes());
        read_fbc::<f32>(&mut cursor)
            .map(FbcDspFactoryAny::Float32)
            .map_err(|e| e.to_string())
    }
}

/// Central post-`argv` FFI factory creation flow.
///
/// All file and string constructors funnel through here to share:
/// - C error-buffer wiring,
/// - cache insertion,
/// - final opaque pointer allocation.
fn create_interp_factory_with_argv<F>(
    argv: &[String],
    error_msg: *mut c_char,
    compile: F,
) -> *mut InterpreterDspFactory
where
    F: FnOnce(&[String]) -> Result<FbcDspFactoryAny, String>,
{
    match compile(argv) {
        Ok(factory) => {
            let sha = factory.sha_key().to_owned();
            let ptr = alloc_factory(factory);
            cache_insert(&sha, ptr);
            ptr
        }
        Err(e) => {
            unsafe { write_error(error_msg, &e) };
            std::ptr::null_mut()
        }
    }
}

/// Compile a Faust source file to an interpreter factory via the compiler
/// facade using the transform fast-lane.
///
/// Respects `-double` in `argv`.
fn compile_factory_from_file_fastlane(
    path: &Path,
    argv: &[String],
) -> Result<FbcDspFactoryAny, String> {
    let parsed = parse_ffi_compile_args(argv)?;
    let real_type = ffi_real_type(&parsed);
    let interp_options = codegen::backends::interp::InterpOptions {
        module_name: parsed.module_name,
        ..codegen::backends::interp::InterpOptions::default()
    };

    let mut search_paths = default_import_search_paths(path);
    search_paths.extend(parsed.search_paths);

    let compiler = FaustCompiler::new().with_real_type(real_type);
    let fbc = compiler
        .compile_file_to_interp_with_lane(
            path,
            &search_paths,
            &interp_options,
            SignalFirLane::TransformFastLane,
        )
        .map_err(|e| format!("{e}"))?;
    compile_factory_from_fbc_text(&fbc)
}

/// Compile a Faust source string to an interpreter factory via the compiler
/// facade using the transform fast-lane.
///
/// Respects `-double` in `argv`.
fn compile_factory_from_string_fastlane(
    source_name: &str,
    source: &str,
    argv: &[String],
) -> Result<FbcDspFactoryAny, String> {
    let parsed = parse_ffi_compile_args(argv)?;
    let real_type = ffi_real_type(&parsed);
    let interp_options = codegen::backends::interp::InterpOptions {
        module_name: parsed.module_name.or_else(|| Some(source_name.to_owned())),
        ..codegen::backends::interp::InterpOptions::default()
    };

    let compiler = FaustCompiler::new().with_real_type(real_type);
    let fbc = compiler
        .compile_source_to_interp_with_lane(
            source_name,
            source,
            &interp_options,
            SignalFirLane::TransformFastLane,
        )
        .map_err(|e| format!("{e}"))?;
    compile_factory_from_fbc_text(&fbc)
}

/// Parse in-memory `.fbc` text back into an owned factory of the appropriate
/// precision (auto-detected from the header).
fn compile_factory_from_fbc_text(fbc: &str) -> Result<FbcDspFactoryAny, String> {
    let mut cursor = std::io::Cursor::new(fbc.as_bytes());
    read_fbc_any(&mut cursor)
}

/// Map `FfiCompileArgs.double` to a `RealType` for the compiler.
fn ffi_real_type(parsed: &FfiCompileArgs) -> RealType {
    if parsed.double {
        RealType::Float64
    } else {
        RealType::Float32
    }
}

/// Build a minimal JSON description of a factory's UI and metadata.
fn build_json(inner: &FbcDspFactoryAny) -> String {
    use std::fmt::Write;

    let mut s = String::new();
    let _ = writeln!(s, "{{");
    let _ = writeln!(s, r#"  "name": "{}","#, json_escape(inner.name()));
    let _ = writeln!(s, r#"  "sha_key": "{}","#, json_escape(inner.sha_key()));
    let _ = writeln!(
        s,
        r#"  "compile_options": "{}","#,
        json_escape(inner.compile_options())
    );
    let _ = writeln!(s, r#"  "version": "{}","#, FAUST_VERSION);
    let _ = writeln!(s, r#"  "inputs": {},"#, inner.num_inputs());
    let _ = writeln!(s, r#"  "outputs": {},"#, inner.num_outputs());
    let _ = writeln!(
        s,
        r#"  "precision": "{}","#,
        if inner.is_double() { "double" } else { "float" }
    );

    // Meta block
    let _ = write!(s, r#"  "meta": ["#);
    for (i, m) in inner.meta_block().iter().enumerate() {
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

    // UI block — widget listing; type-erased via `FbcDspFactoryAny` helpers.
    let _ = write!(s, r#"  "ui": ["#);
    match inner {
        FbcDspFactoryAny::Float32(f) => {
            for (i, u) in f.ui_block.iter().enumerate() {
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
        }
        FbcDspFactoryAny::Float64(f) => {
            for (i, u) in f.ui_block.iter().enumerate() {
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
        }
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

/// Decode the `argc`/`argv` pair from the C API into owned UTF-8 Rust strings.
///
/// # Safety
/// Each entry must be a valid null-terminated C string.
unsafe fn decode_c_argv(argc: i32, argv: *const *const c_char) -> Result<Vec<String>, String> {
    if argc <= 0 {
        return Ok(Vec::new());
    }
    unsafe { decode_c_argv_shared(argc, argv) }
}

/// Parse the FFI-supported subset of Faust CLI options.
fn parse_ffi_compile_args(argv: &[String]) -> Result<FfiCompileArgs, String> {
    parse_ffi_compile_args_shared(argv)
}

#[cfg(test)]
mod tests {
    use super::{compile_factory_from_string_fastlane, parse_ffi_compile_args};

    #[test]
    fn parse_ffi_compile_args_accepts_i_cn_and_double() {
        let argv = vec![
            "-I".to_owned(),
            "lib1".to_owned(),
            "-I".to_owned(),
            "lib2".to_owned(),
            "-cn".to_owned(),
            "MyDSP".to_owned(),
            "-double".to_owned(),
        ];
        let parsed = parse_ffi_compile_args(&argv).expect("ffi args should parse");
        assert_eq!(parsed.search_paths.len(), 2);
        assert_eq!(parsed.search_paths[0], std::path::PathBuf::from("lib1"));
        assert_eq!(parsed.search_paths[1], std::path::PathBuf::from("lib2"));
        assert_eq!(parsed.module_name.as_deref(), Some("MyDSP"));
        assert!(parsed.double);
    }

    #[test]
    fn create_factory_from_string_float_wires_interp_fastlane() {
        let factory = compile_factory_from_string_fastlane(
            "UnitTestDSP",
            "process = _;",
            &["-cn".to_owned(), "UnitTestDSP".to_owned()],
        )
        .expect("fast-lane interp float compilation should succeed");
        assert_eq!(factory.num_inputs(), 1);
        assert_eq!(factory.num_outputs(), 1);
        assert_eq!(factory.name(), "UnitTestDSP");
        assert!(!factory.is_double());
    }

    #[test]
    fn create_factory_from_string_double_produces_double_factory() {
        let factory = compile_factory_from_string_fastlane(
            "UnitTestDSPDouble",
            "process = _;",
            &[
                "-cn".to_owned(),
                "UnitTestDSPDouble".to_owned(),
                "-double".to_owned(),
            ],
        )
        .expect("fast-lane interp double compilation should succeed");
        assert_eq!(factory.num_inputs(), 1);
        assert_eq!(factory.num_outputs(), 1);
        assert_eq!(factory.name(), "UnitTestDSPDouble");
        assert!(factory.is_double(), "expected double factory");
    }

    /// Verify that an f32 factory can actually execute and produce non-zero
    /// audio output.  Uses `process = _;` (passthrough) and feeds a non-zero
    /// f32 buffer; all output samples must be non-zero.
    #[test]
    fn float_factory_execute_produces_nonzero_output() {
        use crate::instance::{
            computeCInterpreterDSPInstance, createCInterpreterDSPInstance,
            deleteCInterpreterDSPInstance, initCInterpreterDSPInstance,
        };
        use crate::types::alloc_factory;

        let factory_any = compile_factory_from_string_fastlane(
            "ExecFloat",
            "process = _;",
            &["-cn".to_owned(), "ExecFloat".to_owned()],
        )
        .expect("float passthrough compilation should succeed");

        assert!(!factory_any.is_double(), "must be an f32 factory");

        // Box the factory and create an instance.
        let factory_ptr = alloc_factory(factory_any);
        let dsp = unsafe { createCInterpreterDSPInstance(factory_ptr) };
        assert!(!dsp.is_null(), "instance creation must succeed");

        unsafe { initCInterpreterDSPInstance(dsp, 44100) };

        // Prepare input buffer with non-zero samples and zeroed output buffer.
        const FRAMES: usize = 64;
        let input_data: Vec<f32> = (0..FRAMES).map(|i| (i as f32) * 0.01 + 0.1).collect();
        let mut output_data = vec![0.0_f32; FRAMES];

        let mut input_ptr: *mut f32 = input_data.as_ptr() as *mut f32;
        let mut output_ptr: *mut f32 = output_data.as_mut_ptr();

        unsafe {
            computeCInterpreterDSPInstance(
                dsp,
                FRAMES as i32,
                &mut input_ptr as *mut *mut f32,
                &mut output_ptr as *mut *mut f32,
            );
        }

        // For a passthrough DSP the output must exactly equal the input.
        // All values should be non-zero (input starts at 0.1).
        let all_nonzero = output_data.iter().all(|&s| s.abs() > 1e-6);
        assert!(
            all_nonzero,
            "float passthrough produced silence: first samples = {:?}",
            &output_data[..8]
        );

        // Cleanup.
        unsafe { deleteCInterpreterDSPInstance(dsp) };
        unsafe { crate::types::free_factory(factory_ptr) };
    }

    /// Verify that a f64 factory can actually execute and produce non-zero
    /// audio output.  Uses `process = _;` (passthrough) and feeds a constant
    /// non-zero f32 buffer; the output must round-trip correctly through the
    /// f32→f64→f32 conversion path.
    #[test]
    fn double_factory_execute_produces_nonzero_output() {
        use crate::instance::{
            computeCInterpreterDSPInstance, createCInterpreterDSPInstance,
            deleteCInterpreterDSPInstance, initCInterpreterDSPInstance,
        };
        use crate::types::alloc_factory;

        let factory_any = compile_factory_from_string_fastlane(
            "ExecDouble",
            "process = _;",
            &[
                "-cn".to_owned(),
                "ExecDouble".to_owned(),
                "-double".to_owned(),
            ],
        )
        .expect("double passthrough compilation should succeed");

        assert!(factory_any.is_double(), "must be a double factory");

        // Box the factory and create an instance.
        let factory_ptr = alloc_factory(factory_any);
        let dsp = unsafe { createCInterpreterDSPInstance(factory_ptr) };
        assert!(!dsp.is_null(), "instance creation must succeed");

        unsafe { initCInterpreterDSPInstance(dsp, 44100) };

        // Prepare input buffer with non-zero samples and zeroed output buffer.
        const FRAMES: usize = 64;
        let input_data: Vec<f32> = (0..FRAMES).map(|i| (i as f32) * 0.01 + 0.1).collect();
        let mut output_data = vec![0.0_f32; FRAMES];

        let mut input_ptr: *mut f32 = input_data.as_ptr() as *mut f32;
        let mut output_ptr: *mut f32 = output_data.as_mut_ptr();

        unsafe {
            computeCInterpreterDSPInstance(
                dsp,
                FRAMES as i32,
                &mut input_ptr as *mut *mut f32,
                &mut output_ptr as *mut *mut f32,
            );
        }

        // For a passthrough DSP the output must match the input after f32→f64→f32.
        // All values should be non-zero (input starts at 0.1).
        let all_nonzero = output_data.iter().all(|&s| s.abs() > 1e-6);
        assert!(
            all_nonzero,
            "double passthrough produced silence: first samples = {:?}",
            &output_data[..8]
        );

        // Cleanup.
        unsafe { deleteCInterpreterDSPInstance(dsp) };
        unsafe { crate::types::free_factory(factory_ptr) };
    }
}
