use std::path::Path;
use std::sync::Arc;

use addr2line::gimli;
use object::{Object, ObjectSection};

use crate::error::SymbolicatorError;
use crate::types::SymbolFrame;

// Reader type: Arc<[u8]> slices so the Context owns its data and is Send.
type Reader = gimli::EndianReader<gimli::RunTimeEndian, Arc<[u8]>>;

/// An open DWARF debug-info context backed by owned (Arc) section slices.
pub struct DwarfContext {
    module_name: String,
    ctx: addr2line::Context<Reader>,
}

impl DwarfContext {
    /// Read `path` into memory, parse DWARF sections, and build an addr2line context.
    pub fn open(path: &Path, module_name: &str) -> Result<Self, SymbolicatorError> {
        let data = std::fs::read(path)?;
        let obj = object::File::parse(data.as_slice())?;

        let endian = if obj.is_little_endian() {
            gimli::RunTimeEndian::Little
        } else {
            gimli::RunTimeEndian::Big
        };

        let load_section =
            |id: gimli::SectionId| -> Result<Reader, gimli::Error> {
                let bytes: Arc<[u8]> = obj
                    .section_by_name(id.name())
                    .and_then(|s| s.uncompressed_data().ok())
                    .map(|cow| Arc::from(cow.as_ref()))
                    .unwrap_or_else(|| Arc::from(&[][..]));
                Ok(gimli::EndianReader::new(bytes, endian))
            };

        let dwarf = gimli::Dwarf::load(load_section)?;
        let ctx = addr2line::Context::from_dwarf(dwarf)
            .map_err(SymbolicatorError::Gimli)?;

        Ok(Self {
            module_name: module_name.to_owned(),
            ctx,
        })
    }

    /// Resolve a module-relative virtual address to a chain of frames (for inlining).
    /// Returns `None` if the address has no debug info.
    pub fn resolve(&self, offset: u64) -> Result<Option<Vec<SymbolFrame>>, SymbolicatorError> {
        // find_frames returns LookupResult (lazy split-DWARF); skip_all_loads() drives
        // it synchronously, which is fine since we're not loading external .dwo files.
        let mut frames_iter = self
            .ctx
            .find_frames(offset)
            .skip_all_loads()
            .map_err(SymbolicatorError::Gimli)?;

        let mut frames = Vec::new();
        loop {
            match frames_iter.next().map_err(SymbolicatorError::Gimli)? {
                None => break,
                Some(frame) => {
                    let function = frame
                        .function
                        .as_ref()
                        .and_then(|f| f.demangle().ok())
                        .map(|s| s.into_owned())
                        .unwrap_or_else(|| "<unknown>".to_owned());

                    let (file, line) = frame
                        .location
                        .map(|loc| (loc.file.map(str::to_owned), loc.line))
                        .unwrap_or((None, None));

                    frames.push(SymbolFrame {
                        function,
                        file,
                        line,
                        module: self.module_name.clone(),
                        offset,
                    });
                }
            }
        }

        if frames.is_empty() {
            Ok(None)
        } else {
            Ok(Some(frames))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_nonexistent_returns_io_err() {
        let result = DwarfContext::open(Path::new("/nonexistent/path/to.so"), "test.so");
        assert!(matches!(result, Err(SymbolicatorError::Io(_))));
    }
}
