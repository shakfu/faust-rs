//! `wasm-ffi` — raw WASM exports for the Rust `faustwasm` compile service.
//!
//! # Purpose
//! This crate is the Phase 2 binding layer from
//! `porting/faustwasm-dual-mode-rust-interface-plan-2026-03-26-en.md`.
//! It exposes a minimal request/response ABI on top of the pure-Rust compiler
//! compile service from `crates/compiler`.
//!
//! # Design
//! - raw exports only: no `wasm-bindgen` glue;
//! - one compile request in, one owned result handle out;
//! - JS reads returned buffers through pointer/length accessors, then frees the
//!   result handle explicitly.
//!
//! # Source provenance (C++)
//! - `compiler/generator/wasm/bindings/adapter.cpp`
//! - `compiler/generator/wasm/bindings/adapter.h`
//!
//! # Mapping status
//! `adapted` relative to the historical `libFaustWasm` Emscripten binding:
//! - preserved semantics: compile Faust source -> `{ wasm, json }`;
//! - adapted ABI: integer result handles + raw ptr/len accessors instead of C++
//!   vectors/factory pointers;
//! - partial helper compatibility:
//!   - implemented now: `getInfos(version|help)`
//!   - explicit stubs: `expandDSP`, `generateAuxFiles`, and the remaining
//!     `getInfos(...)` keys
//! - deferred: compatibility naming shim.

#![allow(non_snake_case)]
#![allow(unsafe_code)]

use std::collections::HashMap;
use std::slice;
use std::str;
use std::sync::{Mutex, OnceLock};

use codegen::backends::wasm::WasmOptions;
use compiler::{Compiler, RealType, WasmArtifactBundle, WasmArtifactRequest};

const WASM_FFI_VERSION: &str = concat!("faust-rs-wasm-ffi/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, PartialEq, Eq)]
enum StoredCompileResult {
    Ok(WasmArtifactBundle),
    Err(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StoredTextResult {
    Ok(String),
    Err(String),
}

#[derive(Default)]
struct ResultRegistry {
    next_handle: u32,
    entries: HashMap<u32, StoredCompileResult>,
}

impl ResultRegistry {
    fn insert(&mut self, result: StoredCompileResult) -> u32 {
        let next = self.next_handle.saturating_add(1).max(1);
        self.next_handle = next;
        self.entries.insert(next, result);
        next
    }

    fn get(&self, handle: u32) -> Option<&StoredCompileResult> {
        self.entries.get(&handle)
    }

    fn remove(&mut self, handle: u32) {
        self.entries.remove(&handle);
    }
}

fn registry() -> &'static Mutex<ResultRegistry> {
    static REGISTRY: OnceLock<Mutex<ResultRegistry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(ResultRegistry::default()))
}

fn text_registry() -> &'static Mutex<ResultRegistryText> {
    static REGISTRY: OnceLock<Mutex<ResultRegistryText>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(ResultRegistryText::default()))
}

fn split_faustwasm_args(args: &str) -> Vec<String> {
    args.split(' ').map(str::to_owned).collect()
}

#[derive(Default)]
struct ResultRegistryText {
    next_handle: u32,
    entries: HashMap<u32, StoredTextResult>,
}

impl ResultRegistryText {
    fn insert(&mut self, result: StoredTextResult) -> u32 {
        let next = self.next_handle.saturating_add(1).max(1);
        self.next_handle = next;
        self.entries.insert(next, result);
        next
    }

    fn get(&self, handle: u32) -> Option<&StoredTextResult> {
        self.entries.get(&handle)
    }

    fn remove(&mut self, handle: u32) {
        self.entries.remove(&handle);
    }
}

fn parse_compile_request(
    name: &str,
    source: &str,
    args: &str,
    internal_memory: bool,
) -> Result<WasmArtifactRequest, String> {
    let argv = split_faustwasm_args(args);
    let parsed = utils::parse_ffi_compile_args(&argv)?;
    let mut request = WasmArtifactRequest::new(name, source);
    request.import_dirs = parsed.search_paths;
    request.wasm_options = WasmOptions {
        double_precision: parsed.double,
        internal_memory,
        ..WasmOptions::default()
    };
    Ok(request)
}

