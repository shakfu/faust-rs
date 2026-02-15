use crate::TreeId;
use ahash::AHashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PropertyKey(u32);

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
    #[must_use]
    pub fn new() -> Self {
        Self {
            values: Vec::new(),
            key_intern: AHashMap::new(),
            next_key: 0,
            len: 0,
        }
    }

    pub fn key(&mut self, key: impl AsRef<str>) -> PropertyKey {
        self.intern_key(key.as_ref())
    }

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

    #[must_use]
    pub fn get_with_key(&self, node: TreeId, key: PropertyKey) -> Option<&T> {
        let idx = node.as_u32() as usize;
        self.values
            .get(key.0 as usize)
            .and_then(|slots| slots.get(idx))
            .and_then(Option::as_ref)
    }

    pub fn get_mut_with_key(&mut self, node: TreeId, key: PropertyKey) -> Option<&mut T> {
        let idx = node.as_u32() as usize;
        self.values
            .get_mut(key.0 as usize)
            .and_then(move |slots| slots.get_mut(idx))
            .and_then(Option::as_mut)
    }

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

    pub fn set(&mut self, node: TreeId, key: impl AsRef<str>, value: T) -> Option<T> {
        let key = self.intern_key(key.as_ref());
        self.set_with_key(node, key, value)
    }

    #[must_use]
    pub fn get(&self, node: TreeId, key: &str) -> Option<&T> {
        let key = self.key_intern.get(key).copied()?;
        self.get_with_key(node, key)
    }

    pub fn get_mut(&mut self, node: TreeId, key: &str) -> Option<&mut T> {
        let key = self.key_intern.get(key).copied()?;
        self.get_mut_with_key(node, key)
    }

    pub fn remove(&mut self, node: TreeId, key: &str) -> Option<T> {
        let key = self.key_intern.get(key).copied()?;
        self.remove_with_key(node, key)
    }

    pub fn clear(&mut self) {
        self.values.clear();
        self.len = 0;
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

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
