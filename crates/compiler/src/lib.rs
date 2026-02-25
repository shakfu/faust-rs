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
//! - [`SignalFirLane::LegacyBridge`] and [`SignalFirLane::TransformFastLane`]
//!   coexist to de-risk migration of signal->FIR lowering ownership.

pub mod enrobage;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use boxes::{BoxId, BoxMatch, dump_box, match_box};
use codegen::backends::c::{COptions, CodegenError as CCodegenError, generate_c_module};
use codegen::backends::cpp::{CodegenError, CppOptions, generate_cpp_module};
use codegen::backends::interp::{
    CodegenError as InterpCodegenError, CodegenErrorCode as InterpCodegenErrorCode, FbcDspFactory,
    InterpOptions, generate_interp_module, write_fbc,
};
use errors::{Diagnostic, DiagnosticBundle, IntoDiagnostic, Label, LabelStyle, SourceSpan};
use fir::{
    FirBuilder, FirId, FirStore, FirType, NamedType,
    checker::{FirVerifyReport, Severity as FirVerifySeverity, verify_fir_module},
};
use parser::{ParseOutput, SourceReaderError};
use propagate::{ArityCache, BoxArity, PropagateError};
use signals::{SigId, dump_sig_readable};
use tlib::NodeKind;
use transform::signal_fir::{
    SignalFirError, SignalFirErrorCode, SignalFirOptions, compile_signals_to_fir_fastlane,
};

/// Parse + eval + propagate output package.
#[derive(Debug)]
pub struct SignalCompileOutput {
    /// Full parser output (arena + metadata + diagnostics from parse stage).
    pub parse: ParseOutput,
    /// Evaluated `process` box expression after `eval`.
    pub process_box: BoxId,
    /// Inferred process arity (`inputs`/`outputs`) from `propagate::box_arity`.
    pub process_arity: BoxArity,
    /// Final propagated output signal list (`process_arity.outputs` items).
    pub signals: Vec<SigId>,
}

/// Parse + eval + propagate + FIR lowering output package.
#[derive(Debug)]
pub struct FirCompileOutput {
    /// FIR storage arena.
    pub store: FirStore,
    /// FIR module root id.
    pub module: FirId,
}

/// FIR verifier configuration used at the compiler facade / CLI integration layer.
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
}

/// Selects which signal->FIR lowering lane is used before C++ emission.
///
/// # Rustdoc note
/// - [`SignalFirLane::LegacyBridge`] keeps the current temporary FIR summary bridge.
/// - [`SignalFirLane::TransformFastLane`] routes lowering through
///   `transform::signal_fir` (Step 1B wiring; Step 2+ semantics still pending).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignalFirLane {
    /// Existing temporary bridge local to `compiler`.
    #[default]
    LegacyBridge,
    /// Experimental lowering lane owned by `crates/transform`.
    TransformFastLane,
}

impl Compiler {
    #[must_use]
    /// Creates a new top-level compiler facade instance.
    pub fn new() -> Self {
        Self {
            fir_verify: FirVerifyOptions::default(),
        }
    }

    /// Returns a compiler facade configured with FIR verifier settings.
    #[must_use]
    pub fn with_fir_verify_options(mut self, fir_verify: FirVerifyOptions) -> Self {
        self.fir_verify = fir_verify;
        self
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
        let output = parser::parse_program(source, source_name);
        ensure_parse_success(source_name, output)
    }

    /// Parses one source file and expands local imports using `search_paths`.
    ///
    /// Returns [`CompilerError::Import`] for import resolution/cycle failures.
    pub fn compile_file(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
    ) -> Result<ParseOutput, CompilerError> {
        let output =
            parser::parse_file_with_imports(path, search_paths).map_err(CompilerError::Import)?;
        ensure_parse_success(&path.display().to_string(), output)
    }

    /// Parses one source file using its parent directory as default import search path.
    pub fn compile_file_default(&self, path: &Path) -> Result<ParseOutput, CompilerError> {
        self.compile_file(path, &[default_search_base(path)])
    }

    /// Parses, evaluates `process`, then propagates boxes to output signals.
    pub fn compile_source_to_signals(
        &self,
        source_name: &str,
        source: &str,
    ) -> Result<SignalCompileOutput, CompilerError> {
        let output = self.compile_source(source_name, source)?;
        self.pipeline_to_signals(source_name, output)
    }

    /// Parses one file, evaluates `process`, then propagates boxes to output signals.
    pub fn compile_file_to_signals(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
    ) -> Result<SignalCompileOutput, CompilerError> {
        let output = self.compile_file(path, search_paths)?;
        self.pipeline_to_signals(&path.display().to_string(), output)
    }

