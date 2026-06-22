use crate::types::ModuleEntry;

/// Tracks loaded modules sorted by base address for O(log n) address→module lookup.
#[derive(Debug, Default)]
pub struct ModuleMap {
    // Sorted by ModuleEntry.base ascending.
    entries: Vec<ModuleEntry>,
}

impl ModuleMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace a module. Keeps the vec sorted.
    pub fn insert(&mut self, entry: ModuleEntry) {
        let idx = self
            .entries
            .partition_point(|e| e.base < entry.base);
        // Replace if there's already an entry at this base.
        if self.entries.get(idx).map(|e| e.base) == Some(entry.base) {
            self.entries[idx] = entry;
        } else {
            self.entries.insert(idx, entry);
        }
    }

    /// Remove any module whose base address matches.
    pub fn remove(&mut self, base: u64) {
        self.entries.retain(|e| e.base != base);
    }

    /// Return the module that contains `addr` (base <= addr < base+size).
    pub fn lookup(&self, addr: u64) -> Option<&ModuleEntry> {
        let idx = self.entries.partition_point(|e| e.base <= addr);
        // partition_point gives the first entry AFTER addr, so check idx-1.
        let idx = idx.checked_sub(1)?;
        let e = &self.entries[idx];
        if addr < e.base + e.size {
            Some(e)
        } else {
            None
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(base: u64, size: u64) -> ModuleEntry {
        ModuleEntry {
            base,
            size,
            path: PathBuf::from("test.so"),
            build_id: None,
        }
    }

    #[test]
    fn lookup_hit() {
        let mut m = ModuleMap::new();
        m.insert(entry(0x1000, 0x500));
        m.insert(entry(0x2000, 0x200));
        assert!(m.lookup(0x1000).is_some());
        assert!(m.lookup(0x14ff).is_some());
        assert_eq!(m.lookup(0x1000).unwrap().base, 0x1000);
    }

    #[test]
    fn lookup_miss_gap() {
        let mut m = ModuleMap::new();
        m.insert(entry(0x1000, 0x100));
        m.insert(entry(0x2000, 0x100));
        assert!(m.lookup(0x1500).is_none());
    }

    #[test]
    fn lookup_miss_beyond() {
        let mut m = ModuleMap::new();
        m.insert(entry(0x1000, 0x100));
        assert!(m.lookup(0x1100).is_none());
    }

    #[test]
    fn remove() {
        let mut m = ModuleMap::new();
        m.insert(entry(0x1000, 0x100));
        m.remove(0x1000);
        assert!(m.lookup(0x1050).is_none());
    }
}
