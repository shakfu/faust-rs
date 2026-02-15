use std::collections::HashMap;

use crate::TreeId;

#[derive(Debug)]
pub struct PropertyStore<T> {
    values: HashMap<(TreeId, String), T>,
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
            values: HashMap::new(),
        }
    }

    pub fn set(&mut self, node: TreeId, key: impl Into<String>, value: T) -> Option<T> {
        self.values.insert((node, key.into()), value)
    }

    #[must_use]
    pub fn get(&self, node: TreeId, key: &str) -> Option<&T> {
        self.values.get(&(node, key.to_owned()))
    }

    pub fn get_mut(&mut self, node: TreeId, key: &str) -> Option<&mut T> {
        self.values.get_mut(&(node, key.to_owned()))
    }

    pub fn remove(&mut self, node: TreeId, key: &str) -> Option<T> {
        self.values.remove(&(node, key.to_owned()))
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}