fn compile_to_stored_result(
    name: &str,
    source: &str,
    args: &str,
    internal_memory: bool,
) -> StoredCompileResult {
    let request = match parse_compile_request(name, source, args, internal_memory) {
        Ok(request) => request,
        Err(error) => return StoredCompileResult::Err(error),
    };
    let compiler = Compiler::new().with_real_type(if request.wasm_options.double_precision {
        RealType::Float64
    } else {
        RealType::Float32
    });
    match compiler.compile_wasm_artifact(&request) {
        Ok(bundle) => StoredCompileResult::Ok(bundle),
        Err(error) => StoredCompileResult::Err(error.to_string()),
    }
}

unsafe fn decode_utf8_arg<'a>(ptr: *const u8, len: usize, label: &str) -> Result<&'a str, String> {
    if ptr.is_null() {
        return Err(format!("null {label} pointer"));
    }
    let bytes = unsafe { slice::from_raw_parts(ptr, len) };
    str::from_utf8(bytes).map_err(|error| format!("invalid UTF-8 in {label}: {error}"))
}

fn with_result<R>(handle: u32, f: impl FnOnce(Option<&StoredCompileResult>) -> R) -> R {
    let guard = registry().lock().expect("wasm-ffi registry poisoned");
    f(guard.get(handle))
}

fn store_result(result: StoredCompileResult) -> u32 {
    let mut guard = registry().lock().expect("wasm-ffi registry poisoned");
    guard.insert(result)
}

fn with_text_result<R>(handle: u32, f: impl FnOnce(Option<&StoredTextResult>) -> R) -> R {
    let guard = text_registry()
        .lock()
        .expect("wasm-ffi text registry poisoned");
    f(guard.get(handle))
}

fn store_text_result(result: StoredTextResult) -> u32 {
    let mut guard = text_registry()
        .lock()
        .expect("wasm-ffi text registry poisoned");
    guard.insert(result)
}

fn result_bytes_ptr(handle: u32) -> *const u8 {
    with_result(handle, |result| match result {
        Some(StoredCompileResult::Ok(bundle)) => bundle.wasm_bytes.as_ptr(),
        _ => std::ptr::null(),
    })
}

fn result_bytes_len(handle: u32) -> usize {
    with_result(handle, |result| match result {
        Some(StoredCompileResult::Ok(bundle)) => bundle.wasm_bytes.len(),
        _ => 0,
    })
}

fn result_json_ptr(handle: u32) -> *const u8 {
    with_result(handle, |result| match result {
        Some(StoredCompileResult::Ok(bundle)) => bundle.dsp_json.as_ptr(),
        _ => std::ptr::null(),
    })
}

fn result_json_len(handle: u32) -> usize {
    with_result(handle, |result| match result {
        Some(StoredCompileResult::Ok(bundle)) => bundle.dsp_json.len(),
        _ => 0,
    })
}

fn result_compile_options_ptr(handle: u32) -> *const u8 {
    with_result(handle, |result| match result {
        Some(StoredCompileResult::Ok(bundle)) => bundle.compile_options.as_ptr(),
        _ => std::ptr::null(),
    })
}

fn result_compile_options_len(handle: u32) -> usize {
    with_result(handle, |result| match result {
        Some(StoredCompileResult::Ok(bundle)) => bundle.compile_options.len(),
        _ => 0,
    })
}

fn result_error_ptr(handle: u32) -> *const u8 {
    with_result(handle, |result| match result {
        Some(StoredCompileResult::Err(message)) => message.as_ptr(),
        _ => std::ptr::null(),
    })
}

fn result_error_len(handle: u32) -> usize {
    with_result(handle, |result| match result {
        Some(StoredCompileResult::Err(message)) => message.len(),
        _ => 0,
    })
}

fn text_result_ptr(handle: u32) -> *const u8 {
    with_text_result(handle, |result| match result {
        Some(StoredTextResult::Ok(text)) | Some(StoredTextResult::Err(text)) => text.as_ptr(),
        None => std::ptr::null(),
    })
}

