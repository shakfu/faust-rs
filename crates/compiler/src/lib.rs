//! Top-level compiler facade crate.
//!
//! # Source provenance (C++)
//! - `compiler/libcode.cpp` (compile entry points and orchestration)
//! - `compiler/global.cpp` (session lifecycle)
//!
//! # Current scope
//! - Exposes minimal compile-session APIs.
//! - Wires parsing through production `crates/parser` APIs.
//!
//! # Canonical pipeline
//! `parse -> eval -> propagate -> normalize/type/interval (incremental) -> transform -> fir -> backend`
//!
//! The currently wired production fast path in this crate is:
//! `parse -> eval -> propagate -> (optional signal->FIR) -> codegen`.
//!
//! # Facade responsibilities
//! - Provide one orchestrator type ([`Compiler`]) for file-based compilation.
//! - Aggregate typed stage errors into one top-level [`CompilerError`] surface.
//! - Provide test/golden-oriented helper outputs (box dump, signal dump, FIR dump).
//! - Route backend generation to C/C++ emitters with consistent options.
//!
//! # API mapping status
//! - External facade API is `adapted`: it targets behavior compatibility with
//!   C++ compile flows while using Rust structs/results and explicit lane options.
//!
//! # Current lane note
//! - The active signal->FIR lowering route is [`SignalFirLane::TransformFastLane`],
//!   owned by `crates/transform`.

pub mod enrobage;

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use boxes::{BoxId, BoxMatch, dump_box, match_box};
use codegen::backends::c::{COptions, CodegenError as CCodegenError, generate_c_module};
use codegen::backends::cpp::{CodegenError, CppOptions, generate_cpp_module};
use codegen::backends::interp::{
    CodegenError as InterpCodegenError, CodegenErrorCode as InterpCodegenErrorCode, FbcDspFactory,
    FbcReal, InterpOptions, generate_interp_module, write_fbc,
};
use codegen::backends::wasm::layout::WasmMemoryLayout;
use codegen::backends::wasm::{
    WasmBackendError, WasmJsonContext, WasmModule, WasmOptions, generate_wasm_module_with_context,
};
use codegen::json::{
    JsonBuildOptions, JsonDescription, JsonMetaEntry, build_json_description_from_fir,
};
use errors::codes::COMP_TYPE_FAILED;
use errors::{
    Diagnostic, DiagnosticBundle, IntoDiagnostic, Label, LabelStyle, Severity, SourceSpan, Stage,
};
use fir::{
    FirId, FirStore,
    checker::{FirVerifyReport, Severity as FirVerifySeverity, verify_fir_module},
};
use parser::VirtualSourceMap;
use parser::{CompilationMetadataKey, CompilationMetadataSnapshot, ParseOutput, SourceReaderError};
use propagate::{ArityCache, BoxArity, PropagateError, PropagateUiOptions};
use signals::SigId;
use sigtype::TypeAnnotator;
use tlib::NodeKind;
pub use transform::signal_fir::RealType;
use transform::signal_fir::{
    SignalFirError, SignalFirErrorCode, SignalFirOptions, compile_signals_to_fir_fastlane_with_ui,
};
use ui::UiProgram;

/// Parse + eval + propagate output package.
///
/// This is the highest-level structural output of the box/signal pipeline
/// before any FIR lowering or backend selection happens.
///
/// Since the grouped-UI rewrite, this facade boundary owns both semantic
/// products of propagation:
/// - propagated DSP signals,
/// - canonical grouped UI layout/metadata.
#[derive(Debug)]
pub struct SignalCompileOutput {
    /// Full parser output (arena + metadata + diagnostics from parse stage).
    pub parse: ParseOutput,
    /// Aggregated top-level `declare key "value";` metadata visible after the
    /// whole parse + eval file-loading session.
    pub compilation_metadata: parser::CompilationMetadataSnapshot,
    /// Additional Faust source files loaded through evaluator-side
    /// `component(...)` / `library(...)` resolution during this session.
    pub loaded_files: Vec<PathBuf>,
    /// Evaluated `process` box expression after `eval`.
    pub process_box: BoxId,
    /// Inferred process arity (`inputs`/`outputs`) from `propagate::box_arity_typed`.
    pub process_arity: BoxArity,
    /// Final propagated output signal list.
    ///
    /// Parity note:
    /// - for ordinary Faust programs, this usually matches
    ///   `process_arity.outputs`,
    /// - for `fad(expr)`, propagation expands the signal list to
    ///   `primal outputs + tangent outputs`, so `signals.len()` may be greater
    ///   than `process_arity.outputs`.
    pub signals: Vec<SigId>,
    /// Canonical grouped UI artifact owned after the propagation boundary.
    ///
    /// Downstream FIR lowering/backends must treat this as the source of truth
    /// for `buildUserInterface`, rather than reconstructing groups from signal
    /// leaf widgets.
    pub ui: UiProgram,
    /// Evaluated `BoxId` → source definition name.
    ///
    /// Populated by the evaluator: when a named closure is forced to a concrete
    /// box, the result `BoxId` is recorded with the definition's string name.
    /// Used by the SVG draw module to label and fold named sub-diagrams.
    pub def_names: std::collections::HashMap<boxes::BoxId, String>,
}

impl SignalCompileOutput {
    /// Returns the effective propagated output arity seen by FIR/backends.
    ///
    /// This differs from [`Self::process_arity`] for forward-mode AD:
    /// `box_arity_typed(...)` intentionally keeps `fad(expr)` transparent at the
    /// box level, while propagation expands the concrete signal forest.
    #[must_use]
    pub fn propagated_output_count(&self) -> usize {
        self.signals.len()
    }
}

/// Parse + eval + propagate + FIR lowering output package.
///
/// This bundle is used by FIR-oriented backends and verifier integration.
#[derive(Debug)]
pub struct FirCompileOutput {
    /// FIR storage arena.
    pub store: FirStore,
    /// FIR module root id.
    pub module: FirId,
}

/// Request payload for the artifact-centric WASM compile service used by the
/// planned `faustwasm` Rust integration.
///
/// # Role
/// This request intentionally avoids the historical C++ `cfactory` model.
/// Callers provide one self-contained compilation request and receive one owned
/// [`WasmArtifactBundle`] back. JS/host-side caches can then own the resulting
/// `{ wasm, json }` pair directly.
///
/// # Mapping status
/// `adapted` relative to the C++ `createDSPFactory(...)` entry point:
/// - preserved semantics: DSP name/source, WASM backend options, and
///   signal->FIR lane selection;
/// - intentionally omitted: factory pointer lifetime and explicit deletion.
#[derive(Debug, Clone)]
pub struct WasmArtifactRequest {
    /// Logical source name reported in diagnostics and JSON provenance.
    pub source_name: String,
    /// Faust DSP source text to compile.
    pub source: String,
    /// Extra import search directories, mirroring CLI/FFI `-I`.
    pub import_dirs: Vec<PathBuf>,
    /// Optional in-memory source bundle used to resolve `import("...")` and
    /// evaluator-side `library(...)` / `component(...)` without a host
    /// filesystem dependency.
    pub virtual_sources: VirtualSourceMap,
    /// WASM backend configuration (`-double`, memory model, etc.).
    pub wasm_options: WasmOptions,
    /// Signal->FIR lowering lane used before WASM code generation.
    pub lane: SignalFirLane,
}

impl WasmArtifactRequest {
    /// Builds a source-backed request with default import search paths, default
    /// WASM options, and the production JSON/WASM lowering lane.
    ///
    /// Mapping status: `adapted`.
    /// The artifact-oriented faustwasm service needs the full FIR module shape
    /// (`metadata`, `buildUserInterface`, lifecycle methods), so its default
    /// lane follows the transform fast lane rather than the temporary legacy
    /// summary bridge.
    #[must_use]
    pub fn new(source_name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            source_name: source_name.into(),
            source: source.into(),
            import_dirs: Vec::new(),
            virtual_sources: VirtualSourceMap::default(),
            wasm_options: WasmOptions::default(),
            lane: SignalFirLane::TransformFastLane,
        }
    }
}

/// Owned `{ wasm, json }` bundle returned by the Rust-side WASM compile service.
///
/// This is the first Phase 1 artifact contract from
/// `porting/faustwasm-dual-mode-rust-interface-plan-2026-03-26-en.md`.
/// The bundle is designed to be consumed directly by a future JS/WASM binding
/// layer or by Rust-native tests without any factory-pointer semantics.
///
/// # ABI contract
/// - [`Self::wasm_bytes`] and [`Self::dsp_json`] are a matched pair and must be
///   consumed together.
/// - [`Self::compile_options`] mirrors the JSON `compile_options` field so a
///   binding layer does not need to re-parse the JSON merely to discover the
///   emitted float/backend mode.
///
/// # Mapping status
/// `adapted` relative to the C++ `FaustWasm { data, json, ... }` result:
/// - preserved semantics: owned WASM bytes plus companion JSON;
/// - adapted: compile provenance is exposed as a dedicated field in addition to
///   the JSON payload;
/// - deferred: warnings and aux files until the corresponding Rust services are
///   ported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmArtifactBundle {
    /// Binary WebAssembly module bytes.
    pub wasm_bytes: Vec<u8>,
    /// Companion Faust JSON description for the same module.
    pub dsp_json: String,
    /// High-level compilation provenance mirrored from the JSON payload.
    pub compile_options: String,
}

/// Auxiliary file payload planned for future `generateAuxFiles` support in the
/// Rust `faustwasm` service surface.
///
/// Mapping status: `deferred`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuxFileArtifact {
    /// Logical relative output path.
    pub path: String,
    /// Raw file contents. Text files use UTF-8 bytes.
    pub content: Vec<u8>,
    /// Whether the payload should be interpreted as binary.
    pub binary: bool,
}

/// Request payload reserved for future `expandDSP(...)` parity support.
///
/// Mapping status: `deferred`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpandDspRequest {
    /// Logical source name reported in diagnostics.
    pub source_name: String,
    /// Faust DSP source text to expand.
    pub source: String,
    /// Raw argument string as passed by `faustwasm`.
    pub args: String,
}

/// Request payload for `generateAuxFiles(...)`.
///
/// Mapping status: `adapted`.
#[derive(Debug, Clone, Default)]
pub struct GenerateAuxFilesRequest {
    /// Logical source name reported in diagnostics.
    pub source_name: String,
    /// Faust DSP source text used to generate the outputs.
    pub source: String,
    /// Raw argument string as passed by `faustwasm`.
    pub args: String,
    /// Optional in-memory library sources (e.g. embedded standard library
    /// bundle from the `wasm-ffi` build).  When non-empty these take
    /// precedence over filesystem resolution so `import("stdfaust.lib")`
    /// works without a writable host filesystem.
    pub virtual_sources: VirtualSourceMap,
}

/// Structured error returned by the `faustwasm`-oriented compiler service
/// methods when the requested helper surface is not implemented yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaustwasmServiceError {
    /// Stable machine-readable reason code.
    pub code: FaustwasmServiceErrorCode,
    /// User-facing explanation intended for JS-side propagation.
    pub message: String,
}

/// Stable error codes for the `faustwasm` helper-service surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaustwasmServiceErrorCode {
    /// The requested operation/key exists conceptually but is not implemented
    /// yet in the Rust service layer.
    Unsupported,
    /// The caller passed an unknown query key.
    InvalidArgument,
}

impl FaustwasmServiceError {
    fn unsupported(message: impl Into<String>) -> Self {
        Self {
            code: FaustwasmServiceErrorCode::Unsupported,
            message: message.into(),
        }
    }

    fn invalid_argument(message: impl Into<String>) -> Self {
        Self {
            code: FaustwasmServiceErrorCode::InvalidArgument,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for FaustwasmServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for FaustwasmServiceError {}

/// FIR verifier configuration used at the compiler facade / CLI integration layer.
///
/// The facade keeps verifier policy explicit because different workflows need
/// different failure semantics: local exploration may allow warnings, while CI
/// or strict lane validation should fail on them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FirVerifyOptions {
    /// Run FIR verifier after FIR generation and before backend codegen.
    pub enabled: bool,
    /// Treat warnings as fatal in addition to errors.
    pub strict: bool,
}

/// Main façade orchestrating the current production compilation pipeline.
///
/// Current canonical flow:
/// `parse -> eval -> propagate -> (optional signal->FIR lowering) -> codegen`.
pub struct Compiler {
    fir_verify: FirVerifyOptions,
    entrypoint_name: Box<str>,
    /// Floating-point precision used for internal DSP computation in the
    /// transform fast lane. `Float32` (single precision) is the default;
    /// set to `Float64` to activate double-precision mode (`--double`).
    ///
    /// This controls the internal FIR real type only. Backend interface types
    /// such as C/C++ `FAUSTFLOAT` remain architecture-controlled.
    real_type: RealType,
    /// Maximum delay (inclusive) for which the shift/copy strategy is used.
    /// Mirrors Faust `-mcd N`. Default: 16.
    max_copy_delay: u32,
    /// Delay above which the if-based wrapping strategy is used.
    /// Mirrors Faust `-dlt N`. Default: `u32::MAX` (disabled).
    delay_line_threshold: u32,
    /// Optional cooperative cancellation flag.
    ///
    /// When set, the evaluator checks this flag on every recursive call and
    /// returns `EvalError::Cancelled` if it has been set to `true`. The CLI
    /// uses this with a watchdog thread for `--timeout`; libfaust hosts can
    /// set it from any thread to abort compilation without killing the process.
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Optional sink for phase timings, used by the CLI `-time` flag and by
    /// embedding layers that want Faust-style internal compilation timings.
    timing_sink: Option<TimingSink>,
}

type TimingSink = Arc<dyn Fn(&str, Duration) + Send + Sync + 'static>;

/// Selects which signal->FIR lowering route is used before backend emission.
///
/// The only remaining public route is the transform-owned fast lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignalFirLane {
    /// Lowering lane owned by `crates/transform`.
    #[default]
    TransformFastLane,
}

impl WasmArtifactBundle {
    fn from_wasm_module(module: WasmModule, compile_options: String) -> Self {
        Self {
            wasm_bytes: module.wasm_binary,
            dsp_json: module.dsp_json,
            compile_options,
        }
    }
}

impl Compiler {
    #[must_use]
    /// Creates a new top-level compiler facade instance.
    pub fn new() -> Self {
        Self {
            fir_verify: FirVerifyOptions::default(),
            entrypoint_name: "process".into(),
            real_type: RealType::default(),
            max_copy_delay: 16,
            delay_line_threshold: u32::MAX,
            cancel: None,
            timing_sink: None,
        }
    }

    /// Returns a compiler facade configured with FIR verifier settings.
    #[must_use]
    pub fn with_fir_verify_options(mut self, fir_verify: FirVerifyOptions) -> Self {
        self.fir_verify = fir_verify;
        self
    }

    /// Returns a compiler facade configured to use a custom top-level DSP
    /// entry-point name instead of the default `process`.
    #[must_use]
    pub fn with_process_name(mut self, entrypoint_name: impl Into<Box<str>>) -> Self {
        self.entrypoint_name = entrypoint_name.into();
        self
    }

    /// Returns a compiler facade configured to use the given floating-point
    /// precision for internal DSP computation (transform fast lane only).
    ///
    /// This mirrors Faust `-double` semantics for the C/C++ backends: the
    /// generated DSP core uses `double`, while the external `FAUSTFLOAT`
    /// interface remains controlled by the architecture layer.
    #[must_use]
    pub fn with_real_type(mut self, real_type: RealType) -> Self {
        self.real_type = real_type;
        self
    }

    /// Sets the max-copy-delay threshold (`-mcd N`).
    ///
    /// Delays ≤ `n` use the shift/copy strategy (no `fIOTA`).  Default: 16.
    #[must_use]
    pub fn with_mcd(mut self, n: u32) -> Self {
        self.max_copy_delay = n;
        self
    }

    /// Sets the delay-line threshold (`-dlt N`).
    ///
    /// Delays > `n` use the if-based wrapping strategy (per-line counter,
    /// exact buffer size).  Default: `u32::MAX` (disabled).
    #[must_use]
    pub fn with_dlt(mut self, n: u32) -> Self {
        self.delay_line_threshold = n;
        self
    }

    /// Returns a compiler facade with a cooperative cancellation flag.
    ///
    /// The caller retains an `Arc<AtomicBool>` clone and can set it to `true`
    /// from any thread to request cancellation. The evaluator checks the flag
    /// on every recursive call and returns a `Cancelled` error.
    ///
    /// This is the library-safe alternative to `process::exit`: the CLI uses
    /// a watchdog thread for `--timeout`; libfaust hosts can set the flag on
    /// user abort without killing the process.
    #[must_use]
    pub fn with_cancel(mut self, cancel: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.cancel = Some(cancel);
        self
    }

    /// Returns a compiler facade that reports internal phase timings.
    #[must_use]
    pub fn with_timing_sink<F>(mut self, sink: F) -> Self
    where
        F: Fn(&str, Duration) + Send + Sync + 'static,
    {
        self.timing_sink = Some(Arc::new(sink));
        self
    }

    fn time_phase<T>(&self, name: &'static str, f: impl FnOnce() -> T) -> T {
        time_phase_with_sink(self.timing_sink.as_ref(), name, f)
    }

    #[must_use]
    /// Returns the crate package version used by this binary/library build.
    pub fn version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    /// Parses one source string through the production parser crate.
    ///
    /// Returns [`CompilerError::Parse`] when parser recovery/errors are present.
    pub fn compile_source(
        &self,
        source_name: &str,
        source: &str,
    ) -> Result<ParseOutput, CompilerError> {
        let output = self.time_phase("parser", || parser::parse_program(source, source_name));
        ensure_parse_success(source_name, output)
    }

    /// Parses one source file and expands local imports using `search_paths`.
    ///
    /// `search_paths` are treated like C++ `-I/--import-dir` entries: they are
    /// searched before the built-in file-backed defaults (`master` directory,
    /// `FAUST_LIB_PATH`, executable-relative `share/faust`, and the usual
    /// system install roots).
    ///
    /// Returns [`CompilerError::Import`] for import resolution/cycle failures.
    pub fn compile_file(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
    ) -> Result<ParseOutput, CompilerError> {
        let import_search_paths = merge_import_search_paths(path, search_paths);
        let output = self
            .time_phase("parser", || {
                parser::parse_file_with_imports(path, &import_search_paths)
            })
            .map_err(CompilerError::Import)?;
        ensure_parse_success(&path.display().to_string(), output)
    }

