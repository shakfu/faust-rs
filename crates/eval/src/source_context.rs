//! Source-resolution context for evaluator file loading.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use parser::{CompilationMetadataSnapshot, CompilationMetadataStore};
use tlib::{TreeArena, TreeId};

/// Internal DSP sample computation precision.
///
/// This mirrors Faust's `-double` flag: [`SamplePrecision::Float32`] selects
/// `float` as the internal computation type (the default), while
/// [`SamplePrecision::Float64`] selects `double`.
///
/// **Note**: this setting has no effect on compile-time constant folding inside
/// the evaluator — pattern-matching numeric constants are always folded at
/// `f64` precision. It is an output annotation for downstream code-generation
/// backends (e.g. FIR lowering) that consume the evaluated box tree.
///
/// The type is attached to [`EvalSourceContext`] so it travels with the
/// evaluation session and can be forwarded to backends without requiring a
/// separate channel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SamplePrecision {
    /// 32-bit single-precision float (`float` in C++). Default.
    #[default]
    Float32,
    /// 64-bit double-precision float (`double` in C++).
    Float64,
}

/// Filesystem source-resolution context captured by evaluator environments.
///
/// # Source provenance (C++)
/// - `compiler/global.cpp`
/// - `compiler/parser/sourcereader.hh/.cpp`
/// - `compiler/evaluate/eval.cpp` (`boxComponent` / `boxLibrary`)
///
/// The C++ evaluator loads `component("...")` and `library("...")` through the
/// process-global `gReader`, whose search state is already configured from the
/// active compile session. The Rust port has no global reader, so the evaluator
/// carries the equivalent resolution context explicitly and captures it inside
/// closures together with the lexical environment.
///
/// Mapping status: `adapted`.
///
/// # Invariants
/// - `current_file` is the file relative to which nested source loads should be
///   resolved when the evaluator is operating on file-backed Faust sources.
/// - `search_paths` preserves deterministic lookup order after the current file.
/// - in-memory evaluation uses [`EvalSourceContext::memory`], which intentionally
///   carries no filesystem base.
/// - one context instance also acts as a per-session cache for already loaded
///   Faust source files, mirroring the role of C++ `SourceReader::fFileCache`.
/// - top-level `declare key "value";` metadata for file-backed loads is written
///   into the shared [`CompilationMetadataStore`] captured by the context.
#[derive(Clone, Debug, Default)]
pub struct EvalSourceContext {
    pub(crate) current_file: Option<PathBuf>,
    pub(crate) search_paths: Vec<PathBuf>,
    pub(crate) cache: Arc<Mutex<HashMap<PathBuf, CachedLoadedSource>>>,
    pub(crate) loaded_files: Arc<Mutex<Vec<PathBuf>>>,
    pub(crate) metadata_store: Option<CompilationMetadataStore>,
    /// Internal DSP computation precision forwarded to code-generation backends.
    ///
    /// Defaults to [`SamplePrecision::Float32`] (C++ `float`).
    /// Set to [`SamplePrecision::Float64`] to request `double`-precision
    /// internal computation, equivalent to passing `-double` to `faust`.
    pub sample_precision: SamplePrecision,
}

impl EvalSourceContext {
    /// Creates a context for in-memory evaluation with no filesystem base.
    #[must_use]
    pub fn memory() -> Self {
        Self::default()
    }

    /// Creates a context for in-memory evaluation with one shared top-level
    /// metadata store.
    #[must_use]
    pub fn memory_with_metadata(metadata_store: CompilationMetadataStore) -> Self {
        Self {
            metadata_store: Some(metadata_store),
            ..Self::default()
        }
    }

    /// Creates an in-memory evaluation context with explicit import search
    /// paths and one shared top-level metadata store.
    ///
    /// This is the source-string counterpart of [`Self::for_file_with_metadata`]:
    /// it preserves the "compile from string but still resolve `component` /
    /// `library` through explicit `-I` entries" workflow used by bindings such
    /// as `faustwasm`.
    #[must_use]
    pub fn memory_with_search_paths_and_metadata(
        search_paths: &[PathBuf],
        metadata_store: CompilationMetadataStore,
    ) -> Self {
        let mut ordered = Vec::with_capacity(search_paths.len());
        for candidate in search_paths {
            if !ordered.iter().any(|existing| existing == candidate) {
                ordered.push(candidate.clone());
            }
        }
        Self {
            current_file: None,
            search_paths: ordered,
            cache: Arc::default(),
            loaded_files: Arc::default(),
            metadata_store: Some(metadata_store),
            sample_precision: SamplePrecision::default(),
        }
    }