fn text_result_len(handle: u32) -> usize {
    with_text_result(handle, |result| match result {
        Some(StoredTextResult::Ok(text)) | Some(StoredTextResult::Err(text)) => text.len(),
        None => 0,
    })
}

/// Allocates a writable region in the module linear memory for host-written
/// request payloads.
///
/// The caller must later release the region with [`faust_wasm_dealloc`].
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_alloc(len: usize) -> *mut u8 {
    let mut bytes = Vec::<u8>::with_capacity(len);
    let ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    ptr
}

/// Releases a region previously allocated by [`faust_wasm_alloc`].
///
/// `len` must match the original allocation capacity.
///
/// # Safety
/// - `ptr` must have been returned by [`faust_wasm_alloc`].
/// - `len` must match the original allocation capacity passed to
///   [`faust_wasm_alloc`].
/// - the region must not be used again after this call returns.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faust_wasm_dealloc(ptr: *mut u8, len: usize) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        drop(Vec::from_raw_parts(ptr, 0, len));
    }
}

/// Returns the static binding version string pointer.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_version_ptr() -> *const u8 {
    WASM_FFI_VERSION.as_ptr()
}

/// Returns the static binding version string length.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_version_len() -> usize {
    WASM_FFI_VERSION.len()
}

/// Compiles one DSP source string to a stored `{ wasm, json }` result handle.
///
/// The `args` string follows the current C++ `libFaustWasm` convention:
/// it is split on plain spaces before parsing the shared Rust FFI option
/// subset (`-I`, `-cn`, `-double`).
///
/// # Safety
/// - `name_ptr`, `source_ptr`, and `args_ptr` must point to readable byte
///   ranges of lengths `name_len`, `source_len`, and `args_len`.
/// - each byte range must contain valid UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faust_wasm_compile_dsp(
    name_ptr: *const u8,
    name_len: usize,
    source_ptr: *const u8,
    source_len: usize,
    args_ptr: *const u8,
    args_len: usize,
    internal_memory: u32,
) -> u32 {
    let result = unsafe {
        let name = match decode_utf8_arg(name_ptr, name_len, "name") {
            Ok(name) => name,
            Err(error) => return store_result(StoredCompileResult::Err(error)),
        };
        let source = match decode_utf8_arg(source_ptr, source_len, "source") {
            Ok(source) => source,
            Err(error) => return store_result(StoredCompileResult::Err(error)),
        };
        let args = match decode_utf8_arg(args_ptr, args_len, "args") {
            Ok(args) => args,
            Err(error) => return store_result(StoredCompileResult::Err(error)),
        };
        compile_to_stored_result(name, source, args, internal_memory != 0)
    };
    store_result(result)
}

/// Returns `1` for a successful compile result and `0` for an error result or
/// unknown handle.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_result_is_ok(handle: u32) -> u32 {
    with_result(handle, |result| match result {
        Some(StoredCompileResult::Ok(_)) => 1,
        _ => 0,
    })
}

/// Returns the pointer to the compiled WASM bytes for `handle`.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_result_wasm_ptr(handle: u32) -> *const u8 {
    result_bytes_ptr(handle)
}

/// Returns the compiled WASM byte length for `handle`.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_result_wasm_len(handle: u32) -> usize {
    result_bytes_len(handle)
}

/// Returns the pointer to the companion JSON UTF-8 bytes for `handle`.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_result_json_ptr(handle: u32) -> *const u8 {
    result_json_ptr(handle)
}

/// Returns the companion JSON byte length for `handle`.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_result_json_len(handle: u32) -> usize {
    result_json_len(handle)
}

/// Returns the pointer to the `compile_options` UTF-8 bytes for `handle`.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_result_compile_options_ptr(handle: u32) -> *const u8 {
    result_compile_options_ptr(handle)
}

/// Returns the `compile_options` byte length for `handle`.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_result_compile_options_len(handle: u32) -> usize {
    result_compile_options_len(handle)
}

/// Returns the pointer to the error message UTF-8 bytes for `handle`.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_result_error_ptr(handle: u32) -> *const u8 {
    result_error_ptr(handle)
}

