use std::io::Cursor;
use std::path::Path;
use std::sync::Mutex;

use crate::error::SymbolicatorError;
use crate::types::SymbolFrame;

// Holds PDB state for one binary. The entire PDB is read into memory so the
// PDB struct lifetime is 'static (owned Cursor), avoiding self-referential
// lifetime gymnastics.
struct Inner {
    module_name: String,
    // Reopen from the buffer on each use; pdb::PDB doesn't implement Send, so we
    // store the raw bytes and construct the PDB inside the Mutex on each call.
    // For a fully-cached symbolicator this is fine: this path runs once per address.
    data: Vec<u8>,
}

impl Inner {
    fn resolve(&self, offset: u32) -> Result<Option<Vec<SymbolFrame>>, SymbolicatorError> {
        use pdb::FallibleIterator;

        let cursor = Cursor::new(self.data.as_slice());
        let mut pdb =
            pdb::PDB::open(cursor).map_err(|e| SymbolicatorError::Pdb(e.to_string()))?;

        let address_map = pdb
            .address_map()
            .map_err(|e| SymbolicatorError::Pdb(e.to_string()))?;

        let dbi = pdb
            .debug_information()
            .map_err(|e| SymbolicatorError::Pdb(e.to_string()))?;
        let mut modules = dbi
            .modules()
            .map_err(|e| SymbolicatorError::Pdb(e.to_string()))?;

        while let Some(module) =
            modules.next().map_err(|e| SymbolicatorError::Pdb(e.to_string()))?
        {
            let info = match pdb
                .module_info(&module)
                .map_err(|e| SymbolicatorError::Pdb(e.to_string()))?
            {
                Some(i) => i,
                None => continue,
            };
            let mut symbols = info
                .symbols()
                .map_err(|e| SymbolicatorError::Pdb(e.to_string()))?;
            while let Some(sym) =
                symbols.next().map_err(|e| SymbolicatorError::Pdb(e.to_string()))?
            {
                if let Ok(pdb::SymbolData::Procedure(proc)) = sym.parse() {
                    let rva = match proc.offset.to_rva(&address_map) {
                        Some(r) => r.0,
                        None => continue,
                    };
                    if offset >= rva && offset < rva + proc.len {
                        return Ok(Some(vec![SymbolFrame {
                            function: format!("{}", proc.name),
                            file: None,
                            line: None,
                            module: self.module_name.clone(),
                            offset: offset as u64,
                        }]));
                    }
                }
            }
        }
        Ok(None)
    }
}

/// Resolves addresses from Windows PDB debug info.
pub struct PdbResolver {
    inner: Mutex<Inner>,
}

impl PdbResolver {
    /// Read a PDB file fully into memory and prepare for resolution.
    pub fn open(pdb_path: &Path, module_name: &str) -> Result<Self, SymbolicatorError> {
        let data = std::fs::read(pdb_path)?;
        // Validate it's a PDB by doing a throwaway open.
        let cursor = Cursor::new(data.as_slice());
        pdb::PDB::open(cursor).map_err(|e| SymbolicatorError::Pdb(e.to_string()))?;

        Ok(Self {
            inner: Mutex::new(Inner {
                module_name: module_name.to_owned(),
                data,
            }),
        })
    }

    /// Resolve a module-relative address (RVA).
    pub fn resolve(&self, offset: u32) -> Result<Option<Vec<SymbolFrame>>, SymbolicatorError> {
        self.inner.lock().unwrap().resolve(offset)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_nonexistent_returns_io_err() {
        let result = PdbResolver::open(Path::new("/nonexistent/path.pdb"), "test.dll");
        assert!(matches!(result, Err(SymbolicatorError::Io(_))));
    }
}
