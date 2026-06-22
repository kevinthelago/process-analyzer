use std::{
    collections::HashMap,
    path::Path,
};

use arrow::array::RecordBatch;
use arrow::datatypes::SchemaRef;
use parquet::{
    arrow::{
        arrow_reader::ParquetRecordBatchReaderBuilder,
        arrow_writer::ArrowWriter,
    },
    basic::Compression,
    file::properties::WriterProperties,
};

use crate::{
    error::{Error, Result},
    manifest::{FORMAT_VERSION, SCHEMA_VERSION, Manifest, TableMeta},
    schema,
};

// ── TableKind ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TableKind {
    CpuSamples,
    Scheduling,
    DiskIo,
    FileIo,
    Memory,
    Network,
    Processes,
    Threads,
    Frames,
    Stacks,
}

impl TableKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TableKind::CpuSamples => "cpu_samples",
            TableKind::Scheduling => "scheduling",
            TableKind::DiskIo     => "disk_io",
            TableKind::FileIo     => "file_io",
            TableKind::Memory     => "memory",
            TableKind::Network    => "network",
            TableKind::Processes  => "processes",
            TableKind::Threads    => "threads",
            TableKind::Frames     => "frames",
            TableKind::Stacks     => "stacks",
        }
    }

    pub fn from_name(s: &str) -> Option<Self> {
        match s {
            "cpu_samples" => Some(TableKind::CpuSamples),
            "scheduling"  => Some(TableKind::Scheduling),
            "disk_io"     => Some(TableKind::DiskIo),
            "file_io"     => Some(TableKind::FileIo),
            "memory"      => Some(TableKind::Memory),
            "network"     => Some(TableKind::Network),
            "processes"   => Some(TableKind::Processes),
            "threads"     => Some(TableKind::Threads),
            "frames"      => Some(TableKind::Frames),
            "stacks"      => Some(TableKind::Stacks),
            _             => None,
        }
    }

    pub fn schema(self) -> SchemaRef {
        match self {
            TableKind::CpuSamples => schema::cpu_samples_schema(),
            TableKind::Scheduling => schema::scheduling_schema(),
            TableKind::DiskIo     => schema::disk_io_schema(),
            TableKind::FileIo     => schema::file_io_schema(),
            TableKind::Memory     => schema::memory_schema(),
            TableKind::Network    => schema::network_schema(),
            TableKind::Processes  => schema::processes_schema(),
            TableKind::Threads    => schema::threads_schema(),
            TableKind::Frames     => schema::frames_schema(),
            TableKind::Stacks     => schema::stacks_schema(),
        }
    }

    /// All v1 table kinds, in canonical order.
    pub fn all() -> &'static [TableKind] {
        &[
            TableKind::CpuSamples,
            TableKind::Scheduling,
            TableKind::DiskIo,
            TableKind::FileIo,
            TableKind::Memory,
            TableKind::Network,
            TableKind::Processes,
            TableKind::Threads,
            TableKind::Frames,
            TableKind::Stacks,
        ]
    }
}

// ── TraceStore ───────────────────────────────────────────────────────────────

/// In-memory representation of a `.patrace` trace.
///
/// Both the recording path (via [`crate::recorder::StandardRecorder`]) and the
/// read path (via [`TraceStore::open`]) ultimately produce a `TraceStore`.
///
/// Memory model: batches for each domain are stored as a `Vec<RecordBatch>`.
/// Each batch corresponds to exactly one Parquet row-group on disk, so the
/// memory cost equals the logical column footprint of the data — no extra
/// copies.  For very large traces (100 M+ rows), callers should process batches
/// via [`TraceStore::batches`] one at a time rather than materializing all
/// domains simultaneously.
#[derive(Debug)]
pub struct TraceStore {
    pub manifest: Manifest,
    tables: HashMap<TableKind, Vec<RecordBatch>>,
}

impl TraceStore {
    pub fn new(manifest: Manifest) -> Self {
        Self { manifest, tables: HashMap::new() }
    }

    // ── read path ────────────────────────────────────────────────────────────

    /// Open an existing `.patrace` directory.
    ///
    /// Returns [`Error::FormatVersionTooNew`] or [`Error::SchemaVersionTooNew`]
    /// if the on-disk versions exceed what this build supports — the caller
    /// must upgrade the library rather than attempting a best-effort read.
    pub fn open(path: &Path) -> Result<Self> {
        if !path.is_dir() {
            return Err(Error::InvalidDirectory(path.display().to_string()));
        }

        let manifest: Manifest = {
            let bytes = std::fs::read(path.join("manifest.json"))?;
            serde_json::from_slice(&bytes)?
        };

        if manifest.format_version > FORMAT_VERSION {
            return Err(Error::FormatVersionTooNew {
                found:     manifest.format_version,
                supported: FORMAT_VERSION,
            });
        }
        if manifest.schema_version > SCHEMA_VERSION {
            return Err(Error::SchemaVersionTooNew {
                found:     manifest.schema_version,
                supported: SCHEMA_VERSION,
            });
        }

        let mut tables: HashMap<TableKind, Vec<RecordBatch>> = HashMap::new();

        for table_meta in &manifest.tables {
            let kind = TableKind::from_name(&table_meta.name)
                .ok_or_else(|| Error::UnknownTable(table_meta.name.clone()))?;

            let file_path = path.join(&table_meta.file);
            if !file_path.exists() {
                continue;
            }

            let file = std::fs::File::open(&file_path)?;
            let reader = ParquetRecordBatchReaderBuilder::try_new(file)?.build()?;

            let expected_schema = kind.schema();
            let mut batches = Vec::new();

            for result in reader {
                let batch = result?;
                // Validate schema on the first batch.
                if batches.is_empty() && batch.schema() != expected_schema {
                    return Err(Error::SchemaMismatch {
                        table: kind.as_str().to_owned(),
                        detail: format!(
                            "file schema {:?} != expected {:?}",
                            batch.schema(), expected_schema
                        ),
                    });
                }
                batches.push(batch);
            }

            if !batches.is_empty() {
                tables.insert(kind, batches);
            }
        }

        Ok(Self { manifest, tables })
    }