/// Returns the error message byte length for `handle`.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_result_error_len(handle: u32) -> usize {
    result_error_len(handle)
}

/// Releases a stored compile result handle and all owned buffers behind it.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_result_free(handle: u32) {
    let mut guard = registry().lock().expect("wasm-ffi registry poisoned");
    guard.remove(handle);
}

/// Queries one `faustwasm` helper-info string and returns a stored text-result
/// handle.
///
/// Supported now: `version`, `help`. Other known keys return explicit
/// `unsupported` errors.
///
/// # Safety
/// - `what_ptr` must point to a readable UTF-8 byte range of length
///   `what_len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faust_wasm_get_info(what_ptr: *const u8, what_len: usize) -> u32 {
    let result = unsafe {
        let what = match decode_utf8_arg(what_ptr, what_len, "what") {
            Ok(what) => what,
            Err(error) => return store_text_result(StoredTextResult::Err(error)),
        };
        let compiler = Compiler::new();
        match compiler.get_faustwasm_info(what) {
            Ok(text) => StoredTextResult::Ok(text),
            Err(error) => StoredTextResult::Err(error.to_string()),
        }
    };
    store_text_result(result)
}

/// Stub export for the future `expandDSP(...)` helper service.
///
/// # Safety
/// - `name_ptr`, `source_ptr`, and `args_ptr` must point to readable UTF-8 byte
///   ranges of lengths `name_len`, `source_len`, and `args_len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faust_wasm_expand_dsp(
    name_ptr: *const u8,
    name_len: usize,
    source_ptr: *const u8,
    source_len: usize,
    args_ptr: *const u8,
    args_len: usize,
) -> u32 {
    let result = unsafe {
        let name = match decode_utf8_arg(name_ptr, name_len, "name") {
            Ok(name) => name,
            Err(error) => return store_text_result(StoredTextResult::Err(error)),
        };
        let source = match decode_utf8_arg(source_ptr, source_len, "source") {
            Ok(source) => source,
            Err(error) => return store_text_result(StoredTextResult::Err(error)),
        };
        let args = match decode_utf8_arg(args_ptr, args_len, "args") {
            Ok(args) => args,
            Err(error) => return store_text_result(StoredTextResult::Err(error)),
        };
        let compiler = Compiler::new();
        match compiler.expand_dsp(&compiler::ExpandDspRequest {
            source_name: name.to_owned(),
            source: source.to_owned(),
            args: args.to_owned(),
        }) {
            Ok(text) => StoredTextResult::Ok(text),
            Err(error) => StoredTextResult::Err(error.to_string()),
        }
    };
    store_text_result(result)
}

/// Stub export for the future `generateAuxFiles(...)` helper service.
///
/// Returns `1` only when aux-file generation is implemented and succeeds.
///
/// # Safety
/// - `name_ptr`, `source_ptr`, and `args_ptr` must point to readable UTF-8 byte
///   ranges of lengths `name_len`, `source_len`, and `args_len`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn faust_wasm_generate_aux_files(
    name_ptr: *const u8,
    name_len: usize,
    source_ptr: *const u8,
    source_len: usize,
    args_ptr: *const u8,
    args_len: usize,
) -> u32 {
    let name = match unsafe { decode_utf8_arg(name_ptr, name_len, "name") } {
        Ok(name) => name,
        Err(_) => return 0,
    };
    let source = match unsafe { decode_utf8_arg(source_ptr, source_len, "source") } {
        Ok(source) => source,
        Err(_) => return 0,
    };
    let args = match unsafe { decode_utf8_arg(args_ptr, args_len, "args") } {
        Ok(args) => args,
        Err(_) => return 0,
    };
    let compiler = Compiler::new();
    match compiler.generate_aux_files(&compiler::GenerateAuxFilesRequest {
        source_name: name.to_owned(),
        source: source.to_owned(),
        args: args.to_owned(),
    }) {
        Ok(_files) => 1,
        Err(_error) => 0,
    }
}

