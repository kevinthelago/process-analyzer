use lru::LruCache;
use std::num::NonZeroUsize;

use crate::types::LookupResult;

/// (module_path_hash, module_relative_offset) → LookupResult
type CacheKey = (u64, u64);

const DEFAULT_CAPACITY: usize = 65_536;

pub struct SymbolCache {
    inner: LruCache<CacheKey, LookupResult>,
}

impl SymbolCache {
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap();
        Self {
            inner: LruCache::new(cap),
        }
    }

    pub fn get(&mut self, module_hash: u64, offset: u64) -> Option<&LookupResult> {
        self.inner.get(&(module_hash, offset))
    }

    pub fn insert(&mut self, module_hash: u64, offset: u64, result: LookupResult) {
        self.inner.put((module_hash, offset), result);
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

impl Default for SymbolCache {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}
