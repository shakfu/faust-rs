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
use std::path::{Path, PathBuf};

use codegen::backends::cranelift::{
    CraneliftOptLevel, CraneliftOptions, JitDspModule, compile_fir_to_cranelift_jit,
};
use compiler::{Compiler as FaustCompiler, SignalFirLane, default_import_search_base};

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
        let jit = match preflight_compile_file_to_cranelift(Path::new(filename), &args, opt_level) {
            Ok(jit) => jit,
            Err(e) => {
                write_error(error_msg, &e);
                return std::ptr::null_mut();
            }
        };
        let ptr = alloc_factory(build_scaffold_factory_from_file(
            filename,
            &args,
            opt_level,
            Some(jit),
        ));
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
        let jit = match preflight_compile_source_to_cranelift(name_app, dsp_content, opt_level) {
            Ok(jit) => jit,
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
            Some(jit),
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
    bit_code: *const c_char,
    error_msg: *mut c_char,
) -> *mut CraneliftDspFactory {
    unsafe {
        if bit_code.is_null() {
            write_error(error_msg, "null bit_code pointer");
            return std::ptr::null_mut();
        }
        let text = match CStr::from_ptr(bit_code).to_str() {
            Ok(s) => s,
            Err(e) => {
                write_error(error_msg, &format!("invalid UTF-8 in bitcode: {e}"));
                return std::ptr::null_mut();
            }
        };
        match decode_scaffold_bitcode(text) {
            Ok(factory) => {
                let ptr = alloc_factory(factory);
                cache_insert(&(*ptr).sha_key, ptr);
                ptr
            }
            Err(e) => {
                write_error(error_msg, &e);
                std::ptr::null_mut()
            }
        }
    }
}

/// Write a Cranelift factory to a backend bitcode string (symbol present, not implemented).
///
/// # Safety
/// `factory` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn writeCCraneliftDSPFactoryToBitcode(
    factory: *mut CraneliftDspFactory,
) -> *mut c_char {
    unsafe {
        if factory.is_null() {
            return std::ptr::null_mut();
        }
        alloc_c_string(&encode_scaffold_bitcode(&*factory))
    }
}

/// Read a Cranelift factory from a bitcode file (symbol present, not implemented).
///
/// # Safety
/// `error_msg` follows the standard Faust C API error-buffer contract.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn readCCraneliftDSPFactoryFromBitcodeFile(
    bit_code_path: *const c_char,
    error_msg: *mut c_char,
) -> *mut CraneliftDspFactory {
    unsafe {
        if bit_code_path.is_null() {
            write_error(error_msg, "null bit_code_path pointer");
            return std::ptr::null_mut();
        }
        let path = match CStr::from_ptr(bit_code_path).to_str() {
            Ok(s) => s,
            Err(e) => {
                write_error(error_msg, &format!("invalid UTF-8 in path: {e}"));
                return std::ptr::null_mut();
            }
        };
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                write_error(
                    error_msg,
                    &format!("cannot read bitcode file '{path}': {e}"),
                );
                return std::ptr::null_mut();
            }
        };
        match decode_scaffold_bitcode(&text) {
            Ok(factory) => {
                let ptr = alloc_factory(factory);
                cache_insert(&(*ptr).sha_key, ptr);
                ptr
            }
            Err(e) => {
                write_error(error_msg, &e);
                std::ptr::null_mut()
            }
        }
    }
}

/// Write a Cranelift factory to a bitcode file (symbol present, not implemented).
///
/// # Safety
/// `factory` and `bit_code_path` may be null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn writeCCraneliftDSPFactoryToBitcodeFile(
    factory: *mut CraneliftDspFactory,
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
        std::fs::write(path, encode_scaffold_bitcode(&*factory)).is_ok()
    }
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
    jit: Option<JitDspModule>,
) -> CraneliftDspFactory {
    let source_name = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("FaustDSP");
    let dsp_code = format!("// scaffold source from file: {filename}");
    build_scaffold_factory_common(source_name, &dsp_code, argv, opt_level, jit)
}

/// Build a placeholder factory from inline DSP source.
fn build_scaffold_factory_from_source(
    name_app: &str,
    dsp_content: &str,
    argv: &[String],
    opt_level: c_int,
    jit: Option<JitDspModule>,
) -> CraneliftDspFactory {
    build_scaffold_factory_common(name_app, dsp_content, argv, opt_level, jit)
}