    /// Parses one source file using the same default library search model as the
    /// C++ compiler frontend.
    pub fn compile_file_default(&self, path: &Path) -> Result<ParseOutput, CompilerError> {
        self.compile_file(path, &[])
    }

    /// Parses, evaluates `process`, then propagates boxes to output signals.
    ///
    /// This in-memory entry point is the closest Rust equivalent of compiling a
    /// standalone Faust string in the C++ frontend. It still installs a shared
    /// top-level metadata store so parser-stage `declare` metadata and any
    /// evaluator-driven loads performed later in the same session contribute to
    /// one coherent compilation snapshot.
    pub fn compile_source_to_signals(
        &self,
        source_name: &str,
        source: &str,
    ) -> Result<SignalCompileOutput, CompilerError> {
        self.compile_source_to_signals_with_search_paths(source_name, source, &[])
    }

    /// Parses, evaluates `process`, then propagates boxes to output signals
    /// using explicit evaluator import search paths.
    ///
    /// This is the string-backed counterpart of
    /// [`Self::compile_file_to_signals`]. It exists so embedding/binding layers
    /// can compile source strings while still honoring `-I` search paths.
    pub fn compile_source_to_signals_with_search_paths(
        &self,
        source_name: &str,
        source: &str,
        search_paths: &[PathBuf],
    ) -> Result<SignalCompileOutput, CompilerError> {
        self.compile_source_to_signals_with_import_context(
            source_name,
            source,
            search_paths,
            &VirtualSourceMap::default(),
        )
    }

    fn compile_source_to_signals_with_import_context(
        &self,
        source_name: &str,
        source: &str,
        search_paths: &[PathBuf],
        virtual_sources: &VirtualSourceMap,
    ) -> Result<SignalCompileOutput, CompilerError> {
        let metadata_store = parser::CompilationMetadataStore::new(source_name);
        let output = if search_paths.is_empty() && virtual_sources.is_empty() {
            ensure_parse_success(
                source_name,
                self.time_phase("parser", || {
                    parser::parse_program_with_metadata(source, source_name, metadata_store.clone())
                }),
            )?
        } else {
            ensure_parse_success(
                source_name,
                self.time_phase("parser", || {
                    parser::parse_program_with_imports_and_metadata(
                        source,
                        source_name,
                        search_paths,
                        virtual_sources,
                        metadata_store.clone(),
                    )
                })
                .map_err(CompilerError::Import)?,
            )?
        };
        let eval_source_context = if search_paths.is_empty() && virtual_sources.is_empty() {
            eval::EvalSourceContext::memory_with_metadata(metadata_store)
        } else {
            eval::EvalSourceContext::memory_with_search_paths_metadata_and_virtual_sources(
                search_paths,
                virtual_sources.clone(),
                metadata_store,
            )
        };
        self.pipeline_to_signals(source_name, output, Some(eval_source_context))
    }

    /// Parses one file, evaluates `process`, then propagates boxes to output signals.
    ///
    /// Unlike [`compile_source_to_signals`](Self::compile_source_to_signals),
    /// this file-backed entry point also installs an [`eval::EvalSourceContext`]
    /// so Phase 4 can resolve `component("...")` and `library("...")` with the
    /// same relative-file/import-search semantics as the C++ compiler.
    pub fn compile_file_to_signals(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
    ) -> Result<SignalCompileOutput, CompilerError> {
        let import_search_paths = merge_import_search_paths(path, search_paths);
        let metadata_store = parser::CompilationMetadataStore::new(
            &path
                .canonicalize()
                .unwrap_or_else(|_| path.to_path_buf())
                .to_string_lossy(),
        );
        let output = ensure_parse_success(
            &path.display().to_string(),
            self.time_phase("parser", || {
                parser::parse_file_with_imports_and_metadata(
                    path,
                    &import_search_paths,
                    metadata_store.clone(),
                )
            })
            .map_err(CompilerError::Import)?,
        )?;
        let eval_source_context = eval::EvalSourceContext::for_file_with_metadata(
            path,
            &import_search_paths,
            metadata_store,
        );
        self.pipeline_to_signals(
            &path.display().to_string(),
            output,
            Some(eval_source_context),
        )
    }

    /// Parses one file with default import search path, then runs eval+propagate.
    ///
    /// The default search set follows the C++ frontend model:
    /// current file directory, `FAUST_LIB_PATH`, executable-relative
    /// `share/faust`, then standard system install roots.
    pub fn compile_file_default_to_signals(
        &self,
        path: &Path,
    ) -> Result<SignalCompileOutput, CompilerError> {
        self.compile_file_to_signals(path, &[])
    }

    /// Runs eval+propagate on an already parsed Faust program.
    ///
    /// This is an advanced entry point used by tooling/tests that need to alter
    /// parse metadata before Phase 4 (for example diagnostics fallback checks).
    /// No file-backed evaluator source context is installed here, so nested
    /// `component(...)` / `library(...)` resolution keeps the in-memory
    /// semantics of [`eval::EvalSourceContext::memory`].
    pub fn compile_parsed_to_signals(
        &self,
        source_name: &str,
        output: ParseOutput,
    ) -> Result<SignalCompileOutput, CompilerError> {
        self.pipeline_to_signals(source_name, output, None)
    }

    /// Parses + evaluates + propagates one source, then emits C++ text.
    pub fn compile_source_to_cpp(
        &self,
        source_name: &str,
        source: &str,
        options: &CppOptions,
    ) -> Result<String, CompilerError> {
        self.compile_source_to_cpp_with_lane(
            source_name,
            source,
            options,
            SignalFirLane::TransformFastLane,
        )
    }

    /// Parses + evaluates + propagates one source, then emits C text.
    pub fn compile_source_to_c(
        &self,
        source_name: &str,
        source: &str,
        options: &COptions,
    ) -> Result<String, CompilerError> {
        self.compile_source_to_c_with_lane(
            source_name,
            source,
            options,
            SignalFirLane::TransformFastLane,
        )
    }

    /// Parses + evaluates + propagates one source, then emits C text using
    /// the selected signal->FIR lowering lane.
    pub fn compile_source_to_c_with_lane(
        &self,
        source_name: &str,
        source: &str,
        options: &COptions,
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        let signals = self.compile_source_to_signals(source_name, source)?;
        let ctx = SignalLoweringContext {
            lane,
            fir_verify: self.fir_verify,
            real_type: self.real_type,
            max_copy_delay: self.max_copy_delay,
            delay_line_threshold: self.delay_line_threshold,
            timing_sink: self.timing_sink.clone(),
        };
        lower_signals_to_c(source_name, &signals, options, ctx)
            .map_err(|e| lower_c_error_to_compiler(source_name, e))
    }

    /// Parses + evaluates + propagates one source, then emits C++ text using
    /// the selected signal->FIR lowering lane.
    pub fn compile_source_to_cpp_with_lane(
        &self,
        source_name: &str,
        source: &str,
        options: &CppOptions,
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        let signals = self.compile_source_to_signals(source_name, source)?;
        let ctx = SignalLoweringContext {
            lane,
            fir_verify: self.fir_verify,
            real_type: self.real_type,
            max_copy_delay: self.max_copy_delay,
            delay_line_threshold: self.delay_line_threshold,
            timing_sink: self.timing_sink.clone(),
        };
        lower_signals_to_cpp(source_name, &signals, options, ctx)
            .map_err(|e| lower_cpp_error_to_compiler(source_name, e))
    }

    /// Parses + evaluates + propagates one source, then lowers to FIR using
    /// the selected signal->FIR lane.
    pub fn compile_source_to_fir_with_lane(
        &self,
        source_name: &str,
        source: &str,
        lane: SignalFirLane,
    ) -> Result<FirCompileOutput, CompilerError> {
        let signals = self.compile_source_to_signals(source_name, source)?;
        lower_signals_to_fir(
            source_name,
            &signals,
            lane,
            self.fir_verify,
            self.real_type,
            self.max_copy_delay,
            self.delay_line_threshold,
        )
        .map_err(|e| lower_fir_error_to_compiler(source_name, e))
    }

    /// Parses + evaluates + propagates one source, then emits a WASM module
    /// plus its matched companion JSON.
    ///
    /// This API defaults to [`SignalFirLane::TransformFastLane`] because the
    /// WASM/JSON-facing artifact surfaces need the canonical lowered FIR module
    /// with working `metadata`/`buildUserInterface` bodies.
    pub fn compile_source_to_wasm(
        &self,
        source_name: &str,
        source: &str,
        options: &WasmOptions,
    ) -> Result<WasmModule, CompilerError> {
        self.compile_source_to_wasm_with_lane(
            source_name,
            source,
            options,
            SignalFirLane::TransformFastLane,
        )
    }

    /// Parses + evaluates + propagates one source, then emits a WASM module
    /// through the selected signal->FIR lane.
    pub fn compile_source_to_wasm_with_lane(
        &self,
        source_name: &str,
        source: &str,
        options: &WasmOptions,
        lane: SignalFirLane,
    ) -> Result<WasmModule, CompilerError> {
        let signals = self.compile_source_to_signals(source_name, source)?;
        let lowered = lower_signals_to_fir(
            source_name,
            &signals,
            lane,
            self.fir_verify,
            self.real_type,
            self.max_copy_delay,
            self.delay_line_threshold,
        )
        .map_err(|error| lower_fir_error_to_compiler(source_name, error))?;
        let json_context = wasm_json_context_for_memory_source(
            source_name,
            &signals,
            compile_options_json_string(Some("wasm"), options.double_precision),
        );
        generate_wasm_module_with_context(&lowered.store, lowered.module, options, &json_context)
            .map_err(|error| CompilerError::CodegenWasm {
                source: source_name.into(),
                error,
            })
    }

    /// Compiles one in-memory DSP source into an owned artifact bundle
    /// containing both the WASM bytes and the companion JSON.
    ///
    /// This is the pure-Rust compile-service entry point intended for the
    /// future `faustwasm` embedded-compiler mode. The returned
    /// [`WasmArtifactBundle`] avoids any explicit compiler-side object lifetime
    /// and can be cached directly by higher-level hosts.
    ///
    /// Requests default to [`SignalFirLane::TransformFastLane`] for the same
    /// reason as [`Compiler::compile_source_to_wasm`]: JSON/WASM artifact
    /// consumers need preserved UI and metadata fidelity.
    pub fn compile_wasm_artifact(
        &self,
        request: &WasmArtifactRequest,
    ) -> Result<WasmArtifactBundle, CompilerError> {
        let compile_options =
            compile_options_json_string(Some("wasm"), request.wasm_options.double_precision);
        let signals = self.compile_source_to_signals_with_import_context(
            &request.source_name,
            &request.source,
            &request.import_dirs,
            &request.virtual_sources,
        )?;
        let lowered = lower_signals_to_fir(
            &request.source_name,
            &signals,
            request.lane,
            self.fir_verify,
            self.real_type,
            self.max_copy_delay,
            self.delay_line_threshold,
        )
        .map_err(|error| lower_fir_error_to_compiler(&request.source_name, error))?;
        let json_context = wasm_json_context_for_memory_source(
            &request.source_name,
            &signals,
            compile_options.clone(),
        );
        let mut json_context = json_context;
        json_context.include_pathnames = request
            .import_dirs
            .iter()
            .map(|dir| dir.to_string_lossy().into_owned())
            .collect();
        let mut library_list: Vec<String> = signals
            .parse
            .used_files
            .iter()
            .skip(1)
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        for file in &signals.loaded_files {
            let file = file.to_string_lossy().into_owned();
            if !library_list.iter().any(|existing| existing == &file) {
                library_list.push(file);
            }
        }
        json_context.library_list = library_list;
        let module = generate_wasm_module_with_context(
            &lowered.store,
            lowered.module,
            &request.wasm_options,
            &json_context,
        )
        .map_err(|error| CompilerError::CodegenWasm {
            source: request.source_name.clone().into(),
            error,
        })?;
        Ok(WasmArtifactBundle::from_wasm_module(
            module,
            compile_options,
        ))
    }

    /// Parses + evaluates + propagates one source, then emits strict C++-style JSON.
    ///
    /// Like the WASM artifact entry points, this API defaults to
    /// [`SignalFirLane::TransformFastLane`] so the reconstructed JSON sees the
    /// canonical FIR `metadata` and `buildUserInterface` bodies.
    pub fn compile_source_to_json(
        &self,
        source_name: &str,
        source: &str,
    ) -> Result<String, CompilerError> {
        self.compile_source_to_json_with_lane(source_name, source, SignalFirLane::TransformFastLane)
    }

    /// Parses + evaluates + propagates one source, then emits strict C++-style JSON
    /// through the selected signal->FIR lane.
    pub fn compile_source_to_json_with_lane(
        &self,
        source_name: &str,
        source: &str,
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        self.compile_source_to_json_with_lane_and_compile_options(
            source_name,
            source,
            lane,
            compile_options_json_string(None, self.real_type == RealType::Float64),
        )
    }

    /// Parses + evaluates + propagates one source, then emits strict C++-style JSON
    /// through the selected signal->FIR lane with explicit `compile_options`.
    pub fn compile_source_to_json_with_lane_and_compile_options(
        &self,
        source_name: &str,
        source: &str,
        lane: SignalFirLane,
        compile_options: String,
    ) -> Result<String, CompilerError> {
        let signals = self.compile_source_to_signals(source_name, source)?;
        let lowered = lower_signals_to_fir(
            source_name,
            &signals,
            lane,
            self.fir_verify,
            self.real_type,
            self.max_copy_delay,
            self.delay_line_threshold,
        )
        .map_err(|error| lower_fir_error_to_compiler(source_name, error))?;
        let json = build_strict_json_description(
            &lowered.store,
            lowered.module,
            StrictJsonContext {
                filename: source_name_to_filename(source_name),
                include_pathnames: Vec::new(),
                library_list: Vec::new(),
                top_level_meta: json_meta_entries_from_snapshot(&signals.compilation_metadata),
                compile_options,
                double_precision: self.real_type == RealType::Float64,
            },
        )
        .map_err(|error| CompilerError::CodegenWasm {
            source: source_name.into(),
            error,
        })?;
        Ok(json.render())
    }

    /// Parses + evaluates + propagates one file, then emits C++ text.
    pub fn compile_file_to_cpp(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        options: &CppOptions,
    ) -> Result<String, CompilerError> {
        self.compile_file_to_cpp_with_lane(
            path,
            search_paths,
            options,
            SignalFirLane::TransformFastLane,
        )
    }

    /// Parses + evaluates + propagates one file, then emits C text.
    pub fn compile_file_to_c(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        options: &COptions,
    ) -> Result<String, CompilerError> {
        self.compile_file_to_c_with_lane(
            path,
            search_paths,
            options,
            SignalFirLane::TransformFastLane,
        )
    }

    /// Parses + evaluates + propagates one file, then emits C text using
    /// the selected signal->FIR lowering lane.
    pub fn compile_file_to_c_with_lane(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        options: &COptions,
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        let signals = self.compile_file_to_signals(path, search_paths)?;
        let source = path.display().to_string();
        let ctx = SignalLoweringContext {
            lane,
            fir_verify: self.fir_verify,
            real_type: self.real_type,
            max_copy_delay: self.max_copy_delay,
            delay_line_threshold: self.delay_line_threshold,
            timing_sink: self.timing_sink.clone(),
        };
        lower_signals_to_c(&source, &signals, options, ctx)
            .map_err(|e| lower_c_error_to_compiler(&source, e))
    }

    /// Parses + evaluates + propagates one file, then emits C++ text using
    /// the selected signal->FIR lowering lane.
    pub fn compile_file_to_cpp_with_lane(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        options: &CppOptions,
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        let signals = self.compile_file_to_signals(path, search_paths)?;
        let source = path.display().to_string();
        let ctx = SignalLoweringContext {
            lane,
            fir_verify: self.fir_verify,
            real_type: self.real_type,
            max_copy_delay: self.max_copy_delay,
            delay_line_threshold: self.delay_line_threshold,
            timing_sink: self.timing_sink.clone(),
        };
        lower_signals_to_cpp(&source, &signals, options, ctx)
            .map_err(|e| lower_cpp_error_to_compiler(&source, e))
    }

    /// Parses + evaluates + propagates one file, then lowers to FIR using
    /// the selected signal->FIR lane.
    pub fn compile_file_to_fir_with_lane(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        lane: SignalFirLane,
    ) -> Result<FirCompileOutput, CompilerError> {
        let signals = self.compile_file_to_signals(path, search_paths)?;
        let source = path.display().to_string();
        lower_signals_to_fir(
            &source,
            &signals,
            lane,
            self.fir_verify,
            self.real_type,
            self.max_copy_delay,
            self.delay_line_threshold,
        )
        .map_err(|e| lower_fir_error_to_compiler(&source, e))
    }

    /// Parses + evaluates + propagates one file, then emits a WASM module
    /// plus its matched companion JSON through the selected signal->FIR lane.
    pub fn compile_file_to_wasm(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        options: &WasmOptions,
    ) -> Result<WasmModule, CompilerError> {
        self.compile_file_to_wasm_with_lane(
            path,
            search_paths,
            options,
            SignalFirLane::TransformFastLane,
        )
    }

    /// Parses + evaluates + propagates one file, then emits a WASM module
    /// through the selected signal->FIR lane.
    pub fn compile_file_to_wasm_with_lane(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        options: &WasmOptions,
        lane: SignalFirLane,
    ) -> Result<WasmModule, CompilerError> {
        let source = path.display().to_string();
        let signals = self.compile_file_to_signals(path, search_paths)?;
        let lowered = lower_signals_to_fir(
            &source,
            &signals,
            lane,
            self.fir_verify,
            self.real_type,
            self.max_copy_delay,
            self.delay_line_threshold,
        )
        .map_err(|error| lower_fir_error_to_compiler(&source, error))?;
        let json_context = wasm_json_context_for_file(
            path,
            search_paths,
            &signals,
            compile_options_json_string(Some("wasm"), options.double_precision),
        );
        generate_wasm_module_with_context(&lowered.store, lowered.module, options, &json_context)
            .map_err(|error| CompilerError::CodegenWasm {
                source: source.into(),
                error,
            })
    }