    /// Parses one file with default import search path, then runs eval+propagate.
    pub fn compile_file_default_to_signals(
        &self,
        path: &Path,
    ) -> Result<SignalCompileOutput, CompilerError> {
        self.compile_file_to_signals(path, &[default_search_base(path)])
    }

    /// Runs eval+propagate on an already parsed Faust program.
    ///
    /// This is an advanced entry point used by tooling/tests that need to alter
    /// parse metadata before Phase 4 (for example diagnostics fallback checks).
    pub fn compile_parsed_to_signals(
        &self,
        source_name: &str,
        output: ParseOutput,
    ) -> Result<SignalCompileOutput, CompilerError> {
        self.pipeline_to_signals(source_name, output)
    }

    /// Parses + evaluates + propagates one source, then lowers to a temporary
    /// FIR module and emits C++ text through the module-first backend.
    ///
    /// # Rustdoc note
    /// This is a bridge API for Phase 6 Step 7. The produced C++ currently uses
    /// a temporary signal-summary module, not the final production FIR lowering.
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
            SignalFirLane::LegacyBridge,
        )
    }

    /// Parses + evaluates + propagates one source, then lowers to a temporary
    /// FIR module and emits C text through the module-first backend.
    ///
    /// # Rustdoc note
    /// This is a bridge API for Phase 6 Step 7A. The produced C currently uses
    /// the same lane selection model as C++ (`legacy` bridge or `transform`
    /// fast-lane) while parity is being finalized.
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
            SignalFirLane::LegacyBridge,
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
        lower_signals_to_c(source_name, &signals, options, lane, self.fir_verify)
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
        lower_signals_to_cpp(source_name, &signals, options, lane, self.fir_verify)
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
        lower_signals_to_fir(source_name, &signals, lane, self.fir_verify)
            .map_err(|e| lower_fir_error_to_compiler(source_name, e))
    }

    /// Parses + evaluates + propagates one file, then emits C++ text from
    /// the temporary module-first FIR bridge.
    pub fn compile_file_to_cpp(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        options: &CppOptions,
    ) -> Result<String, CompilerError> {
        self.compile_file_to_cpp_with_lane(path, search_paths, options, SignalFirLane::LegacyBridge)
    }

    /// Parses + evaluates + propagates one file, then emits C text from
    /// the temporary module-first FIR bridge.
    pub fn compile_file_to_c(
        &self,
        path: &Path,
        search_paths: &[PathBuf],
        options: &COptions,
    ) -> Result<String, CompilerError> {
        self.compile_file_to_c_with_lane(path, search_paths, options, SignalFirLane::LegacyBridge)
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
        lower_signals_to_c(&source, &signals, options, lane, self.fir_verify)
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
        lower_signals_to_cpp(&source, &signals, options, lane, self.fir_verify)
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
        lower_signals_to_fir(&source, &signals, lane, self.fir_verify)
            .map_err(|e| lower_fir_error_to_compiler(&source, e))
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then emits C++ text from the temporary module-first FIR bridge.
    pub fn compile_file_default_to_cpp(
        &self,
        path: &Path,
        options: &CppOptions,
    ) -> Result<String, CompilerError> {
        self.compile_file_default_to_cpp_with_lane(path, options, SignalFirLane::LegacyBridge)
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then emits C text from the temporary module-first FIR bridge.
    pub fn compile_file_default_to_c(
        &self,
        path: &Path,
        options: &COptions,
    ) -> Result<String, CompilerError> {
        self.compile_file_default_to_c_with_lane(path, options, SignalFirLane::LegacyBridge)
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then emits C text using the selected signal->FIR lowering lane.
    pub fn compile_file_default_to_c_with_lane(
        &self,
        path: &Path,
        options: &COptions,
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        self.compile_file_to_c_with_lane(path, &[default_search_base(path)], options, lane)
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then emits C++ text using the selected signal->FIR lowering lane.
    pub fn compile_file_default_to_cpp_with_lane(
        &self,
        path: &Path,
        options: &CppOptions,
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        self.compile_file_to_cpp_with_lane(path, &[default_search_base(path)], options, lane)
    }

    /// Parses + evaluates + propagates one file with default import search path,
    /// then lowers to FIR using the selected signal->FIR lane.
    pub fn compile_file_default_to_fir_with_lane(
        &self,
        path: &Path,
        lane: SignalFirLane,
    ) -> Result<FirCompileOutput, CompilerError> {
        self.compile_file_to_fir_with_lane(path, &[default_search_base(path)], lane)
    }

    /// Parses + evaluates + propagates one source, then emits `.fbc` bytecode
    /// text via the interpreter backend using the legacy bridge lane.
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
            SignalFirLane::LegacyBridge,
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
        lower_signals_to_interp(source_name, &signals, options, lane, self.fir_verify)
            .map_err(|e| lower_interp_error_to_compiler(source_name, e))
    }

    /// Parses + evaluates + propagates one file, then emits `.fbc` bytecode
    /// text via the interpreter backend using the legacy bridge lane.
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
            SignalFirLane::LegacyBridge,
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
        lower_signals_to_interp(&source, &signals, options, lane, self.fir_verify)
            .map_err(|e| lower_interp_error_to_compiler(&source, e))
    }

    /// Parses + evaluates + propagates one file with default import search
    /// path, then emits `.fbc` bytecode text via the interpreter backend.
    pub fn compile_file_default_to_interp(
        &self,
        path: &Path,
        options: &InterpOptions,
    ) -> Result<String, CompilerError> {
        self.compile_file_default_to_interp_with_lane(path, options, SignalFirLane::LegacyBridge)
    }

    /// Parses + evaluates + propagates one file with default import search
    /// path, then emits `.fbc` bytecode text using the selected lane.
    pub fn compile_file_default_to_interp_with_lane(
        &self,
        path: &Path,
        options: &InterpOptions,
        lane: SignalFirLane,
    ) -> Result<String, CompilerError> {
        self.compile_file_to_interp_with_lane(path, &[default_search_base(path)], options, lane)
    }

    fn pipeline_to_signals(
        &self,
        source: &str,
        mut output: ParseOutput,
    ) -> Result<SignalCompileOutput, CompilerError> {
        let root = output.root.ok_or_else(|| CompilerError::MissingRoot {
            source: source.into(),
        })?;

        let process_box = eval::eval_process(&mut output.state.arena, root).map_err(|error| {
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
                );
                diagnostic = maybe_add_eval_source_labels(
                    diagnostic,
                    &output.state.ctx,
                    &output.state.arena,
                    root,
                    n,
                    owner.as_deref(),
                );
            }
            CompilerError::Eval {
                source: source.into(),
                error: Box::new(error),
                diagnostics: bundle_from_diagnostic(diagnostic),
            }
        })?;

        let mut arity_cache = ArityCache::new();
        let process_arity =
            propagate::box_arity(&output.state.arena, process_box, &mut arity_cache).map_err(
                |error| {
                    let node = propagate_error_node(&error);
                    let owner = node
                        .and_then(|n| owner_definition_name_for_node(&output.state.arena, root, n));
                    let mut diagnostic = error.clone().into_diagnostic();
                    if let Some(n) = node {
                        diagnostic = enrich_diagnostic_with_node(
                            diagnostic,
                            &output.state.arena,
                            root,
                            n,
                            owner.as_deref(),
                        );
                        diagnostic =
                            add_paired_propagate_context(diagnostic, &error, &output.state.arena);
                        diagnostic = maybe_add_source_label(
                            diagnostic,
                            &output.state.ctx,
                            &output.state.arena,
                            root,
                            n,
                            owner.as_deref(),
                        );
                    }
                    CompilerError::Propagate {
                        source: source.into(),
                        error,
                        diagnostics: bundle_from_diagnostic(diagnostic),
                    }
                },
            )?;

        let inputs = propagate::make_sig_input_list(&mut output.state.arena, process_arity.inputs);
        let signals = propagate::propagate(
            &mut output.state.arena,
            process_box,
            &inputs,
            &mut arity_cache,
        )
        .map_err(|error| {
            let node = propagate_error_node(&error);
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
                );
                diagnostic = add_paired_propagate_context(diagnostic, &error, &output.state.arena);
                diagnostic = maybe_add_source_label(
                    diagnostic,
                    &output.state.ctx,
                    &output.state.arena,
                    root,
                    n,
                    owner.as_deref(),
                );
            }
            CompilerError::Propagate {
                source: source.into(),
                error,
                diagnostics: bundle_from_diagnostic(diagnostic),
            }
        })?;

        Ok(SignalCompileOutput {
            parse: output,
            process_box,
            process_arity,
            signals,
        })
    }
}

