use std::sync::Arc;

use arrow_array::{
    ArrayRef, Int64Array, StringArray, TimestampNanosecondArray, UInt32Array, UInt64Array,
    UInt8Array,
};
use arrow_array::RecordBatch;
use arrow_schema::{ArrowError, Schema};

use crate::error::NormalizerError;

/// Accumulates rows and emits Arrow RecordBatches at a configurable capacity.
pub struct BatchBuilder {
    schema: Arc<Schema>,
    batch_size: usize,

    col_timestamp_ns: Vec<i64>,
    col_event_type: Vec<u8>,
    col_pid: Vec<u32>,
    col_tid: Vec<u32>,
    col_stack_id: Vec<u32>,
    col_aux_u64: Vec<Option<u64>>,
    col_aux_i64: Vec<Option<i64>>,
    col_aux_u32: Vec<Option<u32>>,
    col_name: Vec<Option<String>>,
}

impl BatchBuilder {
    pub fn new(schema: Arc<Schema>, batch_size: usize) -> Self {
        let batch_size = batch_size.max(1);
        Self {
            schema,
            batch_size,
            col_timestamp_ns: Vec::with_capacity(batch_size),
            col_event_type: Vec::with_capacity(batch_size),
            col_pid: Vec::with_capacity(batch_size),
            col_tid: Vec::with_capacity(batch_size),
            col_stack_id: Vec::with_capacity(batch_size),
            col_aux_u64: Vec::with_capacity(batch_size),
            col_aux_i64: Vec::with_capacity(batch_size),
            col_aux_u32: Vec::with_capacity(batch_size),
            col_name: Vec::with_capacity(batch_size),
        }
    }

    /// Append one row. Returns a completed batch if the row brings us to `batch_size`.
    #[allow(clippy::too_many_arguments)]
    pub fn append(
        &mut self,
        timestamp_ns: i64,
        event_type: u8,
        pid: u32,
        tid: u32,
        stack_id: u32,
        aux_u64: Option<u64>,
        aux_i64: Option<i64>,
        aux_u32: Option<u32>,
        name: Option<&str>,
    ) -> Result<Option<RecordBatch>, NormalizerError> {
        self.col_timestamp_ns.push(timestamp_ns);
        self.col_event_type.push(event_type);
        self.col_pid.push(pid);
        self.col_tid.push(tid);
        self.col_stack_id.push(stack_id);
        self.col_aux_u64.push(aux_u64);
        self.col_aux_i64.push(aux_i64);
        self.col_aux_u32.push(aux_u32);
        self.col_name.push(name.map(str::to_owned));

        if self.col_event_type.len() >= self.batch_size {
            self.finish_batch().map(Some)
        } else {
            Ok(None)
        }
    }

    /// Flush any buffered rows as a batch. Returns `None` if there are no rows.
    pub fn flush(&mut self) -> Result<Option<RecordBatch>, NormalizerError> {
        if self.col_event_type.is_empty() {
            return Ok(None);
        }
        self.finish_batch().map(Some)
    }

    fn finish_batch(&mut self) -> Result<RecordBatch, NormalizerError> {
        let n = self.col_event_type.len();

        let columns: Vec<ArrayRef> = vec![
            Arc::new(TimestampNanosecondArray::from(
                std::mem::replace(&mut self.col_timestamp_ns, Vec::with_capacity(self.batch_size)),
            )),
            Arc::new(UInt8Array::from(
                std::mem::replace(&mut self.col_event_type, Vec::with_capacity(self.batch_size)),
            )),
            Arc::new(UInt32Array::from(
                std::mem::replace(&mut self.col_pid, Vec::with_capacity(self.batch_size)),
            )),
            Arc::new(UInt32Array::from(
                std::mem::replace(&mut self.col_tid, Vec::with_capacity(self.batch_size)),
            )),
            Arc::new(UInt32Array::from(
                std::mem::replace(&mut self.col_stack_id, Vec::with_capacity(self.batch_size)),
            )),
            Arc::new(UInt64Array::from(
                std::mem::replace(&mut self.col_aux_u64, Vec::with_capacity(self.batch_size)),
            )),
            Arc::new(Int64Array::from(
                std::mem::replace(&mut self.col_aux_i64, Vec::with_capacity(self.batch_size)),
            )),
            Arc::new(UInt32Array::from(
                std::mem::replace(&mut self.col_aux_u32, Vec::with_capacity(self.batch_size)),
            )),
            Arc::new(StringArray::from(
                std::mem::replace(&mut self.col_name, Vec::with_capacity(self.batch_size)),
            )),
        ];

        let batch = RecordBatch::try_new(self.schema.clone(), columns)
            .map_err(ArrowError::from)
            .map_err(NormalizerError::Arrow)?;

        debug_assert_eq!(batch.num_rows(), n);
        Ok(batch)
    }

    pub fn len(&self) -> usize {
        self.col_event_type.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
