/// A single resolved symbol frame, potentially one of several in an inlined call chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolFrame {
    /// Demangled function name, or "<unknown>" on failure.
    pub function: String,
    /// Source file path if available.
    pub file: Option<String>,
    /// Source line number if available.
    pub line: Option<u32>,
    /// Module (binary) name, e.g. "libfoo.so" or "ntdll.dll".
    pub module: String,
    /// Byte offset within the module.
    pub offset: u64,
}

/// Result of resolving one virtual address.
#[derive(Debug, Clone)]
pub enum LookupResult {
    /// Full resolution; may contain multiple frames for inlined call chains (outermost first).
    Resolved(Vec<SymbolFrame>),
    /// Debug info absent or mismatched — degraded to module + offset.
    ModuleOffset { module: String, offset: u64 },
    /// Address doesn't map to any known module.
    Unknown { address: u64 },
}

/// A loaded module entry in the process's virtual address space.
#[derive(Debug, Clone)]
pub struct ModuleEntry {
    /// Base virtual address.
    pub base: u64,
    /// Size in bytes.
    pub size: u64,
    /// Path to the on-disk binary (may be empty for anonymous mappings).
    pub path: std::path::PathBuf,
    /// GNU build-id bytes, if present.
    pub build_id: Option<Vec<u8>>,
}