    /// Creates a context rooted at one source file plus optional import search paths.
    ///
    /// The file parent directory is prepended ahead of explicit `search_paths`,
    /// matching the effective C++/parser lookup contract for file-backed sessions.
    /// Reusing the same returned context across multiple `eval_process_*` calls
    /// also reuses the same loaded-source cache.
    #[must_use]
    pub fn for_file(path: &Path, search_paths: &[PathBuf]) -> Self {
        Self::for_file_with_metadata(
            path,
            search_paths,
            CompilationMetadataStore::new(&path.to_string_lossy()),
        )
    }

    /// Creates a file-backed context with one shared top-level metadata store.
    #[must_use]
    pub fn for_file_with_metadata(
        path: &Path,
        search_paths: &[PathBuf],
        metadata_store: CompilationMetadataStore,
    ) -> Self {
        let mut ordered = Vec::with_capacity(search_paths.len() + 1);
        if let Some(parent) = path.parent() {
            ordered.push(parent.to_path_buf());
        }
        for candidate in search_paths {
            if !ordered.iter().any(|existing| existing == candidate) {
                ordered.push(candidate.clone());
            }
        }
        Self {
            current_file: Some(path.to_path_buf()),
            search_paths: ordered,
            cache: Arc::default(),
            loaded_files: Arc::default(),
            metadata_store: Some(metadata_store),
            sample_precision: SamplePrecision::default(),
        }
    }

    /// Returns a context for a newly loaded file while preserving inherited search order.
    ///
    /// The [`SamplePrecision`] of the parent context is propagated to the child
    /// so that sub-files loaded via `component`/`library` share the same
    /// precision setting as the root evaluation session.
    #[must_use]
    pub fn for_loaded_file(&self, path: &Path) -> Self {
        let mut child = match &self.metadata_store {
            Some(metadata_store) => {
                Self::for_file_with_metadata(path, &self.search_paths, metadata_store.clone())
            }
            None => Self::for_file(path, &self.search_paths),
        };
        child.loaded_files = self.loaded_files.clone();
        child.sample_precision = self.sample_precision;
        child
    }

    /// Returns the current file used as the primary relative-resolution anchor.
    #[must_use]
    pub fn current_file(&self) -> Option<&Path> {
        self.current_file.as_deref()
    }

    /// Returns the ordered import search paths used after the current-file base.
    #[must_use]
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    /// Returns the shared top-level metadata store captured by this context, if any.
    #[must_use]
    pub fn metadata_store(&self) -> Option<&CompilationMetadataStore> {
        self.metadata_store.as_ref()
    }

    /// Returns a snapshot of the aggregated top-level metadata visible in this session.
    #[must_use]
    pub fn metadata_snapshot(&self) -> CompilationMetadataSnapshot {
        self.metadata_store.as_ref().map_or_else(
            CompilationMetadataSnapshot::default,
            CompilationMetadataStore::snapshot,
        )
    }

    #[must_use]
    pub fn loaded_files(&self) -> Vec<PathBuf> {
        self.loaded_files
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    pub(crate) fn cached_loaded_source_hits<R>(
        &self,
        paths: &[PathBuf],
        f: impl FnOnce(Option<&CachedLoadedSource>, &Path) -> R,
    ) -> R {
        let guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        for path in paths {
            if let Some(loaded) = guard.get(path) {
                return f(Some(loaded), path);
            }
        }
        f(None, Path::new(""))
    }

    pub(crate) fn insert_loaded_source(&self, path: PathBuf, source: CachedLoadedSource) {
        let mut guard = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        guard.insert(path.clone(), source);
        drop(guard);
        let mut loaded = self.loaded_files.lock().unwrap_or_else(|e| e.into_inner());
        if !loaded.iter().any(|existing| existing == &path) {
            loaded.push(path);
        }
    }
}

impl PartialEq for EvalSourceContext {
    fn eq(&self, other: &Self) -> bool {
        self.current_file == other.current_file
            && self.search_paths == other.search_paths
            && self.metadata_snapshot() == other.metadata_snapshot()
    }
}

impl Eq for EvalSourceContext {}

#[derive(Debug)]
/// One file loaded through the evaluator source-loading cache.
pub(crate) struct CachedLoadedSource {
    pub(crate) root: TreeId,
    pub(crate) arena: TreeArena,
    pub(crate) parse_errors: Vec<String>,
}