    // ── write path ───────────────────────────────────────────────────────────

    /// Persist the store to a `.patrace` directory.
    ///
    /// The directory is created if it does not exist.  Each non-empty domain
    /// is written as a separate Parquet file; each [`RecordBatch`] in memory
    /// becomes one Parquet row-group, preserving the streaming batch boundaries.
    pub fn save(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path)?;

        let props = WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .build();

        let mut table_metas: Vec<TableMeta> = Vec::new();

        for kind in TableKind::all() {
            let Some(batches) = self.tables.get(kind) else { continue };
            if batches.is_empty() { continue; }

            let file_name = format!("{}.parquet", kind.as_str());
            let file = std::fs::File::create(path.join(&file_name))?;

            let schema = kind.schema();
            let mut writer = ArrowWriter::try_new(file, schema, Some(props.clone()))?;

            let mut row_count = 0u64;
            for batch in batches {
                writer.write(batch)?;
                row_count += batch.num_rows() as u64;
            }
            writer.close()?;

            table_metas.push(TableMeta {
                name:      kind.as_str().to_owned(),
                file:      file_name,
                row_count,
            });
        }

        // Write manifest with final table list.
        let mut manifest = self.manifest.clone();
        manifest.tables = table_metas;

        let manifest_json = serde_json::to_vec_pretty(&manifest)?;
        std::fs::write(path.join("manifest.json"), manifest_json)?;

        Ok(())
    }

    // ── accessors ────────────────────────────────────────────────────────────

    /// Append a validated [`RecordBatch`] to the given table.
    ///
    /// Returns [`Error::SchemaMismatch`] if `batch.schema()` differs from the
    /// canonical schema for `kind`.
    pub fn append_batch(&mut self, kind: TableKind, batch: RecordBatch) -> Result<()> {
        let expected = kind.schema();
        if batch.schema() != expected {
            return Err(Error::SchemaMismatch {
                table:  kind.as_str().to_owned(),
                detail: format!(
                    "expected {:?}, got {:?}",
                    expected, batch.schema()
                ),
            });
        }
        self.tables.entry(kind).or_default().push(batch);
        Ok(())
    }

    /// Iterate over all batches for `kind` in insertion order.
    pub fn batches(&self, kind: TableKind) -> &[RecordBatch] {
        self.tables.get(&kind).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Total row count across all batches for `kind`.
    pub fn row_count(&self, kind: TableKind) -> usize {
        self.tables
            .get(&kind)
            .map(|v| v.iter().map(|b| b.num_rows()).sum())
            .unwrap_or(0)
    }

    /// Canonical Arrow schema for `kind`.  Does not require a store instance.
    pub fn table_schema(kind: TableKind) -> SchemaRef {
        kind.schema()
    }

    /// Append all batches from `other` into `self`.
    ///
    /// Tables present in `other` but not `self` are added; tables present in
    /// both are extended (not merged/deduped — the caller is responsible for
    /// ensuring there are no duplicate events).  `self.manifest` is unchanged;
    /// the caller should update it as appropriate.
    pub fn merge(&mut self, other: TraceStore) -> Result<()> {
        for (kind, batches) in other.tables {
            for batch in batches {
                self.append_batch(kind, batch)?;
            }
        }
        Ok(())
    }

    /// Returns `(min_timestamp_ns, max_timestamp_ns)` across all batches for
    /// `kind`, or `None` if the table is empty or has no `timestamp_ns` column.
    ///
    /// Walks the raw Arrow buffers — no DataFusion dependency required.
    pub fn time_range(&self, kind: TableKind) -> Option<(i64, i64)> {
        use arrow::array::{Array, Int64Array};
        use arrow::datatypes::DataType;

        let batches = self.tables.get(&kind)?;
        let schema = kind.schema();
        let ts_idx = schema.fields().iter().position(|f| {
            f.name() == "timestamp_ns" && *f.data_type() == DataType::Int64
        })?;

        let mut global_min = i64::MAX;
        let mut global_max = i64::MIN;
        let mut found_any = false;

        for batch in batches {
            let col = batch.column(ts_idx);
            let arr = col.as_any().downcast_ref::<Int64Array>()?;
            if arr.is_empty() {
                continue;
            }
            // Use arrow's built-in min/max kernels via iterator (avoids pulling
            // in arrow-arith for a simple reduction).
            for i in 0..arr.len() {
                if arr.is_valid(i) {
                    let v = arr.value(i);
                    if v < global_min { global_min = v; }
                    if v > global_max { global_max = v; }
                    found_any = true;
                }
            }
        }

        if found_any { Some((global_min, global_max)) } else { None }
    }
}