    /// Compiles one file-backed DSP source into an owned artifact bundle.
    ///
    /// Compared with [`Self::compile_file_to_wasm_with_lane`], this packages the
    /// result in the artifact-centric shape expected by the `faustwasm`
    /// dual-mode integration plan, so downstream code can treat compile mode
    /// and precompiled-artifact mode uniformly.
    pub fn compile_file_to_wasm_artifact_with_lane(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        options: &WasmOptions,
        lane: SignalFirLane,
    ) -> Result<WasmArtifactBundle, CompilerError> {
        let compile_options = compile_options_json_string(Some("wasm"), options.double_precision);
        let module = self.compile_file_to_wasm_with_lane(path, search_paths, options, lane)?;
        Ok(WasmArtifactBundle::from_wasm_module(
            module,
            compile_options,
        ))
    }

    /// Compiles one file-backed DSP source into an owned artifact bundle using
    /// the production default signal->FIR lane.
    pub fn compile_file_to_wasm_artifact(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        options: &WasmOptions,
    ) -> Result<WasmArtifactBundle, CompilerError> {
        self.compile_file_to_wasm_artifact_with_lane(
            path,
            search_paths,
            options,
            SignalFirLane::TransformFastLane,
        )
    }

    /// Parses + evaluates + propagates one file, then emits strict C++-style JSON.
    pub fn compile_file_to_json(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        self.compile_file_to_json_with_compile_options(
            path,
            search_paths,
            lane,
            compile_options_json_string(None, self.real_type == RealType::Float64),
        )
    }

