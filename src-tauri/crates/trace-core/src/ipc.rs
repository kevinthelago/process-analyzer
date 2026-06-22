//! Arrow IPC encode/decode helpers for the Tauri IPC bridge.
//!
//! Tauri transfers large result-sets from the Rust backend to the React UI as
//! raw bytes, not JSON.  These helpers wrap the Arrow **IPC file** format so
//! the UI can deserialise the bytes directly with the Arrow JS library.
//!
//! Usage:
//! ```no_run
//! use trace_core::ipc;
//! use trace_core::{TraceStore, TableKind};
//!
//! let store = TraceStore::open(std::path::Path::new("trace.patrace")).unwrap();
//! let bytes = ipc::encode_batches(
//!     TableKind::CpuSamples.schema(),
//!     store.batches(TableKind::CpuSamples),
//! ).unwrap();
//! // `bytes` can be returned from a #[tauri::command] as Vec<u8>.
//! ```

use std::io::Cursor;

use arrow::{
    datatypes::SchemaRef,
    ipc::{
        reader::FileReader,
        writer::{FileWriter, IpcWriteOptions},
    },
};
use arrow::array::RecordBatch;

use crate::error::Result;

/// Encode a slice of [`RecordBatch`]es (all sharing `schema`) into Arrow IPC
/// file-format bytes.
///
/// Returns an empty IPC file (header + empty footer) for an empty `batches`
/// slice, which the JS Arrow library can safely deserialise to zero rows.
pub fn encode_batches(schema: SchemaRef, batches: &[RecordBatch]) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    let options = IpcWriteOptions::default();
    let mut writer = FileWriter::try_new_with_options(&mut buf, &schema, options)?;
    for batch in batches {
        writer.write(batch)?;
    }
    writer.finish()?;
    Ok(buf)
}

/// Decode Arrow IPC file-format bytes back into [`RecordBatch`]es.
pub fn decode_batches(bytes: &[u8]) -> Result<(SchemaRef, Vec<RecordBatch>)> {
    let cursor = Cursor::new(bytes);
    let reader = FileReader::try_new(cursor, None)?;
    let schema = reader.schema();
    let mut batches = Vec::new();
    for result in reader {
        batches.push(result?);
    }
    Ok((schema, batches))
}
