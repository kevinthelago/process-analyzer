//! Smoke-tests for query-result schemas.
//!
//! Verifies that each query-result schema can be serialised to Arrow IPC
//! and back, and that the column names and types are stable (any accidental
//! change here would break the frontend contract).

use std::sync::Arc;

use arrow::array::{
    Float64Array, Int32Array, Int64Array, RecordBatch, StringArray,
};
use trace_core::{
    ipc,
    query_schema::{call_tree_diff_schema, call_tree_schema, timeline_events_schema},
};

// ── timeline_events ───────────────────────────────────────────────────────────

#[test]
fn timeline_events_schema_columns() {
    let schema = timeline_events_schema();
    assert_eq!(schema.field(0).name(), "time_ns");
    assert_eq!(schema.field(1).name(), "duration_ns");
    assert_eq!(schema.field(2).name(), "pid");
    assert_eq!(schema.field(3).name(), "tid");
    assert_eq!(schema.field(4).name(), "name");
    assert_eq!(schema.field(5).name(), "kind");
    assert_eq!(schema.field(6).name(), "depth");
    assert_eq!(schema.fields().len(), 7);
}

#[test]
fn timeline_events_ipc_round_trip() {
    let schema = timeline_events_schema();
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int64Array::from(vec![1_000_000_i64, 2_000_000])),
            Arc::new(Int64Array::from(vec![500_i64, 1_000])),
            Arc::new(Int32Array::from(vec![100_i32, 100])),
            Arc::new(Int32Array::from(vec![200_i32, 201])),
            Arc::new(StringArray::from(vec!["pthread_mutex_lock", "read"])),
            Arc::new(StringArray::from(vec!["syscall", "io"])),
            Arc::new(Int32Array::from(vec![0_i32, 1])),
        ],
    )
    .unwrap();

    let bytes = ipc::encode_batches(schema.clone(), &[batch]).unwrap();
    let (decoded_schema, decoded) = ipc::decode_batches(&bytes).unwrap();

    assert_eq!(decoded_schema, schema);
    assert_eq!(decoded[0].num_rows(), 2);
}

// ── call_tree ─────────────────────────────────────────────────────────────────

#[test]
fn call_tree_schema_columns() {
    let schema = call_tree_schema();
    assert_eq!(schema.field(0).name(), "id");
    assert_eq!(schema.field(1).name(), "parent_id");
    assert_eq!(schema.field(2).name(), "frame");
    assert_eq!(schema.field(3).name(), "self_ns");
    assert_eq!(schema.field(4).name(), "total_ns");
    assert_eq!(schema.field(5).name(), "depth");
    assert_eq!(schema.fields().len(), 6);
}

#[test]
fn call_tree_ipc_round_trip() {
    let schema = call_tree_schema();
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int32Array::from(vec![0_i32, 1, 2])),
            Arc::new(Int32Array::from(vec![-1_i32, 0, 0])),
            Arc::new(StringArray::from(vec!["main", "foo", "bar"])),
            Arc::new(Int64Array::from(vec![100_i64, 60, 40])),
            Arc::new(Int64Array::from(vec![200_i64, 80, 50])),
            Arc::new(Int32Array::from(vec![0_i32, 1, 1])),
        ],
    )
    .unwrap();

    let bytes = ipc::encode_batches(schema.clone(), &[batch]).unwrap();
    let (decoded_schema, decoded) = ipc::decode_batches(&bytes).unwrap();

    assert_eq!(decoded_schema, schema);
    assert_eq!(decoded[0].num_rows(), 3);
}

// ── call_tree_diff ────────────────────────────────────────────────────────────

#[test]
fn call_tree_diff_schema_columns() {
    let schema = call_tree_diff_schema();
    assert_eq!(schema.field(0).name(), "id");
    assert_eq!(schema.field(1).name(), "parent_id");
    assert_eq!(schema.field(2).name(), "frame");
    assert_eq!(schema.field(3).name(), "baseline_ns");
    assert_eq!(schema.field(4).name(), "regression_ns");
    assert_eq!(schema.field(5).name(), "delta_pct");
    assert_eq!(schema.field(6).name(), "depth");
    assert_eq!(schema.fields().len(), 7);
}

#[test]
fn call_tree_diff_ipc_round_trip() {
    let schema = call_tree_diff_schema();
    let batch = RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(Int32Array::from(vec![0_i32, 1])),
            Arc::new(Int32Array::from(vec![-1_i32, 0])),
            Arc::new(StringArray::from(vec!["main", "hot_path"])),
            Arc::new(Int64Array::from(vec![1_000_000_i64, 800_000])),
            Arc::new(Int64Array::from(vec![1_200_000_i64, 960_000])),
            Arc::new(Float64Array::from(vec![20.0_f64, 20.0])),
            Arc::new(Int32Array::from(vec![0_i32, 1])),
        ],
    )
    .unwrap();

    let bytes = ipc::encode_batches(schema.clone(), &[batch]).unwrap();
    let (decoded_schema, decoded) = ipc::decode_batches(&bytes).unwrap();

    assert_eq!(decoded_schema, schema);
    assert_eq!(decoded[0].num_rows(), 2);

    // Spot-check delta_pct column round-trips as Float64.
    use arrow::array::Float64Array;
    let delta = decoded[0]
        .column(5)
        .as_any()
        .downcast_ref::<Float64Array>()
        .unwrap();
    assert!((delta.value(0) - 20.0).abs() < f64::EPSILON);
}