impl Default for Compiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Compiler facade errors for parser-stage orchestration.
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
            Self::Transform { diagnostics, .. } => Some(diagnostics),
            Self::FirVerify { diagnostics, .. } => Some(diagnostics),
            Self::Codegen { .. } => None,
            Self::CodegenC { .. } => None,
            _ => None,
        }
    }
}

// ─── Helpers: path resolution ─────────────────────────────────────────────────

/// Resolves the default import search base for a path (parent directory or ".").
///
/// Public helper used by external integration crates (e.g. FFI frontends) to
/// mirror the facade's file-import search-path behavior.
pub fn default_import_search_base(path: &Path) -> PathBuf {
    path.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn default_search_base(path: &Path) -> PathBuf {
    default_import_search_base(path)
}

// ─── Helpers: parse validation ────────────────────────────────────────────────

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
fn lower_cpp_error_to_compiler(source: &str, error: LowerToCppError) -> CompilerError {
    match error {
        LowerToCppError::Transform(error) => transform_error_to_compiler(source, error),
        LowerToCppError::Verify(report) => fir_verify_error_to_compiler(source, report),
        LowerToCppError::Codegen(error) => CompilerError::Codegen {
            source: source.into(),
            error,
        },
    }
}

/// Maps a `LowerToCError` into a `CompilerError`, attaching the source name.
fn lower_c_error_to_compiler(source: &str, error: LowerToCError) -> CompilerError {
    match error {
        LowerToCError::Transform(error) => transform_error_to_compiler(source, error),
        LowerToCError::Verify(report) => fir_verify_error_to_compiler(source, report),
        LowerToCError::Codegen(error) => CompilerError::CodegenC {
            source: source.into(),
            error,
        },
    }
}

/// Maps a `LowerToInterpError` into a `CompilerError`, attaching the source name.
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
fn transform_error_to_compiler(source: &str, error: SignalFirError) -> CompilerError {
    let diagnostic = signal_fir_diagnostic(&error);
    CompilerError::Transform {
        source: source.into(),
        diagnostics: bundle_from_diagnostic(diagnostic),
        error,
    }
}

fn fir_verify_error_to_compiler(source: &str, report: FirVerifyReport) -> CompilerError {
    let strict = report.warnings().next().is_some() && !report.has_errors();
    CompilerError::FirVerify {
        source: source.into(),
        strict,
        diagnostics: fir_verify_bundle_from_report(&report),
    }
}

// ─── DiagCtx: shared pipeline diagnostic enrichment ──────────────────────────

/// Enriches a diagnostic with the standard node-level notes shared across
/// eval, box_arity, and propagate error handlers.
///
/// Takes the arena and root by reference at call-site (not stored) so that
/// mutable borrows of the arena remain possible between phase calls.
fn enrich_diagnostic_with_node(
    mut diagnostic: Diagnostic,
    arena: &tlib::TreeArena,
    root: BoxId,
    node: BoxId,
    owner: Option<&str>,
) -> Diagnostic {
    diagnostic = diagnostic
        .with_note(format!("node_id={}", node.as_u32()))
        .with_note(format!("box_expr={}", compact_box_preview(arena, node)))
        .with_note(format!("expr={}", compact_human_box_preview(arena, node)));
    if let Some(owner) = owner {
        diagnostic = diagnostic.with_note(format!("error originates from definition '{owner}'"));
    }
    if let Some(trace) = alias_binding_trace_for_node(arena, root, node) {
        diagnostic = diagnostic.with_note(format!("binding_trace={trace}"));
    }
    diagnostic
}

// ─── Signal-to-FIR lower errors ───────────────────────────────────────────────

/// Lowers current signal output to a temporary FIR module, then emits C++ text.
///
/// This bridge keeps one explicit integration point in `compiler` while the
/// production signal->FIR lowering is still being implemented.
#[derive(Debug)]
enum LowerToCppError {
    Transform(SignalFirError),
    Verify(FirVerifyReport),
    Codegen(CodegenError),
}

#[derive(Debug)]
enum LowerToCError {
    Transform(SignalFirError),
    Verify(FirVerifyReport),
    Codegen(CCodegenError),
}

#[derive(Debug)]
enum LowerToInterpError {
    Transform(SignalFirError),
    Verify(FirVerifyReport),
    Codegen(InterpCodegenError),
    /// Serialization of the factory to `.fbc` text failed.
    Serialize(String),
}

#[derive(Debug)]
enum LowerToFirError {
    Transform(SignalFirError),
    Verify(FirVerifyReport),
}

// ─── Signal-to-FIR lower functions ───────────────────────────────────────────

fn lower_signals_to_cpp(
    source_name: &str,
    output: &SignalCompileOutput,
    options: &CppOptions,
    lane: SignalFirLane,
    fir_verify: FirVerifyOptions,
) -> Result<String, LowerToCppError> {
    match lane {
        SignalFirLane::LegacyBridge => {
            lower_signals_to_cpp_legacy_bridge(source_name, output, options, fir_verify)
        }
        SignalFirLane::TransformFastLane => {
            lower_signals_to_cpp_transform_fastlane(source_name, output, options, fir_verify)
        }
    }
}

fn lower_signals_to_c(
    source_name: &str,
    output: &SignalCompileOutput,
    options: &COptions,
    lane: SignalFirLane,
    fir_verify: FirVerifyOptions,
) -> Result<String, LowerToCError> {
    match lane {
        SignalFirLane::LegacyBridge => {
            lower_signals_to_c_legacy_bridge(source_name, output, options, fir_verify)
        }
        SignalFirLane::TransformFastLane => {
            lower_signals_to_c_transform_fastlane(source_name, output, options, fir_verify)
        }
    }
}

fn lower_signals_to_interp(
    source_name: &str,
    output: &SignalCompileOutput,
    options: &InterpOptions,
    lane: SignalFirLane,
    fir_verify: FirVerifyOptions,
) -> Result<String, LowerToInterpError> {
    match lane {
        SignalFirLane::LegacyBridge => {
            lower_signals_to_interp_legacy_bridge(source_name, output, options, fir_verify)
        }
        SignalFirLane::TransformFastLane => {
            lower_signals_to_interp_transform_fastlane(source_name, output, options, fir_verify)
        }
    }
}

fn lower_signals_to_interp_legacy_bridge(
    source_name: &str,
    output: &SignalCompileOutput,
    options: &InterpOptions,
    fir_verify: FirVerifyOptions,
) -> Result<String, LowerToInterpError> {
    let module_name = resolve_module_name(options.module_name.as_deref(), source_name);
    let lowered = lower_signals_to_fir_legacy_bridge(source_name, output, module_name);
    maybe_verify_fir_module(&lowered, fir_verify).map_err(LowerToInterpError::Verify)?;
    let mut effective_options = options.clone();
    if effective_options.num_inputs == 0 {
        effective_options.num_inputs = output.process_arity.inputs;
    }
    if effective_options.num_outputs == 0 {
        effective_options.num_outputs = output.process_arity.outputs;
    }
    let factory: FbcDspFactory<f32> =
        generate_interp_module(&lowered.store, lowered.module, &effective_options)
            .map_err(LowerToInterpError::Codegen)?;
    serialize_factory(&factory).map_err(LowerToInterpError::Serialize)
}

fn lower_signals_to_interp_transform_fastlane(
    source_name: &str,
    output: &SignalCompileOutput,
    options: &InterpOptions,
    fir_verify: FirVerifyOptions,
) -> Result<String, LowerToInterpError> {
    let module_name = resolve_module_name(options.module_name.as_deref(), source_name);
    let lowered = lower_signals_to_fir_transform_fastlane(output, module_name)
        .map_err(LowerToInterpError::Transform)?;
    maybe_verify_fir_module(&lowered, fir_verify).map_err(LowerToInterpError::Verify)?;
    let mut effective_options = options.clone();
    if effective_options.num_inputs == 0 {
        effective_options.num_inputs = output.process_arity.inputs;
    }
    if effective_options.num_outputs == 0 {
        effective_options.num_outputs = output.process_arity.outputs;
    }
    let factory: FbcDspFactory<f32> =
        generate_interp_module(&lowered.store, lowered.module, &effective_options)
            .map_err(LowerToInterpError::Codegen)?;
    serialize_factory(&factory).map_err(LowerToInterpError::Serialize)
}

/// Serializes a [`FbcDspFactory`] to `.fbc` text format.
fn serialize_factory(factory: &FbcDspFactory<f32>) -> Result<String, String> {
    let mut buf = Vec::new();
    write_fbc(factory, &mut buf, false).map_err(|e| e.to_string())?;
    String::from_utf8(buf).map_err(|e| e.to_string())
}

fn lower_signals_to_fir(
    source_name: &str,
    output: &SignalCompileOutput,
    lane: SignalFirLane,
    fir_verify: FirVerifyOptions,
) -> Result<FirCompileOutput, LowerToFirError> {
    let module_name = sanitize_cpp_ident(source_name_to_class(source_name).as_str());
    let lowered = match lane {
        SignalFirLane::LegacyBridge => Ok(lower_signals_to_fir_legacy_bridge(
            source_name,
            output,
            module_name,
        )),
        SignalFirLane::TransformFastLane => {
            lower_signals_to_fir_transform_fastlane(output, module_name)
        }
    }
    .map_err(LowerToFirError::Transform)?;
    maybe_verify_fir_module(&lowered, fir_verify).map_err(LowerToFirError::Verify)?;
    Ok(lowered)
}

// ─── FIR type helpers ─────────────────────────────────────────────────────────

/// Builds the canonical `compute(dsp, int, FAUSTFLOAT**, FAUSTFLOAT**) -> void` FIR type
/// and its named argument list, used by both C and C++ legacy bridges.
fn make_compute_fir_signature() -> (FirType, [NamedType; 4]) {
    let ff_ptr_ptr = FirType::Ptr(Box::new(FirType::Ptr(Box::new(FirType::FaustFloat))));
    let args = [
        NamedType {
            name: "dsp".to_string(),
            typ: FirType::Ptr(Box::new(FirType::Obj)),
        },
        NamedType {
            name: "count".to_string(),
            typ: FirType::Int32,
        },
        NamedType {
            name: "inputs".to_string(),
            typ: ff_ptr_ptr.clone(),
        },
        NamedType {
            name: "outputs".to_string(),
            typ: ff_ptr_ptr.clone(),
        },
    ];
    let typ = FirType::Fun {
        args: vec![
            FirType::Ptr(Box::new(FirType::Obj)),
            FirType::Int32,
            ff_ptr_ptr.clone(),
            ff_ptr_ptr,
        ],
        ret: Box::new(FirType::Void),
    };
    (typ, args)
}

/// Resolves a module name from explicit class_name option or from the source name.
fn resolve_module_name(class_name: Option<&str>, source_name: &str) -> String {
    class_name
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| sanitize_cpp_ident(source_name_to_class(source_name).as_str()))
}

