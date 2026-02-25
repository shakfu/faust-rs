//! Global factory cache for `cranelift_dsp` (scaffold implementation).
//!
//! This mirrors the basic shape of the interpreter/LLVM factory cache APIs:
//! SHA key -> factory pointer, with `get/deleteAll/getAll` entry points.
//!
//! # Current limitations
//! - No refcount semantics yet (unlike the production C++ behavior).
//! - Returns raw pointers directly from the cache.
//! - Intended for FFI/lifecycle smoke validation during early Cranelift porting.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex};

use crate::types::CraneliftDspFactory;

// Store pointers as usize so the map remains Send/Sync-safe under Mutex.
static FACTORY_CACHE: LazyLock<Mutex<HashMap<String, usize>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

static MT_MODE: AtomicBool = AtomicBool::new(false);

/// Insert or replace a factory pointer under a SHA key.
pub(crate) fn cache_insert(sha: &str, ptr: *mut CraneliftDspFactory) {
    let mut guard = FACTORY_CACHE.lock().unwrap();
    guard.insert(sha.to_owned(), ptr as usize);
}

/// Look up a factory pointer by SHA key.
#[must_use]
pub(crate) fn cache_lookup(sha: &str) -> *mut CraneliftDspFactory {
    let guard = FACTORY_CACHE.lock().unwrap();
    guard
        .get(sha)
        .copied()
        .map_or(std::ptr::null_mut(), |v| v as *mut CraneliftDspFactory)
}

/// Remove a factory from the cache by pointer value.
pub(crate) fn cache_remove_by_ptr(ptr: *mut CraneliftDspFactory) {
    let mut guard = FACTORY_CACHE.lock().unwrap();
    guard.retain(|_, v| *v != ptr as usize);
}

/// Drain the entire cache and return factory pointers for caller-side freeing.
#[must_use]
pub(crate) fn cache_drain() -> Vec<*mut CraneliftDspFactory> {
    let mut guard = FACTORY_CACHE.lock().unwrap();
    guard
        .drain()
        .map(|(_, v)| v as *mut CraneliftDspFactory)
        .collect()
}

/// Return all SHA keys currently stored in the cache.
#[must_use]
pub(crate) fn cache_all_sha_keys() -> Vec<String> {
    let guard = FACTORY_CACHE.lock().unwrap();
    guard.keys().cloned().collect()
}

/// Enable multi-thread mode (compatibility flag only in the scaffold).
#[must_use]
pub(crate) fn start_mt() -> bool {
    MT_MODE.store(true, Ordering::SeqCst);
    true
}

/// Disable multi-thread mode (compatibility flag only in the scaffold).
pub(crate) fn stop_mt() {
    MT_MODE.store(false, Ordering::SeqCst);
}

/// Returns a short status string used by tests to assert the scaffold module is present.
#[must_use]
pub fn cache_status() -> &'static str {
    "cranelift-ffi cache scaffold"
}

#[cfg(test)]
mod tests {
    use super::{cache_all_sha_keys, cache_drain, cache_insert, cache_lookup, cache_status};

    #[test]
    fn cache_scaffold_status_is_stable() {
        assert_eq!(cache_status(), "cranelift-ffi cache scaffold");
    }

    #[test]
    fn cache_roundtrip_raw_pointer() {
        let p = 0x1234usize as *mut crate::types::CraneliftDspFactory;
        cache_insert("sha-test", p);
        assert_eq!(cache_lookup("sha-test"), p);
        assert!(cache_all_sha_keys().iter().any(|s| s == "sha-test"));
        let drained = cache_drain();
        assert!(drained.contains(&p));
        assert!(cache_lookup("sha-test").is_null());
    }
}
