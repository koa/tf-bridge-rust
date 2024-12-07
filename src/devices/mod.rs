use std::collections::HashMap;
use std::hash::Hash;
use tokio::task::JoinHandle;

pub mod shelly;
pub mod tinkerforge;

#[derive(Debug)]
struct HandleRegistry<K: Hash + Eq>(HashMap<K, JoinHandle<()>>);

impl<K: Hash + Eq> HandleRegistry<K> {
    fn insert(&mut self, key: K, handle: JoinHandle<()>) {
        if let Some(old_handle) = self.0.insert(key, handle) {
            old_handle.abort();
        }
    }
    fn remove(&mut self, key: &K) {
        if let Some(handle) = self.0.remove(key) {
            handle.abort();
        }
    }
}

impl<K: Hash + Eq> Drop for HandleRegistry<K> {
    fn drop(&mut self) {
        for handle in self.0.values() {
            handle.abort();
        }
    }
}

impl<K: Hash + Eq> Default for HandleRegistry<K> {
    fn default() -> Self {
        Self(HashMap::new())
    }
}