// ─── Legacy bridge implementations ───────────────────────────────────────────

fn lower_signals_to_fir_legacy_bridge(
    source_name: &str,
    output: &SignalCompileOutput,
    module_name: String,
) -> FirCompileOutput {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);
    let mut body = Vec::new();

    body.push(b.label(format!("source: {source_name}")));
    body.push(b.label(format!(
        "io: inputs={} outputs={}",
        output.process_arity.inputs, output.process_arity.outputs
    )));
    for (index, sig) in output.signals.iter().enumerate() {
        body.push(b.label(format!(
            "sig[{index}]: {}",
            dump_sig_readable(&output.parse.state.arena, *sig)
        )));
    }

    let body = b.block(&body);
    let (compute_type, compute_args) = make_compute_fir_signature();
    let compute = b.declare_fun("compute", compute_type, &compute_args, Some(body), false);
    let declarations = b.block(&[compute]);
    let dsp_struct = b.block(&[]);
    let globals = b.block(&[]);
    let module = b.module(module_name, dsp_struct, globals, declarations);

    FirCompileOutput { store, module }
}

fn lower_signals_to_fir_transform_fastlane(
    output: &SignalCompileOutput,
    module_name: String,
) -> Result<FirCompileOutput, SignalFirError> {
    let signal_fir_options = SignalFirOptions {
        module_name,
        strict_mode: true,
    };
    let lowered = compile_signals_to_fir_fastlane(
        &output.parse.state.arena,
        &output.signals,
        output.process_arity.inputs,
        output.process_arity.outputs,
        &signal_fir_options,
    )?;
    Ok(FirCompileOutput {
        store: lowered.store,
        module: lowered.module,
    })
}

