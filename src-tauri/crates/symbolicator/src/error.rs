use thiserror::Error;

#[derive(Debug, Error)]
pub enum SymbolicatorError {
    #[error("I/O error reading debug info: {0}")]
    Io(#[from] std::io::Error),

    #[error("object parse error: {0}")]
    Object(#[from] object::Error),

    #[error("DWARF parse error: {0}")]
    Gimli(#[from] gimli::Error),

    #[error("PDB error: {0}")]
    Pdb(String),

    #[error("no debug info found for module {module}")]
    NoDebugInfo { module: String },

    #[error("build-id mismatch for {module}: expected {expected}, found {found}")]
    BuildIdMismatch {
        module: String,
        expected: String,
        found: String,
    },
}
