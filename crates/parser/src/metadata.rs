//! Compilation-global metadata store for Faust top-level `declare key "value";`.
//!
//! # Source provenance (C++)
//! - `compiler/parser/sourcereader.cpp`
//! - `declareMetadata(Tree key, Tree value)`
//! - `compiler/global.hh` (`gMetaDataSet`)
//!
//! # Mapping status
//! - Semantics: `1:1`
//! - Representation: `adapted`
//!
//! C++ stores top-level `declare` metadata in a process-global map keyed either
//! by the raw metadata key (master document) or by `filename/key` for imported
//! documents. Rust keeps the same session-wide semantics in an explicit shared
//! value owned by the active compilation flow instead of a global singleton.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

/// One metadata key in the compilation-global top-level `declare` store.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CompilationMetadataKey {
    /// Key emitted by the master document with no file prefix.
    Global { key: Box<str> },
    /// Key emitted by an imported/loaded file and therefore scoped by source.
    Scoped {
        source_file: Box<str>,
        key: Box<str>,
    },
}

impl CompilationMetadataKey {
    /// Returns a master-document key.
    #[must_use]
    pub fn global(key: impl Into<Box<str>>) -> Self {
        Self::Global { key: key.into() }
    }

    /// Returns an imported-file-scoped key.
    #[must_use]
    pub fn scoped(source_file: impl Into<Box<str>>, key: impl Into<Box<str>>) -> Self {
        Self::Scoped {
            source_file: source_file.into(),
            key: key.into(),
        }
    }
}

/// Deterministic snapshot of the compilation-global top-level metadata store.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CompilationMetadataSnapshot {
    entries: BTreeMap<CompilationMetadataKey, BTreeSet<Box<str>>>,
}

impl CompilationMetadataSnapshot {
    /// Returns the underlying deterministic key/value map.
    #[must_use]
    pub fn entries(&self) -> &BTreeMap<CompilationMetadataKey, BTreeSet<Box<str>>> {
        &self.entries
    }
}

#[derive(Debug)]
struct CompilationMetadataStoreInner {
    master_source: Box<str>,
    entries: BTreeMap<CompilationMetadataKey, BTreeSet<Box<str>>>,
}

/// Shared compilation-session metadata store for top-level `declare key "value";`.
///
/// This is the Rust equivalent of the C++ `gMetaDataSet` semantic role.
#[derive(Clone, Debug)]
pub struct CompilationMetadataStore {
    inner: Arc<Mutex<CompilationMetadataStoreInner>>,
}

impl CompilationMetadataStore {
    /// Creates one metadata store bound to the current master source.
    #[must_use]
    pub fn new(master_source: &str) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CompilationMetadataStoreInner {
                master_source: master_source.into(),
                entries: BTreeMap::new(),
            })),
        }
    }

    /// Records one top-level `declare key "value";` under C++-equivalent scope rules.
    ///
    /// Source provenance (C++):
    /// - `compiler/parser/sourcereader.cpp`
    /// - `declareMetadata(Tree key, Tree value)`
    ///
    /// C++ writes plain `key` in the master document and `filename/key` in
    /// imported documents. Rust preserves that distinction structurally through
    /// [`CompilationMetadataKey`] rather than flattening it into one string.
    pub fn declare_top_level(&self, current_source: &str, key: &str, value: &str) {
        let mut guard = self.inner.lock().expect("metadata store lock poisoned");
        let key = if current_source == guard.master_source.as_ref() {
            CompilationMetadataKey::global(key)
        } else {
            CompilationMetadataKey::scoped(current_source, key)
        };
        guard.entries.entry(key).or_default().insert(value.into());
    }

    /// Returns a deterministic snapshot of the currently aggregated metadata.
    #[must_use]
    pub fn snapshot(&self) -> CompilationMetadataSnapshot {
        let guard = self.inner.lock().expect("metadata store lock poisoned");
        CompilationMetadataSnapshot {
            entries: guard.entries.clone(),
        }
    }
}