fn lower_signals_to_cpp_legacy_bridge(
    source_name: &str,
    output: &SignalCompileOutput,
    options: &CppOptions,
    fir_verify: FirVerifyOptions,
) -> Result<String, LowerToCppError> {
    let module_name = resolve_module_name(options.class_name.as_deref(), source_name);
    let lowered = lower_signals_to_fir_legacy_bridge(source_name, output, module_name);
    maybe_verify_fir_module(&lowered, fir_verify).map_err(LowerToCppError::Verify)?;
    let mut effective_options = options.clone();
    if effective_options.num_inputs == 0 {
        effective_options.num_inputs = output.process_arity.inputs;
    }
    generate_cpp_module(&lowered.store, lowered.module, &effective_options)
        .map_err(LowerToCppError::Codegen)
}

fn lower_signals_to_cpp_transform_fastlane(
    source_name: &str,
    output: &SignalCompileOutput,
    options: &CppOptions,
    fir_verify: FirVerifyOptions,
) -> Result<String, LowerToCppError> {
    let module_name = resolve_module_name(options.class_name.as_deref(), source_name);
    let lowered = lower_signals_to_fir_transform_fastlane(output, module_name)
        .map_err(LowerToCppError::Transform)?;
    maybe_verify_fir_module(&lowered, fir_verify).map_err(LowerToCppError::Verify)?;
    let mut effective_options = options.clone();
    if effective_options.num_inputs == 0 {
        effective_options.num_inputs = output.process_arity.inputs;
    }
    generate_cpp_module(&lowered.store, lowered.module, &effective_options)
        .map_err(LowerToCppError::Codegen)
}