    /// Parses + evaluates + propagates one file, then emits strict C++-style JSON
    /// with explicit `compile_options` provenance.
    pub fn compile_file_to_json_with_compile_options(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        lane: SignalFirLane,
        compile_options: String,
    ) -> Result<String, CompilerError> {
        let source = path.display().to_string();
        let signals = self.compile_file_to_signals(path, search_paths)?;
        let lowered = lower_signals_to_fir(
            &source,
            &signals,
            lane,
            self.fir_verify,
            self.real_type,
            self.max_copy_delay,
            self.delay_line_threshold,
        )
        .map_err(|error| lower_fir_error_to_compiler(&source, error))?;
        let mut library_list: Vec<String> = signals
            .parse
            .used_files
            .iter()
            .skip(1)
            .map(|file| file.to_string_lossy().into_owned())
            .collect();
        for file in &signals.loaded_files {
            let file = file.to_string_lossy().into_owned();
            if !library_list.iter().any(|existing| existing == &file) {
                library_list.push(file);
            }
        }
        let json = build_strict_json_description(
            &lowered.store,
            lowered.module,
            StrictJsonContext {
                filename: path
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| path.to_string_lossy().into_owned()),
                include_pathnames: merge_import_search_paths(path, search_paths)
                    .into_iter()
                    .map(|dir| dir.to_string_lossy().into_owned())
                    .collect(),
                library_list,
                top_level_meta: json_meta_entries_from_snapshot(&signals.compilation_metadata),
                compile_options,
                double_precision: self.real_type == RealType::Float64,
            },
        )
        .map_err(|error| CompilerError::CodegenWasm {
            source: source.into(),
            error,
        })?;
        Ok(json.render())
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then emits C++ text.
    pub fn compile_file_default_to_cpp(
        &self,
        path: &Path,
        options: &CppOptions,
    ) -> Result<String, CompilerError> {
        self.compile_file_default_to_cpp_with_lane(path, options, SignalFirLane::TransformFastLane)
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then emits C text.
    pub fn compile_file_default_to_c(
        &self,
        path: &Path,
        options: &COptions,
    ) -> Result<String, CompilerError> {
        self.compile_file_default_to_c_with_lane(path, options, SignalFirLane::TransformFastLane)
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then emits C text using the selected signal->FIR lowering lane.
    pub fn compile_file_default_to_c_with_lane(
        &self,
        path: &Path,
        options: &COptions,
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        self.compile_file_to_c_with_lane(path, &[], options, lane)
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then emits C++ text using the selected signal->FIR lowering lane.
    pub fn compile_file_default_to_cpp_with_lane(
        &self,
        path: &Path,
        options: &CppOptions,
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        self.compile_file_to_cpp_with_lane(path, &[], options, lane)
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then lowers to FIR using the selected signal->FIR lane.
    pub fn compile_file_default_to_fir_with_lane(
        &self,
        path: &Path,
        lane: SignalFirLane,
    ) -> Result<FirCompileOutput, CompilerError> {
        self.compile_file_to_fir_with_lane(path, &[], lane)
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then emits a WASM module scaffold.
    ///
    /// This file-backed convenience wrapper follows the same default-lane
    /// policy as [`Compiler::compile_source_to_wasm`]: artifact-oriented WASM
    /// entry points default to [`SignalFirLane::TransformFastLane`].
    pub fn compile_file_default_to_wasm(
        &self,
        path: &Path,
        options: &WasmOptions,
    ) -> Result<WasmModule, CompilerError> {
        self.compile_file_default_to_wasm_with_lane(path, options, SignalFirLane::TransformFastLane)
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then emits a WASM module through the selected signal->FIR lane.
    pub fn compile_file_default_to_wasm_with_lane(
        &self,
        path: &Path,
        options: &WasmOptions,
        lane: SignalFirLane,
    ) -> Result<WasmModule, CompilerError> {
        self.compile_file_to_wasm_with_lane(path, &[], options, lane)
    }

    /// Compiles one file-backed DSP source with the default import search model
    /// into an owned artifact bundle.
    ///
    /// This is the file-backed companion to [`Compiler::compile_wasm_artifact`]
    /// and therefore also defaults to [`SignalFirLane::TransformFastLane`].
    pub fn compile_file_default_to_wasm_artifact(
        &self,
        path: &Path,
        options: &WasmOptions,
    ) -> Result<WasmArtifactBundle, CompilerError> {
        self.compile_file_to_wasm_artifact(path, &[], options)
    }

    /// Returns one `faustwasm` helper-info string.
    ///
    /// Current compatibility policy is intentionally strict and explicit:
    /// - supported now: `version`, `help`
    /// - explicit stub: `libdir`, `includedir`, `archdir`, `dspdir`,
    ///   `pathslist`
    /// - invalid: any unknown key
    pub fn get_faustwasm_info(&self, what: &str) -> Result<String, FaustwasmServiceError> {
        match what {
            "version" => Ok(Self::version().to_owned()),
            "help" => Ok(faustwasm_info_help_text()),
            "libdir" | "includedir" | "archdir" | "dspdir" | "pathslist" => {
                Err(FaustwasmServiceError::unsupported(format!(
                    "getInfos({what}) is not implemented yet in the Rust faustwasm service"
                )))
            }
            _ => Err(FaustwasmServiceError::invalid_argument(format!(
                "incorrect argument passed to getInfos: {what}"
            ))),
        }
    }

    /// Validate and expand one Faust DSP source.
    ///
    /// Parses and evaluates the program using any `-I` search paths carried in
    /// `request.args`.  If compilation succeeds the original source text is
    /// returned verbatim; the Rust compiler currently has no box→DSP serializer
    /// analogous to C++ `printBox`, so the expanded form equals the input.
    ///
    /// Mirrors: `expandDSPFromString` / `expandDSPFromFile` (C++ Faust API).
    pub fn expand_dsp(&self, request: &ExpandDspRequest) -> Result<String, FaustwasmServiceError> {
        let argv: Vec<String> = request.args.split_whitespace().map(str::to_owned).collect();
        let search_paths = parse_search_paths_from_argv(&argv);
        self.compile_source_to_signals_with_search_paths(
            &request.source_name,
            &request.source,
            &search_paths,
        )
        .map(|_| request.source.clone())
        .map_err(|e| FaustwasmServiceError::unsupported(e.to_string()))
    }

    /// Generate auxiliary output files from a Faust DSP source.
    ///
    /// Inspects `request.args` for output-format flags (`-cpp`, `-c`, `-wasm`,
    /// `-json`, `-svg`) and returns one [`AuxFileArtifact`] per generated file.
    /// When no output flag is present an empty list is returned (no error).
    ///
    /// SVG generation writes to a temporary directory and collects all produced
    /// `.svg` files into the result.  The other formats are emitted in memory.
    ///
    /// Mirrors: `generateAuxFilesFromString` / `generateAuxFilesFromFile`
    /// (C++ Faust API).
    pub fn generate_aux_files(
        &self,
        request: &GenerateAuxFilesRequest,
    ) -> Result<Vec<AuxFileArtifact>, FaustwasmServiceError> {
        let argv: Vec<String> = request.args.split_whitespace().map(str::to_owned).collect();
        let search_paths = parse_search_paths_from_argv(&argv);
        let double = argv.iter().any(|a| a == "-double");

        let wants_cpp = argv.iter().any(|a| a == "-cpp");
        let wants_c = argv.iter().any(|a| a == "-c");
        let wants_wasm = argv.iter().any(|a| a == "-wasm");
        let wants_json = argv.iter().any(|a| a == "-json");
        let wants_svg = argv.iter().any(|a| a == "-svg");

        let mut artifacts: Vec<AuxFileArtifact> = Vec::new();
        let stem = std::path::Path::new(&request.source_name)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("process");

        if wants_cpp {
            let cpp = self
                .compile_source_to_cpp(
                    &request.source_name,
                    &request.source,
                    &CppOptions::default(),
                )
                .map_err(|e| FaustwasmServiceError::unsupported(e.to_string()))?;
            artifacts.push(AuxFileArtifact {
                path: format!("{stem}.cpp"),
                content: cpp.into_bytes(),
                binary: false,
            });
        }

        if wants_c {
            let c = self
                .compile_source_to_c(&request.source_name, &request.source, &COptions::default())
                .map_err(|e| FaustwasmServiceError::unsupported(e.to_string()))?;
            artifacts.push(AuxFileArtifact {
                path: format!("{stem}.c"),
                content: c.into_bytes(),
                binary: false,
            });
        }

        if wants_wasm {
            let opts = WasmOptions {
                double_precision: double,
                ..Default::default()
            };
            let wasm = self
                .compile_source_to_wasm(&request.source_name, &request.source, &opts)
                .map_err(|e| FaustwasmServiceError::unsupported(e.to_string()))?;
            artifacts.push(AuxFileArtifact {
                path: format!("{stem}.wasm"),
                content: wasm.wasm_binary,
                binary: true,
            });
            artifacts.push(AuxFileArtifact {
                path: format!("{stem}.json"),
                content: wasm.dsp_json.into_bytes(),
                binary: false,
            });
        } else if wants_json {
            let json = self
                .compile_source_to_json(&request.source_name, &request.source)
                .map_err(|e| FaustwasmServiceError::unsupported(e.to_string()))?;
            artifacts.push(AuxFileArtifact {
                path: format!("{stem}.json"),
                content: json.into_bytes(),
                binary: false,
            });
        }

        if wants_svg {
            let signals = self
                .compile_source_to_signals_with_import_context(
                    &request.source_name,
                    &request.source,
                    &search_paths,
                    &request.virtual_sources,
                )
                .map_err(|e| FaustwasmServiceError::unsupported(e.to_string()))?;
            let draw_config = draw::DrawConfig::default();
            // Use "process" as the diagram name so the root SVG file is
            // always process.svg, matching the C++ compiler convention and
            // the faustwasm FaustSvgDiagrams.from(...) expectation.
            // draw_schema_to_memory avoids any filesystem access so this
            // path is safe on wasm32-unknown-unknown targets.
            let svg_pairs = draw::draw_schema_to_memory(
                &signals.parse.state.arena,
                signals.process_box,
                "process",
                &draw_config,
                &signals.def_names,
            )
            .map_err(|e| FaustwasmServiceError::unsupported(e.to_string()))?;
            artifacts.extend(
                svg_pairs
                    .into_iter()
                    .map(|(path, content)| AuxFileArtifact {
                        path,
                        content,
                        binary: false,
                    }),
            );
        }

        Ok(artifacts)
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then emits strict C++-style JSON.
    ///
    /// This file-backed convenience wrapper follows the same default-lane
    /// policy as [`Compiler::compile_source_to_json`].
    pub fn compile_file_default_to_json(&self, path: &Path) -> Result<String, CompilerError> {
        self.compile_file_default_to_json_with_lane(path, SignalFirLane::TransformFastLane)
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then emits strict C++-style JSON through the selected signal->FIR lane.
    pub fn compile_file_default_to_json_with_lane(
        &self,
        path: &Path,
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        self.compile_file_to_json(path, &[], lane)
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then emits strict C++-style JSON through the selected signal->FIR lane
    /// with explicit `compile_options` provenance.
    pub fn compile_file_default_to_json_with_lane_and_compile_options(
        &self,
        path: &Path,
        lane: SignalFirLane,
        compile_options: String,
    ) -> Result<String, CompilerError> {
        self.compile_file_to_json_with_compile_options(path, &[], lane, compile_options)
    }

    /// Parses + evaluates + propagates one source, then emits `.fbc` bytecode
    /// text via the interpreter backend using the transform fast lane.
    pub fn compile_source_to_interp(
        &self,
        source_name: &str,
        source: &str,
        options: &InterpOptions,
    ) -> Result<String, CompilerError> {
        self.compile_source_to_interp_with_lane(
            source_name,
            source,
            options,
            SignalFirLane::TransformFastLane,
        )
    }

    /// Parses + evaluates + propagates one source, then emits `.fbc` bytecode
    /// text using the selected signal->FIR lowering lane.
    pub fn compile_source_to_interp_with_lane(
        &self,
        source_name: &str,
        source: &str,
        options: &InterpOptions,
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        let signals = self.compile_source_to_signals(source_name, source)?;
        let ctx = SignalLoweringContext {
            lane,
            fir_verify: self.fir_verify,
            real_type: self.real_type,
            max_copy_delay: self.max_copy_delay,
            delay_line_threshold: self.delay_line_threshold,
            timing_sink: self.timing_sink.clone(),
        };
        lower_signals_to_interp(source_name, &signals, options, ctx)
            .map_err(|e| lower_interp_error_to_compiler(source_name, e))
    }

    /// Parses + evaluates + propagates one file, then emits `.fbc` bytecode
    /// text via the interpreter backend using the transform fast lane.
    pub fn compile_file_to_interp(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        options: &InterpOptions,
    ) -> Result<String, CompilerError> {
        self.compile_file_to_interp_with_lane(
            path,
            search_paths,
            options,
            SignalFirLane::TransformFastLane,
        )
    }

    /// Parses + evaluates + propagates one file, then emits `.fbc` bytecode
    /// text using the selected signal->FIR lowering lane.
    pub fn compile_file_to_interp_with_lane(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        options: &InterpOptions,
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        let signals = self.compile_file_to_signals(path, search_paths)?;
        let source = path.display().to_string();
        let ctx = SignalLoweringContext {
            lane,
            fir_verify: self.fir_verify,
            real_type: self.real_type,
            max_copy_delay: self.max_copy_delay,
            delay_line_threshold: self.delay_line_threshold,
            timing_sink: self.timing_sink.clone(),
        };
        lower_signals_to_interp(&source, &signals, options, ctx)
            .map_err(|e| lower_interp_error_to_compiler(&source, e))
    }

    /// Parses + evaluates + propagates one file with default import search
    /// path, then emits `.fbc` bytecode text via the interpreter backend.
    pub fn compile_file_default_to_interp(
        &self,
        path: &Path,
        options: &InterpOptions,
    ) -> Result<String, CompilerError> {
        self.compile_file_default_to_interp_with_lane(
            path,
            options,
            SignalFirLane::TransformFastLane,
        )
    }

    /// Parses + evaluates + propagates one file with default import search
    /// path, then emits `.fbc` bytecode text using the selected lane.
    pub fn compile_file_default_to_interp_with_lane(
        &self,
        path: &Path,
        options: &InterpOptions,
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        self.compile_file_to_interp_with_lane(path, &[], options, lane)
    }

    /// Runs the shared `parse output -> eval -> arity -> propagate` pipeline.
    ///
    /// This is the semantic heart of the facade. All higher-level helpers
    /// (`compile_*_to_signals`, backend emitters, FIR dump paths) eventually
    /// flow through this function so they observe the same:
    /// - evaluator source-loading semantics,
    /// - top-level metadata aggregation rules,
    /// - diagnostic enrichment policy,
    /// - process arity inference and signal propagation contract.
    fn pipeline_to_signals(
        &self,
        source: &str,
        mut output: ParseOutput,
        eval_source_context: Option<eval::EvalSourceContext>,
    ) -> Result<SignalCompileOutput, CompilerError> {
        let root = output.root.ok_or_else(|| CompilerError::MissingRoot {
            source: source.into(),
        })?;

        let eval_result = self.time_phase("evaluation", || {
            match (&eval_source_context, &self.cancel) {
                (Some(source_context), Some(cancel)) => {
                    eval::eval_entrypoint_with_source_context_and_cancel(
                        &mut output.state.arena,
                        root,
                        self.entrypoint_name.as_ref(),
                        source_context.clone(),
                        std::sync::Arc::clone(cancel),
                    )
                }
                (Some(source_context), None) => {
                    eval::eval_entrypoint_with_stats_and_source_context(
                        &mut output.state.arena,
                        root,
                        self.entrypoint_name.as_ref(),
                        source_context.clone(),
                    )
                }
                (None, _) => eval::eval_entrypoint_with_stats(
                    &mut output.state.arena,
                    root,
                    self.entrypoint_name.as_ref(),
                ),
            }
        });
        let (process_box, eval_stats) = eval_result.map_err(|error| {
            let node = eval_error_node(&error);
            let owner =
                node.and_then(|n| owner_definition_name_for_node(&output.state.arena, root, n));
            let mut diagnostic = error.clone().into_diagnostic();
            if let Some(n) = node {
                diagnostic = enrich_diagnostic_with_node(
                    diagnostic,
                    &output.state.arena,
                    root,
                    n,
                    owner.as_deref(),
                    self.entrypoint_name.as_ref(),
                );
                diagnostic = maybe_add_eval_source_labels(
                    diagnostic,
                    &output.state.ctx,
                    &output.state.arena,
                    root,
                    n,
                    owner.as_deref(),
                    self.entrypoint_name.as_ref(),
                );
            }
            CompilerError::Eval {
                source: source.into(),
                error: Box::new(error),
                diagnostics: bundle_from_diagnostic(diagnostic),
            }
        })?;

        let ep = self.entrypoint_name.as_ref();
        let process_flat = self
            .time_phase("box-flatten", || {
                propagate::try_build_flat_box(&output.state.arena, process_box)
            })
            .map_err(|e| {
                make_propagate_compiler_error(
                    source,
                    e.into(),
                    &output.state.arena,
                    &output.state.ctx,
                    root,
                    ep,
                    false,
                )
            })?;

        let mut arity_cache = ArityCache::new();
        let process_arity = self
            .time_phase("arity", || {
                propagate::box_arity_typed(&output.state.arena, process_flat, &mut arity_cache)
            })
            .map_err(|e| {
                make_propagate_compiler_error(
                    source,
                    e,
                    &output.state.arena,
                    &output.state.ctx,
                    root,
                    ep,
                    true,
                )
            })?;

        let compilation_metadata = eval_source_context.as_ref().map_or_else(
            || output.compilation_metadata.clone(),
            eval::EvalSourceContext::metadata_snapshot,
        );
        let ui_options =
            PropagateUiOptions::new(resolve_ui_root_label(source, &compilation_metadata));
        let inputs = propagate::make_sig_input_list(&mut output.state.arena, process_arity.inputs);
        let propagated = self
            .time_phase("propagation", || {
                propagate::propagate_typed_with_ui_options(
                    &mut output.state.arena,
                    process_flat,
                    &inputs,
                    &mut arity_cache,
                    &ui_options,
                )
            })
            .map_err(|e| {
                make_propagate_compiler_error(
                    source,
                    e,
                    &output.state.arena,
                    &output.state.ctx,
                    root,
                    ep,
                    true,
                )
            })?;
        self.time_phase("signal-type-validation", || {
            validate_signal_types(
                source,
                &output.state.arena,
                &propagated.signals,
                &propagated.ui,
            )
        })?;

        Ok(SignalCompileOutput {
            compilation_metadata,
            parse: output,
            loaded_files: eval_source_context
                .as_ref()
                .map_or_else(Vec::new, eval::EvalSourceContext::loaded_files),
            process_box,
            process_arity,
            signals: propagated.signals,
            ui: propagated.ui,
            def_names: eval_stats.def_names,
        })
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Compiler facade errors for parser-stage orchestration.
/// Top-level compiler error surface aggregating all stage failures.
#[derive(Debug)]
pub enum CompilerError {
    /// Import resolution/read failure before parse completion.
    Import(SourceReaderError),
    /// Parse output did not expose a root node.
    MissingRoot { source: Box<str> },
    /// Parse failed (`errors` or `recoveries` present).
    Parse {
        source: Box<str>,
        parse_errors: usize,
        recoveries: u32,
        diagnostics: DiagnosticBundle,
    },
    /// Eval stage failed while reducing boxes.
    Eval {
        source: Box<str>,
        error: Box<eval::EvalError>,
        diagnostics: DiagnosticBundle,
    },
    /// Propagate stage failed while lowering boxes to signals.
    Propagate {
        source: Box<str>,
        error: PropagateError,
        diagnostics: DiagnosticBundle,
    },
    /// Signal type validation failed after propagation.
    Type {
        source: Box<str>,
        error: Box<str>,
        diagnostics: DiagnosticBundle,
    },
    /// Transform stage failed while lowering signals to FIR.
    Transform {
        source: Box<str>,
        error: SignalFirError,
        diagnostics: DiagnosticBundle,
    },
    /// FIR verifier rejected a lowered FIR module before backend codegen.
    FirVerify {
        source: Box<str>,
        strict: bool,
        diagnostics: DiagnosticBundle,
    },
    /// C++ backend emission failed from FIR.
    Codegen {
        source: Box<str>,
        error: CodegenError,
    },
    /// C backend emission failed from FIR.
    CodegenC {
        source: Box<str>,
        error: CCodegenError,
    },
    /// Interpreter backend emission failed from FIR.
    CodegenInterp {
        source: Box<str>,
        error: InterpCodegenError,
    },
    /// WASM backend emission failed from FIR.
    CodegenWasm {
        source: Box<str>,
        error: WasmBackendError,
    },
}

impl std::fmt::Display for CompilerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Import(err) => write!(f, "{err}"),
            Self::MissingRoot { source } => write!(f, "parse returned no root for {source}"),
            Self::Parse {
                source,
                parse_errors,
                recoveries,
                diagnostics,
            } => write!(
                f,
                "parse failed for {source}: errors={parse_errors}, recoveries={recoveries}, diagnostics={}",
                diagnostics.len()
            ),
            Self::Eval { source, error, .. } => {
                write!(f, "evaluation failed for {source}: {error}")
            }
            Self::Propagate { source, error, .. } => {
                write!(f, "propagation failed for {source}: {error}")
            }
            Self::Type { source, error, .. } => {
                write!(f, "type validation failed for {source}: {error}")
            }
            Self::Transform { source, error, .. } => {
                write!(f, "transform failed for {source}: {error}")
            }
            Self::FirVerify {
                source,
                strict,
                diagnostics,
            } => write!(
                f,
                "FIR verification failed for {source}{}: diagnostics={}",
                if *strict { " (strict mode)" } else { "" },
                diagnostics.len()
            ),
            Self::Codegen { source, error } => {
                write!(f, "code generation failed for {source}: {error}")
            }
            Self::CodegenC { source, error } => {
                write!(f, "code generation failed for {source}: {error}")
            }
            Self::CodegenInterp { source, error } => {
                write!(f, "code generation failed for {source}: {error}")
            }
            Self::CodegenWasm { source, error } => {
                write!(f, "code generation failed for {source}: {error}")
            }
        }
    }
}

impl std::error::Error for CompilerError {}

impl CompilerError {
    /// Returns structured diagnostics when this error variant carries them.
    #[must_use]
    pub fn diagnostics(&self) -> Option<&DiagnosticBundle> {
        match self {
            Self::Parse { diagnostics, .. } => Some(diagnostics),
            Self::Eval { diagnostics, .. } => Some(diagnostics),
            Self::Propagate { diagnostics, .. } => Some(diagnostics),
            Self::Type { diagnostics, .. } => Some(diagnostics),
            Self::Transform { diagnostics, .. } => Some(diagnostics),
            Self::FirVerify { diagnostics, .. } => Some(diagnostics),
            Self::Codegen { .. } => None,
            Self::CodegenC { .. } => None,
            Self::CodegenWasm { .. } => None,
            _ => None,
        }
    }
}

// ─── Helpers: path resolution ─────────────────────────────────────────────────

/// Resolves the default built-in import search paths for one file-backed
/// compilation session.
///
/// # Source provenance (C++)
/// - `global::initDocumentNames()` / `global::initDirectories()` in
///   `compiler/global.cpp`
///
/// # Effective order
/// 1. current file parent directory (or `"."` for a bare filename)
/// 2. `FAUST_LIB_PATH` when present
/// 3. executable-relative `../share/faust`
/// 4. `/usr/local/share/faust`
/// 5. `/usr/share/faust`
///
/// This mirrors the C++ hardcoded library-search model as closely as possible
/// in a standalone Rust binary.
#[must_use]
/// Returns the default Faust import search paths for `path`.
pub fn default_import_search_paths(path: &Path) -> Vec<PathBuf> {
    build_import_search_paths(
        path,
        &[],
        std::env::var_os("FAUST_LIB_PATH"),
        std::env::current_exe().ok(),
    )
}

/// Builds the import search path list for a given source file, merging user-supplied
/// extra paths with the built-in defaults discovered from the environment.
///
/// This is a convenience wrapper over [`build_import_search_paths`] that reads
/// `FAUST_LIB_PATH` and the current executable location automatically.
fn merge_import_search_paths(path: &Path, extra_paths: &[PathBuf]) -> Vec<PathBuf> {
    build_import_search_paths(
        path,
        extra_paths,
        std::env::var_os("FAUST_LIB_PATH"),
        std::env::current_exe().ok(),
    )
}

/// Core implementation of the import search path algorithm.
///
/// Produces an ordered, deduplicated list following the same priority rules as
/// the C++ Faust compiler:
///
/// 1. User-supplied `extra_paths` (highest priority).
/// 2. Directory containing the source file.
/// 3. Paths from the `FAUST_LIB_PATH` environment variable (colon/semicolon-separated).
/// 4. Standard library locations relative to the running executable.
///
/// Parameters are explicit so the function is pure and fully testable without
/// touching the environment.
fn build_import_search_paths(
    path: &Path,
    extra_paths: &[PathBuf],
    faust_lib_path: Option<OsString>,
    current_exe: Option<PathBuf>,
) -> Vec<PathBuf> {
    /// Appends `candidate` only if it is not already present in `paths`.
    fn push_unique(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
        if !paths.iter().any(|existing| existing == &candidate) {
            paths.push(candidate);
        }
    }

    let mut ordered = Vec::with_capacity(extra_paths.len() + 5);
    for path in extra_paths {
        push_unique(&mut ordered, path.clone());
    }

    push_unique(
        &mut ordered,
        path.parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from(".")),
    );

    if let Some(env_path) = faust_lib_path {
        push_unique(&mut ordered, PathBuf::from(env_path));
    }

    if let Some(share_root) = current_exe
        .as_deref()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .map(|root| root.join("share").join("faust"))
    {
        push_unique(&mut ordered, share_root);
    }

    push_unique(&mut ordered, PathBuf::from("/usr/local/share/faust"));
    push_unique(&mut ordered, PathBuf::from("/usr/share/faust"));
    ordered
}

// ─── Helpers: parse validation ────────────────────────────────────────────────

/// Converts raw parser output into the facade-level success/error contract.
///
/// The parser may return a root node even when recoveries or hard errors were
/// recorded. The compiler facade treats any non-zero parse error or recovery
/// count as a stage failure, matching the stricter "ready for later phases"
/// contract expected by `eval` and `propagate`.
fn ensure_parse_success(source: &str, output: ParseOutput) -> Result<ParseOutput, CompilerError> {
    let parse_errors = usize::try_from(output.state.ctx.parse_error_count()).unwrap_or(usize::MAX);
    let recoveries = output.state.ctx.recovery_count();
    let has_root = output.root.is_some();
    if has_root && parse_errors == 0 && recoveries == 0 {
        Ok(output)
    } else {
        Err(CompilerError::Parse {
            source: source.into(),
            parse_errors,
            recoveries,
            diagnostics: output.diagnostics,
        })
    }
}

// ─── Helpers: error mapping ───────────────────────────────────────────────────

/// Maps a `LowerToCppError` into a `CompilerError`, attaching the source name.
///
/// This keeps the backend-specific lower pipeline internal while exposing one
/// stable facade error surface to callers.
fn lower_cpp_error_to_compiler(source: &str, error: LowerToCppError) -> CompilerError {
    match error {
        LowerError::Transform(error) => transform_error_to_compiler(source, error),
        LowerError::Verify(report) => fir_verify_error_to_compiler(source, report),
        LowerError::Codegen(error) => CompilerError::Codegen {
            source: source.into(),
            error,
        },
    }
}

/// Maps a `LowerToCError` into a `CompilerError`, attaching the source name.
///
/// Each backend-specific error variant is mapped to the matching `CompilerError`
/// variant so callers never need to depend on the internal lower-pipeline types.
fn lower_c_error_to_compiler(source: &str, error: LowerToCError) -> CompilerError {
    match error {
        LowerError::Transform(error) => transform_error_to_compiler(source, error),
        LowerError::Verify(report) => fir_verify_error_to_compiler(source, report),
        LowerError::Codegen(error) => CompilerError::CodegenC {
            source: source.into(),
            error,
        },
    }
}

/// Maps a `LowerToInterpError` into a `CompilerError`, attaching the source name.
///
/// The serialization failure arm is normalized into the interpreter backend
/// error surface so CLI and library callers do not need a fourth dedicated
/// interpreter-specific error branch.
fn lower_interp_error_to_compiler(source: &str, error: LowerToInterpError) -> CompilerError {
    match error {
        LowerToInterpError::Transform(error) => transform_error_to_compiler(source, error),
        LowerToInterpError::Verify(report) => fir_verify_error_to_compiler(source, report),
        LowerToInterpError::Codegen(error) => CompilerError::CodegenInterp {
            source: source.into(),
            error,
        },
        LowerToInterpError::Serialize(message) => CompilerError::CodegenInterp {
            source: source.into(),
            error: InterpCodegenError {
                code: InterpCodegenErrorCode::CompilationFailed,
                message,
            },
        },
    }
}

/// Maps a `LowerToFirError` into a `CompilerError`, attaching the source name.
fn lower_fir_error_to_compiler(source: &str, error: LowerToFirError) -> CompilerError {
    match error {
        LowerToFirError::Transform(error) => transform_error_to_compiler(source, error),
        LowerToFirError::Verify(report) => fir_verify_error_to_compiler(source, report),
    }
}

/// Wraps a `SignalFirError` into a `CompilerError::Transform` with one diagnostic.
///
/// The diagnostic bundle is built by [`signal_fir_diagnostic`] which extracts
/// source location and note information from the transform error.
fn transform_error_to_compiler(source: &str, error: SignalFirError) -> CompilerError {
    let diagnostic = signal_fir_diagnostic(&error);
    CompilerError::Transform {
        source: source.into(),
        diagnostics: bundle_from_diagnostic(diagnostic),
        error,
    }
}

/// Wraps a FIR verifier report into the facade error surface.
///
/// `strict` is recorded only for the warning-only case promoted to a failure by
/// compiler policy. Reports containing real verifier errors are always fatal,
/// independent from the strictness flag.
fn fir_verify_error_to_compiler(source: &str, report: FirVerifyReport) -> CompilerError {
    let strict = report.warnings().next().is_some() && !report.has_errors();
    CompilerError::FirVerify {
        source: source.into(),
        strict,
        diagnostics: fir_verify_bundle_from_report(&report),
    }
}

/// Runs canonical `sigtype` validation on propagated signals before later stages.
fn validate_signal_types(
    source: &str,
    arena: &tlib::TreeArena,
    signals: &[SigId],
    ui: &UiProgram,
) -> Result<(), CompilerError> {
    let mut annotator = TypeAnnotator::new(arena, ui);
    annotator
        .annotate(signals)
        .map(|_| ())
        .map_err(|error| type_error_to_compiler(source, error.0))
}

/// Wraps a signal type validation error into the compiler facade error surface.
fn type_error_to_compiler(source: &str, error: String) -> CompilerError {
    let diagnostic = Diagnostic::new(
        Severity::Error,
        Stage::Compiler,
        COMP_TYPE_FAILED,
        error.clone(),
    )
    .with_note("stage=sigtype");
    CompilerError::Type {
        source: source.into(),
        error: error.into_boxed_str(),
        diagnostics: bundle_from_diagnostic(diagnostic),
    }
}

// ─── DiagCtx: shared pipeline diagnostic enrichment ──────────────────────────

/// Builds a `CompilerError::Propagate` with standard node-level enrichment.
///
/// Used by the three propagate-stage steps in `pipeline_to_signals`
/// (flat-box boundary, arity inference, signal propagation) which share the
/// same enrichment policy.  Set `add_paired` for composition errors
/// (seq/split/merge/rec) that benefit from paired A/B arity context.
fn make_propagate_compiler_error(
    source: &str,
    error: propagate::PropagateError,
    arena: &tlib::TreeArena,
    ctx: &parser::ParserCtx,
    root: BoxId,
    entrypoint_name: &str,
    add_paired: bool,
) -> CompilerError {
    let node = propagate_error_node(&error);
    let owner = node.and_then(|n| owner_definition_name_for_node(arena, root, n));
    let mut diagnostic = error.clone().into_diagnostic();
    if let Some(n) = node {
        diagnostic = enrich_diagnostic_with_node(
            diagnostic,
            arena,
            root,
            n,
            owner.as_deref(),
            entrypoint_name,
        );
        if add_paired {
            diagnostic = add_paired_propagate_context(diagnostic, &error, arena);
        }
        diagnostic = maybe_add_source_label(
            diagnostic,
            ctx,
            arena,
            root,
            n,
            owner.as_deref(),
            entrypoint_name,
        );
    }
    CompilerError::Propagate {
        source: source.into(),
        error,
        diagnostics: bundle_from_diagnostic(diagnostic),
    }
}

/// Enriches a diagnostic with the standard node-level notes shared across
/// eval, arity, and propagate error handlers.
///
/// Takes the arena and root by reference at call-site (not stored) so that
/// mutable borrows of the arena remain possible between phase calls.
fn enrich_diagnostic_with_node(
    mut diagnostic: Diagnostic,
    arena: &tlib::TreeArena,
    root: BoxId,
    node: BoxId,
    owner: Option<&str>,
    entrypoint_name: &str,
) -> Diagnostic {
    diagnostic = diagnostic
        .with_note(format!("node_id={}", node.as_u32()))
        .with_note(format!("box_expr={}", compact_box_preview(arena, node)))
        .with_note(format!("expr={}", compact_human_box_preview(arena, node)));
    if let Some(owner) = owner {
        diagnostic = diagnostic.with_note(format!("error originates from definition '{owner}'"));
    }
    if let Some(trace) = alias_binding_trace_for_node(arena, root, node, entrypoint_name) {
        diagnostic = diagnostic.with_note(format!("binding_trace={trace}"));
    }
    diagnostic
}

// ─── Signal-to-FIR lower errors ───────────────────────────────────────────────

/// Generic lower-to-backend error for backends that follow the
/// Transform → Verify → Codegen pattern.
///
/// `E` is the backend-specific codegen error type.
/// Specialised as [`LowerToCppError`] and [`LowerToCError`].
#[derive(Debug)]
enum LowerError<E> {
    /// Fast-lane signal-to-FIR lowering failed.
    Transform(SignalFirError),
    /// Optional FIR verification rejected the lowered module.
    Verify(FirVerifyReport),
    /// Backend emission failed after successful FIR lowering.
    Codegen(E),
}

/// Lower error for the C++ backend.
type LowerToCppError = LowerError<CodegenError>;
/// Lower error for the C backend.
type LowerToCError = LowerError<CCodegenError>;

#[derive(Debug)]
enum LowerToInterpError {
    /// Fast-lane signal-to-FIR lowering failed.
    Transform(SignalFirError),
    /// Optional FIR verification rejected the lowered module.
    Verify(FirVerifyReport),
    /// Interpreter backend emission failed after successful lowering.
    Codegen(InterpCodegenError),
    /// Serialization of the factory to `.fbc` text failed.
    Serialize(String),
}

#[derive(Debug)]
enum LowerToFirError {
    /// Fast-lane signal-to-FIR lowering failed.
    Transform(SignalFirError),
    /// Optional FIR verification rejected the lowered module.
    Verify(FirVerifyReport),
}

fn time_phase_with_sink<T>(
    timing_sink: Option<&TimingSink>,
    name: &'static str,
    f: impl FnOnce() -> T,
) -> T {
    if let Some(sink) = timing_sink {
        let start = Instant::now();
        let result = f();
        sink(name, start.elapsed());
        result
    } else {
        f()
    }
}

// ─── Signal-to-FIR lower functions ───────────────────────────────────────────

#[derive(Clone)]
struct SignalLoweringContext {
    lane: SignalFirLane,
    fir_verify: FirVerifyOptions,
    real_type: RealType,
    max_copy_delay: u32,
    delay_line_threshold: u32,
    timing_sink: Option<TimingSink>,
}

/// Dispatches C++ lowering through the selected signal->FIR lane.
///
/// The backend itself always consumes FIR; the lane choice controls only how
/// the intermediate FIR module is produced from the propagated signal list.
fn lower_signals_to_cpp(
    source_name: &str,
    output: &SignalCompileOutput,
    options: &CppOptions,
    ctx: SignalLoweringContext,
) -> Result<String, LowerToCppError> {
    let _ = ctx.lane;
    lower_signals_to_cpp_transform_fastlane(source_name, output, options, &ctx)
}

/// Dispatches C lowering through the selected signal->FIR lane.
fn lower_signals_to_c(
    source_name: &str,
    output: &SignalCompileOutput,
    options: &COptions,
    ctx: SignalLoweringContext,
) -> Result<String, LowerToCError> {
    let _ = ctx.lane;
    lower_signals_to_c_transform_fastlane(source_name, output, options, &ctx)
}

/// Dispatches interpreter lowering through the selected signal->FIR lane.
fn lower_signals_to_interp(
    source_name: &str,
    output: &SignalCompileOutput,
    options: &InterpOptions,
    ctx: SignalLoweringContext,
) -> Result<String, LowerToInterpError> {
    let _ = ctx.lane;
    lower_signals_to_interp_transform_fastlane(source_name, output, options, &ctx)
}

/// Lowers signals through the transform fast lane then serializes an interpreter `.fbc`.
///
/// This function reuses `lower_signals_to_fir_transform_fastlane` so that the
/// C, C++, and interp transform paths share one FIR lowering implementation.
fn lower_signals_to_interp_transform_fastlane(
    source_name: &str,
    output: &SignalCompileOutput,
    options: &InterpOptions,
    ctx: &SignalLoweringContext,
) -> Result<String, LowerToInterpError> {
    let module_name = resolve_module_name(options.module_name.as_deref(), source_name);
    let timing_sink = ctx.timing_sink.as_ref();
    let lowered = time_phase_with_sink(timing_sink, "signal-fir", || {
        lower_signals_to_fir_transform_fastlane(
            output,
            module_name,
            ctx.real_type,
            ctx.max_copy_delay,
            ctx.delay_line_threshold,
        )
    })
    .map_err(LowerToInterpError::Transform)?;
    time_phase_with_sink(timing_sink, "fir-verify", || {
        maybe_verify_fir_module(&lowered, ctx.fir_verify)
    })
    .map_err(LowerToInterpError::Verify)?;
    match ctx.real_type {
        RealType::Float32 => {
            let factory: FbcDspFactory<f32> =
                time_phase_with_sink(timing_sink, "interp-codegen", || {
                    generate_interp_module(&lowered.store, lowered.module, options)
                })
                .map_err(LowerToInterpError::Codegen)?;
            time_phase_with_sink(timing_sink, "interp-serialize", || {
                serialize_factory(&factory)
            })
            .map_err(LowerToInterpError::Serialize)
        }
        RealType::Float64 => {
            let factory: FbcDspFactory<f64> =
                time_phase_with_sink(timing_sink, "interp-codegen", || {
                    generate_interp_module(&lowered.store, lowered.module, options)
                })
                .map_err(LowerToInterpError::Codegen)?;
            time_phase_with_sink(timing_sink, "interp-serialize", || {
                serialize_factory(&factory)
            })
            .map_err(LowerToInterpError::Serialize)
        }
    }
}

/// Serializes a [`FbcDspFactory`] to `.fbc` text format.
fn serialize_factory<R: FbcReal>(factory: &FbcDspFactory<R>) -> Result<String, String> {
    let mut buf = Vec::new();
    write_fbc(factory, &mut buf, false).map_err(|e| e.to_string())?;
    String::from_utf8(buf).map_err(|e| e.to_string())
}

/// Lowers propagated signals to FIR without invoking a backend emitter.
///
/// This is the shared implementation behind FIR dump/verification flows and is
/// also used as the backend-independent boundary for lane comparisons.
fn lower_signals_to_fir(
    source_name: &str,
    output: &SignalCompileOutput,
    _lane: SignalFirLane,
    fir_verify: FirVerifyOptions,
    real_type: RealType,
    max_copy_delay: u32,
    delay_line_threshold: u32,
) -> Result<FirCompileOutput, LowerToFirError> {
    let module_name = sanitize_cpp_ident(source_name_to_class(source_name).as_str());
    let lowered = lower_signals_to_fir_transform_fastlane(
        output,
        module_name,
        real_type,
        max_copy_delay,
        delay_line_threshold,
    )
    .map_err(LowerToFirError::Transform)?;
    maybe_verify_fir_module(&lowered, fir_verify).map_err(LowerToFirError::Verify)?;
    Ok(lowered)
}

/// Resolves a module name from explicit class_name option or from the source name.
fn resolve_module_name(class_name: Option<&str>, _source_name: &str) -> String {
    class_name
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| "mydsp".to_owned())
}

/// Transform fast-lane FIR lowering used by native backends and FIR dumps.
fn lower_signals_to_fir_transform_fastlane(
    output: &SignalCompileOutput,
    module_name: String,
    real_type: RealType,
    max_copy_delay: u32,
    delay_line_threshold: u32,
) -> Result<FirCompileOutput, SignalFirError> {
    let signal_fir_options = SignalFirOptions {
        module_name,
        strict_mode: true,
        real_type,
        max_copy_delay,
        delay_line_threshold,
    };
    let lowered = compile_signals_to_fir_fastlane_with_ui(
        &output.parse.state.arena,
        &output.signals,
        output.process_arity.inputs,
        output.propagated_output_count(),
        &output.ui,
        &signal_fir_options,
    )?;
    Ok(FirCompileOutput {
        store: lowered.store,
        module: lowered.module,
    })
}

/// Lowers signals through the transform fast lane, verifies FIR, then emits C++.
fn lower_signals_to_cpp_transform_fastlane(
    source_name: &str,
    output: &SignalCompileOutput,
    options: &CppOptions,
    ctx: &SignalLoweringContext,
) -> Result<String, LowerToCppError> {
    let module_name = resolve_module_name(options.class_name.as_deref(), source_name);
    let timing_sink = ctx.timing_sink.as_ref();
    let lowered = time_phase_with_sink(timing_sink, "signal-fir", || {
        lower_signals_to_fir_transform_fastlane(
            output,
            module_name,
            ctx.real_type,
            ctx.max_copy_delay,
            ctx.delay_line_threshold,
        )
    })
    .map_err(LowerError::Transform)?;
    time_phase_with_sink(timing_sink, "fir-verify", || {
        maybe_verify_fir_module(&lowered, ctx.fir_verify)
    })
    .map_err(LowerError::Verify)?;
    time_phase_with_sink(timing_sink, "cpp-codegen", || {
        generate_cpp_module(&lowered.store, lowered.module, options)
    })
    .map_err(LowerError::Codegen)
}

/// Lowers signals through the transform fast lane, verifies FIR, then emits C.
fn lower_signals_to_c_transform_fastlane(
    source_name: &str,
    output: &SignalCompileOutput,
    options: &COptions,
    ctx: &SignalLoweringContext,
) -> Result<String, LowerToCError> {
    let module_name = resolve_module_name(options.class_name.as_deref(), source_name);
    let timing_sink = ctx.timing_sink.as_ref();
    let lowered = time_phase_with_sink(timing_sink, "signal-fir", || {
        lower_signals_to_fir_transform_fastlane(
            output,
            module_name,
            ctx.real_type,
            ctx.max_copy_delay,
            ctx.delay_line_threshold,
        )
    })
    .map_err(LowerError::Transform)?;
    time_phase_with_sink(timing_sink, "fir-verify", || {
        maybe_verify_fir_module(&lowered, ctx.fir_verify)
    })
    .map_err(LowerError::Verify)?;
    time_phase_with_sink(timing_sink, "c-codegen", || {
        generate_c_module(&lowered.store, lowered.module, options)
    })
    .map_err(LowerError::Codegen)
}

/// Runs optional FIR verification according to the compiler facade policy.
///
/// In strict mode, warnings are promoted to fatal errors to support CI and
/// parity-audit workflows that want a clean FIR module before backend lowering.
fn maybe_verify_fir_module(
    lowered: &FirCompileOutput,
    options: FirVerifyOptions,
) -> Result<(), FirVerifyReport> {
    if !options.enabled {
        return Ok(());
    }
    let report = verify_fir_module(&lowered.store, lowered.module);
    let fatal = report.has_errors() || (options.strict && report.warnings().next().is_some());
    if fatal { Err(report) } else { Ok(()) }
}

// ─── Diagnostic helpers ───────────────────────────────────────────────────────

/// Converts a FIR verifier report into the workspace diagnostic bundle format.
fn fir_verify_bundle_from_report(report: &FirVerifyReport) -> DiagnosticBundle {
    let mut bundle = DiagnosticBundle::new();
    for d in &report.diagnostics {
        let code = match d.severity {
            FirVerifySeverity::Error => errors::codes::FIR_VERIFY_ERROR,
            FirVerifySeverity::Warning => errors::codes::FIR_VERIFY_WARNING,
        };
        let severity = match d.severity {
            FirVerifySeverity::Error => errors::Severity::Error,
            FirVerifySeverity::Warning => errors::Severity::Warning,
        };
        let mut diag = Diagnostic::new(severity, errors::Stage::Fir, code, d.message.clone())
            .with_note(format!("fir_code={}", d.code))
            .with_note(format!("fir_node_id={}", d.node.as_u32()));
        if let Some(fun) = d.context.function_name.as_deref() {
            diag = diag.with_note(format!("fir_function={fun}"));
        }
        if let Some(var) = d.context.variable_name.as_deref() {
            diag = diag.with_note(format!("fir_variable={var}"));
        }
        bundle.push(diag);
    }
    bundle
}

/// Converts a `signal_fir` lowering error into a structured compiler diagnostic.
fn signal_fir_diagnostic(error: &SignalFirError) -> Diagnostic {
    let code = match error.code() {
        SignalFirErrorCode::InvalidOptions => errors::codes::SFIR_INVALID_OPTIONS,
        SignalFirErrorCode::EmptySignalList => errors::codes::SFIR_EMPTY_SIGNAL_LIST,
        SignalFirErrorCode::OutputArityMismatch => errors::codes::SFIR_OUTPUT_ARITY_MISMATCH,
        SignalFirErrorCode::UnsupportedSignalNode => errors::codes::SFIR_UNSUPPORTED_SIGNAL_NODE,
        SignalFirErrorCode::UnsupportedBinOp => errors::codes::SFIR_UNSUPPORTED_BINOP,
        SignalFirErrorCode::InputIndexOutOfRange => errors::codes::SFIR_INPUT_INDEX_OUT_OF_RANGE,
    };
    Diagnostic::new(
        errors::Severity::Error,
        errors::Stage::Transform,
        code,
        error.to_string(),
    )
}

// ─── Name utilities ───────────────────────────────────────────────────────────

/// Derives the base class/module name from a source filename.
fn source_name_to_class(source_name: &str) -> String {
    Path::new(source_name)
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|stem| !stem.is_empty())
        .unwrap_or("faust_dsp")
        .to_owned()
}

fn source_name_to_filename(source_name: &str) -> String {
    Path::new(source_name)
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|name| !name.is_empty())
        .unwrap_or(source_name)
        .to_owned()
}

fn faustwasm_info_help_text() -> String {
    let mut out = String::new();
    out.push_str("faust-rs faustwasm helper info\n");
    out.push_str("supported keys:\n");
    out.push_str("- version\n");
    out.push_str("- help\n");
    out.push_str("stubbed keys (unsupported for now):\n");
    out.push_str("- libdir\n");
    out.push_str("- includedir\n");
    out.push_str("- archdir\n");
    out.push_str("- dspdir\n");
    out.push_str("- pathslist\n");
    out
}

struct StrictJsonContext {
    filename: String,
    include_pathnames: Vec<String>,
    library_list: Vec<String>,
    top_level_meta: Vec<JsonMetaEntry>,
    compile_options: String,
    double_precision: bool,
}

fn build_strict_json_description(
    store: &FirStore,
    module: FirId,
    context: StrictJsonContext,
) -> Result<JsonDescription, WasmBackendError> {
    let fir::FirMatch::Module {
        name,
        functions,
        num_inputs,
        num_outputs,
        ..
    } = fir::match_fir(store, module)
    else {
        return Err(WasmBackendError::new(
            codegen::backends::wasm::WasmBackendErrorCode::UnsupportedModuleShape,
            "JSON generation expects a FIR Module root",
        ));
    };
    let fir::FirMatch::Block(function_items) = fir::match_fir(store, functions) else {
        return Err(WasmBackendError::new(
            codegen::backends::wasm::WasmBackendErrorCode::UnsupportedFirNode,
            "JSON generation expects the functions section to be a FIR Block",
        ));
    };
    let layout = WasmMemoryLayout::from_module(
        store,
        module,
        &WasmOptions {
            double_precision: context.double_precision,
            ..WasmOptions::default()
        },
        0,
    )?;
    build_json_description_from_fir(
        store,
        &function_items,
        JsonBuildOptions {
            name,
            filename: Some(context.filename),
            version: Some(Compiler::version().to_owned()),
            compile_options: Some(context.compile_options),
            library_list: context.library_list,
            include_pathnames: context.include_pathnames,
            top_level_meta: context.top_level_meta,
            size: Some(layout.struct_size),
            inputs: num_inputs,
            outputs: num_outputs,
            sr_index: None,
        },
        |_var| None,
    )
    .map_err(|error| {
        WasmBackendError::new(
            codegen::backends::wasm::WasmBackendErrorCode::UnsupportedFirNode,
            error.to_string(),
        )
    })
}

/// C++-parity baseline for the subset of `global::printCompilationOptions1()`
/// currently exposed by the Rust CLI/compiler path.
///
/// Mapping status: `adapted`.
/// - Included now: only the options that the Rust CLI actually exposes for the
///   selected flow (`-lang <backend>` when relevant, plus the float mode).
/// - Deferred: the rest of the C++ global option matrix until the
///   corresponding CLI/compiler knobs exist here.
pub fn compile_options_json_string(lang: Option<&str>, double_precision: bool) -> String {
    let float_mode = if double_precision {
        "-double"
    } else {
        "-single"
    };
    match lang {
        Some(lang) => format!("-lang {lang} {float_mode}"),
        None => float_mode.to_owned(),
    }
}

fn wasm_json_context_for_memory_source(
    source_name: &str,
    signals: &SignalCompileOutput,
    compile_options: String,
) -> WasmJsonContext {
    WasmJsonContext {
        filename: Some(source_name_to_filename(source_name)),
        version: Some(Compiler::version().to_owned()),
        compile_options: Some(compile_options),
        library_list: Vec::new(),
        include_pathnames: Vec::new(),
        top_level_meta: json_meta_entries_from_snapshot(&signals.compilation_metadata),
    }
}

fn wasm_json_context_for_file(
    path: &Path,
    search_paths: &[PathBuf],
    signals: &SignalCompileOutput,
    compile_options: String,
) -> WasmJsonContext {
    let filename = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_owned)
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    let mut library_list: Vec<String> = signals
        .parse
        .used_files
        .iter()
        .skip(1)
        .map(|file| file.to_string_lossy().into_owned())
        .collect();
    for file in &signals.loaded_files {
        let file = file.to_string_lossy().into_owned();
        if !library_list.iter().any(|existing| existing == &file) {
            library_list.push(file);
        }
    }
    WasmJsonContext {
        filename: Some(filename),
        version: Some(Compiler::version().to_owned()),
        compile_options: Some(compile_options),
        library_list,
        include_pathnames: merge_import_search_paths(path, search_paths)
            .into_iter()
            .map(|dir| dir.to_string_lossy().into_owned())
            .collect(),
        top_level_meta: json_meta_entries_from_snapshot(&signals.compilation_metadata),
    }
}

fn json_meta_entries_from_snapshot(snapshot: &CompilationMetadataSnapshot) -> Vec<JsonMetaEntry> {
    let mut out = Vec::new();
    for (key, values) in snapshot.entries() {
        let mut values = values.iter();
        let Some(first_value) = values.next() else {
            continue;
        };
        let base_key = match key {
            CompilationMetadataKey::Global { key } => key.as_ref().to_owned(),
            CompilationMetadataKey::Scoped { source_file, key } => {
                format!("{source_file}/{}", key.as_ref())
            }
        };
        out.push(JsonMetaEntry {
            key: base_key.clone(),
            value: first_value.as_ref().to_owned(),
        });
        if base_key == "author" {
            for value in values {
                out.push(JsonMetaEntry {
                    key: "contributor".to_owned(),
                    value: value.as_ref().to_owned(),
                });
            }
        } else {
            for value in values {
                out.push(JsonMetaEntry {
                    key: base_key.clone(),
                    value: value.as_ref().to_owned(),
                });
            }
        }
    }
    out
}

/// Extracts `-I <path>` search paths from a whitespace-tokenized argv slice.
fn parse_search_paths_from_argv(argv: &[String]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        if argv[i] == "-I" {
            if let Some(p) = argv.get(i + 1) {
                paths.push(PathBuf::from(p));
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    paths
}

/// Replaces non-identifier characters so the result is safe as a C/C++ identifier.
fn sanitize_cpp_ident(input: &str) -> String {
    let mut out = String::with_capacity(input.len().max(8));
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("faust_dsp");
    }
    if out.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// Resolves the canonical root UI label used when the top-level UI group is unnamed.
///
/// Source provenance (C++):
/// - `compiler/generator/compile.cpp`
/// - `compiler/generator/instructions_compiler.cpp`
///
/// Parity rule:
/// - prefer top-level `declare name "..."` metadata from the master document,
/// - otherwise fall back to the source filename stem,
/// - never use the backend class name for UI root labeling.
fn resolve_ui_root_label(source_name: &str, metadata: &CompilationMetadataSnapshot) -> String {
    metadata
        .entries()
        .get(&CompilationMetadataKey::global("name"))
        .and_then(|values| values.iter().next())
        .map(|value| value.as_ref().to_owned())
        .unwrap_or_else(|| source_name_to_class(source_name))
}

/// Wraps a single diagnostic into a one-item bundle.
fn bundle_from_diagnostic(diagnostic: Diagnostic) -> DiagnosticBundle {
    let mut diagnostics = DiagnosticBundle::new();
    diagnostics.push(diagnostic);
    diagnostics
}

// ─── Error node extraction ────────────────────────────────────────────────────

/// Returns the offending node id for eval errors that carry one.
fn eval_error_node(error: &eval::EvalError) -> Option<BoxId> {
    match error {
        eval::EvalError::MissingProcessDefinition {
            definitions: node, ..
        }
        | eval::EvalError::UndefinedSymbol { node, .. }
        | eval::EvalError::MalformedDefinitionNode { node }
        | eval::EvalError::MalformedListNode { node }
        | eval::EvalError::MalformedCaseNode { node }
        | eval::EvalError::EmptyArgumentList { node }
        | eval::EvalError::NonIdentifierParameter { node }
        | eval::EvalError::NonIdentifierIterationVariable { node }
        | eval::EvalError::IterationCountNotInt { node }
        | eval::EvalError::PatternArityMismatch { node, .. }
        | eval::EvalError::PatternMatchFailed { node }
        | eval::EvalError::TooManyArguments { node, .. }
        | eval::EvalError::LoopDetected { node } => Some(*node),
        _ => None,
    }
}

/// Returns the offending node id for propagate errors that carry one.
fn propagate_error_node(error: &PropagateError) -> Option<BoxId> {
    match error {
        PropagateError::UnsupportedBox { node, .. }
        | PropagateError::InvalidIntegerValue { node, .. }
        | PropagateError::InputArityMismatch { node, .. }
        | PropagateError::OutputArityMismatch { node, .. }
        | PropagateError::SeqArityMismatch { node, .. }
        | PropagateError::SplitArityMismatch { node, .. }
        | PropagateError::MergeArityMismatch { node, .. }
        | PropagateError::RecArityMismatch { node, .. } => Some(*node),
        _ => None,
    }
}

// ─── Box preview helpers ──────────────────────────────────────────────────────

/// Compacts one box subtree dump to a bounded single-line preview for diagnostics notes.
fn compact_box_preview(arena: &tlib::TreeArena, node: BoxId) -> String {
    let preview = dump_box(arena, node);
    let mut one_line = preview.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 180;
    if one_line.chars().count() > MAX_CHARS {
        one_line = one_line.chars().take(MAX_CHARS).collect::<String>() + "...";
    }
    one_line
}

/// Compacts one readable box expression preview to a bounded single-line note payload.
fn compact_human_box_preview(arena: &tlib::TreeArena, node: BoxId) -> String {
    let mut rendered = render_human_box_expr(arena, node, 0);
    const MAX_CHARS: usize = 180;
    if rendered.chars().count() > MAX_CHARS {
        rendered = rendered.chars().take(MAX_CHARS).collect::<String>() + "...";
    }
    rendered
}

/// Renders one box subtree to a human-oriented Faust-like expression string.
///
/// The output intentionally trades completeness for readability: composite
/// boxes are rendered as infix expressions when possible, and unknown shapes
/// fall back to a compact [`compact_box_preview`].
///
/// Recursion is bounded at depth 96 to prevent stack overflow on pathological
/// or cyclically-aliased box graphs; deeper sub-trees are replaced with `"..."`.
fn render_human_box_expr(arena: &tlib::TreeArena, node: BoxId, depth: usize) -> String {
    if depth > 96 {
        return "...".to_owned();
    }

    if let Some(kind) = arena.kind(node) {
        match kind {
            NodeKind::StringLiteral(s) => return format!("\"{}\"", s),
            NodeKind::Symbol(s) => return s.to_string(),
            _ => {}
        }
    }

    match match_box(arena, node) {
        BoxMatch::Wire => "_".to_owned(),
        BoxMatch::Cut => "!".to_owned(),
        BoxMatch::Ident(name) => name.to_owned(),
        BoxMatch::Int(v) => v.to_string(),
        BoxMatch::Real(v) => v.to_string(),
        BoxMatch::Par(left, right) => format!(
            "({}, {})",
            render_human_box_expr(arena, left, depth + 1),
            render_human_box_expr(arena, right, depth + 1)
        ),
        BoxMatch::Seq(left, right) => {
            if let BoxMatch::Par(lhs, rhs) = match_box(arena, left)
                && let Some(op) = prim_infix_symbol(arena, right)
            {
                return format!(
                    "({} {} {})",
                    render_human_box_expr(arena, lhs, depth + 1),
                    op,
                    render_human_box_expr(arena, rhs, depth + 1)
                );
            }
            format!(
                "({} : {})",
                render_human_box_expr(arena, left, depth + 1),
                render_human_box_expr(arena, right, depth + 1)
            )
        }
        BoxMatch::Split(left, right) => format!(
            "({} <: {})",
            render_human_box_expr(arena, left, depth + 1),
            render_human_box_expr(arena, right, depth + 1)
        ),
        BoxMatch::Merge(left, right) => format!(
            "({} :> {})",
            render_human_box_expr(arena, left, depth + 1),
            render_human_box_expr(arena, right, depth + 1)
        ),
        BoxMatch::Rec(left, right) => format!(
            "({} ~ {})",
            render_human_box_expr(arena, left, depth + 1),
            render_human_box_expr(arena, right, depth + 1)
        ),
        BoxMatch::Button(label) => {
            format!("button({})", render_human_box_expr(arena, label, depth + 1))
        }
        BoxMatch::Checkbox(label) => {
            format!(
                "checkbox({})",
                render_human_box_expr(arena, label, depth + 1)
            )
        }
        BoxMatch::VSlider(label, cur, min, max, step) => format!(
            "vslider({}, {}, {}, {}, {})",
            render_human_box_expr(arena, label, depth + 1),
            render_human_box_expr(arena, cur, depth + 1),
            render_human_box_expr(arena, min, depth + 1),
            render_human_box_expr(arena, max, depth + 1),
            render_human_box_expr(arena, step, depth + 1)
        ),
        BoxMatch::HSlider(label, cur, min, max, step) => format!(
            "hslider({}, {}, {}, {}, {})",
            render_human_box_expr(arena, label, depth + 1),
            render_human_box_expr(arena, cur, depth + 1),
            render_human_box_expr(arena, min, depth + 1),
            render_human_box_expr(arena, max, depth + 1),
            render_human_box_expr(arena, step, depth + 1)
        ),
        BoxMatch::NumEntry(label, cur, min, max, step) => format!(
            "nentry({}, {}, {}, {}, {})",
            render_human_box_expr(arena, label, depth + 1),
            render_human_box_expr(arena, cur, depth + 1),
            render_human_box_expr(arena, min, depth + 1),
            render_human_box_expr(arena, max, depth + 1),
            render_human_box_expr(arena, step, depth + 1)
        ),
        BoxMatch::VBargraph(label, min, max) => format!(
            "vbargraph({}, {}, {})",
            render_human_box_expr(arena, label, depth + 1),
            render_human_box_expr(arena, min, depth + 1),
            render_human_box_expr(arena, max, depth + 1)
        ),
        BoxMatch::HBargraph(label, min, max) => format!(
            "hbargraph({}, {}, {})",
            render_human_box_expr(arena, label, depth + 1),
            render_human_box_expr(arena, min, depth + 1),
            render_human_box_expr(arena, max, depth + 1)
        ),
        BoxMatch::VGroup(label, expr) => format!(
            "vgroup({}, {})",
            render_human_box_expr(arena, label, depth + 1),
            render_human_box_expr(arena, expr, depth + 1)
        ),
        BoxMatch::HGroup(label, expr) => format!(
            "hgroup({}, {})",
            render_human_box_expr(arena, label, depth + 1),
            render_human_box_expr(arena, expr, depth + 1)
        ),
        BoxMatch::TGroup(label, expr) => format!(
            "tgroup({}, {})",
            render_human_box_expr(arena, label, depth + 1),
            render_human_box_expr(arena, expr, depth + 1)
        ),
        BoxMatch::Soundfile(label, chan) => format!(
            "soundfile({}, {})",
            render_human_box_expr(arena, label, depth + 1),
            render_human_box_expr(arena, chan, depth + 1)
        ),
        BoxMatch::Add
        | BoxMatch::Sub
        | BoxMatch::Mul
        | BoxMatch::Div
        | BoxMatch::Rem
        | BoxMatch::And
        | BoxMatch::Or
        | BoxMatch::Xor
        | BoxMatch::Lsh
        | BoxMatch::Rsh
        | BoxMatch::Lt
        | BoxMatch::Le
        | BoxMatch::Gt
        | BoxMatch::Ge
        | BoxMatch::Eq
        | BoxMatch::Ne
        | BoxMatch::Pow
        | BoxMatch::Delay
        | BoxMatch::Delay1
        | BoxMatch::Min
        | BoxMatch::Max
        | BoxMatch::Acos
        | BoxMatch::Asin
        | BoxMatch::Atan
        | BoxMatch::Atan2
        | BoxMatch::Cos
        | BoxMatch::Sin
        | BoxMatch::Tan
        | BoxMatch::Exp
        | BoxMatch::Log
        | BoxMatch::Log10
        | BoxMatch::Sqrt
        | BoxMatch::Abs
        | BoxMatch::Fmod
        | BoxMatch::Remainder
        | BoxMatch::Floor
        | BoxMatch::Ceil
        | BoxMatch::Rint
        | BoxMatch::Round
        | BoxMatch::Prefix
        | BoxMatch::IntCast
        | BoxMatch::FloatCast
        | BoxMatch::ReadOnlyTable
        | BoxMatch::WriteReadTable
        | BoxMatch::Select2
        | BoxMatch::Select3
        | BoxMatch::AssertBounds
        | BoxMatch::Lowest
        | BoxMatch::Highest
        | BoxMatch::Attach
        | BoxMatch::Enable
        | BoxMatch::Control => prim_infix_symbol(arena, node)
            .or_else(|| prim_readable_name(arena, node))
            .unwrap_or("?")
            .to_owned(),
        _ => compact_box_preview(arena, node),
    }
}

/// Maps a primitive box node to its Faust infix operator symbol.
///
/// Returns `None` for primitives that are not infix operators (e.g. prefix
/// or postfix forms). Used by [`render_human_box_expr`] to produce readable
/// `A + B`-style diagnostic strings rather than box-type names.
fn prim_infix_symbol(arena: &tlib::TreeArena, node: BoxId) -> Option<&'static str> {
    match match_box(arena, node) {
        BoxMatch::Add => Some("+"),
        BoxMatch::Sub => Some("-"),
        BoxMatch::Mul => Some("*"),
        BoxMatch::Div => Some("/"),
        BoxMatch::Rem => Some("%"),
        BoxMatch::Pow => Some("^"),
        BoxMatch::Lt => Some("<"),
        BoxMatch::Le => Some("<="),
        BoxMatch::Gt => Some(">"),
        BoxMatch::Ge => Some(">="),
        BoxMatch::Eq => Some("=="),
        BoxMatch::Ne => Some("!="),
        BoxMatch::And => Some("&"),
        BoxMatch::Or => Some("|"),
        BoxMatch::Xor => Some("xor"),
        BoxMatch::Lsh => Some("<<"),
        BoxMatch::Rsh => Some(">>"),
        _ => None,
    }
}

/// Returns one readable primitive name for non-infix `BoxMatch` primitive nodes.
fn prim_readable_name(arena: &tlib::TreeArena, node: BoxId) -> Option<&'static str> {
    match match_box(arena, node) {
        BoxMatch::Delay => Some("@"),
        BoxMatch::Delay1 => Some("'"),
        BoxMatch::Min => Some("min"),
        BoxMatch::Max => Some("max"),
        BoxMatch::Acos => Some("acos"),
        BoxMatch::Asin => Some("asin"),
        BoxMatch::Atan => Some("atan"),
        BoxMatch::Atan2 => Some("atan2"),
        BoxMatch::Cos => Some("cos"),
        BoxMatch::Sin => Some("sin"),
        BoxMatch::Tan => Some("tan"),
        BoxMatch::Exp => Some("exp"),
        BoxMatch::Log => Some("log"),
        BoxMatch::Log10 => Some("log10"),
        BoxMatch::Sqrt => Some("sqrt"),
        BoxMatch::Abs => Some("abs"),
        BoxMatch::Fmod => Some("fmod"),
        BoxMatch::Remainder => Some("remainder"),
        BoxMatch::Floor => Some("floor"),
        BoxMatch::Ceil => Some("ceil"),
        BoxMatch::Rint => Some("rint"),
        BoxMatch::Round => Some("round"),
        BoxMatch::Prefix => Some("prefix"),
        BoxMatch::IntCast => Some("int"),
        BoxMatch::FloatCast => Some("float"),
        BoxMatch::ReadOnlyTable => Some("rdtable"),
        BoxMatch::WriteReadTable => Some("rwtable"),
        BoxMatch::Select2 => Some("select2"),
        BoxMatch::Select3 => Some("select3"),
        BoxMatch::AssertBounds => Some("assertbounds"),
        BoxMatch::Lowest => Some("lowest"),
        BoxMatch::Highest => Some("highest"),
        BoxMatch::Attach => Some("attach"),
        BoxMatch::Enable => Some("enable"),
        BoxMatch::Control => Some("control"),
        _ => None,
    }
}

// ─── Propagate diagnostic enrichment ─────────────────────────────────────────

/// Enriches arity-mismatch diagnostics with explicit paired A/B expression context.
///
/// The paired notes make propagate failures easier to read when the offending
/// node is a composed expression (`A:B`, `A<:B`, `A:>B`, `A~B`) rather than a
/// leaf. They are intentionally additive: if arity inference for either side
/// fails, the original diagnostic is kept and only the successfully computed
/// side notes are attached.
fn add_paired_propagate_context(
    mut diagnostic: Diagnostic,
    error: &PropagateError,
    arena: &tlib::TreeArena,
) -> Diagnostic {
    let (node, op_name) = match error {
        PropagateError::SeqArityMismatch { node, .. } => (*node, "seq"),
        PropagateError::SplitArityMismatch { node, .. } => (*node, "split"),
        PropagateError::MergeArityMismatch { node, .. } => (*node, "merge"),
        PropagateError::RecArityMismatch { node, .. } => (*node, "rec"),
        _ => return diagnostic,
    };

    let (left, right) = match match_box(arena, node) {
        BoxMatch::Seq(left, right)
        | BoxMatch::Split(left, right)
        | BoxMatch::Merge(left, right)
        | BoxMatch::Rec(left, right) => (left, right),
        _ => return diagnostic,
    };

    let left_expr = compact_human_box_preview(arena, left);
    let right_expr = compact_human_box_preview(arena, right);
    diagnostic = diagnostic.with_note(format!("A ({op_name} left) = {left_expr}"));
    diagnostic = diagnostic.with_note(format!("B ({op_name} right) = {right_expr}"));

    let mut arity_cache = ArityCache::new();
    if let Ok(left_flat) = propagate::try_build_flat_box(arena, left)
        && let Ok(a) = propagate::box_arity_typed(arena, left_flat, &mut arity_cache)
    {
        diagnostic = diagnostic.with_note(format!(
            "A arity: inputs={} outputs={}",
            a.inputs, a.outputs
        ));
    }
    if let Ok(right_flat) = propagate::try_build_flat_box(arena, right)
        && let Ok(b) = propagate::box_arity_typed(arena, right_flat, &mut arity_cache)
    {
        diagnostic = diagnostic.with_note(format!(
            "B arity: inputs={} outputs={}",
            b.inputs, b.outputs
        ));
    }

    diagnostic
}

// ─── Source label helpers ─────────────────────────────────────────────────────

/// Attaches source labels for propagate/arity diagnostics.
///
/// When the owning definition is known, this prefers that origin as primary and
/// keeps process call-site as secondary to improve alias-chain readability.
/// This policy reflects the common Faust failure mode where the concrete bad
/// composition is inside a helper definition but only becomes observable once
/// referenced by `process`.
fn maybe_add_source_label(
    mut diagnostic: Diagnostic,
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    node: BoxId,
    owner_definition: Option<&str>,
    entrypoint_name: &str,
) -> Diagnostic {
    if let Some(owner) = owner_definition {
        let owner_span = source_span_for_definition_name(ctx, arena, defs_root, owner);
        let call_span =
            source_span_for_entrypoint_binding_target(ctx, arena, defs_root, entrypoint_name)
                .or_else(|| {
                    source_span_for_entrypoint_definition(ctx, arena, defs_root, entrypoint_name)
                });
        if let Some(primary_span) = owner_span {
            diagnostic = diagnostic.with_label(Label::new(
                LabelStyle::Primary,
                primary_span.clone(),
                "related source",
            ));
            if let Some(secondary_span) = call_span
                && secondary_span != primary_span
            {
                diagnostic = diagnostic.with_label(Label::new(
                    LabelStyle::Secondary,
                    secondary_span,
                    "related call site",
                ));
            }
            return diagnostic;
        }
        diagnostic = diagnostic
            .with_note("origin span unavailable; pointing to nearest call/owner site".to_owned());
    }

    let span = source_span_from_node_or_descendant(ctx, arena, node)
        .or_else(|| source_span_for_definition_of_expr(ctx, arena, defs_root, node))
        .or_else(|| {
            source_span_for_entrypoint_binding_target(ctx, arena, defs_root, entrypoint_name)
        })
        .or_else(|| source_span_for_entrypoint_definition(ctx, arena, defs_root, entrypoint_name));
    if let Some(span) = span {
        diagnostic = diagnostic.with_label(Label::new(LabelStyle::Primary, span, "related source"));
    }
    diagnostic
}

/// Attaches eval-oriented primary/secondary labels when available.
///
/// Label policy:
/// - alias-chain mode (`owner_definition` known): primary origin definition,
///   secondary process call-site.
/// - fallback mode: primary nearest call/use site, secondary owning definition.
///
/// This differs slightly from propagate labeling because eval failures often
/// arise during symbol resolution or application, where the use-site can be
/// more actionable than the eventual enclosing composition site.
fn maybe_add_eval_source_labels(
    mut diagnostic: Diagnostic,
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    node: BoxId,
    owner_definition: Option<&str>,
    entrypoint_name: &str,
) -> Diagnostic {
    if let Some(owner) = owner_definition {
        let origin_span = source_span_for_definition_name(ctx, arena, defs_root, owner);
        let call_span =
            source_span_for_entrypoint_definition(ctx, arena, defs_root, entrypoint_name);
        if let Some(primary_span) = origin_span {
            diagnostic = diagnostic.with_label(Label::new(
                LabelStyle::Primary,
                primary_span.clone(),
                "definition site",
            ));
            if let Some(secondary_span) = call_span
                && secondary_span != primary_span
            {
                diagnostic = diagnostic.with_label(Label::new(
                    LabelStyle::Secondary,
                    secondary_span,
                    "call site",
                ));
            }
            return diagnostic;
        }
        diagnostic = diagnostic
            .with_note("origin span unavailable; pointing to nearest call/owner site".to_owned());
    }

    let primary = source_span_from_node_or_descendant(ctx, arena, node)
        .or_else(|| source_span_for_definition_of_expr(ctx, arena, defs_root, node))
        .or_else(|| {
            source_span_for_entrypoint_binding_target(ctx, arena, defs_root, entrypoint_name)
        })
        .or_else(|| source_span_for_entrypoint_definition(ctx, arena, defs_root, entrypoint_name));
    let Some(primary_span) = primary else {
        return diagnostic;
    };
    diagnostic = diagnostic.with_label(Label::new(
        LabelStyle::Primary,
        primary_span.clone(),
        "call site",
    ));
    let secondary = source_span_for_definition_of_expr(ctx, arena, defs_root, node)
        .or_else(|| source_span_for_entrypoint_definition(ctx, arena, defs_root, entrypoint_name));
    if let Some(secondary_span) = secondary
        && secondary_span != primary_span
    {
        diagnostic = diagnostic.with_label(Label::new(
            LabelStyle::Secondary,
            secondary_span,
            "definition site",
        ));
    }
    diagnostic
}

// ─── Source span resolution ───────────────────────────────────────────────────

/// Resolves one source span from the node itself, then falls back to labeled descendants.
fn source_span_from_node_or_descendant(
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    node: BoxId,
) -> Option<SourceSpan> {
    if let Some(span) = source_span_for_node(ctx, node) {
        return Some(span);
    }

    let mut stack = vec![node];
    let mut visited = 0usize;
    while let Some(cur) = stack.pop() {
        visited = visited.saturating_add(1);
        if visited > 4096 {
            break;
        }

        if let Some(span) = source_span_for_node(ctx, cur) {
            return Some(span);
        }

        if let Some(children) = arena.children(cur) {
            for child in children.iter().rev() {
                stack.push(*child);
            }
        }
    }
    None
}

/// Resolves one source span for a node from parser `use_prop` / `def_prop`.
fn source_span_for_node(ctx: &parser::ParserCtx, node: BoxId) -> Option<SourceSpan> {
    let loc = ctx.use_prop(node).or_else(|| ctx.def_prop(node))?;
    Some(SourceSpan::new(
        loc.file(),
        loc.line(),
        loc.col(),
        loc.end_line(),
        loc.end_col(),
    ))
}

/// Resolves one source span for a definition node, preferring `def_prop`.
///
/// This is used for alias fallback (`process = foo;`) where we want the location
/// of the defining equation, not the use-site of `foo`.
fn source_span_for_definition_node(ctx: &parser::ParserCtx, node: BoxId) -> Option<SourceSpan> {
    let loc = ctx.def_prop(node).or_else(|| ctx.use_prop(node))?;
    Some(SourceSpan::new(
        loc.file(),
        loc.line(),
        loc.col(),
        loc.end_line(),
        loc.end_col(),
    ))
}

/// Fallback source span for the configured entry-point definition identifier.
///
/// Used when the offending propagated/evaluated node cannot be mapped to a more
/// specific source location.
fn source_span_for_entrypoint_definition(
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    entrypoint_name: &str,
) -> Option<SourceSpan> {
    let mut defs = defs_root;
    let mut visited = 0usize;
    while !arena.is_nil(defs) {
        visited = visited.saturating_add(1);
        if visited > 4096 {
            break;
        }
        let def = arena.hd(defs)?;
        let name = arena.hd(def)?;
        if let BoxMatch::Ident(name_str) = match_box(arena, name)
            && name_str == entrypoint_name
        {
            return source_span_for_node(ctx, name);
        }
        defs = arena.tl(defs)?;
    }
    None
}

/// Fallback source span for direct entry-point aliases (`entry = <ident>;`).
///
/// When the configured entry-point is a direct identifier alias, this resolves
/// the target definition location (for example `foo = ...; synth = foo;` ->
/// label on `foo = ...`).
fn source_span_for_entrypoint_binding_target(
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    entrypoint_name: &str,
) -> Option<SourceSpan> {
    let (_entry_name, entry_expr) =
        find_definition_name_and_expr(arena, defs_root, entrypoint_name)?;
    let BoxMatch::Ident(target_name) = match_box(arena, entry_expr) else {
        return None;
    };
    let (target_def_name, _target_expr) =
        find_definition_name_and_expr(arena, defs_root, target_name)?;
    source_span_for_definition_node(ctx, target_def_name)
}

/// Finds one `(definition_name, definition_expr)` pair by identifier name
/// in the parser root definitions list.
fn find_definition_name_and_expr(
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    wanted: &str,
) -> Option<(BoxId, BoxId)> {
    let mut defs = defs_root;
    let mut visited = 0usize;
    while !arena.is_nil(defs) {
        visited = visited.saturating_add(1);
        if visited > 4096 {
            break;
        }
        let def = arena.hd(defs)?;
        let name = arena.hd(def)?;
        let args_expr = arena.tl(def)?;
        let expr = arena.tl(args_expr)?;
        if let BoxMatch::Ident(name_str) = match_box(arena, name)
            && name_str == wanted
        {
            return Some((name, expr));
        }
        defs = arena.tl(defs)?;
    }
    None
}

/// Fallback source span from a definition whose expression matches (or contains) `node`.
///
/// This covers alias chains such as:
/// `foo = <bad>; bar = foo; process = bar,bar;`
/// where the failing node belongs to `foo` but `process` is not a direct identifier alias.
fn source_span_for_definition_of_expr(
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    node: BoxId,
) -> Option<SourceSpan> {
    let mut defs = defs_root;
    let mut visited = 0usize;
    while !arena.is_nil(defs) {
        visited = visited.saturating_add(1);
        if visited > 4096 {
            break;
        }
        let def = arena.hd(defs)?;
        let name = arena.hd(def)?;
        let args_expr = arena.tl(def)?;
        let expr = arena.tl(args_expr)?;
        if expr == node || subtree_contains_node(arena, expr, node) {
            return source_span_for_definition_node(ctx, name);
        }
        defs = arena.tl(defs)?;
    }
    None
}

/// Resolves a source span for one top-level definition name.
///
/// Resolution prefers the definition identifier span, then falls back to the
/// definition expression subtree when identifier metadata is unavailable.
fn source_span_for_definition_name(
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    wanted: &str,
) -> Option<SourceSpan> {
    let (name, expr) = find_definition_name_and_expr(arena, defs_root, wanted)?;
    source_span_for_definition_node(ctx, name)
        .or_else(|| source_span_from_node_or_descendant(ctx, arena, expr))
}

/// Returns `true` when the subtree rooted at `root` contains `needle`.
///
/// Uses iterative depth-first traversal bounded at 4096 visited nodes to avoid
/// infinite loops on DAG-shared or aliased subtrees.  The conservative bound
/// means very large subtrees may produce a false negative; callers that use
/// this for ownership detection already tolerate that with a `None` fallback.
fn subtree_contains_node(arena: &tlib::TreeArena, root: BoxId, needle: BoxId) -> bool {
    if root == needle {
        return true;
    }
    let mut stack = vec![root];
    let mut visited = 0usize;
    while let Some(cur) = stack.pop() {
        visited = visited.saturating_add(1);
        if visited > 4096 {
            break;
        }
        if cur == needle {
            return true;
        }
        if let Some(children) = arena.children(cur) {
            for child in children.iter().rev() {
                stack.push(*child);
            }
        }
    }
    false
}

// ─── Definition graph helpers ─────────────────────────────────────────────────

/// Returns the owning definition name for one offending expression node.
///
/// The search is structural and bounded. It is used only for diagnostics, so a
/// conservative `None` fallback is preferable to panicking on malformed or
/// unusually deep definition lists.
fn owner_definition_name_for_node(
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    node: BoxId,
) -> Option<Box<str>> {
    let mut defs = defs_root;
    let mut visited = 0usize;
    while !arena.is_nil(defs) {
        visited = visited.saturating_add(1);
        if visited > 4096 {
            break;
        }
        let def = arena.hd(defs)?;
        let name = arena.hd(def)?;
        let args_expr = arena.tl(def)?;
        let expr = arena.tl(args_expr)?;
        if (expr == node || subtree_contains_node(arena, expr, node))
            && let BoxMatch::Ident(name_str) = match_box(arena, name)
        {
            return Some(name_str.into());
        }
        defs = arena.tl(defs)?;
    }
    None
}

/// Builds one deterministic reference graph between top-level definition names.
///
/// Each edge `A -> B` means definition `A` references identifier `B` somewhere in its expression.
fn definition_reference_edges(
    arena: &tlib::TreeArena,
    defs_root: BoxId,
) -> HashMap<Box<str>, Vec<Box<str>>> {
    let mut defs = defs_root;
    let mut visited = 0usize;
    let mut rows: Vec<(Box<str>, BoxId)> = Vec::new();
    while !arena.is_nil(defs) {
        visited = visited.saturating_add(1);
        if visited > 4096 {
            break;
        }
        let Some(def) = arena.hd(defs) else {
            break;
        };
        let Some(name) = arena.hd(def) else {
            break;
        };
        let Some(args_expr) = arena.tl(def) else {
            break;
        };
        let Some(expr) = arena.tl(args_expr) else {
            break;
        };
        if let BoxMatch::Ident(name_str) = match_box(arena, name) {
            rows.push((name_str.into(), expr));
        }
        defs = match arena.tl(defs) {
            Some(next) => next,
            None => break,
        };
    }

    let known = rows
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<HashSet<_>>();

    let mut out: HashMap<Box<str>, Vec<Box<str>>> = HashMap::new();
    for (name, expr) in rows {
        let mut refs = collect_definition_refs(arena, expr, &known);
        refs.sort_unstable();
        refs.dedup();
        out.insert(name, refs);
    }
    out
}

/// Collects all definition-name identifiers referenced in one expression subtree.
fn collect_definition_refs(
    arena: &tlib::TreeArena,
    root: BoxId,
    known: &HashSet<Box<str>>,
) -> Vec<Box<str>> {
    let mut refs = Vec::new();
    let mut stack = vec![root];
    let mut visited = 0usize;
    while let Some(cur) = stack.pop() {
        visited = visited.saturating_add(1);
        if visited > 4096 {
            break;
        }
        if let BoxMatch::Ident(name) = match_box(arena, cur)
            && known.contains(name)
        {
            refs.push(name.into());
        }
        if let Some(children) = arena.children(cur) {
            for child in children.iter().rev() {
                stack.push(*child);
            }
        }
    }
    refs
}

/// Finds one alias/binding trace from the configured entry-point to the owner of `node`.
///
/// The trace is expression-reference based (not only direct aliases), allowing contextual chains
/// such as `process = bar,bar; bar = foo; foo = ...` -> `process -> bar -> foo`.
fn alias_binding_trace_for_node(
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    node: BoxId,
    entrypoint_name: &str,
) -> Option<String> {
    let owner = owner_definition_name_for_node(arena, defs_root, node)?;
    if owner.as_ref() == entrypoint_name {
        return Some(entrypoint_name.to_owned());
    }

    let edges = definition_reference_edges(arena, defs_root);
    if !edges.contains_key(entrypoint_name) {
        return None;
    }

    let mut queue: VecDeque<Vec<Box<str>>> = VecDeque::new();
    let mut seen: HashSet<Box<str>> = HashSet::new();
    queue.push_back(vec![entrypoint_name.into()]);
    seen.insert(entrypoint_name.into());

    while let Some(path) = queue.pop_front() {
        let Some(last) = path.last() else {
            continue;
        };
        if last.as_ref() == owner.as_ref() {
            return Some(path.join(" -> "));
        }
        let Some(nexts) = edges.get(last) else {
            continue;
        };
        for next in nexts {
            if seen.insert(next.clone()) {
                let mut extended = path.clone();
                extended.push(next.clone());
                queue.push_back(extended);
            }
        }
    }

    None
}

// ─── Golden snapshot helpers ──────────────────────────────────────────────────

/// Generates a stable source snapshot string for regression testing.
///
/// The snapshot encodes the source name, byte count, line count, and an
/// FNV-1a 64-bit hash of the newline-normalized source text.  Comparing
/// snapshots across compiler versions or platforms detects unintended changes
/// to code generation output without storing full generated files.
///
/// The format is plain text, one key/value pair per line:
/// ```text
/// faust-rs-golden-v1
/// source=<name>
/// bytes=<n>
/// lines=<n>
/// fnv1a64=<hex>
/// ```
#[must_use]
pub fn golden_snapshot(source_name: &str, source: &str) -> String {
    let normalized_source = normalize_newlines(source);
    let line_count = normalized_source.lines().count();
    let byte_count = normalized_source.len();
    let hash = fnv1a64(normalized_source.as_bytes());

    format!(
        "faust-rs-golden-v1\nsource={source_name}\nbytes={byte_count}\nlines={line_count}\nfnv1a64={hash:016x}\n"
    )
}

/// File-backed variant of [`golden_snapshot`]: reads `path`, then delegates.
///
/// Useful for comparing generated output files in CI by snapshotting their
/// contents rather than storing full copies.
pub fn golden_snapshot_from_file(path: &Path) -> Result<String, std::io::Error> {
    let source = std::fs::read_to_string(path)?;
    Ok(golden_snapshot(&path.display().to_string(), &source))
}

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0001_0000_01b3;

/// Computes a FNV-1a 64-bit hash of `input`.
///
/// Used exclusively by [`golden_snapshot`] to produce a stable, portable
/// fingerprint.  FNV-1a is chosen for simplicity and determinism across
/// platforms (no endianness or SIMD dependency), not for cryptographic strength.
fn fnv1a64(input: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in input {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Normalizes Windows (`\r\n`) and old Mac (`\r`) line endings to Unix `\n`.
///
/// Applied before hashing in [`golden_snapshot`] so that snapshots are
/// identical regardless of whether the source or generated file uses LF or CRLF.
fn normalize_newlines(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        Compiler, CompilerError, ExpandDspRequest, GenerateAuxFilesRequest, SignalFirLane,
        WasmArtifactRequest, build_import_search_paths, compile_options_json_string,
        default_import_search_paths, golden_snapshot, resolve_module_name, resolve_ui_root_label,
    };
    use codegen::backends::wasm::WasmOptions;
    use parser::VirtualSourceMap;
    use serde_json::Value;

    fn temp_root(test_name: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "faust_rs_compiler_{test_name}_{}_{}",
            std::process::id(),
            stamp
        ));
        fs::create_dir_all(&root).expect("create temp root");
        root
    }

    fn json_include_pathnames(dsp_json: &str) -> Vec<PathBuf> {
        let parsed: Value = serde_json::from_str(dsp_json).expect("valid DSP JSON");
        parsed["include_pathnames"]
            .as_array()
            .expect("include_pathnames array")
            .iter()
            .map(|value| {
                PathBuf::from(
                    value
                        .as_str()
                        .expect("include_pathnames entries should be strings"),
                )
            })
            .collect()
    }

    fn json_library_list(dsp_json: &str) -> Vec<PathBuf> {
        let parsed: Value = serde_json::from_str(dsp_json).expect("valid DSP JSON");
        parsed["library_list"]
            .as_array()
            .expect("library_list array")
            .iter()
            .map(|value| {
                PathBuf::from(
                    value
                        .as_str()
                        .expect("library_list entries should be strings"),
                )
            })
            .collect()
    }

    fn json_filename(dsp_json: &str) -> Option<String> {
        let parsed: Value = serde_json::from_str(dsp_json).expect("valid DSP JSON");
        parsed["filename"].as_str().map(str::to_owned)
    }

    // ── golden helpers ────────────────────────────────────────────────────────

    #[test]
    fn golden_snapshot_is_stable_for_lf_vs_crlf() {
        let lf = "process = _;\n";
        let crlf = "process = _;\r\n";
        assert_eq!(
            golden_snapshot("pass_through.dsp", lf),
            golden_snapshot("pass_through.dsp", crlf)
        );
    }

    // ── default_import_search_paths ───────────────────────────────────────────

    #[test]
    fn default_import_search_paths_starts_with_parent_directory() {
        let path = PathBuf::from("/some/dir/file.dsp");
        let paths = build_import_search_paths(&path, &[], None, None);
        assert_eq!(paths.first(), Some(&PathBuf::from("/some/dir")));
        assert!(paths.contains(&PathBuf::from("/usr/local/share/faust")));
        assert!(paths.contains(&PathBuf::from("/usr/share/faust")));
    }

    #[test]
    fn default_import_search_paths_use_dot_for_bare_filename() {
        let path = PathBuf::from("file.dsp");
        let paths = build_import_search_paths(&path, &[], None, None);
        assert!(
            matches!(paths.first(), Some(first) if first == &PathBuf::from(".") || first == &PathBuf::from("")),
            "expected first search path to stay local for bare filename, got {paths:?}"
        );
    }

    #[test]
    fn import_search_paths_place_explicit_dirs_before_cpp_defaults() {
        let path = PathBuf::from("/project/main.dsp");
        let explicit = [PathBuf::from("/custom/a"), PathBuf::from("/custom/b")];
        let paths = build_import_search_paths(
            &path,
            &explicit,
            Some(OsString::from("/env/faust")),
            Some(PathBuf::from("/opt/faust/bin/faust-rs")),
        );

        assert_eq!(
            paths,
            vec![
                PathBuf::from("/custom/a"),
                PathBuf::from("/custom/b"),
                PathBuf::from("/project"),
                PathBuf::from("/env/faust"),
                PathBuf::from("/opt/faust/share/faust"),
                PathBuf::from("/usr/local/share/faust"),
                PathBuf::from("/usr/share/faust"),
            ]
        );
    }

    #[test]
    fn import_search_paths_deduplicate_repeated_entries() {
        let path = PathBuf::from("/project/main.dsp");
        let explicit = [
            PathBuf::from("/project"),
            PathBuf::from("/usr/local/share/faust"),
        ];
        let paths = build_import_search_paths(
            &path,
            &explicit,
            Some(OsString::from("/usr/local/share/faust")),
            Some(PathBuf::from("/usr/local/bin/faust-rs")),
        );

        assert_eq!(
            paths,
            vec![
                PathBuf::from("/project"),
                PathBuf::from("/usr/local/share/faust"),
                PathBuf::from("/usr/share/faust"),
            ]
        );
    }

    #[test]
    fn public_default_import_search_paths_never_return_empty() {
        let path = PathBuf::from("file.dsp");
        let paths = default_import_search_paths(&path);
        assert!(!paths.is_empty());
    }

    // ── resolve_module_name ───────────────────────────────────────────────────

    #[test]
    fn resolve_module_name_uses_explicit_class_name() {
        let name = resolve_module_name(Some("MyDsp"), "ignored.dsp");
        assert_eq!(name, "MyDsp");
    }

    #[test]
    fn resolve_module_name_defaults_to_mydsp() {
        let name = resolve_module_name(None, "sine_phasor.dsp");
        assert_eq!(name, "mydsp");
    }

    #[test]
    fn resolve_ui_root_label_prefers_declared_name_metadata() {
        let store = parser::CompilationMetadataStore::new("root.dsp");
        store.declare_top_level("root.dsp", "name", "main");
        let name = resolve_ui_root_label("root.dsp", &store.snapshot());
        assert_eq!(name, "main");
    }

    #[test]
    fn resolve_ui_root_label_falls_back_to_source_stem() {
        let name = resolve_ui_root_label(
            "nested/path/sine_phasor.dsp",
            &parser::CompilationMetadataSnapshot::default(),
        );
        assert_eq!(name, "sine_phasor");
    }

    // ── Compiler::compile_source ──────────────────────────────────────────────

    #[test]
    fn compiler_compile_source_accepts_valid_dsp() {
        let compiler = Compiler::new();
        let out = compiler
            .compile_source("valid.dsp", "process = _;")
            .expect("valid source should parse");
        assert!(out.root.is_some());
        assert!(out.errors.is_empty());
    }

    #[test]
    fn compiler_compile_source_rejects_malformed_dsp() {
        let compiler = Compiler::new();
        let err = compiler
            .compile_source("invalid.dsp", "process = ;")
            .expect_err("malformed source should fail compile facade");
        match err {
            CompilerError::Parse {
                parse_errors,
                diagnostics,
                ..
            } => {
                assert!(parse_errors >= 1);
                assert!(!diagnostics.is_empty());
            }
            other => panic!("expected CompilerError::Parse, got {other:?}"),
        }
    }

    #[test]
    fn compiler_compile_source_to_signals_accepts_custom_entrypoint_name() {
        let compiler = Compiler::new().with_process_name("dsp");
        let out = compiler
            .compile_source_to_signals("custom_entry.dsp", "dsp = _;")
            .expect("custom entrypoint should evaluate and propagate");
        assert_eq!(out.process_arity.inputs, 1);
        assert_eq!(out.process_arity.outputs, 1);
    }

    #[test]
    fn compiler_compile_file_to_signals_loads_component_through_eval_context() {
        let root = temp_root("component_eval_context");
        let entry = root.join("main.dsp");
        let child = root.join("child.dsp");
        fs::write(&entry, "process = component(\"child.dsp\");\n").expect("write entry");
        fs::write(&child, "process = _;\n").expect("write child");

        let compiler = Compiler::new();
        let output = compiler
            .compile_file_default_to_signals(&entry)
            .expect("file-backed compile should load component");

        assert_eq!(output.process_arity.inputs, 1);
        assert_eq!(output.process_arity.outputs, 1);
    }

    #[test]
    fn compiler_compile_file_to_signals_aggregates_component_metadata() {
        let root = temp_root("component_metadata");
        let entry = root.join("main.dsp");
        let child = root.join("child.dsp");
        fs::write(&entry, "process = component(\"child.dsp\");\n").expect("write entry");
        fs::write(&child, "declare author \"child-author\";\nprocess = _;\n").expect("write child");

        let compiler = Compiler::new();
        let output = compiler
            .compile_file_default_to_signals(&entry)
            .expect("file-backed compile should aggregate metadata");

        let key = parser::CompilationMetadataKey::scoped(
            child
                .canonicalize()
                .expect("child should canonicalize")
                .to_string_lossy()
                .into_owned(),
            "author",
        );
        let values = output
            .compilation_metadata
            .entries()
            .get(&key)
            .expect("component metadata should exist in final compiler output");
        assert!(values.contains("child-author"));
    }

    #[test]
    fn compiler_compile_source_to_wasm_emits_magic_header() {
        let compiler = Compiler::new();
        let out = compiler
            .compile_source_to_wasm("zero.dsp", "process = 0;", &WasmOptions::default())
            .expect("WASM scaffold should compile from source");
        assert!(out.wasm_binary.starts_with(b"\0asm"));
        assert!(out.dsp_json.contains("\"size\":"));
        assert!(out.dsp_json.contains("\"ui\":["));
        assert!(out.dsp_json.contains("\"filename\":\"zero.dsp\""));
        assert!(
            out.dsp_json
                .contains(&format!("\"version\":\"{}\"", Compiler::version()))
        );
        assert!(out.dsp_json.contains(&format!(
            "\"compile_options\":\"{}\"",
            compile_options_json_string(Some("wasm"), false)
        )));
    }

    #[test]
    fn compiler_compile_wasm_artifact_returns_matched_wasm_and_json_pair() {
        let compiler = Compiler::new();
        let request = WasmArtifactRequest::new("zero.dsp", "process = 0;");
        let out = compiler
            .compile_wasm_artifact(&request)
            .expect("artifact compile should succeed");

        assert!(out.wasm_bytes.starts_with(b"\0asm"));
        assert_eq!(json_filename(&out.dsp_json).as_deref(), Some("zero.dsp"));
        assert_eq!(
            out.compile_options,
            compile_options_json_string(Some("wasm"), false)
        );
        assert!(
            out.dsp_json
                .contains(&format!("\"compile_options\":\"{}\"", out.compile_options))
        );
    }

    #[test]
    fn timing_helper_without_sink_runs_without_measuring() {
        let mut called = false;
        let value = super::time_phase_with_sink(None, "test-phase", || {
            called = true;
            42
        });

        assert!(called);
        assert_eq!(value, 42);
    }

    #[test]
    fn wasm_artifact_request_defaults_to_transform_fastlane() {
        let request = WasmArtifactRequest::new("zero.dsp", "process = 0;");
        assert_eq!(request.lane, SignalFirLane::TransformFastLane);
    }

    #[test]
    fn compiler_compile_wasm_artifact_supports_memory_source_import_dirs() {
        let root = temp_root("wasm_artifact_memory_import_dirs");
        let child = root.join("child.lib");
        fs::write(&child, "process = _;\n").expect("write child");

        let compiler = Compiler::new();
        let mut request =
            WasmArtifactRequest::new("main.dsp", "process = component(\"child.lib\");");
        request.import_dirs.push(root.clone());
        let out = compiler
            .compile_wasm_artifact(&request)
            .expect("artifact compile with import dirs should succeed");

        assert!(out.wasm_bytes.starts_with(b"\0asm"));
        assert!(out.dsp_json.contains("child.lib"));
        let include_pathnames = json_include_pathnames(&out.dsp_json);
        assert!(include_pathnames.contains(&root), "{include_pathnames:?}");
    }

    #[test]
    fn compiler_compile_wasm_artifact_supports_virtual_faust_library_bundle() {
        let compiler = Compiler::new();
        let mut request = WasmArtifactRequest::new(
            "main.dsp",
            "import(\"stdfaust.lib\");\nprocess = os.freq;\n",
        );
        request.virtual_sources = VirtualSourceMap::new([
            (
                PathBuf::from("stdfaust.lib"),
                "os = library(\"osc.lib\");\n".to_owned(),
            ),
            (PathBuf::from("osc.lib"), "freq = 440;\n".to_owned()),
        ]);
        let out = compiler
            .compile_wasm_artifact(&request)
            .expect("artifact compile with virtual libraries should succeed");

        assert!(out.wasm_bytes.starts_with(b"\0asm"));
        let library_list = json_library_list(&out.dsp_json);
        assert!(library_list.contains(&PathBuf::from("stdfaust.lib")));
        assert!(library_list.contains(&PathBuf::from("osc.lib")));
    }

    #[test]
    fn compiler_compile_wasm_artifact_keeps_ui_for_memory_source_without_extension() {
        let compiler = Compiler::new();
        let source = "process = *(hslider(\"gain\", 0.5, 0.0, 1.0, 0.01));";
        let strict_json = compiler
            .compile_source_to_json("gain", source)
            .expect("strict JSON should preserve UI controls");
        let request = WasmArtifactRequest::new("gain", source);
        let out = compiler
            .compile_wasm_artifact(&request)
            .expect("artifact compile should preserve UI controls");

        assert!(strict_json.contains("\"filename\":\"gain\""));
        assert!(strict_json.contains("\"label\":\"gain\""));
        assert!(strict_json.contains("\"type\":\"hslider\""));
        assert!(strict_json.contains("\"address\":\"/gain/gain\""));
        assert!(out.wasm_bytes.starts_with(b"\0asm"));
        assert_eq!(json_filename(&out.dsp_json).as_deref(), Some("gain"));
        assert!(out.dsp_json.contains("\"label\":\"gain\""));
        assert!(out.dsp_json.contains("\"type\":\"hslider\""));
        assert!(out.dsp_json.contains("\"address\":\"/gain/gain\""));
    }

    #[test]
    fn compiler_memory_eval_source_context_preserves_ui_widgets() {
        let compiler = Compiler::new();
        let source = "process = *(hslider(\"gain\", 0.5, 0.0, 1.0, 0.01));";
        let store_without_ctx = parser::CompilationMetadataStore::new("gain");
        let store_with_ctx = parser::CompilationMetadataStore::new("gain");
        let output_without_ctx =
            parser::parse_program_with_metadata(source, "gain", store_without_ctx.clone());
        let output_with_ctx =
            parser::parse_program_with_metadata(source, "gain", store_with_ctx.clone());

        let without_ctx = compiler
            .pipeline_to_signals("gain", output_without_ctx, None)
            .expect("pipeline without source context should succeed");
        let with_ctx = compiler
            .pipeline_to_signals(
                "gain",
                output_with_ctx,
                Some(eval::EvalSourceContext::memory_with_metadata(
                    store_with_ctx,
                )),
            )
            .expect("pipeline with memory source context should succeed");

        assert!(
            !without_ctx.ui.controls.is_empty(),
            "pipeline without source context should preserve widget UI"
        );
        assert_eq!(
            with_ctx.ui.controls.len(),
            without_ctx.ui.controls.len(),
            "memory source context should not change widget UI extraction"
        );
    }

    #[test]
    fn compiler_compile_file_to_wasm_emits_file_provenance_fields() {
        let root = temp_root("wasm_json_provenance");
        let entry = root.join("main.dsp");
        let child = root.join("child.lib");
        fs::write(
            &entry,
            "declare name \"Main DSP\";\nprocess = component(\"child.lib\");\n",
        )
        .expect("write entry");
        fs::write(&child, "process = _;\n").expect("write child");

        let compiler = Compiler::new();
        let out = compiler
            .compile_file_default_to_wasm(&entry, &WasmOptions::default())
            .expect("file-backed WASM compile should succeed");

        assert!(out.dsp_json.contains("\"name\":\"Main DSP\""));
        assert_eq!(json_filename(&out.dsp_json).as_deref(), Some("main.dsp"));
        assert!(
            out.dsp_json
                .contains(&format!("\"version\":\"{}\"", Compiler::version()))
        );
        let library_list = json_library_list(&out.dsp_json);
        assert!(
            library_list.contains(&child),
            "library_list should include the imported file: {library_list:?}"
        );
        let include_pathnames = json_include_pathnames(&out.dsp_json);
        assert!(
            include_pathnames.contains(&root),
            "include_pathnames should include the source directory: {include_pathnames:?}"
        );
    }

    #[test]
    fn compiler_compile_file_to_wasm_artifact_preserves_file_provenance_and_options() {
        let root = temp_root("wasm_artifact_file_provenance");
        let entry = root.join("main.dsp");
        let child = root.join("child.lib");
        fs::write(
            &entry,
            "declare name \"Main DSP\";\nprocess = component(\"child.lib\");\n",
        )
        .expect("write entry");
        fs::write(&child, "process = _;\n").expect("write child");

        let compiler = Compiler::new();
        let out = compiler
            .compile_file_default_to_wasm_artifact(&entry, &WasmOptions::default())
            .expect("file-backed artifact compile should succeed");

        assert!(out.wasm_bytes.starts_with(b"\0asm"));
        assert_eq!(
            out.compile_options,
            compile_options_json_string(Some("wasm"), false)
        );
        assert_eq!(json_filename(&out.dsp_json).as_deref(), Some("main.dsp"));
        let library_list = json_library_list(&out.dsp_json);
        assert!(library_list.contains(&child), "{library_list:?}");
        assert!(
            out.dsp_json
                .contains(&format!("\"compile_options\":\"{}\"", out.compile_options))
        );
    }

    #[test]
    fn compiler_compile_source_to_json_emits_strict_json_without_widget_indices() {
        let compiler = Compiler::new();
        let json = compiler
            .compile_source_to_json(
                "gain.dsp",
                "declare name \"Gain\";\ngain = hslider(\"gain\", 0.5, 0, 1, 0.01);\nprocess = _ * gain;\n",
            )
            .expect("strict JSON should compile from source");

        assert!(json.contains("\"name\":\"Gain\""));
        assert!(json.contains("\"filename\":\"gain.dsp\""));
        assert!(json.contains("\"ui\":["));
        assert!(json.contains(&format!(
            "\"compile_options\":\"{}\"",
            compile_options_json_string(None, false)
        )));
        assert!(!json.contains("\"index\":"));
    }

    #[test]
    fn compile_options_json_string_tracks_lang_and_float_mode() {
        assert_eq!(
            compile_options_json_string(Some("wasm"), false),
            "-lang wasm -single"
        );
        assert_eq!(
            compile_options_json_string(Some("wasm"), true),
            "-lang wasm -double"
        );
        assert_eq!(
            compile_options_json_string(Some("cpp"), false),
            "-lang cpp -single"
        );
        assert_eq!(compile_options_json_string(None, false), "-single");
        assert_eq!(compile_options_json_string(None, true), "-double");
    }

    #[test]
    fn compiler_get_faustwasm_info_supports_version_and_help_only() {
        let compiler = Compiler::new();

        assert_eq!(
            compiler
                .get_faustwasm_info("version")
                .expect("version should be supported"),
            Compiler::version()
        );
        let help = compiler
            .get_faustwasm_info("help")
            .expect("help should be supported");
        assert!(help.contains("supported keys"));
        assert!(help.contains("stubbed keys"));

        let unsupported = compiler
            .get_faustwasm_info("libdir")
            .expect_err("libdir should stay stubbed");
        assert!(unsupported.message.contains("not implemented yet"));

        let invalid = compiler
            .get_faustwasm_info("wat")
            .expect_err("unknown keys should be rejected");
        assert!(invalid.message.contains("incorrect argument"));
    }

    #[test]
    fn compiler_expand_dsp_returns_source_when_valid() {
        let compiler = Compiler::new();
        let source = "process = 0;".to_owned();
        let expanded = compiler
            .expand_dsp(&ExpandDspRequest {
                source_name: "zero.dsp".to_owned(),
                source: source.clone(),
                args: String::new(),
            })
            .expect("expand_dsp should succeed for valid source");
        assert_eq!(expanded, source);
    }

    #[test]
    fn compiler_expand_dsp_fails_for_invalid_source() {
        let compiler = Compiler::new();
        let err = compiler
            .expand_dsp(&ExpandDspRequest {
                source_name: "bad.dsp".to_owned(),
                source: "process = undefined_symbol;".to_owned(),
                args: String::new(),
            })
            .expect_err("expand_dsp should fail for invalid source");
        assert_eq!(err.code, crate::FaustwasmServiceErrorCode::Unsupported);
    }

    #[test]
    fn compiler_generate_aux_files_no_flags_returns_empty() {
        let compiler = Compiler::new();
        let artifacts = compiler
            .generate_aux_files(&GenerateAuxFilesRequest {
                source_name: "zero.dsp".to_owned(),
                source: "process = 0;".to_owned(),
                args: String::new(),
                ..Default::default()
            })
            .expect("generate_aux_files should succeed with no flags");
        assert!(artifacts.is_empty());
    }

    #[test]
    fn compiler_generate_aux_files_json_flag_produces_json_artifact() {
        let compiler = Compiler::new();
        let artifacts = compiler
            .generate_aux_files(&GenerateAuxFilesRequest {
                source_name: "zero.dsp".to_owned(),
                source: "process = 0;".to_owned(),
                args: "-json".to_owned(),
                ..Default::default()
            })
            .expect("generate_aux_files with -json should succeed");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].path, "zero.json");
        assert!(!artifacts[0].binary);
        let text = std::str::from_utf8(&artifacts[0].content).expect("json must be utf-8");
        assert!(text.contains("\"name\""));
    }

    #[test]
    fn compiler_generate_aux_files_cpp_flag_produces_cpp_artifact() {
        let compiler = Compiler::new();
        let artifacts = compiler
            .generate_aux_files(&GenerateAuxFilesRequest {
                source_name: "zero.dsp".to_owned(),
                source: "process = 0;".to_owned(),
                args: "-cpp".to_owned(),
                ..Default::default()
            })
            .expect("generate_aux_files with -cpp should succeed");
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].path, "zero.cpp");
        assert!(!artifacts[0].binary);
    }
}
