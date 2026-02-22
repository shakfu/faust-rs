//! Global factory cache.
//!
//! Mirrors the `gFactoryDSPTable` cache in the C++ Faust implementation.
//! Maps SHA key strings to raw factory pointers.
//!
//! Thread safety: protected by a `Mutex`.  The `startMTDSPFactories` /
//! `stopMTDSPFactories` functions exist for API compatibility; actual thread
//! safety is provided by the `Mutex` regardless of the MT mode flag.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use crate::types::InterpreterDspFactory;

// Store pointers as `usize` to avoid `*mut T: !Send` issues with the Mutex.
static FACTORY_CACHE: LazyLock<Mutex<HashMap<String, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static MT_MODE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Insert a factory into the cache under its SHA key.
pub(crate) fn cache_insert(sha: &str, ptr: *mut InterpreterDspFactory) {
    let mut guard = FACTORY_CACHE.lock().unwrap();
    guard.insert(sha.to_owned(), ptr as usize);
}

/// Look up a factory by SHA key.  Returns `null` if not found.
pub(crate) fn cache_lookup(sha: &str) -> *mut InterpreterDspFactory {
    let guard = FACTORY_CACHE.lock().unwrap();
    guard
        .get(sha)
        .copied()
        .map_or(std::ptr::null_mut(), |v| v as *mut InterpreterDspFactory)
}

/// Remove a factory from the cache by pointer value.
/// Does nothing if the pointer is not in the cache.
pub(crate) fn cache_remove_by_ptr(ptr: *mut InterpreterDspFactory) {
    let mut guard = FACTORY_CACHE.lock().unwrap();
    guard.retain(|_, v| *v != ptr as usize);
}

/// Drain the entire cache and return all factory pointers.
///
/// The caller is responsible for freeing them.
pub(crate) fn cache_drain() -> Vec<*mut InterpreterDspFactory> {
    let mut guard = FACTORY_CACHE.lock().unwrap();
    guard
        .drain()
        .map(|(_, v)| v as *mut InterpreterDspFactory)
        .collect()
}

/// Return all SHA keys in the cache.
pub(crate) fn cache_all_sha_keys() -> Vec<String> {
    let guard = FACTORY_CACHE.lock().unwrap();
    guard.keys().cloned().collect()
}

/// Enable multi-thread access mode.
pub(crate) fn start_mt() -> bool {
    MT_MODE.store(true, std::sync::atomic::Ordering::SeqCst);
    true
}

/// Disable multi-thread access mode.
pub(crate) fn stop_mt() {
    MT_MODE.store(false, std::sync::atomic::Ordering::SeqCst);
}
