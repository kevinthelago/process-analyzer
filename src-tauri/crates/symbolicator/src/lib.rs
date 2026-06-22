mod cache;
mod dwarf;
mod error;
mod module_map;
mod pdb_resolver;
mod types;

pub use error::SymbolicatorError;
pub use types::{LookupResult, ModuleEntry, SymbolFrame};

use cache::SymbolCache;
use dwarf::DwarfContext;
use module_map::ModuleMap;
use pdb_resolver::PdbResolver;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

fn hash_path(path: &Path) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    let mut h = DefaultHasher::new();
    path.hash(&mut h);
    h.finish()
}

enum DebugSource {
    Dwarf(DwarfContext),
    Pdb(PdbResolver),
    /// Debug info was looked for but not found or mismatched; degrade gracefully.
    Degraded { reason: String },
}

/// Per-process symbolicator: tracks loaded modules and resolves addresses with caching.
///
/// # Degradation policy
/// When debug info is missing, mismatched, or fails to parse, the result is
/// `LookupResult::ModuleOffset` rather than an error. Callers never get a hard
/// failure for an address — the trace is always preserved, just with less detail.
pub struct Symbolicator {
    modules: ModuleMap,
    /// module path → loaded debug source
    debug_sources: HashMap<PathBuf, DebugSource>,
    cache: SymbolCache,
}

impl Symbolicator {
    pub fn new() -> Self {
        Self {
            modules: ModuleMap::new(),
            debug_sources: HashMap::new(),
            cache: SymbolCache::default(),
        }
    }

    pub fn with_cache_capacity(capacity: usize) -> Self {
        Self {
            modules: ModuleMap::new(),
            debug_sources: HashMap::new(),
            cache: SymbolCache::new(capacity),
        }
    }

    /// Register a newly loaded module. Eagerly opens debug info if available.
    pub fn add_module(&mut self, entry: ModuleEntry) {
        if !entry.path.as_os_str().is_empty() {
            let source = self.load_debug_source(&entry);
            self.debug_sources.insert(entry.path.clone(), source);
        }
        self.modules.insert(entry);
    }

    /// Unregister a module at this base address.
    pub fn remove_module(&mut self, base: u64) {
        if let Some(entry) = self.modules.lookup(base).cloned() {
            self.debug_sources.remove(&entry.path);
        }
        self.modules.remove(base);
    }

    /// Resolve a virtual address in the process's address space.
    ///
    /// Never fails: on any error the result degrades to `ModuleOffset` or `Unknown`.
    pub fn lookup(&mut self, address: u64) -> LookupResult {
        let module = match self.modules.lookup(address) {
            Some(m) => m.clone(),
            None => return LookupResult::Unknown { address },
        };

        let offset = address - module.base;
        let path_hash = hash_path(&module.path);
        let module_name = module
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("module@{:#x}", module.base));

        // Cache hit.
        if let Some(cached) = self.cache.get(path_hash, offset) {
            return cached.clone();
        }

        let result = match self.debug_sources.get(&module.path) {
            Some(DebugSource::Dwarf(ctx)) => {
                match ctx.resolve(offset) {
                    Ok(Some(frames)) => LookupResult::Resolved(frames),
                    Ok(None) => LookupResult::ModuleOffset {
                        module: module_name.clone(),
                        offset,
                    },
                    Err(_) => LookupResult::ModuleOffset {
                        module: module_name.clone(),
                        offset,
                    },
                }
            }
            Some(DebugSource::Pdb(resolver)) => {
                let rva = offset as u32;
                match resolver.resolve(rva) {
                    Ok(Some(frames)) => LookupResult::Resolved(frames),
                    Ok(None) => LookupResult::ModuleOffset {
                        module: module_name.clone(),
                        offset,
                    },
                    Err(_) => LookupResult::ModuleOffset {
                        module: module_name.clone(),
                        offset,
                    },
                }
            }
            Some(DebugSource::Degraded { .. }) | None => LookupResult::ModuleOffset {
                module: module_name.clone(),
                offset,
            },
        };

        self.cache.insert(path_hash, offset, result.clone());
        result
    }

    /// Resolve a slice of addresses, returning one result per address.
    pub fn lookup_many(&mut self, addresses: &[u64]) -> Vec<LookupResult> {
        addresses.iter().map(|&a| self.lookup(a)).collect()
    }

    fn load_debug_source(&self, entry: &ModuleEntry) -> DebugSource {
        let path = &entry.path;

        // Try PDB first on Windows (by extension), else DWARF.
        if path.extension().map(|e| e.eq_ignore_ascii_case("pdb")).unwrap_or(false) {
            match PdbResolver::open(path, &module_name_from_path(path)) {
                Ok(r) => return DebugSource::Pdb(r),
                Err(e) => {
                    return DebugSource::Degraded {
                        reason: format!("PDB open failed: {e}"),
                    }
                }
            }
        }

        // Try companion .pdb alongside the binary (Windows pattern).
        let pdb_companion = path.with_extension("pdb");
        if pdb_companion.exists() {
            match PdbResolver::open(&pdb_companion, &module_name_from_path(path)) {
                Ok(r) => return DebugSource::Pdb(r),
                Err(e) => {
                    return DebugSource::Degraded {
                        reason: format!("companion PDB open failed: {e}"),
                    }
                }
            }
        }

        // Try .dSYM bundle (macOS): <binary>.dSYM/Contents/Resources/DWARF/<binary-name>
        let dsym_path = dsym_path_for(path);
        let dwarf_path = if dsym_path.exists() { dsym_path } else { path.to_path_buf() };

        match DwarfContext::open(&dwarf_path, &module_name_from_path(path)) {
            Ok(ctx) => DebugSource::Dwarf(ctx),
            Err(e) => DebugSource::Degraded {
                reason: format!("DWARF open failed: {e}"),
            },
        }
    }
}

impl Default for Symbolicator {
    fn default() -> Self {
        Self::new()
    }
}

fn module_name_from_path(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn dsym_path_for(binary: &Path) -> PathBuf {
    let name = binary
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    binary
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{name}.dSYM/Contents/Resources/DWARF/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_address_with_no_modules() {
        let mut sym = Symbolicator::new();
        assert!(matches!(
            sym.lookup(0xdeadbeef),
            LookupResult::Unknown { .. }
        ));
    }

    #[test]
    fn module_offset_when_no_debug_info() {
        let mut sym = Symbolicator::new();
        sym.add_module(ModuleEntry {
            base: 0x1000,
            size: 0x1000,
            path: PathBuf::from("/nonexistent/libfoo.so"),
            build_id: None,
        });
        // Address maps to the module but there's no debug info file.
        let result = sym.lookup(0x1500);
        assert!(matches!(result, LookupResult::ModuleOffset { offset: 0x500, .. }));
    }

    #[test]
    fn remove_module_reverts_to_unknown() {
        let mut sym = Symbolicator::new();
        sym.add_module(ModuleEntry {
            base: 0x1000,
            size: 0x1000,
            path: PathBuf::from("/nonexistent/libfoo.so"),
            build_id: None,
        });
        sym.remove_module(0x1000);
        assert!(matches!(sym.lookup(0x1500), LookupResult::Unknown { .. }));
    }

    #[test]
    fn lookup_many_returns_one_per_address() {
        let mut sym = Symbolicator::new();
        let results = sym.lookup_many(&[0x1000, 0x2000]);
        assert_eq!(results.len(), 2);
    }
}