/// Returns `1` for a successful text result and `0` for an error result or
/// unknown handle.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_text_result_is_ok(handle: u32) -> u32 {
    with_text_result(handle, |result| match result {
        Some(StoredTextResult::Ok(_)) => 1,
        _ => 0,
    })
}

/// Returns the UTF-8 pointer stored behind one text result handle.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_text_result_ptr(handle: u32) -> *const u8 {
    text_result_ptr(handle)
}

/// Returns the UTF-8 byte length stored behind one text result handle.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_text_result_len(handle: u32) -> usize {
    text_result_len(handle)
}

/// Releases a stored text result handle and the owned payload behind it.
#[unsafe(no_mangle)]
pub extern "C" fn faust_wasm_text_result_free(handle: u32) {
    let mut guard = text_registry()
        .lock()
        .expect("wasm-ffi text registry poisoned");
    guard.remove(handle);
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        StoredCompileResult, StoredTextResult, compile_to_stored_result, parse_compile_request,
        split_faustwasm_args, store_result, store_text_result,
    };

    fn temp_root(test_name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "faust_rs_wasm_ffi_{test_name}_{}_{}",
            std::process::id(),
            stamp
        ));
        fs::create_dir_all(&root).expect("create temp root");
        root
    }

    #[test]
    fn split_faustwasm_args_matches_cxx_space_tokenization_shape() {
        assert_eq!(
            split_faustwasm_args("-double  -I dsp"),
            vec!["-double", "", "-I", "dsp"]
        );
    }

    #[test]
    fn parse_compile_request_maps_double_and_import_dirs() {
        let root = temp_root("parse_request");
        let request = parse_compile_request(
            "osc.dsp",
            "process = 0;",
            &format!("-double -I {}", root.display()),
            true,
        )
        .expect("request should parse");

        assert!(request.wasm_options.double_precision);
        assert!(request.wasm_options.internal_memory);
        assert_eq!(request.import_dirs, vec![root]);
    }

    #[test]
    fn compile_to_stored_result_returns_wasm_and_json_payloads() {
        let result = compile_to_stored_result("osc.dsp", "process = 0;", "", true);
        let StoredCompileResult::Ok(bundle) = result else {
            panic!("compile should succeed");
        };

        assert!(bundle.wasm_bytes.starts_with(b"\0asm"));
        assert!(bundle.dsp_json.contains("\"filename\":\"osc.dsp\""));
        assert_eq!(bundle.compile_options, "-lang wasm -single");
    }

    #[test]
    fn compile_to_stored_result_supports_string_source_import_dirs() {
        let root = temp_root("memory_import_dir");
        let child = root.join("child.lib");
        fs::write(&child, "process = _;\n").expect("write child");

        let result = compile_to_stored_result(
            "main.dsp",
            "process = component(\"child.lib\");",
            &format!("-I {}", root.display()),
            true,
        );
        let StoredCompileResult::Ok(bundle) = result else {
            panic!("compile with import dir should succeed");
        };

        assert!(bundle.wasm_bytes.starts_with(b"\0asm"));
        assert!(bundle.dsp_json.contains("child.lib"));
    }

    #[test]
    fn stored_handles_keep_error_payloads_addressable_until_free() {
        let handle = store_result(StoredCompileResult::Err("boom".to_owned()));
        assert_eq!(super::faust_wasm_result_is_ok(handle), 0);
        assert_eq!(super::faust_wasm_result_error_len(handle), 4);
        assert!(!super::faust_wasm_result_error_ptr(handle).is_null());
        super::faust_wasm_result_free(handle);
        assert_eq!(super::faust_wasm_result_error_len(handle), 0);
    }

    #[test]
    fn text_result_handles_keep_payloads_addressable_until_free() {
        let handle = store_text_result(StoredTextResult::Ok("help".to_owned()));
        assert_eq!(super::faust_wasm_text_result_is_ok(handle), 1);
        assert_eq!(super::faust_wasm_text_result_len(handle), 4);
        assert!(!super::faust_wasm_text_result_ptr(handle).is_null());
        super::faust_wasm_text_result_free(handle);
        assert_eq!(super::faust_wasm_text_result_len(handle), 0);
    }
}
