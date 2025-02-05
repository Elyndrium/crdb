use crate::BinPtr;
use std::{collections::HashMap, sync::Arc};

pub struct BinariesCache {
    data: HashMap<BinPtr, Arc<Vec<u8>>>,
}

impl BinariesCache {
    pub fn new() -> BinariesCache {
        BinariesCache {
            data: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.data.retain(|_, v| Arc::strong_count(v) != 1)
    }

    pub fn insert(&mut self, id: BinPtr, value: Arc<Vec<u8>>) {
        self.data.insert(id, value);
    }

    pub fn get(&self, id: &BinPtr) -> Option<Arc<Vec<u8>>> {
        self.data.get(id).cloned()
    }
}