/// Shared placeholder factory builder.
fn build_scaffold_factory_common(
    name: &str,
    dsp_code: &str,
    argv: &[String],
    opt_level: c_int,
    jit: Option<JitDspModule>,
) -> CraneliftDspFactory {
    let compute_body_lowered = jit
        .as_ref()
        .is_some_and(codegen::backends::cranelift::JitDspModule::compute_body_lowered);
    let compile_options = if argv.is_empty() {
        format!("opt_level={opt_level}; compute_body_lowered={compute_body_lowered}")
    } else {
        format!(
            "opt_level={opt_level}; compute_body_lowered={compute_body_lowered}; argv={}",
            argv.join(" ")
        )
    };
    let sha_key = format!(
        "cranelift-scaffold:{}:{}:{}",
        name,
        opt_level,
        argv.join("\x1f")
    );
    CraneliftDspFactory {
        name: name.to_owned(),
        sha_key,
        dsp_code: dsp_code.to_owned(),
        compile_options,
        json: format!(
            "{{\"name\":\"{}\",\"backend\":\"cranelift\",\"status\":\"scaffold\",\"compute_body_lowered\":{}}}",
            json_escape(name),
            if compute_body_lowered {
                "true"
            } else {
                "false"
            }
        ),
        compiled_jit: jit,
        compute_body_lowered,
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

/// Runs the real compiler pipeline to FIR, then calls the Cranelift backend placeholder.
///
/// `NotImplemented` from the backend is treated as success during scaffold phases,
/// because the goal is to validate the front-end and FIR path integration first.
fn preflight_compile_file_to_cranelift(
    path: &Path,
    argv: &[String],
    opt_level: c_int,
) -> Result<JitDspModule, String> {
    let compiler = FaustCompiler::new();
    let search_paths = collect_search_paths_for_file(path, argv);
    let fir = compiler
        .compile_file_to_fir_with_lane(path, &search_paths, SignalFirLane::TransformFastLane)
        .map_err(|e| e.to_string())?;
    compile_with_cranelift_backend(fir, opt_level)
}

/// Runs the real compiler pipeline on inline source to FIR, then the backend placeholder.
fn preflight_compile_source_to_cranelift(
    source_name: &str,
    source: &str,
    opt_level: c_int,
) -> Result<JitDspModule, String> {
    let compiler = FaustCompiler::new();
    let fir = compiler
        .compile_source_to_fir_with_lane(source_name, source, SignalFirLane::TransformFastLane)
        .map_err(|e| e.to_string())?;
    compile_with_cranelift_backend(fir, opt_level)
}

/// Calls the Cranelift backend and returns the compiled JIT module.
fn compile_with_cranelift_backend(
    fir: compiler::FirCompileOutput,
    opt_level: c_int,
) -> Result<JitDspModule, String> {
    let options = CraneliftOptions {
        opt_level: map_c_opt_level(opt_level),
        ..CraneliftOptions::default()
    };
    match compile_fir_to_cranelift_jit(&fir.store, fir.module, &options) {
        Ok(jit) => Ok(jit),
        Err(err) => Err(err.to_string()),
    }
}

/// Maps C integer optimization levels to the current Cranelift backend scaffold enum.
fn map_c_opt_level(level: c_int) -> CraneliftOptLevel {
    match level {
        i if i <= 0 => CraneliftOptLevel::None,
        1 | 2 => CraneliftOptLevel::Speed,
        _ => CraneliftOptLevel::SpeedAndSize,
    }
}

/// Builds import search paths for file compilation from default base + `-I` args.
fn collect_search_paths_for_file(path: &Path, argv: &[String]) -> Vec<PathBuf> {
    let mut paths = vec![default_import_search_base(path)];
    let mut i = 0usize;
    while i < argv.len() {
        let arg = &argv[i];
        if arg == "-I" {
            if let Some(next) = argv.get(i + 1) {
                paths.push(PathBuf::from(next));
                i += 2;
                continue;
            }
        } else if let Some(rest) = arg.strip_prefix("-I")
            && !rest.is_empty()
        {
            paths.push(PathBuf::from(rest));
        }
        i += 1;
    }
    paths
}

/// Encodes a scaffold factory into a temporary text payload for the bitcode family.
///
/// This is a placeholder serialization format used only until real Cranelift
/// backend serialization is implemented.
fn encode_scaffold_bitcode(factory: &CraneliftDspFactory) -> String {
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\").replace('\n', "\\n")
    }
    format!(
        "CRANELIFT_FFI_SCAFFOLD_V1\nname={}\nsha={}\ninputs={}\noutputs={}\ncompile_options={}\ndsp_code={}\njson={}\n",
        esc(&factory.name),
        esc(&factory.sha_key),
        factory.num_inputs,
        factory.num_outputs,
        esc(&factory.compile_options),
        esc(&factory.dsp_code),
        esc(&factory.json),
    )
}

/// Decodes the temporary scaffold bitcode payload back into a factory scaffold.
fn decode_scaffold_bitcode(text: &str) -> Result<CraneliftDspFactory, String> {
    fn unesc(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut it = s.chars();
        while let Some(ch) = it.next() {
            if ch == '\\' {
                match it.next() {
                    Some('n') => out.push('\n'),
                    Some('\\') => out.push('\\'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    let mut lines = text.lines();
    match lines.next() {
        Some("CRANELIFT_FFI_SCAFFOLD_V1") => {}
        Some(_) => return Err("unsupported cranelift scaffold bitcode format".to_owned()),
        None => return Err("empty bitcode payload".to_owned()),
    }

    let mut name = None;
    let mut sha_key = None;
    let mut compile_options = None;
    let mut dsp_code = None;
    let mut json = None;
    let mut num_inputs = None;
    let mut num_outputs = None;

    for line in lines {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "name" => name = Some(unesc(v)),
            "sha" => sha_key = Some(unesc(v)),
            "compile_options" => compile_options = Some(unesc(v)),
            "dsp_code" => dsp_code = Some(unesc(v)),
            "json" => json = Some(unesc(v)),
            "inputs" => num_inputs = v.parse::<i32>().ok(),
            "outputs" => num_outputs = v.parse::<i32>().ok(),
            _ => {}
        }
    }

    Ok(CraneliftDspFactory {
        name: name.ok_or_else(|| "missing 'name' field".to_owned())?,
        sha_key: sha_key.ok_or_else(|| "missing 'sha' field".to_owned())?,
        dsp_code: dsp_code.ok_or_else(|| "missing 'dsp_code' field".to_owned())?,
        compile_options: compile_options
            .ok_or_else(|| "missing 'compile_options' field".to_owned())?,
        json: json.ok_or_else(|| "missing 'json' field".to_owned())?,
        compiled_jit: None,
        compute_body_lowered: false,
        num_inputs: num_inputs.ok_or_else(|| "missing 'inputs' field".to_owned())?,
        num_outputs: num_outputs.ok_or_else(|| "missing 'outputs' field".to_owned())?,
    })
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
        readCCraneliftDSPFactoryFromBitcode, readCCraneliftDSPFactoryFromBitcodeFile,
        writeCCraneliftDSPFactoryToBitcode, writeCCraneliftDSPFactoryToBitcodeFile,
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
            assert!((*factory).compiled_jit.is_some());
            let lowered = (*factory).compute_body_lowered;
            assert!(json_s.contains(&format!(
                "\"compute_body_lowered\":{}",
                if lowered { "true" } else { "false" }
            )));
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
    fn create_factory_from_string_reports_compiler_error_for_invalid_faust() {
        let name = c"bad";
        let src = c"process = ;";
        let mut err = [0_i8; 4096];

        let factory = unsafe {
            createCCraneliftDSPFactoryFromString(
                name.as_ptr(),
                src.as_ptr(),
                0,
                std::ptr::null(),
                err.as_mut_ptr(),
                0,
            )
        };
        assert!(factory.is_null());
        let msg = unsafe { CStr::from_ptr(err.as_ptr()) }.to_str().unwrap();
        assert!(!msg.is_empty());
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

    #[test]
    fn scaffold_bitcode_roundtrip_in_memory() {
        let name = c"bitcode";
        let src = c"process = _;";
        let mut err = [0_i8; 4096];
        let factory = unsafe {
            createCCraneliftDSPFactoryFromString(
                name.as_ptr(),
                src.as_ptr(),
                0,
                std::ptr::null(),
                err.as_mut_ptr(),
                1,
            )
        };
        assert!(!factory.is_null());

        let bitcode = unsafe { writeCCraneliftDSPFactoryToBitcode(factory) };
        assert!(!bitcode.is_null());

        let restored =
            unsafe { readCCraneliftDSPFactoryFromBitcode(bitcode.cast_const(), err.as_mut_ptr()) };
        assert!(!restored.is_null());

        unsafe {
            freeCMemory(bitcode.cast());
            assert!(deleteCCraneliftDSPFactory(factory));
            assert!(deleteCCraneliftDSPFactory(restored));
        }
    }

    #[test]
    fn scaffold_bitcode_roundtrip_via_file() {
        let name = c"bitfile";
        let src = c"process = _;";
        let mut err = [0_i8; 4096];
        let factory = unsafe {
            createCCraneliftDSPFactoryFromString(
                name.as_ptr(),
                src.as_ptr(),
                0,
                std::ptr::null(),
                err.as_mut_ptr(),
                1,
            )
        };
        assert!(!factory.is_null());

        let path = std::env::temp_dir().join(format!(
            "faust-rs-cranelift-ffi-{}-{}.fbc.txt",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path_c = std::ffi::CString::new(path.as_os_str().to_string_lossy().as_bytes()).unwrap();

        let wrote = unsafe { writeCCraneliftDSPFactoryToBitcodeFile(factory, path_c.as_ptr()) };
        assert!(wrote);

        let restored =
            unsafe { readCCraneliftDSPFactoryFromBitcodeFile(path_c.as_ptr(), err.as_mut_ptr()) };
        assert!(!restored.is_null());

        let _ = std::fs::remove_file(&path);
        unsafe {
            assert!(deleteCCraneliftDSPFactory(factory));
            assert!(deleteCCraneliftDSPFactory(restored));
        }
    }

    #[test]
    fn scaffold_bitcode_read_rejects_invalid_format() {
        let bad = c"NOT_A_CRANELIFT_FORMAT";
        let mut err = [0_i8; 4096];
        let restored =
            unsafe { readCCraneliftDSPFactoryFromBitcode(bad.as_ptr(), err.as_mut_ptr()) };
        assert!(restored.is_null());
        let msg = unsafe { CStr::from_ptr(err.as_ptr()) }.to_str().unwrap();
        assert!(msg.contains("unsupported") || msg.contains("format"));
    }
}
