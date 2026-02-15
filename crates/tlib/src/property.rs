//! Node-keyed property storage for `tlib` trees.
//!
//! # Source provenance (C++)
//! - `compiler/tlib/property.hh` (`property<T>` wrappers)
//! - `compiler/tlib/tree.hh` (`CTree::setProperty/getProperty`)
//!
//! # Parity invariants
//! - Properties are attached to node identity (`TreeId`) and a property key.
//! - String keys are interned once, then fast path uses numeric keys (`PropertyKey`).

use crate::TreeId;
use ahash::AHashMap;

/// Interned property key identifier.
///
/// Keys are stable for the lifetime of one [`PropertyStore`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PropertyKey(u32);

/// Property storage indexed by `(PropertyKey, TreeId)`.
///
/// The storage layout is optimized for hot parser/evaluator passes:
/// - key interning map (`AHashMap<Box<str>, PropertyKey>`),
/// - slot vectors indexed by `TreeId` for O(1)-like keyed access.
#[derive(Debug)]
pub struct PropertyStore<T> {
    values: Vec<Vec<Option<T>>>,
    key_intern: AHashMap<Box<str>, PropertyKey>,
    next_key: u32,
    len: usize,
}

impl<T> Default for PropertyStore<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> PropertyStore<T> {
    /// Creates an empty property store.
    #[must_use]
    pub fn new() -> Self {
        Self::with_key_capacity(0)
    }

    /// Creates an empty property store with expected key capacity.
    #[must_use]
    pub fn with_key_capacity(key_capacity: usize) -> Self {
        Self {
            values: Vec::with_capacity(key_capacity),
            key_intern: AHashMap::with_capacity(key_capacity),
            next_key: 0,
            len: 0,
        }
    }

    /// Interns `key` (or reuses an existing key) and returns its [`PropertyKey`].
    pub fn key(&mut self, key: impl AsRef<str>) -> PropertyKey {
        self.intern_key(key.as_ref())
    }

    /// Sets value for `(node, key)` using interned key path.
    ///
    /// Returns previous value if one existed.
    pub fn set_with_key(&mut self, node: TreeId, key: PropertyKey, value: T) -> Option<T> {
        let key_idx = key.0 as usize;
        if key_idx >= self.values.len() {
            self.values.resize_with(key_idx + 1, Vec::new);
        }
        let idx = node.as_u32() as usize;
        let slots = &mut self.values[key_idx];
        if idx >= slots.len() {
            slots.resize_with(idx + 1, || None);
        }
        let prev = slots[idx].replace(value);
        if prev.is_none() {
            self.len += 1;
        }
        prev
    }

    /// Gets value for `(node, key)` using interned key path.
    #[must_use]
    pub fn get_with_key(&self, node: TreeId, key: PropertyKey) -> Option<&T> {
        let idx = node.as_u32() as usize;
        self.values
            .get(key.0 as usize)
            .and_then(|slots| slots.get(idx))
            .and_then(Option::as_ref)
    }

    /// Mutable variant of [`Self::get_with_key`].
    pub fn get_mut_with_key(&mut self, node: TreeId, key: PropertyKey) -> Option<&mut T> {
        let idx = node.as_u32() as usize;
        self.values
            .get_mut(key.0 as usize)
            .and_then(move |slots| slots.get_mut(idx))
            .and_then(Option::as_mut)
    }

    /// Removes value for `(node, key)` and returns removed value if any.
    pub fn remove_with_key(&mut self, node: TreeId, key: PropertyKey) -> Option<T> {
        let idx = node.as_u32() as usize;
        let slots = self.values.get_mut(key.0 as usize)?;
        if idx >= slots.len() {
            return None;
        }
        let prev = slots[idx].take();
        if prev.is_some() {
            self.len -= 1;
        }
        prev
    }

    /// Sets value using string key path (interns key on first use).
    pub fn set(&mut self, node: TreeId, key: impl AsRef<str>, value: T) -> Option<T> {
        let key = self.intern_key(key.as_ref());
        self.set_with_key(node, key, value)
    }

    /// Gets value using string key path.
    #[must_use]
    pub fn get(&self, node: TreeId, key: &str) -> Option<&T> {
        let key = self.key_intern.get(key).copied()?;
        self.get_with_key(node, key)
    }

    /// Mutable variant of [`Self::get`].
    pub fn get_mut(&mut self, node: TreeId, key: &str) -> Option<&mut T> {
        let key = self.key_intern.get(key).copied()?;
        self.get_mut_with_key(node, key)
    }

    /// Removes value using string key path.
    pub fn remove(&mut self, node: TreeId, key: &str) -> Option<T> {
        let key = self.key_intern.get(key).copied()?;
        self.remove_with_key(node, key)
    }

    /// Ensures that storage for `key` can index up to `slots_len` entries.
    ///
    /// New entries are initialized to `None`.
    pub fn reserve_slots(&mut self, key: PropertyKey, slots_len: usize) {
        let key_idx = key.0 as usize;
        if key_idx >= self.values.len() {
            self.values.resize_with(key_idx + 1, Vec::new);
        }
        let slots = &mut self.values[key_idx];
        if slots.len() < slots_len {
            slots.resize_with(slots_len, || None);
        }
    }

    /// Clears all values.
    ///
    /// Interned key mapping is preserved to keep key ids stable for the store lifetime.
    pub fn clear(&mut self) {
        self.values.clear();
        self.len = 0;
    }

    /// Number of stored `(node, key)` values.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// `true` if no values are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn intern_key(&mut self, key: &str) -> PropertyKey {
        if let Some(id) = self.key_intern.get(key) {
            return *id;
        }
        let id = PropertyKey(self.next_key);
        self.next_key = self
            .next_key
            .checked_add(1)
            .expect("property key id overflow");
        let _ = self.key_intern.insert(key.to_owned().into_boxed_str(), id);
        self.values.push(Vec::new());
        id
    }
}