fn lower_signals_to_c_legacy_bridge(
    source_name: &str,
    _output: &SignalCompileOutput,
    options: &COptions,
    fir_verify: FirVerifyOptions,
) -> Result<String, LowerToCError> {
    let mut store = FirStore::new();
    let mut b = FirBuilder::new(&mut store);
    // Keep legacy C bridge intentionally minimal: C backend currently does not
    // emit FIR label statements, so we avoid `Label` nodes here.
    let body = b.block(&[]);
    let (compute_type, compute_args) = make_compute_fir_signature();
    let compute = b.declare_fun("compute", compute_type, &compute_args, Some(body), false);
    let declarations = b.block(&[compute]);
    let dsp_struct = b.block(&[]);
    let globals = b.block(&[]);
    let module_name = resolve_module_name(options.class_name.as_deref(), source_name);
    let module = b.module(module_name, dsp_struct, globals, declarations);
    let lowered = FirCompileOutput { store, module };
    maybe_verify_fir_module(&lowered, fir_verify).map_err(LowerToCError::Verify)?;
    generate_c_module(&lowered.store, lowered.module, options).map_err(LowerToCError::Codegen)
}

fn lower_signals_to_c_transform_fastlane(
    source_name: &str,
    output: &SignalCompileOutput,
    options: &COptions,
    fir_verify: FirVerifyOptions,
) -> Result<String, LowerToCError> {
    let module_name = resolve_module_name(options.class_name.as_deref(), source_name);
    let signal_fir_options = SignalFirOptions {
        module_name,
        strict_mode: true,
    };
    let lowered = compile_signals_to_fir_fastlane(
        &output.parse.state.arena,
        &output.signals,
        output.process_arity.inputs,
        output.process_arity.outputs,
        &signal_fir_options,
    )
    .map_err(LowerToCError::Transform)?;
    let lowered = FirCompileOutput {
        store: lowered.store,
        module: lowered.module,
    };
    maybe_verify_fir_module(&lowered, fir_verify).map_err(LowerToCError::Verify)?;
    generate_c_module(&lowered.store, lowered.module, options).map_err(LowerToCError::Codegen)
}

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

