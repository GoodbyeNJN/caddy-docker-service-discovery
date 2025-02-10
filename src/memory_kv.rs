use std::collections::HashMap;
use std::net::Ipv4Addr;

pub struct MemoryKV {
    store: HashMap<String, Ipv4Addr>,
}

impl MemoryKV {
    pub fn new() -> Self {
        Self {
            store: HashMap::new(),
        }
    }

    pub fn set(&mut self, key: String, value: Ipv4Addr) {
        self.store.insert(key, value);
    }

    pub fn get(&self, key: &str) -> Option<&Ipv4Addr> {
        self.store.get(key)
    }

    pub fn clear(&mut self) {
        self.store.clear();
    }
}