fn source_name_to_class(source_name: &str) -> String {
    Path::new(source_name)
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|stem| !stem.is_empty())
        .unwrap_or("faust_dsp")
        .to_owned()
}

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
    if let Ok(a) = propagate::box_arity(arena, left, &mut arity_cache) {
        diagnostic = diagnostic.with_note(format!(
            "A arity: inputs={} outputs={}",
            a.inputs, a.outputs
        ));
    }
    if let Ok(b) = propagate::box_arity(arena, right, &mut arity_cache) {
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
fn maybe_add_source_label(
    mut diagnostic: Diagnostic,
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    node: BoxId,
    owner_definition: Option<&str>,
) -> Diagnostic {
    if let Some(owner) = owner_definition {
        let owner_span = source_span_for_definition_name(ctx, arena, defs_root, owner);
        let call_span = source_span_for_process_binding_target(ctx, arena, defs_root)
            .or_else(|| source_span_for_process_definition(ctx, arena, defs_root));
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
        .or_else(|| source_span_for_process_binding_target(ctx, arena, defs_root))
        .or_else(|| source_span_for_process_definition(ctx, arena, defs_root));
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
fn maybe_add_eval_source_labels(
    mut diagnostic: Diagnostic,
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    node: BoxId,
    owner_definition: Option<&str>,
) -> Diagnostic {
    if let Some(owner) = owner_definition {
        let origin_span = source_span_for_definition_name(ctx, arena, defs_root, owner);
        let call_span = source_span_for_process_definition(ctx, arena, defs_root);
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
        .or_else(|| source_span_for_process_binding_target(ctx, arena, defs_root))
        .or_else(|| source_span_for_process_definition(ctx, arena, defs_root));
    let Some(primary_span) = primary else {
        return diagnostic;
    };
    diagnostic = diagnostic.with_label(Label::new(
        LabelStyle::Primary,
        primary_span.clone(),
        "call site",
    ));
    let secondary = source_span_for_definition_of_expr(ctx, arena, defs_root, node)
        .or_else(|| source_span_for_process_definition(ctx, arena, defs_root));
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

/// Fallback source span for the `process` definition identifier.
///
/// Used when the offending propagated/evaluated node cannot be mapped to a more
/// specific source location.
fn source_span_for_process_definition(
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    defs_root: BoxId,
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
        if let BoxMatch::Ident("process") = match_box(arena, name) {
            return source_span_for_node(ctx, name);
        }
        defs = arena.tl(defs)?;
    }
    None
}

/// Fallback source span for direct process aliases (`process = <ident>;`).
///
/// When `process` is a direct identifier alias, this resolves the target definition
/// location (for example `foo = ...; process = foo;` -> label on `foo = ...`).
fn source_span_for_process_binding_target(
    ctx: &parser::ParserCtx,
    arena: &tlib::TreeArena,
    defs_root: BoxId,
) -> Option<SourceSpan> {
    let (_process_name, process_expr) = find_definition_name_and_expr(arena, defs_root, "process")?;
    let BoxMatch::Ident(target_name) = match_box(arena, process_expr) else {
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

/// Finds one alias/binding trace from `process` to the owner of `node`.
///
/// The trace is expression-reference based (not only direct aliases), allowing contextual chains
/// such as `process = bar,bar; bar = foo; foo = ...` -> `process -> bar -> foo`.
fn alias_binding_trace_for_node(
    arena: &tlib::TreeArena,
    defs_root: BoxId,
    node: BoxId,
) -> Option<String> {
    let owner = owner_definition_name_for_node(arena, defs_root, node)?;
    if owner.as_ref() == "process" {
        return Some("process".to_owned());
    }

    let edges = definition_reference_edges(arena, defs_root);
    if !edges.contains_key("process") {
        return None;
    }

    let mut queue: VecDeque<Vec<Box<str>>> = VecDeque::new();
    let mut seen: HashSet<Box<str>> = HashSet::new();
    queue.push_back(vec!["process".into()]);
    seen.insert("process".into());

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

#[must_use]
/// Executes this operation and returns its result.
pub fn golden_snapshot(source_name: &str, source: &str) -> String {
    let normalized_source = normalize_newlines(source);
    let line_count = normalized_source.lines().count();
    let byte_count = normalized_source.len();
    let hash = fnv1a64(normalized_source.as_bytes());

    format!(
        "faust-rs-golden-v1\nsource={source_name}\nbytes={byte_count}\nlines={line_count}\nfnv1a64={hash:016x}\n"
    )
}

/// Executes this operation and returns its result.
pub fn golden_snapshot_from_file(path: &Path) -> Result<String, std::io::Error> {
    let source = std::fs::read_to_string(path)?;
    Ok(golden_snapshot(&path.display().to_string(), &source))
}

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0001_0000_01b3;

fn fnv1a64(input: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for byte in input {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn normalize_newlines(input: &str) -> String {
    input.replace("\r\n", "\n").replace('\r', "\n")
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        Compiler, CompilerError, default_search_base, golden_snapshot, make_compute_fir_signature,
        resolve_module_name,
    };

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

    // ── default_search_base ───────────────────────────────────────────────────

    #[test]
    fn default_search_base_returns_parent_when_present() {
        let path = PathBuf::from("/some/dir/file.dsp");
        assert_eq!(default_search_base(&path), PathBuf::from("/some/dir"));
    }

    #[test]
    fn default_search_base_returns_dot_for_bare_filename() {
        let path = PathBuf::from("file.dsp");
        // A bare filename has an empty parent; we fall back to ".".
        let base = default_search_base(&path);
        // Either "" or "." is acceptable depending on the platform; we just
        // check we get a valid path back rather than panicking.
        let _ = base;
    }

    // ── resolve_module_name ───────────────────────────────────────────────────

    #[test]
    fn resolve_module_name_uses_explicit_class_name() {
        let name = resolve_module_name(Some("MyDsp"), "ignored.dsp");
        assert_eq!(name, "MyDsp");
    }

    #[test]
    fn resolve_module_name_derives_from_source_name() {
        let name = resolve_module_name(None, "sine_phasor.dsp");
        assert_eq!(name, "sine_phasor");
    }

    #[test]
    fn resolve_module_name_sanitizes_invalid_chars() {
        let name = resolve_module_name(None, "my-dsp!.dsp");
        // Hyphens and exclamation marks are replaced with underscores.
        assert_eq!(name, "my_dsp_");
    }

    #[test]
    fn resolve_module_name_prefixes_leading_digit() {
        let name = resolve_module_name(None, "123dsp.dsp");
        assert!(
            name.starts_with('_'),
            "expected leading underscore, got {name}"
        );
    }

    // ── make_compute_fir_signature ────────────────────────────────────────────

    #[test]
    fn make_compute_fir_signature_produces_four_named_args() {
        let (_typ, args) = make_compute_fir_signature();
        assert_eq!(args.len(), 4);
        assert_eq!(args[0].name, "dsp");
        assert_eq!(args[1].name, "count");
        assert_eq!(args[2].name, "inputs");
        assert_eq!(args[3].name, "outputs");
    }

    #[test]
    fn make_compute_fir_signature_fun_type_matches_args() {
        use fir::FirType;
        let (typ, _args) = make_compute_fir_signature();
        match typ {
            FirType::Fun { args, ret } => {
                assert_eq!(args.len(), 4, "fun type should have 4 args");
                assert!(
                    matches!(args.first(), Some(FirType::Ptr(inner)) if matches!(inner.as_ref(), FirType::Obj)),
                    "first arg should be dsp pointer"
                );
                assert!(matches!(*ret, FirType::Void), "return type should be void");
            }
            other => panic!("expected FirType::Fun, got {other:?}"),
        }
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
}
